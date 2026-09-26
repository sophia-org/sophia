//! Persistent catalog action wire codec. Encodes and decodes the typed
//! records defined in `crate::shell::catalog_actions`; the byte shape of
//! each record lives in `crate::shell::encoding::catalog_actions`, and
//! validation lives with those types.
use crate::shell::encoding::catalog_actions::{
    ShellCatalogActionValueKind, decode_shell_catalog_action_value,
    encode_shell_catalog_action_value, shell_catalog_action_value_kind,
};
use crate::{
    IpcCodecError, IpcMessageKind, ShellCatalogActionRecord, TransactionId, decode_frame,
    encode_frame,
};

/// Frame-level bound check: `IpcCodecError`, not the typed validators'
/// neutral error, since this only guards the transaction ID.
fn require(ok: bool, field: &'static str) -> Result<(), IpcCodecError> {
    if ok {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidRecord(field))
    }
}

pub fn encode_shell_catalog_action_frame(
    transaction: TransactionId,
    record: &ShellCatalogActionRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    require(transaction.is_valid(), "catalog action transaction")?;
    let bytes = encode_shell_catalog_action_value(record)?;
    let kind = value_kind_to_ipc(shell_catalog_action_value_kind(record));
    encode_frame(kind, transaction, &bytes)
}
pub fn decode_shell_catalog_action_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellCatalogActionRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require(header.transaction.is_valid(), "catalog action transaction")?;
    let value_kind = value_kind_from_ipc(header.message_kind).ok_or(
        IpcCodecError::InvalidRecord("not a persistent catalog record"),
    )?;
    let record = decode_shell_catalog_action_value(value_kind, payload)?;
    Ok((header.transaction, record))
}

/// The `IpcMessageKind` <-> `ShellCatalogActionValueKind` mapping. This is
/// the only place that knows both namings.
fn value_kind_from_ipc(kind: IpcMessageKind) -> Option<ShellCatalogActionValueKind> {
    use IpcMessageKind as K;
    use ShellCatalogActionValueKind as V;
    Some(match kind {
        K::ShellCatalogIdentity => V::Identity,
        K::ShellCatalogCandidateBegin => V::CandidateBegin,
        K::ShellCatalogCandidateChunk => V::CandidateChunk,
        K::ShellCatalogActivate => V::Activate,
        K::ShellCatalogActivationOutcome => V::ActivationOutcome,
        _ => return None,
    })
}

fn value_kind_to_ipc(kind: ShellCatalogActionValueKind) -> IpcMessageKind {
    use IpcMessageKind as K;
    use ShellCatalogActionValueKind as V;
    match kind {
        V::Identity => K::ShellCatalogIdentity,
        V::CandidateBegin => K::ShellCatalogCandidateBegin,
        V::CandidateChunk => K::ShellCatalogCandidateChunk,
        V::Activate => K::ShellCatalogActivate,
        V::ActivationOutcome => K::ShellCatalogActivationOutcome,
    }
}
