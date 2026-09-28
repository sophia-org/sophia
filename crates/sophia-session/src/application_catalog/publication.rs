use super::*;
use sophia_protocol::{
    IpcCodecError, ShellApplicationCatalog, ShellPersistentCatalog,
    validate_shell_application_catalog,
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
        // The catalog's own bounds, independent of any wire's framing.
        validate_shell_application_catalog(&wire)?;
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
    /// dock's persistent view). Duplicate identities are refused.
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
