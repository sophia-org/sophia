use super::*;
use sophia_protocol::{
    IpcCodecError, ShellApplicationCatalog, TransactionId, encode_shell_application_catalog,
};
use std::sync::Arc;

/// The catalog bytes and executable provenance originate from one immutable
/// Session snapshot. A client can name a slot, never replace its command/source.
#[derive(Clone)]
pub struct PublishedApplicationCatalog {
    wire: ShellApplicationCatalog,
    source: Arc<ApplicationCatalog>,
}

impl PublishedApplicationCatalog {
    pub fn new(
        connection_epoch: u64,
        generation: u64,
        source: ApplicationCatalog,
    ) -> Result<Self, IpcCodecError> {
        let wire = ShellApplicationCatalog {
            connection_epoch,
            generation,
            entries: source
                .entries
                .iter()
                .map(|e| e.descriptor.clone())
                .collect(),
        };
        // Validate the same bounded catalog contract the publication uses.
        encode_shell_application_catalog(TransactionId::from_raw(1), &wire)?;
        Ok(Self {
            wire,
            source: Arc::new(source),
        })
    }
    pub fn wire(&self) -> &ShellApplicationCatalog {
        &self.wire
    }
    pub fn frames(&self, transaction: TransactionId) -> Result<Vec<Vec<u8>>, IpcCodecError> {
        encode_shell_application_catalog(transaction, &self.wire)
    }
    /// Revision-8 identity records share the catalog transaction and precede End.
    /// Legacy callers retain the unchanged revision-4 catalog encoding.
    pub fn persistent_frames(
        &self,
        transaction: TransactionId,
    ) -> Result<Vec<Vec<u8>>, IpcCodecError> {
        use sophia_protocol::{
            ShellCatalogActionRecord, ShellCatalogIdentity, encode_shell_catalog_action_frame,
        };
        let mut frames = self.frames(transaction)?;
        let end = frames
            .pop()
            .ok_or(IpcCodecError::InvalidRecord("catalog End"))?;
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.source.entries {
            if !seen.insert(&entry.identity) {
                return Err(IpcCodecError::InvalidRecord("duplicate catalog identity"));
            }
            frames.push(encode_shell_catalog_action_frame(
                transaction,
                &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                    connection_epoch: self.wire.connection_epoch,
                    catalog_generation: self.wire.generation,
                    slot: entry.descriptor.slot,
                    identity: entry.identity.clone(),
                }),
            )?);
        }
        frames.push(end);
        Ok(frames)
    }
    pub fn entry(&self, slot: u16) -> Option<Arc<ApplicationCatalogEntry>> {
        let mut entries = self
            .source
            .entries
            .iter()
            .filter(|e| e.descriptor.slot == slot);
        let entry = entries.next()?;
        (entries.next().is_none() && entry.descriptor.available && entry.command.is_some())
            .then(|| Arc::new(entry.clone()))
    }
}
