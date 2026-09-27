//! Value encoding for the revision-4 shell application catalog
//! (`crate::shell::applications`) and the revision-8 persistent catalog
//! (`crate::shell::catalog_transaction`), as the whole `Catalog` file object.
//!
//! `sophia_protocol::ipc::shell_launcher` carries
//! [`ShellApplicationCatalog`] only as a Begin/Entry/End transfer, and
//! `sophia_protocol::ipc::shell_catalog_transaction` only as that transfer
//! plus a separate identity phase; both stay exactly as they were (deleted
//! at t255, not reused). This module is the file wire's own whole-object
//! layout: one body, entries in one table, each entry carrying its own r8
//! identity when the catalog discloses one, per
//! docs/sophia-shell-files.md Amendment 1.
use super::{Wire, put_text_padded, reserved, take_text_padded};
use crate::byte_cursor::Cursor;
use crate::shell::encoding::ValueError;
use crate::*;
use std::collections::BTreeMap;

const LABEL_MAX: usize = crate::SOPHIA_SHELL_APPLICATION_LABEL_MAX_BYTES;
const KEYWORDS_MAX: usize = crate::SOPHIA_SHELL_APPLICATION_KEYWORDS_MAX_BYTES;

impl Wire for ShellApplicationDescriptor {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.slot.put(bytes);
        u16::from(self.available).put(bytes);
        put_text_padded(bytes, &self.label, LABEL_MAX);
        put_text_padded(bytes, &self.keywords, KEYWORDS_MAX);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let slot = u16::take(cursor)?;
        let available = u16::take(cursor)?;
        if available > 1 {
            return Err(ValueError::InvalidEnum {
                field: "shell application entry available",
                value: u32::from(available),
            });
        }
        let label = take_text_padded(cursor, LABEL_MAX)?;
        let keywords = take_text_padded(cursor, KEYWORDS_MAX)?;
        Ok(Self {
            slot,
            available: available == 1,
            label,
            keywords,
        })
    }
}

/// Validates the whole `Catalog` object's value: the plain catalog shape
/// (`identities` empty, matching what the launcher and legacy descriptor
/// profiles disclose) needs only the reused base-catalog validator, since
/// [`crate::shell::catalog_transaction::validate`] is the dock's r8 shape
/// specifically and rejects a nonempty catalog with no identities at all
/// (never a partial one). A nonempty `identities` reuses that r8 validator
/// unchanged.
fn validate_catalog_object(value: &ShellPersistentCatalog) -> Result<(), ValueError> {
    if value.identities.is_empty() {
        crate::validate_shell_application_catalog(&value.catalog)?;
    } else {
        crate::shell::catalog_transaction::validate(value)?;
    }
    Ok(())
}

/// Encodes the whole `Catalog` object body: the plain application catalog
/// when `value.identities` is empty, or the r8 persistent catalog -- each
/// entry carrying its own identity -- when it is not. Both shapes share one
/// layout; `identities_present` just names which one a reader is holding
/// without having to inspect every entry.
pub fn encode_shell_persistent_catalog(
    value: &ShellPersistentCatalog,
) -> Result<Vec<u8>, ValueError> {
    validate_catalog_object(value)?;
    let mut bytes = Vec::new();
    value.catalog.connection_epoch.put(&mut bytes);
    value.catalog.generation.put(&mut bytes);
    (value.catalog.entries.len() as u16).put(&mut bytes);
    u16::from(!value.identities.is_empty()).put(&mut bytes);
    0u32.put(&mut bytes);
    for entry in &value.catalog.entries {
        entry.put(&mut bytes);
        let identity = value
            .identities
            .get(&entry.slot)
            .map(String::as_str)
            .unwrap_or("");
        put_text_padded(
            &mut bytes,
            identity,
            SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES,
        );
    }
    Ok(bytes)
}

pub fn decode_shell_persistent_catalog(
    payload: &[u8],
) -> Result<ShellPersistentCatalog, ValueError> {
    let mut cursor = Cursor::new(payload);
    let connection_epoch = u64::take(&mut cursor)?;
    let generation = u64::take(&mut cursor)?;
    let entry_count = super::table_count(&mut cursor, SOPHIA_SHELL_MAX_APPLICATIONS)?;
    let identities_present = u16::take(&mut cursor)?;
    if identities_present > 1 {
        return Err(ValueError::InvalidEnum {
            field: "shell catalog identities present",
            value: u32::from(identities_present),
        });
    }
    reserved::<u32>(&mut cursor)?;
    let mut entries = Vec::with_capacity(entry_count);
    let mut identities = BTreeMap::new();
    for _ in 0..entry_count {
        let entry = ShellApplicationDescriptor::take(&mut cursor)?;
        let identity = take_text_padded(&mut cursor, SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES)?;
        if !identity.is_empty() {
            identities.insert(entry.slot, identity);
        }
        entries.push(entry);
    }
    cursor.finish()?;
    if !entries.is_empty() && (identities_present == 1) == identities.is_empty() {
        return Err(ValueError::InvalidRecord("shell catalog identities flag"));
    }
    let value = ShellPersistentCatalog {
        catalog: ShellApplicationCatalog {
            connection_epoch,
            generation,
            entries,
        },
        identities,
    };
    validate_catalog_object(&value)?;
    Ok(value)
}
