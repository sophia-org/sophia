use sophia_protocol::{ContentOutputId, ShellContentRecord, TransactionId};

use super::{ShellSessionTransport, ShellTransportError, content_admission};
use crate::{ContentCandidateContext, ContentRenderBundle};

// Header plus the largest fixed candidate response. CandidateOutcome is 72
// payload bytes; FramePermit is smaller. Reserve before consuming peer input.
const MAX_CANDIDATE_RESPONSE_BYTES: usize = sophia_protocol::SOPHIA_IPC_HEADER_LEN + 72;

impl ShellSessionTransport {
    /// Accept and coalesce frame demands and exact cancellations. Allocation
    /// validity is supplied by the Engine owner, never inferred from the wire.
    pub fn service_content_demands(
        &mut self,
        outputs: &[ContentOutputId],
        allocations: &[crate::ContentAllocationSnapshot],
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if self
                .output
                .len()
                .saturating_add(MAX_CANDIDATE_RESPONSE_BYTES)
                > limits.max_output_queue_bytes as usize
            {
                break;
            }
            let Some((transaction, record)) = self.poll_content_demand_record()? else {
                break;
            };
            let candidates = self
                .content_epochs
                .active_candidates_mut()
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
            self.flush_content_candidate_events()?;
        }
        Ok(processed)
    }

    pub fn next_content_demand(
        &self,
    ) -> Option<(TransactionId, sophia_protocol::ContentFrameDemand)> {
        self.content_epochs
            .active_candidates()
            .and_then(|candidates| candidates.next_demand())
    }

    pub fn grant_content_demand(
        &mut self,
        transaction: TransactionId,
        output: ContentOutputId,
        permit_id: u64,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        if self
            .output
            .len()
            .saturating_add(MAX_CANDIDATE_RESPONSE_BYTES)
            > limits.max_output_queue_bytes as usize
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.content_epochs
            .active_candidates_mut()
            .ok_or(ShellTransportError::MissingCapability)?
            .grant_demand(transaction, output, permit_id, now_msec)?;
        self.flush_content_candidate_events()
    }

    /// Publish one Engine-issued permit after the owner has accepted/coalesced a
    /// demand. This reserves the candidate's complete response lifecycle.
    pub fn grant_content_permit(
        &mut self,
        transaction: TransactionId,
        output: ContentOutputId,
        demand_id: u64,
        permit_id: u64,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        if self
            .output
            .len()
            .saturating_add(MAX_CANDIDATE_RESPONSE_BYTES)
            > limits.max_output_queue_bytes as usize
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.content_epochs
            .active_candidates_mut()
            .ok_or(ShellTransportError::MissingCapability)?
            .grant_permit(transaction, output, demand_id, permit_id, now_msec)?;
        self.flush_content_candidate_events()
    }

    /// Service only candidate assembly. Allocation requests, frame demands and
    /// actions remain queued for their separate Engine owners.
    pub fn service_content_candidates(
        &mut self,
        contexts: &[ContentCandidateContext<'_>],
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        self.content_epochs
            .active_candidates_mut()
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_candidate_events()?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if self
                .output
                .len()
                .saturating_add(MAX_CANDIDATE_RESPONSE_BYTES)
                > limits.max_output_queue_bytes as usize
            {
                break;
            }
            let Some((transaction, record)) = self.poll_content_candidate_record()? else {
                break;
            };
            let context = match &record {
                ShellContentRecord::CandidateEnd(value) => {
                    let output = self
                        .content_epochs
                        .active_candidates()
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
            let (outcome, reported) = {
                let (resources, candidates) = self
                    .content_epochs
                    .active_parts_mut()
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
            processed += 1;
            self.flush_content_candidate_events()?;
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
        output: ContentOutputId,
        candidate_generation: u64,
        now_msec: u64,
    ) -> Result<ContentRenderBundle, ShellTransportError> {
        self.content_epochs
            .active_candidates_mut()
            .ok_or(ShellTransportError::MissingCapability)?
            .begin_submission(output, candidate_generation, now_msec)
            .map_err(Into::into)
    }

    pub fn next_content_submission(&self) -> Option<(ContentOutputId, u64)> {
        self.content_epochs
            .active_candidates()
            .and_then(|candidates| candidates.next_pending_candidate())
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
        let connected = self.content_grant == Some(grant);
        self.content_epochs
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
            self.flush_content_candidate_events()?;
        }
        Ok(())
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
        let connected = self.content_grant == Some(grant);
        self.content_epochs
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
            self.flush_content_candidate_events()?;
        }
        self.content_epochs.collect();
        Ok(())
    }

    pub fn content_renderer_failed(
        &mut self,
        grant: sophia_protocol::ContentGrant,
        output: ContentOutputId,
        candidate_generation: u64,
    ) -> Result<(), ShellTransportError> {
        let connected = self.content_grant == Some(grant);
        self.content_epochs
            .candidates_mut(grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .renderer_failed(output, candidate_generation)?;
        if connected {
            self.flush_content_candidate_events()?;
        }
        self.content_epochs.collect();
        Ok(())
    }

    fn poll_content_candidate_record(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io()?;
        let at = self
            .inbox
            .iter()
            .position(|frame| matches!(u16::from_le_bytes([frame[6], frame[7]]), 172..=174));
        let Some(frame) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, record) = sophia_protocol::decode_shell_content_frame(&frame)?;
        if !content_admission::client_record(&record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(&record) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(Some((transaction, record)))
    }

    fn poll_content_demand_record(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io()?;
        let at = self
            .inbox
            .iter()
            .position(|frame| matches!(u16::from_le_bytes([frame[6], frame[7]]), 176 | 178));
        let Some(frame) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, record) = sophia_protocol::decode_shell_content_frame(&frame)?;
        if !content_admission::client_record(&record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(&record) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(Some((transaction, record)))
    }

    fn flush_content_candidate_events(&mut self) -> Result<(), ShellTransportError> {
        loop {
            let event = self
                .content_epochs
                .active_candidates_mut()
                .and_then(|store| store.pending_event().cloned());
            let Some(event) = event else {
                return Ok(());
            };
            self.send_content_record(event.transaction, &event.record)?;
            self.content_epochs
                .active_candidates_mut()
                .expect("event came from active candidate owner")
                .take_event();
        }
    }
}
