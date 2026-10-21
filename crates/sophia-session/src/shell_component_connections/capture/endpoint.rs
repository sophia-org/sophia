//! One demand-driven native mailbox for all connection bindings. No admission.
use super::*;
use sophia_protocol::OutputId;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureInbox,
};
use std::{io, sync::Arc};

#[derive(Clone)]
pub(crate) struct Destination {
    pub output: OutputId,
    pub budget: Arc<LiveRendererSnapshotBudget>,
    pub epoch: Arc<LiveRendererSnapshotEpoch>,
}

struct Endpoint {
    inbox: LiveShellCaptureInbox,
    destination: Destination,
}

// Private seam supplies mailboxes, never completion receipts or authorization.
trait Backend {
    fn request(&mut self, output: OutputId) -> io::Result<()>;
    fn poll(&mut self) -> io::Result<Option<Endpoint>>;
    fn close(&mut self) -> io::Result<()>;
}

#[cfg(feature = "native-session")]
impl Backend for sophia_backend_live::LiveProductionNativeScanout {
    fn request(&mut self, output: OutputId) -> io::Result<()> {
        self.request_shell_capture(output)
    }
    fn poll(&mut self) -> io::Result<Option<Endpoint>> {
        Ok(self.poll_shell_capture()?.map(|endpoint| {
            let output = endpoint.output();
            let (inbox, budget, epoch) = endpoint.into_parts();
            Endpoint {
                inbox,
                destination: Destination {
                    output,
                    budget,
                    epoch,
                },
            }
        }))
    }
    fn close(&mut self) -> io::Result<()> {
        self.close_shell_capture()
    }
}

#[derive(Default)]
pub(crate) struct Link {
    desired: Option<OutputId>,
    requester: Option<ComponentConnectionKey>,
    requested: bool,
    destination: Option<Destination>,
    closing: bool,
}

impl Link {
    pub(super) fn needs_turn(&self) -> bool {
        self.closing
            || (self.desired.is_some() && self.destination.is_none())
            || self
                .destination
                .as_ref()
                .is_some_and(|d| !d.epoch.is_valid())
    }

    fn demand(
        &mut self,
        output: OutputId,
        owner: &Option<Owner>,
    ) -> Result<bool, ContentStoreError> {
        if self.closing || self.desired.is_some_and(|held| held != output) {
            return Err(ContentStoreError::Stale);
        }
        if let Some(destination) = &self.destination {
            return if destination.output == output
                && destination.epoch.is_valid()
                && owner.as_ref().is_some_and(|o| !o.is_closed())
            {
                Ok(true)
            } else {
                Err(ContentStoreError::Stale)
            };
        }
        if owner.as_ref().is_some_and(|o| !o.replaceable()) {
            return Err(ContentStoreError::Incomplete);
        }
        self.desired = Some(output);
        Ok(false)
    }

    fn withdraw(&mut self, owner: &mut Option<Owner>) {
        if self.desired.is_some() || self.destination.is_some() {
            self.closing = true;
        }
        if let Some(destination) = &self.destination {
            destination.epoch.invalidate();
        }
        if let Some(owner) = owner {
            owner.stop();
        }
    }

    fn observe_requester(&mut self, connections: &[Connection], owner: &mut Option<Owner>) {
        if self.destination.is_none()
            && self.requester.is_some_and(|key| {
                !connections.get(key.slot).is_some_and(|c| {
                    c.attempt == Some((key.grant, ComponentConnectionPhase::Connected))
                })
            })
        {
            self.withdraw(owner);
        }
    }

    fn service(
        &mut self,
        backend: &mut impl Backend,
        owner: &mut Option<Owner>,
        available: bool,
    ) -> io::Result<()> {
        if !available
            || self
                .destination
                .as_ref()
                .is_some_and(|d| !d.epoch.is_valid())
            || (self.destination.is_some() && owner.as_ref().is_some_and(Owner::is_closed))
        {
            self.withdraw(owner);
        }
        if self.closing {
            backend.close()?;
            self.desired = None;
            self.requester = None;
            self.requested = false;
            self.destination = None;
            self.closing = false;
            return Ok(());
        }
        if self.destination.is_some() {
            return Ok(());
        }
        let Some(output) = self.desired else {
            return Ok(());
        };
        if !self.requested {
            if let Err(error) = backend.request(output) {
                // A rejected request owns no backend endpoint to close. Retry
                // queue pressure only; permanent refusal requires new demand.
                if error.kind() != io::ErrorKind::WouldBlock {
                    self.desired = None;
                    self.requester = None;
                }
                return Err(error);
            }
            self.requested = true;
        }
        let endpoint = match backend.poll() {
            Ok(Some(endpoint)) => endpoint,
            Ok(None) => return Ok(()),
            Err(error) => {
                self.withdraw(owner);
                return Err(error);
            }
        };
        if endpoint.destination.output != output
            || !endpoint.destination.epoch.is_valid()
            || owner.as_ref().is_some_and(|o| !o.replaceable())
        {
            // The unpublished inbox has no submitted jobs. Close the native
            // executor, retrying a full command queue on the next owner turn.
            endpoint.destination.epoch.invalidate();
            self.withdraw(owner);
            return Err(io::ErrorKind::InvalidData.into());
        }
        *owner = Some(Owner::new(endpoint.inbox));
        self.destination = Some(endpoint.destination);
        self.requester = None;
        self.requested = false;
        Ok(())
    }
}

impl ShellComponentConnections {
    /// A live, reserved allocation is the only source of endpoint demand.
    /// Borrowing the ticket leaves failure/source settlement with its owner.
    /// Production admission is still disabled, so this has no ingress caller.
    #[allow(dead_code)]
    pub(crate) fn prepare_capture_endpoint(
        &mut self,
        key: ComponentConnectionKey,
        reservation: &sophia_runtime::ContentCaptureReservation,
        now: u64,
    ) -> Result<Option<Destination>, ContentStoreError> {
        if reservation.description().grant != key.grant
            || !self.connections.get(key.slot).is_some_and(|c| {
                c.attempt == Some((key.grant, ComponentConnectionPhase::Connected))
            })
        {
            return Err(ContentStoreError::Stale);
        }
        let output = self.epochs.capture_output(reservation, now)?;
        if self
            .capture_link
            .demand(OutputId::from_raw(output.id), &self.capture)?
        {
            Ok(self.capture_link.destination.clone())
        } else {
            self.capture_link.requester.get_or_insert(key);
            Ok(None)
        }
    }

    /// A request must use the exact renderer destination returned by prepare.
    /// Refusal returns both source and reservation; it never failure-settles
    /// a ticket while silently dropping its source.
    #[allow(dead_code)]
    pub(crate) fn submit_capture(
        &mut self,
        binding: Binding,
        request: sophia_renderer_live::LiveShellCaptureRequest,
        reservation: sophia_runtime::ContentCaptureReservation,
        deadline: std::time::Instant,
        now: u64,
    ) -> Result<sophia_renderer_live::LiveShellCaptureJob, crate::shell_capture_owner::Rejected>
    {
        if self.capture_pending() >= crate::shell_capture_owner::MAX_JOBS
            || self.capture_sources.contains(binding)
        {
            return Err(crate::shell_capture_owner::Rejected {
                reason: crate::shell_capture_owner::SubmitFailure::Capacity,
                request,
                reservation,
            });
        }
        let key = ComponentConnectionKey {
            slot: binding.slot,
            grant: binding.grant,
        };
        let valid = self
            .prepare_capture_endpoint(key, &reservation, now)
            .ok()
            .flatten()
            .is_some_and(|d| {
                Arc::ptr_eq(&d.budget, &request.budget) && Arc::ptr_eq(&d.epoch, &request.epoch)
            });
        if !valid {
            return Err(crate::shell_capture_owner::Rejected {
                reason: crate::shell_capture_owner::SubmitFailure::Binding,
                request,
                reservation,
            });
        }
        self.capture.as_mut().expect("prepared owner").submit(
            binding,
            request,
            reservation,
            deadline,
        )
    }

    #[cfg(feature = "native-session")]
    pub(crate) fn service_capture_endpoint(
        &mut self,
        native: &mut sophia_backend_live::LiveProductionNativeScanout,
        available: bool,
    ) -> io::Result<()> {
        self.capture_link
            .observe_requester(&self.connections, &mut self.capture);
        self.capture_link
            .service(native, &mut self.capture, available)
    }
}

#[cfg(test)]
#[path = "../../../tests/support/capture_endpoint.rs"]
mod tests;
