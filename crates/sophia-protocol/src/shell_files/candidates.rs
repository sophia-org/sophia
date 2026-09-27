//! Candidates, pacing and content actions. Each record is its transaction ID
//! followed by the record's value; a `Candidate` is one whole candidate, so a
//! submitted candidate is complete or absent.
use super::codec::u64_at;
use super::payload::*;
use super::*;
use crate::shell::encoding::content::{
    ShellContentValueKind, decode_content_candidate, decode_shell_content_value,
    encode_content_candidate, encode_shell_content_value,
};
use crate::*;

/// The payload a single-payload transaction record carries.
fn carried(kind: ShellFileKind) -> Option<ShellContentValueKind> {
    Some(match kind {
        ShellFileKind::CandidateOutcome => ShellContentValueKind::CandidateOutcome,
        ShellFileKind::FramePermit => ShellContentValueKind::FramePermit,
        ShellFileKind::Action => ShellContentValueKind::Action,
        ShellFileKind::FrameDemand => ShellContentValueKind::FrameDemand,
        ShellFileKind::FrameDemandCancel => ShellContentValueKind::FrameDemandCancel,
        ShellFileKind::ActionAck => ShellContentValueKind::ActionAck,
        _ => return None,
    })
}

/// The file kind that carries `record` as one transaction record, if any.
pub fn shell_file_transaction_kind(record: &ShellContentRecord) -> Option<ShellFileKind> {
    Some(match record {
        ShellContentRecord::CandidateOutcome(_) => ShellFileKind::CandidateOutcome,
        ShellContentRecord::FramePermit(_) => ShellFileKind::FramePermit,
        ShellContentRecord::Action(_) => ShellFileKind::Action,
        ShellContentRecord::FrameDemand(_) => ShellFileKind::FrameDemand,
        ShellContentRecord::FrameDemandCancel(_) => ShellFileKind::FrameDemandCancel,
        ShellContentRecord::ActionAck(_) => ShellFileKind::ActionAck,
        _ => return None,
    })
}

/// The body of a single-payload transaction record and the kind that carries
/// it. The journal supplies an event's header.
pub fn encode_shell_file_transaction_body(
    tx_record: &ShellFileTransactionRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellFilePayloadError> {
    let kind = shell_file_transaction_kind(&tx_record.record).ok_or(ShellFileCodecError::Kind)?;
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_content_value(&tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok((kind, body))
}

pub fn encode_shell_file_transaction(
    header: ShellFileHeader,
    tx_record: &ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let (kind, body) = encode_shell_file_transaction_body(tx_record)?;
    header_kind(header, kind)?;
    Ok(encode_shell_file_record(header, &body)?)
}

/// Decodes a single-payload transaction record of `kind`, with every
/// existing record check.
pub fn decode_shell_file_transaction(
    bytes: &[u8],
    kind: ShellFileKind,
) -> Result<ShellFileTransactionRecord, ShellFilePayloadError> {
    let value_kind = carried(kind).ok_or(ShellFileCodecError::Kind)?;
    let r = record(bytes, kind, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = decode_shell_content_value(value_kind, &r.body[8..])?;
    if shell_file_transaction_kind(&decoded) != Some(kind) {
        return Err(ShellFileCodecError::Kind.into());
    }
    Ok(ShellFileTransactionRecord {
        transaction,
        record: decoded,
    })
}

/// One complete candidate under its transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileCandidate {
    pub transaction: TransactionId,
    pub candidate: ContentCandidate,
}

/// Body: the transaction `u64`, then the whole candidate value (header, row
/// counts, rows). The whole record stays within
/// [`SHELL_FILE_MAX_CANDIDATE_BYTES`].
pub fn encode_shell_file_candidate(
    header: ShellFileHeader,
    value: &ShellFileCandidate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Candidate)?;
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let candidate = encode_content_candidate(&value.candidate)?;
    let mut body = Vec::with_capacity(8 + candidate.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&candidate);
    let bytes = encode_shell_file_record(header, &body)?;
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(bytes)
}

pub fn decode_shell_file_candidate(
    bytes: &[u8],
) -> Result<ShellFileCandidate, ShellFilePayloadError> {
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    let r = record(bytes, ShellFileKind::Candidate, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileCandidate {
        transaction,
        candidate: decode_content_candidate(&r.body[8..])?,
    })
}
