//! One bounded catalog publication hands its whole typed value to the
//! transport, which owns per-wire encoding, pacing and retry-safe custody
//! (`ShellComponentTransport::publish_catalog`). A saturated transport takes
//! nothing, so this keeps no partial state of its own: the next visit simply
//! retries the same publication.
use super::PublishedApplicationCatalog;
use sophia_protocol::{ContentGrant, TransactionId};
use sophia_runtime::{ShellTransportConnection, ShellTransportError};

pub struct NativeCatalogPublication {
    grant: ContentGrant,
    persistent: bool,
    transaction: TransactionId,
    catalog: PublishedApplicationCatalog,
    published: bool,
}
impl NativeCatalogPublication {
    pub fn new(
        transport: &ShellTransportConnection<'_>,
        transaction: TransactionId,
        catalog: PublishedApplicationCatalog,
    ) -> Result<Self, ShellTransportError> {
        if !transport.supports_native_launcher() && !transport.supports_persistent_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        let grant = transport
            .content_grant()
            .ok_or(ShellTransportError::WrongContentGrant)?;
        if catalog.wire().connection_epoch != grant.connection_epoch {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let persistent = transport.supports_persistent_catalog();
        // Validate the same bounded/identity contract the publication uses,
        // once, up front, rather than discovering it mid-transfer.
        catalog.value(persistent)?;
        Ok(Self {
            grant,
            persistent,
            transaction,
            catalog,
            published: false,
        })
    }
    pub const fn grant(&self) -> ContentGrant {
        self.grant
    }

    #[cfg(feature = "native-session")]
    pub(crate) const fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Available once the transport has taken custody of the whole
    /// publication, not on peer receipt. Queue an Opening on the same wire
    /// only after this becomes available.
    pub fn published(&self) -> Option<&PublishedApplicationCatalog> {
        self.published.then_some(&self.catalog)
    }

    /// Hands the whole typed catalog to the transport. A saturated transport
    /// takes nothing and leaves this unpublished for the next visit to retry;
    /// there is no partial front to track any more.
    pub fn service(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
    ) -> Result<bool, ShellTransportError> {
        if transport.content_grant() != Some(self.grant)
            || if self.persistent {
                !transport.supports_persistent_catalog()
            } else {
                !transport.supports_native_launcher()
            }
        {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if self.published {
            return Ok(true);
        }
        let value = self.catalog.value(self.persistent)?;
        match transport.publish_catalog(self.transaction, &value) {
            Ok(()) => {
                self.published = true;
                Ok(true)
            }
            Err(ShellTransportError::ActivationQueueSaturated) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
