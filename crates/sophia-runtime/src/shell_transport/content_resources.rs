use super::ShellComponentTransport;
use sophia_protocol::{ContentReason, ContentResourceStatus, ShellContentRecord, TransactionId};

use super::control_budget::{CONTROL_RECORD_BYTES, Class};
use super::outbound::{Admitted, OutboundRecord};
use super::wire::ContentWant;
use super::{ShellSessionTransport, ShellTransportError, content_admission};
use crate::ContentStoreError;

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
        // Each request's possible response is bounded by its control credit,
        // checked before the request leaves the wire's queue.
        while processed < limits.max_frames_per_service_tick as usize {
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
        let admitted = self.prepare_content_record(epochs, transaction, record, reserved)?;
        self.transfer_record(admitted);
        Ok(())
    }

    /// Admits one server content record against its grant, its wire and the
    /// FIFO. `reserved` names a record whose producer credit (or registry
    /// charge) moves with it. Nothing changes on refusal.
    pub(super) fn prepare_content_record(
        &self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: &ShellContentRecord,
        reserved: bool,
    ) -> Result<Admitted, ShellTransportError> {
        let grant = self
            .content_grant
            .ok_or(ShellTransportError::MissingCapability)?;
        if !content_admission::server_record(record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(record) != Some(grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let bulk = matches!(
            record,
            ShellContentRecord::Limits(_) | ShellContentRecord::OutputFacts(_)
        );
        let class = if bulk {
            Class::Bulk
        } else {
            Class::Control {
                limit: CONTROL_RECORD_BYTES,
                oversize: ShellTransportError::ContentQueueSaturated,
            }
        };
        let admitted =
            self.admit_record(OutboundRecord::Content(transaction, record.clone()), class)?;
        if !self.record_capacity_available(epochs, admitted.charge, !bulk, reserved) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        // No wire I/O after ownership transfer. A later partial/failed write
        // cannot make the producer recreate an already-owned response.
        Ok(admitted)
    }

    /// The oldest resource request whose response credit is available. The
    /// record stays queued, on either wire, until that credit is.
    fn poll_content_resource_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io(epochs)?;
        let Some((_, record)) = self.peek_content(ContentWant::Resource)? else {
            return self.nothing_inbound();
        };
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
            return Ok(None);
        }
        self.take_content(ContentWant::Resource)
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
            let admitted =
                self.prepare_content_record(epochs, event.transaction, &event.record, true)?;
            let Some(store) = epochs.resources_mut(self.store_grant) else {
                return Err(ShellTransportError::MissingCapability);
            };
            // Validate custody before the only allocating operation (push).
            // Afterwards pop_front is infallible and has no user drop code.
            if store.pending_event() != Some(&event) {
                return Err(ShellTransportError::WrongContentRecord);
            }
            self.transfer_record(admitted);
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
