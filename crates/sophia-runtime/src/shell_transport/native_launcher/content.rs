use super::*;
use crate::{ContentCandidateContext, ContentRenderBundle, NativeLauncherCandidateContext};

impl ShellComponentTransport {
    /// One bounded native allocation/candidate visit, using the existing inbox,
    /// stores and response FIFO. Session supplies current published identities.
    /// Resources, pacing, allocations and assembly share one record/payload
    /// allowance. Focus/action service and Session scheduling are separate.
    pub fn service_native_launcher_content(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        context: ContentCandidateContext<'_>,
        current: NativeLauncherCandidateContext<'_>,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_launcher_state() != Some((current.opening, current.state_revision)) {
            return Err(ShellTransportError::WrongCandidate);
        }
        if current.opening.grant != self.store_grant || current.opening.output != context.output {
            return Err(ShellTransportError::WrongContentGrant);
        }
        self.poll_io_bounded(epochs, 64 * 1024)?;
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_candidate_events(epochs)?;
        self.flush_content_allocation_events(epochs)?;
        epochs
            .resources_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_resource_events(epochs)?;
        let limit = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?
            .max_frames_per_service_tick
            .min(32) as usize;
        let mut processed = 0;
        let mut remaining = 64 * 1024usize;
        while processed < limit {
            let Some(index) = self.inbox.iter().position(|frame| matches!(u16::from_le_bytes([frame[6], frame[7]]), 163 | 165 | 167..=170 | 172..=174 | 176 | 178 | 188..=190)) else { break; };
            let frame = &self.inbox[index];
            let payload_bytes = frame.len() - SOPHIA_IPC_HEADER_LEN;
            if payload_bytes
                > self
                    .content_limits
                    .as_ref()
                    .expect("checked limits")
                    .max_frame_payload as usize
            {
                return Err(ShellTransportError::WrongContentRecord);
            }
            if payload_bytes > remaining {
                break;
            }
            // Decode and validate role/grant before removing the exact frame.
            let (transaction, record) = decode_native_content_record(frame)?;
            if record.grant() != self.store_grant {
                return Err(ShellTransportError::WrongContentGrant);
            }
            let credit = match &record {
                NativeContentRecord::Allocation(_) | NativeContentRecord::Demand(_) => 1,
                NativeContentRecord::Begin(v) => usize::from(
                    self.native_control
                        .closed
                        .is_some_and(|closed| v.opening == closed.opening),
                ),
                NativeContentRecord::Resource(v) => epochs
                    .resources(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?
                    .additional_response_credit(v),
                _ => 0,
            };
            if !self.control_capacity_available(epochs, credit) {
                break;
            }
            remaining -= payload_bytes;
            self.inbox.remove(index);
            if self.service_previous_native_record(epochs, transaction, &record, context)? {
                processed += 1;
                self.flush_content_candidate_events(epochs)?;
                self.flush_content_allocation_events(epochs)?;
                continue;
            }
            match record {
                NativeContentRecord::Resource(v) => {
                    self.apply_content_resource_record(epochs, transaction, v, now_msec)?
                }
                NativeContentRecord::Demand(v) => {
                    epochs
                        .active_candidates_mut(self.store_grant)
                        .ok_or(ShellTransportError::MissingCapability)?
                        .demand(transaction, v, &[context.output], context.allocations)?;
                }
                NativeContentRecord::Cancel(v) => {
                    let candidates = epochs
                        .active_candidates_mut(self.store_grant)
                        .ok_or(ShellTransportError::MissingCapability)?;
                    match candidates.cancel_demand(transaction, v.clone()) {
                        Ok(()) => {}
                        Err(ContentCandidateError::Stale)
                            if self.native_control.closed.is_some() =>
                        {
                            candidates
                                .closed_native_cancel(v, self.native_control.closed.unwrap())?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                NativeContentRecord::Allocation(request) => {
                    let outcome = epochs
                        .allocations_mut(self.store_grant)
                        .ok_or(ShellTransportError::MissingCapability)?
                        .request_native_launcher(transaction, request, current.opening, now_msec);
                    if let Err(error) = outcome {
                        if error == ContentAllocationError::ClockRegression {
                            return Err(error.into());
                        }
                        self.send_content_record(
                            epochs,
                            transaction,
                            &ShellContentRecord::AllocationResult(ContentAllocationResult {
                                grant: request.grant,
                                allocation_request_id: request.request_id,
                                status: 2,
                                reason: error.reason() as u16,
                                output: request.output,
                                allocation: ContentAllocationId::default(),
                                parent: ContentAllocationId::default(),
                                scale_generation: 0,
                                logical: ContentLogicalRect::default(),
                                pixel: ContentPixelRect::default(),
                                scale_numerator: 0,
                                scale_denominator: 0,
                                allowed_reservation_extent: 0,
                                margins: ContentMargins::default(),
                                acknowledged_anchor: ContentPixelRect::default(),
                            }),
                        )?;
                    }
                }
                record => {
                    let (resources, candidates) = epochs
                        .active_parts_mut(self.store_grant)
                        .ok_or(ShellTransportError::MissingCapability)?;
                    let result = match record {
                        NativeContentRecord::Begin(v) => {
                            candidates.begin_native_launcher(transaction, v, current, now_msec)
                        }
                        NativeContentRecord::Chunk(v) => {
                            candidates.chunk_native_launcher(transaction, v, now_msec)
                        }
                        NativeContentRecord::End(v) => candidates.end_native_launcher(
                            transaction,
                            v,
                            context,
                            current,
                            resources,
                            now_msec,
                        ),
                        NativeContentRecord::Allocation(_)
                        | NativeContentRecord::Resource(_)
                        | NativeContentRecord::Demand(_)
                        | NativeContentRecord::Cancel(_) => unreachable!(),
                    };
                    if let Err(error) = result
                        && candidates.pending_event().is_none()
                    {
                        return Err(error.into());
                    }
                }
            }
            processed += 1;
            self.flush_content_candidate_events(epochs)?;
            self.flush_content_allocation_events(epochs)?;
        }
        if processed == 0 && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(processed)
    }

    pub fn begin_native_launcher_submission(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        generation: u64,
        context: ContentCandidateContext<'_>,
        current: NativeLauncherCandidateContext<'_>,
        now_msec: u64,
    ) -> Result<ContentRenderBundle, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_launcher_state() != Some((current.opening, current.state_revision)) {
            return Err(ShellTransportError::WrongCandidate);
        }
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .begin_native_launcher_submission(generation, context, current, now_msec)
            .map_err(Into::into)
    }
}

pub(super) enum NativeContentRecord {
    Allocation(NativeLauncherAllocationRequest),
    Begin(NativeLauncherCandidateBegin),
    Chunk(ContentCandidateChunk),
    End(ContentCandidateEnd),
    Resource(ShellContentRecord),
    Demand(ContentFrameDemand),
    Cancel(ContentFrameDemandCancel),
}
impl NativeContentRecord {
    pub(super) fn grant(&self) -> ContentGrant {
        match self {
            Self::Allocation(v) => v.grant,
            Self::Begin(v) => v.content.grant,
            Self::Chunk(v) => v.grant,
            Self::End(v) => v.grant,
            Self::Resource(v) => content_admission::record_grant(v).expect("resource has grant"),
            Self::Demand(v) => v.grant,
            Self::Cancel(v) => v.grant,
        }
    }
}

pub(super) fn decode_native_content_record(
    frame: &[u8],
) -> Result<(TransactionId, NativeContentRecord), ShellTransportError> {
    let kind = u16::from_le_bytes([frame[6], frame[7]]);
    let result = if matches!(kind, 165 | 167..=170 | 174 | 176 | 178) {
        let (tx, record) = decode_shell_content_frame(frame)?;
        let record = match record {
            ShellContentRecord::CandidateEnd(end) => NativeContentRecord::End(end),
            ShellContentRecord::FrameDemand(v) => NativeContentRecord::Demand(v),
            ShellContentRecord::FrameDemandCancel(v) => NativeContentRecord::Cancel(v),
            v if content_admission::resource_identity(&v).is_some() => {
                NativeContentRecord::Resource(v)
            }
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        (tx, record)
    } else if (188..=190).contains(&kind) {
        let (tx, record) = decode_shell_native_launcher_frame(frame)?;
        let record = match record {
            ShellNativeLauncherRecord::AllocationRequest(v) => NativeContentRecord::Allocation(v),
            ShellNativeLauncherRecord::CandidateBegin(v) => NativeContentRecord::Begin(v),
            ShellNativeLauncherRecord::CandidateChunk(v) => NativeContentRecord::Chunk(v),
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        (tx, record)
    } else {
        return Err(ShellTransportError::WrongContentRecord);
    };
    Ok(result)
}
