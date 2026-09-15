use sophia_protocol::{ContentReason, ContentResourceStatus, ShellContentRecord, TransactionId};

use super::{ShellSessionTransport, ShellTransportError, content_admission};
use crate::ContentStoreError;

// Header plus the fixed 48-byte ResourceStatus payload. Every resource request
// produces at most one immediate status/release record of no greater size.
const MAX_RESOURCE_RESPONSE_BYTES: usize = sophia_protocol::SOPHIA_IPC_HEADER_LEN + 48;

impl ShellSessionTransport {
    /// Service only immutable resource-transfer records. Candidate and
    /// allocation records remain queued for their separate lifecycle owners.
    pub fn service_content_resources(
        &mut self,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        self.content_epochs.collect();
        if let Some(store) = self.content_epochs.active_mut() {
            store.expire(now_msec)?;
        }
        self.flush_content_resource_events()?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if self
                .output
                .len()
                .saturating_add(MAX_RESOURCE_RESPONSE_BYTES)
                > limits.max_output_queue_bytes as usize
            {
                break;
            }
            let Some((transaction, record)) = self.poll_content_resource_record()? else {
                break;
            };
            let resource = content_admission::resource_identity(&record)
                .ok_or(ShellTransportError::WrongContentRecord)?;
            let (outcome, store_reported) = {
                let store = self
                    .content_epochs
                    .active_mut()
                    .ok_or(ShellTransportError::MissingCapability)?;
                let outcome = match &record {
                    ShellContentRecord::ResourceBegin(value) => {
                        store.begin(transaction, value.clone(), now_msec)
                    }
                    ShellContentRecord::ResourceChunk(value) => {
                        store.chunk(transaction, value, now_msec)
                    }
                    ShellContentRecord::ResourceEnd(value) => {
                        store.end(transaction, value, now_msec)
                    }
                    ShellContentRecord::ResourceCancel(value) => store.cancel(transaction, value),
                    ShellContentRecord::ResourceRetire(value) => store.retire(transaction, value),
                    _ => return Err(ShellTransportError::WrongContentRecord),
                };
                (outcome, store.pending_event().is_some())
            };
            processed += 1;
            self.flush_content_resource_events()?;
            if let Err(error) = outcome
                && !store_reported
            {
                if error == ContentStoreError::ClockRegression {
                    return Err(error.into());
                }
                self.send_content_record(
                    transaction,
                    &ShellContentRecord::ResourceStatus(ContentResourceStatus {
                        grant: limits.grant,
                        resource,
                        status: 3,
                        reason: content_reason(error) as u16,
                        next_ordinal: 0,
                        admitted_bytes: 0,
                    }),
                )?;
            }
        }
        Ok(processed)
    }

    pub fn send_content_record(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<(), ShellTransportError> {
        if let ShellContentRecord::Action(action) = record {
            return self.send_content_action(transaction, action);
        }
        self.queue_content_record(transaction, record, false)
    }

    pub(super) fn queue_content_record(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
        reserved: bool,
    ) -> Result<(), ShellTransportError> {
        let (frame, control) = self.prepare_content_frame(transaction, record, reserved)?;
        self.output.push(frame, control);
        Ok(())
    }

    pub(super) fn prepare_content_frame(
        &self,
        transaction: TransactionId,
        record: &ShellContentRecord,
        reserved: bool,
    ) -> Result<(Vec<u8>, bool), ShellTransportError> {
        let grant = self
            .content_grant
            .ok_or(ShellTransportError::MissingCapability)?;
        if !content_admission::server_record(record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(record) != Some(grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let frame = sophia_protocol::encode_shell_content_frame(transaction, record)?;
        let bulk = matches!(
            record,
            ShellContentRecord::Limits(_) | ShellContentRecord::OutputFacts(_)
        );
        if (!bulk && frame.len() > super::control_budget::CONTROL_FRAME_BYTES)
            || !self.frame_capacity_available(frame.len(), !bulk, reserved)
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        // No socket I/O after ownership transfer. A later partial/failed write
        // cannot make the producer recreate an already-owned response.
        Ok((frame, !bulk))
    }

    fn poll_content_resource_record(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io()?;
        let at = self.inbox.iter().position(|frame| {
            matches!(
                u16::from_le_bytes([frame[6], frame[7]]),
                165 | 167 | 168 | 169 | 170
            )
        });
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
        let needed = self
            .content_epochs
            .active()
            .ok_or(ShellTransportError::MissingCapability)?
            .additional_response_credit(&record);
        if !self.control_capacity_available(needed) {
            self.inbox
                .insert(at.expect("selected frame has an index"), frame);
            return Ok(None);
        }
        Ok(Some((transaction, record)))
    }

    fn flush_content_resource_events(&mut self) -> Result<(), ShellTransportError> {
        loop {
            let event = self
                .content_epochs
                .active_mut()
                .and_then(|store| store.pending_event().cloned());
            let Some(event) = event else {
                return Ok(());
            };
            let (frame, control) =
                self.prepare_content_frame(event.transaction, &event.record, true)?;
            let Some(store) = self.content_epochs.active_mut() else {
                return Err(ShellTransportError::MissingCapability);
            };
            // Validate custody before the only allocating operation (push).
            // Afterwards pop_front is infallible and has no user drop code.
            if store.pending_event() != Some(&event) {
                return Err(ShellTransportError::WrongContentRecord);
            }
            self.output.push(frame, control);
            store.take_event();
        }
    }
}

fn content_reason(error: ContentStoreError) -> ContentReason {
    match error {
        ContentStoreError::Stale => ContentReason::Stale,
        ContentStoreError::Budget => ContentReason::Budget,
        ContentStoreError::Malformed => ContentReason::Malformed,
        ContentStoreError::Incomplete => ContentReason::Incomplete,
        ContentStoreError::Revoked => ContentReason::Revoked,
        ContentStoreError::ClockRegression => ContentReason::Malformed,
    }
}
