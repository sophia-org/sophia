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
    /// Share publication flushes within one already-started owner visit.
    /// Records keep their existing admission, ordering and queue charges.
    /// The final flush runs even if the callback returns an error value;
    /// unwinding instead disconnects, so no deferred batch can escape.
    pub fn with_publication_batch<R, E>(
        &mut self,
        service: impl FnOnce(&mut ShellTransportConnection<'_>) -> Result<R, E>,
    ) -> Result<R, E>
    where
        E: From<super::ShellTransportError>,
    {
        if !self.state.owner_serviced || self.state.publication_batch {
            return Err(super::ShellTransportError::InvalidServiceBatch.into());
        }
        if self.state.peer_closed || self.state.wire.is_none() {
            return Err(super::ShellTransportError::NotConnected.into());
        }
        self.state.publication_batch = true;
        let mut batch = PublicationBatch {
            connection: self,
            armed: true,
        };
        let value = service(batch.connection);
        batch.connection.state.publication_batch = false;
        batch.armed = false;
        let flushed = batch
            .connection
            .state
            .poll_io(batch.connection.content_epochs)
            .and({
                if batch.connection.state.peer_closed {
                    Err(super::ShellTransportError::NotConnected)
                } else {
                    Ok(())
                }
            });
        // A callback may have admitted an external effect before reporting
        // failure. Never replace that classification with a retryable flush error.
        let value = value?;
        flushed.map_err(E::from)?;
        Ok(value)
    }

    /// Service the component's persistent image owner using the same clock as
    /// ordinary content resources. The owner itself must outlive this borrow.
    pub fn service_isb_images(
        &mut self,
        owner: &mut super::ShellIsbImageOwner,
        now_msec: u64,
        now: std::time::Instant,
    ) -> Result<(), super::ShellTransportError> {
        owner.service(self.state, self.content_epochs, now_msec, now)
    }

    /// Observe existing admission; this never enables negotiation or changes a
    /// store profile. Persistent launch cannot inherit a transient menu grant.
    pub fn supports_persistent_catalog(&self) -> bool {
        self.state.supports_persistent_catalog(self.content_epochs)
    }
}

struct PublicationBatch<'a, 'b> {
    connection: &'a mut ShellTransportConnection<'b>,
    armed: bool,
}

impl Drop for PublicationBatch<'_, '_> {
    fn drop(&mut self) {
        if self.armed {
            self.connection.state.publication_batch = false;
            let _ = self
                .connection
                .state
                .disconnect(self.connection.content_epochs);
        }
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
