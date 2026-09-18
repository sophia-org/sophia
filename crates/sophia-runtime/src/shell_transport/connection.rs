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

impl ShellTransportConnection<'_> {
    /// Observe existing admission; this never enables negotiation or changes a
    /// store profile. Persistent launch cannot inherit a transient menu grant.
    pub fn supports_persistent_catalog(&self) -> bool {
        self.state.supports_persistent_catalog(self.content_epochs)
    }
}

impl ShellComponentTransport {
    pub(super) fn supports_persistent_catalog(&self, epochs: &ContentEpochRegistry) -> bool {
        use sophia_protocol::*;
        let required = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
            | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        self.content_grant().is_some_and(|grant| {
            self.capabilities & required == required
                && epochs.profile(grant) == Some(crate::ContentStoreProfile::PersistentCatalog)
        })
    }
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
