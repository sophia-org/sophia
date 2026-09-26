//! Persistent catalog action wire codec. Encodes and decodes the typed
//! records defined in `crate::shell::catalog_actions`; validation lives with
//! those types.
use crate::ipc::cursor::Cursor;
use crate::ipc::shell_content::fields::{Wire, reserved};
use crate::{
    CatalogActivation, CatalogActivationOutcome, CatalogCandidateBegin, ContentAction,
    ContentCandidateBegin, ContentCandidateChunk, IpcCodecError, IpcMessageKind,
    ShellCatalogActionRecord, ShellCatalogIdentity, TransactionId, decode_frame, encode_frame,
};

/// Wire-only bound check; the typed validators in `crate::shell` use their
/// own neutral error and are not involved in this decode-time length check.
fn require(ok: bool, field: &'static str) -> Result<(), IpcCodecError> {
    if ok {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidRecord(field))
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
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
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
            .map_err(|_| IpcCodecError::InvalidRecord("catalog identity UTF-8"))?
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
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
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
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
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
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        Ok(Self {
            activation: CatalogActivation::take(c)?,
            status: u16::take(c)?,
            reason: u16::take(c)?,
        })
    }
}

pub fn encode_shell_catalog_action_frame(
    transaction: TransactionId,
    record: &ShellCatalogActionRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    require(transaction.is_valid(), "catalog action transaction")?;
    crate::shell::catalog_actions::validate(record)?;
    let mut payload = Vec::new();
    use IpcMessageKind as K;
    let kind = match record {
        ShellCatalogActionRecord::Identity(v) => {
            v.put(&mut payload);
            K::ShellCatalogIdentity
        }
        ShellCatalogActionRecord::CandidateBegin(v) => {
            v.put(&mut payload);
            K::ShellCatalogCandidateBegin
        }
        ShellCatalogActionRecord::CandidateChunk(v) => {
            v.put(&mut payload);
            K::ShellCatalogCandidateChunk
        }
        ShellCatalogActionRecord::Activate(v) => {
            v.put(&mut payload);
            K::ShellCatalogActivate
        }
        ShellCatalogActionRecord::ActivationOutcome(v) => {
            v.put(&mut payload);
            K::ShellCatalogActivationOutcome
        }
    };
    encode_frame(kind, transaction, &payload)
}
pub fn decode_shell_catalog_action_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellCatalogActionRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require(header.transaction.is_valid(), "catalog action transaction")?;
    let mut c = Cursor::new(payload);
    use IpcMessageKind as K;
    let record = match header.message_kind {
        K::ShellCatalogIdentity => {
            ShellCatalogActionRecord::Identity(ShellCatalogIdentity::take(&mut c)?)
        }
        K::ShellCatalogCandidateBegin => {
            ShellCatalogActionRecord::CandidateBegin(CatalogCandidateBegin::take(&mut c)?)
        }
        K::ShellCatalogCandidateChunk => {
            ShellCatalogActionRecord::CandidateChunk(ContentCandidateChunk::take(&mut c)?)
        }
        K::ShellCatalogActivate => {
            ShellCatalogActionRecord::Activate(CatalogActivation::take(&mut c)?)
        }
        K::ShellCatalogActivationOutcome => {
            ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome::take(&mut c)?)
        }
        _ => {
            return Err(IpcCodecError::InvalidRecord(
                "not a persistent catalog record",
            ));
        }
    };
    c.finish()?;
    crate::shell::catalog_actions::validate(&record)?;
    Ok((header.transaction, record))
}
