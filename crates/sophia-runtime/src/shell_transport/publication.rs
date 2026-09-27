//! Typed publication for the persistent catalog and view-indicator objects.
//! The socket wire still emits exactly today's multi-frame transfers, built
//! from the same protocol encoders Session uses; the file wire publishes the
//! whole value as one snapshot object. Socket publications drain
//! within the bulk budget across I/O turns; file objects follow the export's
//! own object rules.
use super::{ShellComponentTransport, ShellSessionTransport, ShellTransportError};
use sophia_protocol::{
    ShellCatalogActionRecord, ShellCatalogIdentity, ShellIndicatorSnapshot, ShellPersistentCatalog,
    TransactionId, encode_shell_application_catalog, encode_shell_catalog_action_frame,
    encode_shell_indicator_snapshot,
};

impl ShellComponentTransport {
    /// Publishes the whole indicator snapshot: the socket wire as today's
    /// `ShellIndicatorsBegin`/status/indicator/.../End frames, the file wire
    /// as the `Indicators` object.
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
            return self.publish_object(
                sophia_protocol::shell_files::ShellFileKind::Indicators,
                &body,
            );
        }
        let frames = encode_shell_indicator_snapshot(transaction, snapshot)?;
        self.queue_publication(epochs, frames)
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
            return self
                .publish_object(sophia_protocol::shell_files::ShellFileKind::Catalog, &body);
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
        self.queue_publication(epochs, frames)
    }

    /// The file wire publishes a snapshot object into the export under the
    /// object's own cap and retention rules; it is not a streamed record, so
    /// the output queue's byte budget does not apply. Events queued before it
    /// are journaled first, so the announcement never overtakes them. If they
    /// cannot all be journaled now, or the journal cannot take the
    /// announcement, nothing is published and the caller retries.
    fn publish_object(
        &mut self,
        kind: sophia_protocol::shell_files::ShellFileKind,
        body: &[u8],
    ) -> Result<(), ShellTransportError> {
        let files = self
            .files
            .as_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        if !Self::drain_file_output(files, &mut self.output)? {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        if files.publish(kind, body, false)? {
            Ok(())
        } else {
            Err(ShellTransportError::ActivationQueueSaturated)
        }
    }

    /// Takes custody of one whole socket publication. Its frames enter the
    /// output queue as bulk capacity allows, now and on later I/O turns, so a
    /// catalog larger than the queue is still delivered whole and in order.
    /// A publication still draining refuses the next one until it is done.
    fn queue_publication(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        frames: Vec<Vec<u8>>,
    ) -> Result<(), ShellTransportError> {
        if !self.publication.is_empty() {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.publication.extend(frames);
        self.flush_publication(epochs);
        Ok(())
    }

    pub(super) fn flush_publication(&mut self, epochs: &crate::ContentEpochRegistry) {
        while let Some(frame) = self.publication.front() {
            if !self.bulk_capacity_available(epochs, frame.len()) {
                break;
            }
            let frame = self.publication.pop_front().expect("front checked");
            self.output.push(frame, false);
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
