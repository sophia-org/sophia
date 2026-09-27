//! Persistent catalog (r8) and application catalog file records: the whole
//! `Catalog` object, the catalog activation outcome event and the catalog
//! candidate/activation records, as whole typed values with no transfer
//! shape.
use super::codec::u64_at;
use super::payload::*;
use super::*;
use crate::shell::encoding::applications::{
    decode_shell_persistent_catalog, encode_shell_persistent_catalog,
};
use crate::shell::encoding::catalog_actions::{
    ShellCatalogActionValueKind, decode_catalog_content_candidate,
    decode_shell_catalog_action_value, encode_catalog_content_candidate,
    encode_shell_catalog_action_value,
};
use crate::*;

/// The whole `Catalog` object: the plain application catalog for the
/// launcher and legacy-descriptor profiles, or the r8 persistent catalog
/// (each entry with its own identity) for the dock, under the object's own
/// domain transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileCatalog {
    pub transaction: TransactionId,
    pub catalog: ShellPersistentCatalog,
}

pub fn encode_shell_file_catalog(
    header: ShellFileHeader,
    value: &ShellFileCatalog,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Catalog)?;
    let body = encode_shell_file_catalog_body(value)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn encode_shell_file_catalog_body(
    value: &ShellFileCatalog,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_persistent_catalog(&value.catalog)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode_shell_file_catalog(bytes: &[u8]) -> Result<ShellFileCatalog, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::Catalog, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileCatalog {
        transaction,
        catalog: decode_shell_persistent_catalog(&r.body[8..])?,
    })
}

/// One catalog-action record (`CatalogActivationOutcome` or `CatalogActivate`)
/// under its transaction. `Identity`, `CandidateBegin` and `CandidateChunk`
/// are never carried this way: identities ride the `Catalog` object and
/// candidates ride [`ShellFileCatalogCandidate`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileCatalogActionRecord {
    pub transaction: TransactionId,
    pub record: ShellCatalogActionRecord,
}

fn carried(kind: ShellFileKind) -> Option<ShellCatalogActionValueKind> {
    Some(match kind {
        ShellFileKind::CatalogActivationOutcome => ShellCatalogActionValueKind::ActivationOutcome,
        ShellFileKind::CatalogActivate => ShellCatalogActionValueKind::Activate,
        _ => return None,
    })
}

pub fn shell_file_catalog_action_kind(record: &ShellCatalogActionRecord) -> Option<ShellFileKind> {
    Some(match record {
        ShellCatalogActionRecord::ActivationOutcome(_) => ShellFileKind::CatalogActivationOutcome,
        ShellCatalogActionRecord::Activate(_) => ShellFileKind::CatalogActivate,
        ShellCatalogActionRecord::Identity(_)
        | ShellCatalogActionRecord::CandidateBegin(_)
        | ShellCatalogActionRecord::CandidateChunk(_) => return None,
    })
}

pub fn encode_shell_file_catalog_action_body(
    tx_record: &ShellFileCatalogActionRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellFilePayloadError> {
    let kind =
        shell_file_catalog_action_kind(&tx_record.record).ok_or(ShellFileCodecError::Kind)?;
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_catalog_action_value(&tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok((kind, body))
}

pub fn encode_shell_file_catalog_action(
    header: ShellFileHeader,
    tx_record: &ShellFileCatalogActionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let (kind, body) = encode_shell_file_catalog_action_body(tx_record)?;
    header_kind(header, kind)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_catalog_action(
    bytes: &[u8],
    kind: ShellFileKind,
) -> Result<ShellFileCatalogActionRecord, ShellFilePayloadError> {
    let value_kind = carried(kind).ok_or(ShellFileCodecError::Kind)?;
    let r = record(bytes, kind, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = decode_shell_catalog_action_value(value_kind, &r.body[8..])?;
    if shell_file_catalog_action_kind(&decoded) != Some(kind) {
        return Err(ShellFileCodecError::Kind.into());
    }
    Ok(ShellFileCatalogActionRecord {
        transaction,
        record: decoded,
    })
}

/// One whole catalog candidate under its transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileCatalogCandidate {
    pub transaction: TransactionId,
    pub candidate: CatalogContentCandidate,
}

pub fn encode_shell_file_catalog_candidate(
    header: ShellFileHeader,
    value: &ShellFileCatalogCandidate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::CatalogCandidate)?;
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let candidate = encode_catalog_content_candidate(&value.candidate)?;
    let mut body = Vec::with_capacity(8 + candidate.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&candidate);
    let bytes = encode_shell_file_record(header, &body)?;
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(bytes)
}

pub fn decode_shell_file_catalog_candidate(
    bytes: &[u8],
) -> Result<ShellFileCatalogCandidate, ShellFilePayloadError> {
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    let r = record(bytes, ShellFileKind::CatalogCandidate, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileCatalogCandidate {
        transaction,
        candidate: decode_catalog_content_candidate(&r.body[8..])?,
    })
}
