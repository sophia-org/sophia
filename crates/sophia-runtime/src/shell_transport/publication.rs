//! Typed publication for the persistent catalog and view-indicator objects.
//! The socket wire still emits exactly today's multi-frame transfers, built
//! from the same protocol encoders Session uses; the file wire publishes the
//! whole value as one snapshot object. Session is not switched to call these
//! yet: this only gives the transport a typed seam for a later phase.
use super::{ShellComponentTransport, ShellSessionTransport, ShellTransportError};
use sophia_protocol::{
    ShellCatalogActionRecord, ShellCatalogIdentity, ShellIndicatorSnapshot, ShellPersistentCatalog,
    TransactionId, encode_shell_application_catalog, encode_shell_catalog_action_frame,
    encode_shell_indicator_snapshot,
};

impl ShellComponentTransport {
    /// Publishes the whole indicator snapshot: the socket wire as today's
    /// `ShellIndicatorsBegin`/status/indicator/.../End frames, the file wire
    /// as the `Indicators` object. Each socket frame is throttled exactly as
    /// `send_async` throttles Session's own outbound frames today.
    pub fn publish_indicators(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellIndicatorSnapshot,
    ) -> Result<(), ShellTransportError> {
        if self.stream.is_none() && self.files.is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        if self.files.is_some() {
            let body = sophia_protocol::shell_files::encode_shell_file_indicators_body(
                &sophia_protocol::shell_files::ShellFileIndicators {
                    transaction,
                    snapshot: snapshot.clone(),
                },
            )
            .map_err(|_| ShellTransportError::WrongContentRecord)?;
            if !self.bulk_capacity_available(epochs, body.len()) {
                return Err(ShellTransportError::ActivationQueueSaturated);
            }
            self.output.push_file(
                sophia_protocol::shell_files::ShellFileKind::Indicators,
                body,
                false,
            );
            return Ok(());
        }
        for frame in encode_shell_indicator_snapshot(transaction, snapshot)? {
            if !self.bulk_capacity_available(epochs, frame.len()) {
                return Err(ShellTransportError::ActivationQueueSaturated);
            }
            self.output.push(frame, false);
        }
        Ok(())
    }

    /// Publishes the whole application catalog, plain or with r8 identities:
    /// the socket wire as today's `ShellApplicationsBegin`/Entry/.../End
    /// frames, with one `Identity` record per entry inserted before End when
    /// `catalog.identities` is not empty (exactly
    /// `PublishedApplicationCatalog::persistent_frames` today); the file wire
    /// as the `Catalog` object.
    pub fn publish_catalog(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        catalog: &ShellPersistentCatalog,
    ) -> Result<(), ShellTransportError> {
        if self.stream.is_none() && self.files.is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        if self.files.is_some() {
            let body = sophia_protocol::shell_files::encode_shell_file_catalog_body(
                &sophia_protocol::shell_files::ShellFileCatalog {
                    transaction,
                    catalog: catalog.clone(),
                },
            )
            .map_err(|_| ShellTransportError::WrongContentRecord)?;
            if !self.bulk_capacity_available(epochs, body.len()) {
                return Err(ShellTransportError::ActivationQueueSaturated);
            }
            self.output.push_file(
                sophia_protocol::shell_files::ShellFileKind::Catalog,
                body,
                false,
            );
            return Ok(());
        }
        let mut frames = encode_shell_application_catalog(transaction, &catalog.catalog)?;
        if !catalog.identities.is_empty() {
            let end = frames
                .pop()
                .ok_or(ShellTransportError::WrongContentRecord)?;
            for entry in &catalog.catalog.entries {
                let identity = catalog
                    .identities
                    .get(&entry.slot)
                    .ok_or(ShellTransportError::WrongContentRecord)?;
                frames.push(encode_shell_catalog_action_frame(
                    transaction,
                    &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                        connection_epoch: catalog.catalog.connection_epoch,
                        catalog_generation: catalog.catalog.generation,
                        slot: entry.slot,
                        identity: identity.clone(),
                    }),
                )?);
            }
            frames.push(end);
        }
        for frame in frames {
            if !self.bulk_capacity_available(epochs, frame.len()) {
                return Err(ShellTransportError::ActivationQueueSaturated);
            }
            self.output.push(frame, false);
        }
        Ok(())
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
