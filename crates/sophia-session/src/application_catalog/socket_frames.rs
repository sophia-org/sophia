//! The legacy socket encodings of a published catalog, kept for encoder
//! verification against `sophia_protocol`'s frame codec. Component roles hand
//! the typed value to the transport, which frames it only on the socket; this
//! file goes with the socket wire.
use super::PublishedApplicationCatalog;
use sophia_protocol::{IpcCodecError, TransactionId, encode_shell_application_catalog};

impl PublishedApplicationCatalog {
    /// The raw socket encoding of `value(false)`. Component-role transfer now
    /// goes through `publish_catalog`/`value`; this stays for direct encoder
    /// verification against `sophia_protocol::encode_shell_application_catalog`.
    pub fn frames(&self, transaction: TransactionId) -> Result<Vec<Vec<u8>>, IpcCodecError> {
        encode_shell_application_catalog(transaction, self.wire())
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
        let persistent = self.value(true)?;
        let mut frames = self.frames(transaction)?;
        let end = frames
            .pop()
            .ok_or(IpcCodecError::InvalidRecord("catalog End"))?;
        for entry in &persistent.catalog.entries {
            frames.push(encode_shell_catalog_action_frame(
                transaction,
                &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                    connection_epoch: persistent.catalog.connection_epoch,
                    catalog_generation: persistent.catalog.generation,
                    slot: entry.slot,
                    identity: persistent.identities[&entry.slot].clone(),
                }),
            )?);
        }
        frames.push(end);
        Ok(frames)
    }
}
