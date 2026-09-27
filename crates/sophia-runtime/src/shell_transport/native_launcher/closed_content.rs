//! Bounded late-content drain for the exact closed opening, without admission.
use super::content::{NativeContentRecord, decode_native_content_record};
use super::*;

impl ShellComponentTransport {
    pub fn service_closed_native_content(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        expected: NativeLauncherOpening,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_launcher_closed_opening() != Some(expected)
            || expected.grant != self.store_grant
        {
            return Err(ShellTransportError::WrongActivation);
        }
        self.poll_io_bounded(epochs, 64 * 1024)?;
        epochs
            .active_candidates_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        epochs.collect();
        epochs
            .resources_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_resource_events(epochs)?;
        self.flush_content_candidate_events(epochs)?;
        self.flush_content_allocation_events(epochs)?;
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        let maximum = limits.max_frames_per_service_tick.min(32) as usize;
        let max_payload = limits.max_frame_payload as usize;
        let mut processed = 0;
        if self.files.is_some() {
            while processed < maximum {
                let Some((transaction, record)) = self.peek_native_file_record()? else {
                    break;
                };
                if record.grant() != self.store_grant {
                    return Err(ShellTransportError::WrongContentGrant);
                }
                let credit = closed_native_content_credit(epochs, self.store_grant, &record)?;
                if !self.control_capacity_available(epochs, credit) {
                    break;
                }
                self.take_native_file_record();
                self.apply_closed_native_content_record(
                    epochs,
                    transaction,
                    record,
                    expected,
                    now_msec,
                )?;
                processed += 1;
                self.flush_content_candidate_events(epochs)?;
                self.flush_content_allocation_events(epochs)?;
            }
            if processed == 0 && self.peer_closed {
                return Err(ShellTransportError::NotConnected);
            }
            return Ok(processed);
        }
        let mut bytes = 64 * 1024usize;
        while processed < maximum {
            let Some(index) = self.inbox.iter().position(|frame| {
                matches!(u16::from_le_bytes([frame[6], frame[7]]),
                163 | 165 | 167..=170 | 172..=174 | 176 | 178 | 188..=190)
            }) else {
                break;
            };
            let frame = &self.inbox[index];
            let size = frame.len() - SOPHIA_IPC_HEADER_LEN;
            if size > max_payload {
                return Err(ShellTransportError::WrongContentRecord);
            }
            if size > bytes {
                break;
            }
            let (transaction, record) = decode_native_content_record(frame)?;
            if record.grant() != self.store_grant {
                return Err(ShellTransportError::WrongContentGrant);
            }
            // Conservatively reserve one possible terminal before removing its
            // request. Tails/cancel consume no new response; resources use their
            // actual existing store credit rule.
            let credit = closed_native_content_credit(epochs, self.store_grant, &record)?;
            if !self.control_capacity_available(epochs, credit) {
                break;
            }
            self.inbox.remove(index);
            bytes -= size;
            self.apply_closed_native_content_record(
                epochs,
                transaction,
                record,
                expected,
                now_msec,
            )?;
            processed += 1;
            self.flush_content_candidate_events(epochs)?;
            self.flush_content_allocation_events(epochs)?;
        }
        if processed == 0 && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(processed)
    }

    /// The wire-agnostic effect of one decoded record after Closed: reject or
    /// tail it, exactly as the socket path applies inline.
    fn apply_closed_native_content_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: NativeContentRecord,
        expected: NativeLauncherOpening,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        match record {
            NativeContentRecord::Resource(v) => {
                self.apply_content_resource_record(epochs, transaction, v, now_msec)?
            }
            NativeContentRecord::Allocation(v) => epochs
                .allocations_mut(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?
                .reject_closed_native_request(transaction, v, expected)?,
            v => {
                let candidates = epochs
                    .active_candidates_mut(self.store_grant)
                    .ok_or(ShellTransportError::MissingCapability)?;
                match v {
                    NativeContentRecord::Begin(v) => {
                        candidates.reject_closed_native_begin(transaction, v, expected)?
                    }
                    NativeContentRecord::Chunk(v) => {
                        candidates.closed_native_tail(v.grant, v.candidate_generation)?
                    }
                    NativeContentRecord::End(v) => {
                        candidates.closed_native_tail(v.grant, v.candidate_generation)?
                    }
                    NativeContentRecord::Demand(v) => {
                        candidates.reject_closed_native_demand(transaction, v, expected)?
                    }
                    NativeContentRecord::Cancel(v) => {
                        candidates.closed_native_cancel(v, expected)?
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    }
}

/// Conservatively reserve one possible terminal before removing its request.
/// Tails/cancel consume no new response; resources use their actual existing
/// store credit rule.
fn closed_native_content_credit(
    epochs: &crate::ContentEpochRegistry,
    store_grant: ContentGrant,
    record: &NativeContentRecord,
) -> Result<usize, ShellTransportError> {
    Ok(match record {
        NativeContentRecord::Resource(v) => epochs
            .resources(store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .additional_response_credit(v),
        NativeContentRecord::Chunk(_)
        | NativeContentRecord::End(_)
        | NativeContentRecord::Cancel(_) => 0,
        _ => 1,
    })
}

impl ShellTransportConnection<'_> {
    pub fn service_closed_native_content(
        &mut self,
        expected: NativeLauncherOpening,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.state
            .service_closed_native_content(self.content_epochs, expected, now_msec)
    }
}
