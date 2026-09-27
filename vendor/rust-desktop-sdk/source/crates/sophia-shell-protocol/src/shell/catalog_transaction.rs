//! Wire-neutral typed model for the atomic revision-8 persistent catalog:
//! no partial catalog ever becomes current.
use crate::{InvalidRecord, ShellApplicationCatalog};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellPersistentCatalog {
    pub catalog: ShellApplicationCatalog,
    pub identities: BTreeMap<u16, String>,
}

/// The identity-bijection rule
/// `sophia_protocol::ipc::shell_catalog_transaction::decode_shell_persistent_catalog`
/// used to check inline after reassembling its Begin/Identity/Entry/End
/// transaction: no partial catalog, so every entry carries an identity or
/// none does (including the trivial case of no entries at all), and a
/// present identity is a well-formed `registered:`/`desktop:` name. The
/// transfer-only half of that check -- that every identity frame in one
/// transaction named the same connection epoch and catalog generation --
/// has nothing left to check once the catalog is one assembled value, so it
/// is not restated here.
///
/// This is the dock's r8 shape specifically: `identities` covers every
/// entry or none. A plain catalog with real entries and no identities at
/// all (the launcher's and legacy descriptor's view) is not this value's
/// concern; callers that also accept that shape check for empty identities
/// themselves before reaching here (`shell::encoding::applications`).
pub fn validate_shell_persistent_catalog(
    value: &ShellPersistentCatalog,
) -> Result<(), InvalidRecord> {
    validate(value)
}

pub(crate) fn validate(value: &ShellPersistentCatalog) -> Result<(), InvalidRecord> {
    crate::validate_shell_application_catalog(&value.catalog)?;
    let bad = InvalidRecord("persistent catalog identity bijection");
    if value.identities.len() != value.catalog.entries.len() {
        return Err(bad);
    }
    for entry in &value.catalog.entries {
        let identity = value.identities.get(&entry.slot).ok_or(bad)?;
        let valid = !identity.is_empty()
            && identity.len() <= crate::SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES
            && crate::shell_launcher_text_valid(
                identity,
                crate::SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES,
            )
            && ["registered:", "desktop:"].iter().any(|prefix| {
                identity
                    .strip_prefix(prefix)
                    .is_some_and(|tail| !tail.is_empty())
            });
        if !valid {
            return Err(bad);
        }
    }
    Ok(())
}
