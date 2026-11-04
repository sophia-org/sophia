//! Owner admission reserves the complete control response before taking input.
use super::{ShellComponentTransport, ShellTransportError};
use crate::frame_credits::FrameStreamKey;
use crate::{ContentCandidateContext, ContentEpochRegistry, ContentFrameRequestAdmission};
use sophia_protocol::shell_files::{SHELL_FRAME_CAPABILITY, ShellFrameRecord};

impl ShellComponentTransport {
    /// Admit the oldest SFT controls against fresh owner-supplied output contexts.
    /// A complete frame stays ahead of later controls until its own owner takes it.
    /// Missing output context produces a reserved Stale refusal, never an invented grant.
    ///
    /// # Errors
    /// Invalid connection/custody, grant or clock fails without consuming that input.
    pub fn service_content_frame_controls(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        contexts: &[ContentCandidateContext<'_>],
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        if self.capabilities & SHELL_FRAME_CAPABILITY == 0 {
            return Err(ShellTransportError::MissingCapability);
        }
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        let maximum = limits.max_frames_per_service_tick as usize;
        let grant = self.store_grant;
        if self.content_grant != Some(grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        self.poll_io(epochs)?;
        epochs
            .active_candidates_mut(grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_frame_events(epochs, now_msec)?;
        let mut processed = 0;
        while processed < maximum {
            let request = self
                .files_mut()
                .ok_or(ShellTransportError::NotConnected)?
                .export()
                .peek_frame_request();
            let Some(request) = request else {
                break;
            };
            let (transaction, key, open) = match request {
                ShellFrameRecord::Open { transaction, key } => (*transaction, *key, true),
                ShellFrameRecord::Close { transaction, key } => (*transaction, *key, false),
                ShellFrameRecord::Commit(_) => break,
                _ => return Err(ShellTransportError::WrongContentRecord),
            };
            let expected = if open {
                ShellFrameRecord::Open { transaction, key }
            } else {
                ShellFrameRecord::Close { transaction, key }
            };
            let key = FrameStreamKey {
                grant: key.grant,
                output: key.output,
                allocation: key.allocation,
                facts_generation: key.facts_generation,
                stream_id: key.stream_id,
            };
            let room = (0..=2)
                .rev()
                .find(|room| self.control_capacity_available(epochs, *room))
                .unwrap_or(0);
            let files = self.files_mut().ok_or(ShellTransportError::NotConnected)?;
            let candidates = epochs
                .active_candidates_mut(grant)
                .ok_or(ShellTransportError::MissingCapability)?;
            let consume = || {
                assert_eq!(
                    files.export_mut().take_frame_request(),
                    Some(expected),
                    "take exactly the reserved SFT control"
                )
            };
            let decision = if open {
                let context = contexts
                    .iter()
                    .find(|context| context.output == key.output)
                    .copied();
                candidates.admit_frame_open(transaction, key, context, now_msec, room, consume)?
            } else {
                candidates.admit_frame_close(transaction, key, now_msec, room, consume)?
            };
            if decision == ContentFrameRequestAdmission::Deferred {
                break;
            }
            processed += 1;
            self.flush_content_frame_events(epochs, now_msec)?;
        }
        Ok(processed)
    }
}
