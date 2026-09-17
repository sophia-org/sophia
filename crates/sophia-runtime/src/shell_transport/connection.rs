//! A temporary borrow of the two actual owners, never another epoch registry.
use super::{ShellComponentTransport, ShellSessionTransport};
use crate::ContentEpochRegistry;

/// Connection service borrows the Session's common registry. Dropping this view
/// neither disconnects the peer nor releases resources. No mutable registry or
/// transport escapes independently through this façade.
pub struct ShellTransportConnection<'a> {
    pub(super) state: &'a mut ShellComponentTransport,
    pub(super) content_epochs: &'a mut ContentEpochRegistry,
}

impl ShellComponentTransport {
    pub fn connection<'a>(
        &'a mut self,
        epochs: &'a mut ContentEpochRegistry,
    ) -> ShellTransportConnection<'a> {
        ShellTransportConnection {
            state: self,
            content_epochs: epochs,
        }
    }
}

impl ShellSessionTransport {
    /// Compatibility callers may lend the existing owner to the same service
    /// functions used by independently owned component connections.
    pub fn connection(&mut self) -> ShellTransportConnection<'_> {
        self.state.connection(&mut self.content_epochs)
    }
}
