//! Atomic revision-8 catalog decoding. No partial catalog becomes current.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellPersistentCatalog {
    pub catalog: ShellApplicationCatalog,
    pub identities: BTreeMap<u16, String>,
}

/// Decode one bounded Begin/Entry/Identity/End transaction. Identities are a
/// bijection with the entries, never a best-effort hint or display-label match.
pub fn decode_shell_persistent_catalog(
    frames: &[Vec<u8>],
) -> Result<(TransactionId, ShellPersistentCatalog), IpcCodecError> {
    let bad = || IpcCodecError::InvalidRecord("persistent catalog transaction");
    if !(2..=2 * SOPHIA_SHELL_MAX_APPLICATIONS + 2).contains(&frames.len()) {
        return Err(bad());
    }
    let (first, _) = decode_frame(&frames[0])?;
    if first.message_kind != IpcMessageKind::ShellApplicationsBegin
        || decode_frame(frames.last().ok_or_else(bad)?)?.0.message_kind
            != IpcMessageKind::ShellApplicationsEnd
    {
        return Err(bad());
    }
    let mut legacy = Vec::new();
    let mut identities = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut binding = None;
    let mut identity_phase = false;
    for frame in frames {
        let (header, _) = decode_frame(frame)?;
        if header.transaction != first.transaction {
            return Err(bad());
        }
        if header.message_kind == IpcMessageKind::ShellCatalogIdentity {
            identity_phase = true;
            let (_, ShellCatalogActionRecord::Identity(value)) =
                decode_shell_catalog_action_frame(frame)?
            else {
                return Err(bad());
            };
            let key = (value.connection_epoch, value.catalog_generation);
            if binding.is_some_and(|expected| expected != key)
                || !names.insert(value.identity.clone())
                || identities.insert(value.slot, value.identity).is_some()
            {
                return Err(bad());
            }
            binding = Some(key);
        } else {
            if identity_phase && header.message_kind != IpcMessageKind::ShellApplicationsEnd {
                return Err(bad());
            }
            legacy.push(frame.clone());
        }
    }
    let (transaction, catalog) = decode_shell_application_catalog(&legacy)?;
    if identities.len() != catalog.entries.len()
        || (!identities.is_empty()
            && binding != Some((catalog.connection_epoch, catalog.generation)))
        || catalog
            .entries
            .iter()
            .any(|entry| !identities.contains_key(&entry.slot))
    {
        return Err(bad());
    }
    Ok((
        transaction,
        ShellPersistentCatalog {
            catalog,
            identities,
        },
    ))
}
