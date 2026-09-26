use super::ShellComponentTransport;
use sophia_protocol::{ContentReason, ContentResourceStatus, ShellContentRecord, TransactionId};

use super::{ShellSessionTransport, ShellTransportError, content_admission};
use crate::ContentStoreError;

/// One encoded server record before custody moves into the wire's FIFO:
/// a socket frame, or a file event body whose header the journal supplies.
pub(super) struct PreparedRecord {
    pub bytes: Vec<u8>,
    pub control: bool,
    pub file_kind: Option<sophia_protocol::shell_files::ShellFileKind>,
}

// Header plus the fixed 48-byte ResourceStatus payload. Every resource request
// produces at most one immediate status/release record of no greater size.
const MAX_RESOURCE_RESPONSE_BYTES: usize = sophia_protocol::SOPHIA_IPC_HEADER_LEN + 48;

impl ShellComponentTransport {
    /// Service only immutable resource-transfer records. Candidate and
    /// allocation records remain queued for their separate lifecycle owners.
    pub fn service_content_resources(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        epochs.collect();
        if let Some(store) = epochs.resources_mut(self.store_grant) {
            store.expire(now_msec)?;
        }
        self.flush_content_resource_events(epochs)?;
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
            let Some((transaction, record)) = self.poll_content_resource_record(epochs)? else {
                break;
            };
            self.apply_content_resource_record(epochs, transaction, record, now_msec)?;
            processed += 1;
        }
        Ok(processed)
    }

    /// One already credit-admitted request; both role dispatchers use this
    /// actual resource transition and exact response owner.
    pub(super) fn apply_content_resource_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: ShellContentRecord,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        let grant = self.store_grant;
        let resource = content_admission::resource_identity(&record)
            .ok_or(ShellTransportError::WrongContentRecord)?;
        let (outcome, store_reported) = {
            let store = epochs
                .resources_mut(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?;
            let outcome = match &record {
                ShellContentRecord::ResourceBegin(value) => {
                    store.begin(transaction, value.clone(), now_msec)
                }
                ShellContentRecord::ResourceChunk(value) => {
                    store.chunk(transaction, value, now_msec)
                }
                ShellContentRecord::ResourceEnd(value) => store.end(transaction, value, now_msec),
                ShellContentRecord::ResourceCancel(value) => store.cancel(transaction, value),
                ShellContentRecord::ResourceRetire(value) => store.retire(transaction, value),
                _ => return Err(ShellTransportError::WrongContentRecord),
            };
            (outcome, store.pending_event().is_some())
        };
        self.flush_content_resource_events(epochs)?;
        if let Err(error) = outcome
            && !store_reported
        {
            if error == ContentStoreError::ClockRegression {
                return Err(error.into());
            }
            self.send_content_record(
                epochs,
                transaction,
                &ShellContentRecord::ResourceStatus(ContentResourceStatus {
                    grant,
                    resource,
                    status: 3,
                    reason: content_reason(error) as u16,
                    next_ordinal: 0,
                    admitted_bytes: 0,
                }),
            )?;
        }
        Ok(())
    }

    pub fn send_content_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<(), ShellTransportError> {
        if let ShellContentRecord::Action(action) = record {
            return self.send_content_action(epochs, transaction, action);
        }
        self.queue_content_record(epochs, transaction, record, false)
    }

    pub(super) fn queue_content_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: &ShellContentRecord,
        reserved: bool,
    ) -> Result<(), ShellTransportError> {
        let prepared = self.prepare_content_frame(epochs, transaction, record, reserved)?;
        self.push_prepared(prepared);
        Ok(())
    }

    /// Transfers one prepared record into the wire's FIFO. Custody moves here
    /// and nowhere else, so neither wire can recreate an owned response.
    pub(super) fn push_prepared(&mut self, prepared: PreparedRecord) {
        match prepared.file_kind {
            None => self.output.push(prepared.bytes, prepared.control),
            Some(kind) => self
                .output
                .push_file(kind, prepared.bytes, prepared.control),
        }
    }

    pub(super) fn prepare_content_frame(
        &self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: &ShellContentRecord,
        reserved: bool,
    ) -> Result<PreparedRecord, ShellTransportError> {
        let grant = self
            .content_grant
            .ok_or(ShellTransportError::MissingCapability)?;
        if !content_admission::server_record(record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(record) != Some(grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let (bytes, file_kind, size) = if self.files.is_some() {
            let (kind, body) = super::files::encode_content_event(transaction, record)?;
            let size = body.len() + sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES;
            (body, Some(kind), size)
        } else {
            let frame = sophia_protocol::encode_shell_content_frame(transaction, record)?;
            let size = frame.len();
            (frame, None, size)
        };
        let bulk = matches!(
            record,
            ShellContentRecord::Limits(_) | ShellContentRecord::OutputFacts(_)
        );
        if (!bulk && size > super::control_budget::CONTROL_FRAME_BYTES)
            || !self.frame_capacity_available(epochs, size, !bulk, reserved)
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        // No wire I/O after ownership transfer. A later partial/failed write
        // cannot make the producer recreate an already-owned response.
        Ok(PreparedRecord {
            bytes,
            control: !bulk,
            file_kind,
        })
    }

    fn poll_content_resource_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io(epochs)?;
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
        let needed = epochs
            .resources(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .additional_response_credit(&record);
        if !self.control_capacity_available(epochs, needed) {
            self.inbox
                .insert(at.expect("selected frame has an index"), frame);
            return Ok(None);
        }
        Ok(Some((transaction, record)))
    }

    pub(super) fn flush_content_resource_events(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        loop {
            let event = epochs
                .resources_mut(self.store_grant)
                .and_then(|store| store.pending_event().cloned());
            let Some(event) = event else {
                return Ok(());
            };
            let prepared =
                self.prepare_content_frame(epochs, event.transaction, &event.record, true)?;
            let Some(store) = epochs.resources_mut(self.store_grant) else {
                return Err(ShellTransportError::MissingCapability);
            };
            // Validate custody before the only allocating operation (push).
            // Afterwards pop_front is infallible and has no user drop code.
            if store.pending_event() != Some(&event) {
                return Err(ShellTransportError::WrongContentRecord);
            }
            self.push_prepared(prepared);
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

// Legacy single-shell facade, delegating to the same shared registry path.

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn service_content_resources(
                &mut self,
                now_msec: u64,
            ) -> Result<usize, ShellTransportError> {
                self.state
                    .service_content_resources(&mut self.content_epochs, now_msec)
            }

            pub fn send_content_record(
                &mut self,
                transaction: TransactionId,
                record: &ShellContentRecord,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_content_record(&mut self.content_epochs, transaction, record)
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
