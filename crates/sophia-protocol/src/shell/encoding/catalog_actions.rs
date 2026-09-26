//! Value encoding for the revision-8 persistent catalog actions
//! (`crate::shell::catalog_actions`): the per-variant body encode and decode
//! of a [`ShellCatalogActionRecord`].
//!
//! The frame codec's message-kind enum, frame headers and transaction-id
//! rules stay with the frame codec, which maps its own message kinds onto
//! [`ShellCatalogActionValueKind`] and wraps [`ValueError`] into its own
//! error type at the boundary.
use super::Wire;
use crate::byte_cursor::Cursor;
use crate::shell::encoding::{ValueError, reserved};
use crate::*;

/// Wire-only bound check; the typed validators in `crate::shell` use their
/// own neutral error and are not involved in this decode-time length check.
fn require(ok: bool, field: &'static str) -> Result<(), ValueError> {
    if ok {
        Ok(())
    } else {
        Err(ValueError::InvalidRecord(field))
    }
}

impl Wire for ShellCatalogIdentity {
    fn put(&self, out: &mut Vec<u8>) {
        self.connection_epoch.put(out);
        self.catalog_generation.put(out);
        self.slot.put(out);
        0u16.put(out);
        (self.identity.len() as u16).put(out);
        0u16.put(out);
        out.extend_from_slice(self.identity.as_bytes());
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(c)?;
        let catalog_generation = u64::take(c)?;
        let slot = u16::take(c)?;
        reserved::<u16>(c)?;
        let length = usize::from(u16::take(c)?);
        reserved::<u16>(c)?;
        require(
            length <= crate::SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES,
            "catalog identity length",
        )?;
        let identity = std::str::from_utf8(c.slice(length)?)
            .map_err(|_| ValueError::InvalidRecord("catalog identity UTF-8"))?
            .to_owned();
        Ok(Self {
            connection_epoch,
            catalog_generation,
            slot,
            identity,
        })
    }
}
impl Wire for CatalogCandidateBegin {
    fn put(&self, out: &mut Vec<u8>) {
        self.content.put(out);
        self.catalog_generation.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(Self {
            content: ContentCandidateBegin::take(c)?,
            catalog_generation: u64::take(c)?,
        })
    }
}
impl Wire for CatalogActivation {
    fn put(&self, out: &mut Vec<u8>) {
        self.action.put(out);
        self.catalog_generation.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(Self {
            action: ContentAction::take(c)?,
            catalog_generation: u64::take(c)?,
        })
    }
}
impl Wire for CatalogActivationOutcome {
    fn put(&self, out: &mut Vec<u8>) {
        self.activation.put(out);
        self.status.put(out);
        self.reason.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(Self {
            activation: CatalogActivation::take(c)?,
            status: u16::take(c)?,
            reason: u16::take(c)?,
        })
    }
}

/// The neutral counterpart of the frame codec's `ShellCatalog*` message
/// kinds: names which [`ShellCatalogActionRecord`] variant a byte body
/// decodes into, without naming any frame message kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellCatalogActionValueKind {
    Identity,
    CandidateBegin,
    CandidateChunk,
    Activate,
    ActivationOutcome,
}

/// The value kind a given record encodes as. The frame codec uses this to
/// pick the message kind a frame carries it under.
pub fn shell_catalog_action_value_kind(
    record: &ShellCatalogActionRecord,
) -> ShellCatalogActionValueKind {
    use ShellCatalogActionValueKind as V;
    match record {
        ShellCatalogActionRecord::Identity(_) => V::Identity,
        ShellCatalogActionRecord::CandidateBegin(_) => V::CandidateBegin,
        ShellCatalogActionRecord::CandidateChunk(_) => V::CandidateChunk,
        ShellCatalogActionRecord::Activate(_) => V::Activate,
        ShellCatalogActionRecord::ActivationOutcome(_) => V::ActivationOutcome,
    }
}

/// Encodes one record's value body. Validates first, exactly as the IPC
/// frame codec did before this split.
pub fn encode_shell_catalog_action_value(
    record: &ShellCatalogActionRecord,
) -> Result<Vec<u8>, ValueError> {
    crate::shell::catalog_actions::validate(record)?;
    let mut bytes = Vec::new();
    match record {
        ShellCatalogActionRecord::Identity(v) => v.put(&mut bytes),
        ShellCatalogActionRecord::CandidateBegin(v) => v.put(&mut bytes),
        ShellCatalogActionRecord::CandidateChunk(v) => v.put(&mut bytes),
        ShellCatalogActionRecord::Activate(v) => v.put(&mut bytes),
        ShellCatalogActionRecord::ActivationOutcome(v) => v.put(&mut bytes),
    }
    Ok(bytes)
}

/// Decodes one record's value body for the given kind. Rejects trailing
/// bytes and then validates, exactly as the IPC frame codec did before this
/// split.
pub fn decode_shell_catalog_action_value(
    kind: ShellCatalogActionValueKind,
    payload: &[u8],
) -> Result<ShellCatalogActionRecord, ValueError> {
    use ShellCatalogActionValueKind as V;
    let mut c = Cursor::new(payload);
    let record = match kind {
        V::Identity => ShellCatalogActionRecord::Identity(ShellCatalogIdentity::take(&mut c)?),
        V::CandidateBegin => {
            ShellCatalogActionRecord::CandidateBegin(CatalogCandidateBegin::take(&mut c)?)
        }
        V::CandidateChunk => {
            ShellCatalogActionRecord::CandidateChunk(ContentCandidateChunk::take(&mut c)?)
        }
        V::Activate => ShellCatalogActionRecord::Activate(CatalogActivation::take(&mut c)?),
        V::ActivationOutcome => {
            ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome::take(&mut c)?)
        }
    };
    c.finish()?;
    crate::shell::catalog_actions::validate(&record)?;
    Ok(record)
}
