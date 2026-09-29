//! Typed publication for the persistent catalog and view-indicator objects.
//! The file wire publishes the whole value as one snapshot object under the
//! export's own object rules.
use super::{ShellComponentTransport, ShellSessionTransport, ShellTransportError};
use sophia_protocol::{ShellIndicatorSnapshot, ShellPersistentCatalog, TransactionId};

impl ShellComponentTransport {
    /// Publishes the whole indicator snapshot as one `Indicators` object.
    pub fn publish_indicators(
        &mut self,
        _epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellIndicatorSnapshot,
    ) -> Result<(), ShellTransportError> {
        match self.wire.as_ref() {
            None => Err(ShellTransportError::NotConnected),
            Some(_) => {
                let body = sophia_protocol::shell_files::encode_shell_file_indicators_body(
                    &sophia_protocol::shell_files::ShellFileIndicators {
                        transaction,
                        snapshot: snapshot.clone(),
                    },
                )
                .map_err(|_| ShellTransportError::WrongContentRecord)?;
                self.publish_object(
                    sophia_protocol::shell_files::ShellFileKind::Indicators,
                    &body,
                )
            }
        }
    }

    /// Publishes the whole application catalog, plain or with r8 identities:
    /// the `Catalog` object includes the identities when supplied.
    pub fn publish_catalog(
        &mut self,
        _epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        catalog: &ShellPersistentCatalog,
    ) -> Result<(), ShellTransportError> {
        match self.wire.as_ref() {
            None => Err(ShellTransportError::NotConnected),
            Some(_) => {
                let body = sophia_protocol::shell_files::encode_shell_file_catalog_body(
                    &sophia_protocol::shell_files::ShellFileCatalog {
                        transaction,
                        catalog: catalog.clone(),
                    },
                )
                .map_err(|_| ShellTransportError::WrongContentRecord)?;
                self.publish_object(sophia_protocol::shell_files::ShellFileKind::Catalog, &body)
            }
        }
    }

    /// The file wire publishes a snapshot object into the export under the
    /// object's own cap and retention rules; it is not a streamed record, so
    /// the output queue's byte budget does not apply. Records queued before it
    /// are journaled first, so the announcement never overtakes them. If they
    /// cannot all be journaled now, or the journal cannot take the
    /// announcement, nothing is published and the caller retries.
    pub(super) fn publish_object(
        &mut self,
        kind: sophia_protocol::shell_files::ShellFileKind,
        body: &[u8],
    ) -> Result<(), ShellTransportError> {
        let Some(files) = self.wire.as_mut() else {
            return Err(ShellTransportError::NotConnected);
        };
        if !Self::drain_file_output(files, &mut self.output)? {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        if files.publish(kind, body, false)? {
            Ok(())
        } else {
            Err(ShellTransportError::ActivationQueueSaturated)
        }
    }
}

// Legacy single-shell facade, delegating to the same shared registry path.
// `content_epochs` is an owned registry on the legacy facade but a borrowed
// `&mut ContentEpochRegistry` on the per-connection view, so each takes it
// differently; a shared macro body cannot express both.
impl ShellSessionTransport {
    pub fn publish_indicators(
        &mut self,
        transaction: TransactionId,
        snapshot: &ShellIndicatorSnapshot,
    ) -> Result<(), ShellTransportError> {
        self.state
            .publish_indicators(&self.content_epochs, transaction, snapshot)
    }

    pub fn publish_catalog(
        &mut self,
        transaction: TransactionId,
        catalog: &ShellPersistentCatalog,
    ) -> Result<(), ShellTransportError> {
        self.state
            .publish_catalog(&self.content_epochs, transaction, catalog)
    }
}

impl crate::shell_transport::ShellTransportConnection<'_> {
    pub fn publish_indicators(
        &mut self,
        transaction: TransactionId,
        snapshot: &ShellIndicatorSnapshot,
    ) -> Result<(), ShellTransportError> {
        self.state
            .publish_indicators(self.content_epochs, transaction, snapshot)
    }

    pub fn publish_catalog(
        &mut self,
        transaction: TransactionId,
        catalog: &ShellPersistentCatalog,
    ) -> Result<(), ShellTransportError> {
        self.state
            .publish_catalog(self.content_epochs, transaction, catalog)
    }
}
