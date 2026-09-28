use super::ShellComponentTransport;
use sophia_protocol::{ContentOutputId, ShellContentRecord, TransactionId};

use super::wire::ContentWant;
use super::{ShellSessionTransport, ShellTransportError};
use crate::{ContentCandidateContext, ContentRenderBundle};

// Every candidate response owns a control credit, charged at least its whole
// record on either wire; credits are reserved before peer input is consumed.

impl ShellComponentTransport {
    /// Accept and coalesce frame demands and exact cancellations. Allocation
    /// validity is supplied by the Engine owner, never inferred from the wire.
    pub fn service_content_demands(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        outputs: &[ContentOutputId],
        allocations: &[crate::ContentAllocationSnapshot],
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if !self.control_capacity_available(epochs, 1) {
                break;
            }
            let Some((transaction, record)) = self.poll_content_demand_record(epochs)? else {
                break;
            };
            let candidates = epochs
                .active_candidates_mut(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?;
            match record {
                ShellContentRecord::FrameDemand(value) => {
                    candidates.demand(transaction, value, outputs, allocations)?;
                }
                ShellContentRecord::FrameDemandCancel(value) => {
                    candidates.cancel_demand(transaction, value)?;
                }
                _ => return Err(ShellTransportError::WrongContentRecord),
            }
            processed += 1;
            self.flush_content_candidate_events(epochs)?;
        }
        Ok(processed)
    }

    pub fn next_content_demand(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Option<(TransactionId, sophia_protocol::ContentFrameDemand)> {
        epochs
            .active_candidates(self.store_grant)
            .and_then(|candidates| candidates.next_demand())
    }

    pub fn grant_content_demand(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        output: ContentOutputId,
        permit_id: u64,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        if self.content_limits.is_none() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.control_capacity_available(epochs, 2) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .grant_demand(transaction, output, permit_id, now_msec)?;
        self.flush_content_candidate_events(epochs)
    }

    /// Publish one Engine-issued permit after the owner has accepted/coalesced a
    /// demand. This reserves the candidate's complete response lifecycle.
    pub fn grant_content_permit(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        output: ContentOutputId,
        demand_id: u64,
        permit_id: u64,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        if self.content_limits.is_none() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.control_capacity_available(epochs, 3) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .grant_permit(transaction, output, demand_id, permit_id, now_msec)?;
        self.flush_content_candidate_events(epochs)
    }

    /// Service only candidate assembly. Allocation requests, frame demands and
    /// actions remain queued for their separate Engine owners.
    pub fn service_content_candidates(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        contexts: &[ContentCandidateContext<'_>],
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_candidate_events(epochs)?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if !self.control_capacity_available(epochs, 0) {
                break;
            }
            let Some((transaction, record)) = self.poll_content_candidate_record(epochs)? else {
                break;
            };
            if std::env::var_os("SOPHIA_SHELL_CONTENT_TRACE").is_some() {
                let (stage, generation, output) = match &record {
                    ShellContentRecord::CandidateBegin(value) => {
                        ("begin", value.candidate_generation, value.output.id)
                    }
                    ShellContentRecord::CandidateChunk(value) => {
                        ("chunk", value.candidate_generation, 0)
                    }
                    ShellContentRecord::CandidateEnd(value) => {
                        ("end", value.candidate_generation, 0)
                    }
                    _ => unreachable!("candidate record was selected above"),
                };
                tracing::info!(
                    target: "sophia_shell_content_trace",
                    schema = 1,
                    status = "intake",
                    stage,
                    candidate_generation = generation,
                    output,
                    transaction = transaction.raw(),
                    now_msec,
                    "shell content candidate intake"
                );
            }
            let context = match &record {
                ShellContentRecord::CandidateEnd(value) => {
                    let output = epochs
                        .active_candidates(self.store_grant)
                        .and_then(|candidates| {
                            candidates.assembling_output(value.candidate_generation)
                        })
                        .ok_or(ShellTransportError::WrongContentRecord)?;
                    Some(
                        contexts
                            .iter()
                            .find(|context| context.output == output)
                            .copied()
                            .ok_or(ShellTransportError::WrongContentRecord)?,
                    )
                }
                _ => None,
            };
            let trace_record = match &record {
                ShellContentRecord::CandidateBegin(value) => {
                    ("begin", value.candidate_generation, value.output.id)
                }
                ShellContentRecord::CandidateChunk(value) => {
                    ("chunk", value.candidate_generation, 0)
                }
                ShellContentRecord::CandidateEnd(value) => ("end", value.candidate_generation, 0),
                _ => unreachable!("candidate record was selected above"),
            };
            let (outcome, reported) = {
                let (resources, candidates) = epochs
                    .active_parts_mut(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?;
                let outcome = match record {
                    ShellContentRecord::CandidateBegin(value) => {
                        candidates.begin(transaction, value, now_msec)
                    }
                    ShellContentRecord::CandidateChunk(value) => {
                        candidates.chunk(transaction, value, now_msec)
                    }
                    ShellContentRecord::CandidateEnd(value) => candidates.end(
                        transaction,
                        value,
                        context.expect("End selected one exact context"),
                        resources,
                        now_msec,
                    ),
                    _ => return Err(ShellTransportError::WrongContentRecord),
                };
                (outcome, candidates.pending_event().is_some())
            };
            if std::env::var_os("SOPHIA_SHELL_CONTENT_TRACE").is_some()
                && let Err(error) = &outcome
            {
                let (stage, generation, output) = trace_record;
                tracing::info!(
                    target: "sophia_shell_content_trace",
                    schema = 1,
                    status = "rejected",
                    stage,
                    candidate_generation = generation,
                    output,
                    ?error,
                    reported,
                    "shell content candidate rejected"
                );
            }
            processed += 1;
            if outcome.is_err() && reported {
                self.discard_candidate_rest(super::files::CandidateFamily::Base);
            }
            self.flush_content_candidate_events(epochs)?;
            if let Err(error) = outcome
                && !reported
            {
                return Err(error.into());
            }
        }
        Ok(processed)
    }

    pub fn begin_content_submission(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        output: ContentOutputId,
        candidate_generation: u64,
        now_msec: u64,
    ) -> Result<ContentRenderBundle, ShellTransportError> {
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .begin_submission(output, candidate_generation, now_msec)
            .map_err(Into::into)
    }

    pub fn next_content_submission(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Option<(ContentOutputId, u64)> {
        self.next_content_submission_for(epochs, |_| true)
    }

    pub fn next_content_submission_for(
        &self,
        epochs: &crate::ContentEpochRegistry,
        available: impl FnMut(ContentOutputId) -> bool,
    ) -> Option<(ContentOutputId, u64)> {
        epochs
            .active_candidates(self.store_grant)
            .and_then(|candidates| candidates.next_pending_candidate_for(available))
    }

    pub fn content_prepared(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        grant: sophia_protocol::ContentGrant,
        output: ContentOutputId,
        candidate_generation: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        self.validate_completion_grant(epochs, grant)?;
        let connected = self.content_grant == Some(grant);
        // The already-owned response credit must still fit before the native
        // result changes reducer state. No I/O occurs during the subsequent
        // credit-to-FIFO transfer, so backpressure cannot strand a retry.
        if connected && !self.control_capacity_available(epochs, 0) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        epochs
            .candidates_mut(grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .prepared(
                output,
                candidate_generation,
                work_area_generation,
                wm_commit_generation,
                now_msec,
            )?;
        if connected {
            self.flush_content_candidate_events(epochs)?;
        }
        Ok(())
    }

    pub fn content_presented(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        grant: sophia_protocol::ContentGrant,
        output: ContentOutputId,
        candidate_generation: u64,
        presentation_epoch: u64,
        work_area_generation: u64,
        wm_commit_generation: u64,
    ) -> Result<(), ShellTransportError> {
        self.validate_completion_grant(epochs, grant)?;
        let connected = self.content_grant == Some(grant);
        // The already-owned response credit must still fit before the native
        // result changes reducer state. No I/O occurs during the subsequent
        // credit-to-FIFO transfer, so backpressure cannot strand a retry.
        if connected && !self.control_capacity_available(epochs, 0) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        let native_presented = epochs.candidates_mut(grant).and_then(|s| {
            s.native_presented_metadata(output, candidate_generation, presentation_epoch)
        });
        epochs
            .candidates_mut(grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .presented(
                output,
                candidate_generation,
                presentation_epoch,
                work_area_generation,
                wm_commit_generation,
            )?;
        if connected {
            if let Some(shown) = native_presented {
                self.native_control.presented = Some(shown);
            }
            self.flush_content_candidate_events(epochs)?;
        }
        epochs.collect();
        Ok(())
    }

    pub fn content_renderer_failed(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        grant: sophia_protocol::ContentGrant,
        output: ContentOutputId,
        candidate_generation: u64,
    ) -> Result<(), ShellTransportError> {
        self.validate_completion_grant(epochs, grant)?;
        let connected = self.content_grant == Some(grant);
        // The already-owned response credit must still fit before the native
        // result changes reducer state. No I/O occurs during the subsequent
        // credit-to-FIFO transfer, so backpressure cannot strand a retry.
        if connected && !self.control_capacity_available(epochs, 0) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        epochs
            .candidates_mut(grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .renderer_failed(output, candidate_generation)?;
        if connected {
            self.flush_content_candidate_events(epochs)?;
        }
        epochs.collect();
        Ok(())
    }

    // Disconnected completion may settle its exact retained epoch, but a
    // neighbor's live grant must be routed through its own response owner.
    fn validate_completion_grant(
        &self,
        epochs: &crate::ContentEpochRegistry,
        grant: sophia_protocol::ContentGrant,
    ) -> Result<(), ShellTransportError> {
        if self.content_grant != Some(grant) && epochs.active_candidates(grant).is_some() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(())
    }

    fn poll_content_candidate_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io(epochs)?;
        let taken = self.take_content(ContentWant::CandidatePart)?;
        self.admit_client_record(taken)
    }

    fn poll_content_demand_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io(epochs)?;
        let taken = self.take_content(ContentWant::Demand)?;
        self.admit_client_record(taken)
    }

    pub(super) fn flush_content_candidate_events(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        loop {
            let event = epochs
                .active_candidates_mut(self.store_grant)
                .and_then(|store| store.pending_event().cloned());
            let Some(event) = event else {
                return Ok(());
            };
            let admitted =
                self.prepare_content_record(epochs, event.transaction, &event.record, true)?;
            let Some(store) = epochs.active_candidates_mut(self.store_grant) else {
                return Err(ShellTransportError::MissingCapability);
            };
            if store.pending_event() != Some(&event) {
                return Err(ShellTransportError::WrongContentRecord);
            }
            self.transfer_record(admitted);
            store.take_event();
        }
    }
}

// Legacy single-shell facade, delegating to the same shared registry path.

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn service_content_demands(
                &mut self,
                outputs: &[ContentOutputId],
                allocations: &[crate::ContentAllocationSnapshot],
            ) -> Result<usize, ShellTransportError> {
                self.state
                    .service_content_demands(&mut self.content_epochs, outputs, allocations)
            }

            pub fn next_content_demand(
                &self,
            ) -> Option<(TransactionId, sophia_protocol::ContentFrameDemand)> {
                self.state.next_content_demand(&self.content_epochs)
            }

            pub fn grant_content_demand(
                &mut self,
                transaction: TransactionId,
                output: ContentOutputId,
                permit_id: u64,
                now_msec: u64,
            ) -> Result<(), ShellTransportError> {
                self.state.grant_content_demand(
                    &mut self.content_epochs,
                    transaction,
                    output,
                    permit_id,
                    now_msec,
                )
            }

            pub fn grant_content_permit(
                &mut self,
                transaction: TransactionId,
                output: ContentOutputId,
                demand_id: u64,
                permit_id: u64,
                now_msec: u64,
            ) -> Result<(), ShellTransportError> {
                self.state.grant_content_permit(
                    &mut self.content_epochs,
                    transaction,
                    output,
                    demand_id,
                    permit_id,
                    now_msec,
                )
            }

            pub fn service_content_candidates(
                &mut self,
                contexts: &[ContentCandidateContext<'_>],
                now_msec: u64,
            ) -> Result<usize, ShellTransportError> {
                self.state
                    .service_content_candidates(&mut self.content_epochs, contexts, now_msec)
            }

            pub fn begin_content_submission(
                &mut self,
                output: ContentOutputId,
                candidate_generation: u64,
                now_msec: u64,
            ) -> Result<ContentRenderBundle, ShellTransportError> {
                self.state.begin_content_submission(
                    &mut self.content_epochs,
                    output,
                    candidate_generation,
                    now_msec,
                )
            }

            pub fn next_content_submission(&self) -> Option<(ContentOutputId, u64)> {
                self.state.next_content_submission(&self.content_epochs)
            }

            pub fn next_content_submission_for(
                &self,
                available: impl FnMut(ContentOutputId) -> bool,
            ) -> Option<(ContentOutputId, u64)> {
                self.state
                    .next_content_submission_for(&self.content_epochs, available)
            }

            pub fn content_prepared(
                &mut self,
                grant: sophia_protocol::ContentGrant,
                output: ContentOutputId,
                candidate_generation: u64,
                work_area_generation: u64,
                wm_commit_generation: u64,
                now_msec: u64,
            ) -> Result<(), ShellTransportError> {
                self.state.content_prepared(
                    &mut self.content_epochs,
                    grant,
                    output,
                    candidate_generation,
                    work_area_generation,
                    wm_commit_generation,
                    now_msec,
                )
            }

            pub fn content_presented(
                &mut self,
                grant: sophia_protocol::ContentGrant,
                output: ContentOutputId,
                candidate_generation: u64,
                presentation_epoch: u64,
                work_area_generation: u64,
                wm_commit_generation: u64,
            ) -> Result<(), ShellTransportError> {
                self.state.content_presented(
                    &mut self.content_epochs,
                    grant,
                    output,
                    candidate_generation,
                    presentation_epoch,
                    work_area_generation,
                    wm_commit_generation,
                )
            }

            pub fn content_renderer_failed(
                &mut self,
                grant: sophia_protocol::ContentGrant,
                output: ContentOutputId,
                candidate_generation: u64,
            ) -> Result<(), ShellTransportError> {
                self.state.content_renderer_failed(
                    &mut self.content_epochs,
                    grant,
                    output,
                    candidate_generation,
                )
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
