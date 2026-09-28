use super::ShellComponentTransport;
use sophia_protocol::*;

use super::wire::ContentWant;
use super::{ShellSessionTransport, ShellTransportError};
use crate::{ContentAllocationError, ContentAllocationSnapshot};

impl ShellComponentTransport {
    pub fn publish_content_output_facts(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        facts_generation: u64,
        outputs: Vec<ContentOutputFactsEntry>,
    ) -> Result<(), ShellTransportError> {
        let bytes = super::outbound::output_facts_charge(outputs.len());
        if !self.bulk_capacity_available(epochs, bytes) {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .publish_outputs(transaction, facts_generation, outputs)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn service_content_allocation_requests(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        presented_parents: &[(ContentAllocationId, u64)],
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        let limits = self
            .content_limits
            .clone()
            .ok_or(ShellTransportError::MissingCapability)?;
        let mut processed = 0;
        while processed < limits.max_frames_per_service_tick as usize {
            if self.require_allocation_output_capacity(epochs, 1).is_err() {
                break;
            }
            let Some((transaction, record)) = self.poll_content_allocation_record(epochs)? else {
                break;
            };
            let ShellContentRecord::AllocationRequest(request) = record else {
                return Err(ShellTransportError::WrongContentRecord);
            };
            let outcome = epochs
                .allocations_mut(self.store_grant)
                .ok_or(ShellTransportError::MissingCapability)?
                .request(transaction, request.clone(), presented_parents, now_msec);
            processed += 1;
            if let Err(error) = outcome {
                if error == ContentAllocationError::ClockRegression {
                    return Err(error.into());
                }
                self.send_content_record(
                    epochs,
                    transaction,
                    &ShellContentRecord::AllocationResult(ContentAllocationResult {
                        grant: limits.grant,
                        allocation_request_id: request.allocation_request_id,
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
        self.flush_content_allocation_events(epochs)?;
        Ok(processed)
    }

    pub fn next_content_allocation_request(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Option<(TransactionId, ContentAllocationRequest)> {
        epochs
            .allocations(self.store_grant)
            .and_then(|allocations| allocations.pending_request())
    }

    pub fn grant_content_allocation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        request_id: u64,
        snapshot: ContentAllocationSnapshot,
        presented_parents: &[(ContentAllocationId, u64)],
    ) -> Result<(), ShellTransportError> {
        self.require_allocation_output_capacity(epochs, 0)?;
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .grant(request_id, snapshot, presented_parents)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn reject_content_allocation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        request_id: u64,
        error: ContentAllocationError,
    ) -> Result<(), ShellTransportError> {
        self.require_allocation_output_capacity(epochs, 0)?;
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .reject(request_id, error)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn release_content_allocation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        request_id: u64,
    ) -> Result<(), ShellTransportError> {
        self.require_allocation_output_capacity(epochs, 0)?;
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .release(request_id)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn invalidate_content_allocation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        allocation: ContentAllocationId,
        reason: ContentReason,
    ) -> Result<(), ShellTransportError> {
        self.require_allocation_output_capacity(epochs, 1)?;
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .invalidate(transaction, allocation, reason)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn expire_content_allocations(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        self.require_allocation_output_capacity(epochs, 0)?;
        epochs
            .allocations_mut(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .expire(now_msec)?;
        self.flush_content_allocation_events(epochs)
    }

    pub fn content_allocation_snapshots(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Vec<ContentAllocationSnapshot> {
        epochs
            .allocations(self.store_grant)
            .map(|allocations| allocations.snapshots())
            .unwrap_or_default()
    }

    /// The allocation response's control credit, and `additional` new ones,
    /// fit the connection's record and byte budget. The credit charges at
    /// least the response's whole record, so this also bounds its bytes.
    fn require_allocation_output_capacity(
        &self,
        epochs: &crate::ContentEpochRegistry,
        additional: usize,
    ) -> Result<(), ShellTransportError> {
        if self.content_limits.is_none() {
            return Err(ShellTransportError::MissingCapability);
        }
        if !self.control_capacity_available(epochs, additional) {
            Err(ShellTransportError::ContentQueueSaturated)
        } else {
            Ok(())
        }
    }

    fn poll_content_allocation_record(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellTransportError> {
        self.poll_io(epochs)?;
        let taken = self.take_content(ContentWant::AllocationRequest)?;
        self.admit_client_record(taken)
    }

    pub(super) fn flush_content_allocation_events(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        loop {
            let event = epochs
                .allocations(self.store_grant)
                .and_then(|allocations| allocations.pending_event().cloned());
            let Some(event) = event else {
                return Ok(());
            };
            let admitted =
                self.prepare_content_record(epochs, event.transaction, &event.record, true)?;
            let Some(store) = epochs.allocations_mut(self.store_grant) else {
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
            pub fn publish_content_output_facts(
                &mut self,
                transaction: TransactionId,
                facts_generation: u64,
                outputs: Vec<ContentOutputFactsEntry>,
            ) -> Result<(), ShellTransportError> {
                self.state.publish_content_output_facts(
                    &mut self.content_epochs,
                    transaction,
                    facts_generation,
                    outputs,
                )
            }

            pub fn service_content_allocation_requests(
                &mut self,
                presented_parents: &[(ContentAllocationId, u64)],
                now_msec: u64,
            ) -> Result<usize, ShellTransportError> {
                self.state.service_content_allocation_requests(
                    &mut self.content_epochs,
                    presented_parents,
                    now_msec,
                )
            }

            pub fn next_content_allocation_request(
                &self,
            ) -> Option<(TransactionId, ContentAllocationRequest)> {
                self.state
                    .next_content_allocation_request(&self.content_epochs)
            }

            pub fn grant_content_allocation(
                &mut self,
                request_id: u64,
                snapshot: ContentAllocationSnapshot,
                presented_parents: &[(ContentAllocationId, u64)],
            ) -> Result<(), ShellTransportError> {
                self.state.grant_content_allocation(
                    &mut self.content_epochs,
                    request_id,
                    snapshot,
                    presented_parents,
                )
            }

            pub fn reject_content_allocation(
                &mut self,
                request_id: u64,
                error: ContentAllocationError,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .reject_content_allocation(&mut self.content_epochs, request_id, error)
            }

            pub fn release_content_allocation(
                &mut self,
                request_id: u64,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .release_content_allocation(&mut self.content_epochs, request_id)
            }

            pub fn invalidate_content_allocation(
                &mut self,
                transaction: TransactionId,
                allocation: ContentAllocationId,
                reason: ContentReason,
            ) -> Result<(), ShellTransportError> {
                self.state.invalidate_content_allocation(
                    &mut self.content_epochs,
                    transaction,
                    allocation,
                    reason,
                )
            }

            pub fn expire_content_allocations(
                &mut self,
                now_msec: u64,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .expire_content_allocations(&mut self.content_epochs, now_msec)
            }

            pub fn content_allocation_snapshots(&self) -> Vec<ContentAllocationSnapshot> {
                self.state
                    .content_allocation_snapshots(&self.content_epochs)
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
