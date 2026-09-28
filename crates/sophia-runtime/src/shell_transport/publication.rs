//! Typed publication for the persistent catalog and view-indicator objects.
//! The file wire publishes the whole value as one snapshot object under the
//! export's own object rules. The socket wire expands it into its multi-frame
//! transfer, which drains within the bulk budget across I/O turns.
use super::wire::Wire;
use super::{ShellComponentTransport, ShellSessionTransport, ShellTransportError};
use sophia_protocol::{ShellIndicatorSnapshot, ShellPersistentCatalog, TransactionId};

impl ShellComponentTransport {
    /// Publishes the whole indicator snapshot: the `Indicators` object on the
    /// file wire, the `ShellIndicatorsBegin`/status/indicator/.../End frames on
    /// the socket.
    pub fn publish_indicators(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellIndicatorSnapshot,
    ) -> Result<(), ShellTransportError> {
        match self.wire.as_ref() {
            None => Err(ShellTransportError::NotConnected),
            Some(Wire::Files(_)) => {
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
            Some(Wire::Socket(_)) => {
                let frames = super::socket::indicator_frames(transaction, snapshot)?;
                self.queue_publication(epochs, frames)
            }
        }
    }

    /// Publishes the whole application catalog, plain or with r8 identities:
    /// the `Catalog` object on the file wire; on the socket, the
    /// `ShellApplicationsBegin`/Entry/.../End frames with one `Identity`
    /// record per entry before End when `catalog.identities` is not empty.
    pub fn publish_catalog(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        catalog: &ShellPersistentCatalog,
    ) -> Result<(), ShellTransportError> {
        match self.wire.as_ref() {
            None => Err(ShellTransportError::NotConnected),
            Some(Wire::Files(_)) => {
                let body = sophia_protocol::shell_files::encode_shell_file_catalog_body(
                    &sophia_protocol::shell_files::ShellFileCatalog {
                        transaction,
                        catalog: catalog.clone(),
                    },
                )
                .map_err(|_| ShellTransportError::WrongContentRecord)?;
                self.publish_object(sophia_protocol::shell_files::ShellFileKind::Catalog, &body)
            }
            Some(Wire::Socket(_)) => {
                let frames = super::socket::catalog_frames(transaction, catalog)?;
                self.queue_publication(epochs, frames)
            }
        }
    }

    /// The file wire publishes a snapshot object into the export under the
    /// object's own cap and retention rules; it is not a streamed record, so
    /// the output queue's byte budget does not apply. Records queued before it
    /// are journaled first, so the announcement never overtakes them. If they
    /// cannot all be journaled now, or the journal cannot take the
    /// announcement, nothing is published and the caller retries.
    fn publish_object(
        &mut self,
        kind: sophia_protocol::shell_files::ShellFileKind,
        body: &[u8],
    ) -> Result<(), ShellTransportError> {
        let Some(Wire::Files(files)) = self.wire.as_mut() else {
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

    /// Takes custody of one whole socket publication. Its frames enter the
    /// output order as bulk capacity allows, now and on later I/O turns, so a
    /// catalog larger than the queue is still delivered whole and in order.
    /// A publication still draining refuses the next one until it is done.
    fn queue_publication(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        frames: Vec<Vec<u8>>,
    ) -> Result<(), ShellTransportError> {
        let socket = self.socket_mut().ok_or(ShellTransportError::NotConnected)?;
        if socket.publication_pending() {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        socket.queue_publication(frames);
        self.flush_publication(epochs);
        Ok(())
    }

    pub(super) fn flush_publication(&mut self, epochs: &crate::ContentEpochRegistry) {
        while let Some(bytes) = self.socket().and_then(|socket| socket.publication_front()) {
            if !self.bulk_capacity_available(epochs, bytes) {
                break;
            }
            let Some(Wire::Socket(socket)) = self.wire.as_mut() else {
                unreachable!("a pending publication is the socket's");
            };
            socket.release_publication_front(&mut self.output);
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
