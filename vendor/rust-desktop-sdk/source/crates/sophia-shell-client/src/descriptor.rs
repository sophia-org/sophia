//! Descriptor observations use the connection's existing bounded inbox and
//! custody lane. Receiving a record never commits a work area or action.
use crate::wire::{Inbound, Outbound};
use crate::{Admission, ShellClientError, ShellConnection};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{ShellApplicationCatalog, TransactionId};

#[derive(Debug)]
pub enum DescriptorObservation {
    Record(ShellFileDescriptorRecord),
    /// The revision-4 catalog, without persistent application identities.
    Catalog(TransactionId, ShellApplicationCatalog),
}

impl ShellConnection {
    /// Enqueues one complete descriptor candidate or activation acknowledgement.
    /// Its ticket reports custody, not presentation or action completion. A
    /// saturation/refusal admits nothing; this call performs no socket I/O.
    pub fn enqueue_descriptor_tracked(
        &mut self,
        value: &ShellFileDescriptorRecord,
    ) -> Result<Admission, ShellClientError> {
        if shell_file_class(shell_file_descriptor_kind(&value.record)) != ShellFileClass::Candidate
        {
            return Err(ShellClientError::WrongDirection);
        }
        let outbound = Outbound::Descriptor(value.clone());
        let control = outbound.is_control();
        self.admit(outbound, control)
    }

    /// Takes a complete descriptor-family record in wire order. Content and
    /// indicator observations remain available through their existing APIs.
    pub fn take_descriptor_observation(
        &mut self,
    ) -> Result<Option<DescriptorObservation>, ShellClientError> {
        let at = self
            .inbox
            .iter()
            .position(|v| matches!(v, Inbound::Descriptor(_) | Inbound::ApplicationCatalog(..)));
        let Some(item) = at.and_then(|at| self.inbox.remove(at)) else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        Ok(Some(match item {
            Inbound::Descriptor(value) => DescriptorObservation::Record(value),
            Inbound::ApplicationCatalog(tx, value) => DescriptorObservation::Catalog(tx, value),
            _ => unreachable!("only descriptor observations selected"),
        }))
    }

    pub fn poll_descriptor_observation(
        &mut self,
    ) -> Result<Option<DescriptorObservation>, ShellClientError> {
        self.poll_io()?;
        self.take_descriptor_observation()
    }
}
