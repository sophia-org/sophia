use super::*;
use sophia_protocol::{
    IpcCodecError, ShellApplicationCatalog, ShellPersistentCatalog, TransactionId,
    encode_shell_application_catalog,
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
    /// The typed value `publish_catalog` hands to the transport: the plain
    /// catalog when `persistent` is false (the native launcher's r4 view),
    /// or the catalog with one r8 identity per entry when it is true (the
    /// dock's persistent view). Same duplicate-identity contract as
    /// `persistent_frames`, checked once here instead of per encoded frame.
    pub fn value(&self, persistent: bool) -> Result<ShellPersistentCatalog, IpcCodecError> {
        if !persistent {
            return Ok(ShellPersistentCatalog {
                catalog: self.wire.clone(),
                identities: Default::default(),
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeMap::new();
        for entry in &self.source.entries {
            if !seen.insert(&entry.identity) {
                return Err(IpcCodecError::InvalidRecord("duplicate catalog identity"));
            }
            identities.insert(entry.descriptor.slot, entry.identity.clone());
        }
        Ok(ShellPersistentCatalog {
            catalog: self.wire.clone(),
            identities,
        })
    }
    /// The raw socket encoding of `value(false)`. Component-role transfer now
    /// goes through `publish_catalog`/`value`; this stays for direct encoder
    /// verification against `sophia_protocol::encode_shell_application_catalog`.
    pub fn frames(&self, transaction: TransactionId) -> Result<Vec<Vec<u8>>, IpcCodecError> {
        encode_shell_application_catalog(transaction, &self.wire)
    }
    /// The raw socket encoding of `value(true)`: revision-8 identity records
    /// share the catalog transaction and precede End. Kept for direct encoder
    /// verification; component-role transfer now goes through `publish_catalog`.
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
