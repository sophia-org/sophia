//! Candidates, pacing and content actions. Each record is its transaction ID
//! followed by the unchanged `sophia_shell_v1` payload, except `Candidate`:
//! its Begin, one Chunk and End share one record, so a submitted candidate is
//! complete or absent.
use super::codec::{u32_at, u64_at};
use super::payload::*;
use super::*;
use crate::shell::encoding::content::{
    ShellContentValueKind, decode_shell_content_value, encode_shell_content_value,
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

/// One complete candidate: the Begin, its only Chunk (ordinal 0) and the End,
/// under one transaction and one grant and candidate generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileCandidate {
    pub transaction: TransactionId,
    pub begin: ContentCandidateBegin,
    pub chunk: ContentCandidateChunk,
    pub end: ContentCandidateEnd,
}

impl ShellFileCandidate {
    fn coherent(&self) -> bool {
        self.transaction.is_valid()
            && self.chunk.chunk_ordinal == 0
            && self.chunk.grant == self.begin.grant
            && self.end.grant == self.begin.grant
            && self.chunk.candidate_generation == self.begin.candidate_generation
            && self.end.candidate_generation == self.begin.candidate_generation
    }

    /// The records the candidate owner receives, in wire order.
    pub fn records(self) -> [ShellContentRecord; 3] {
        [
            ShellContentRecord::CandidateBegin(self.begin),
            ShellContentRecord::CandidateChunk(self.chunk),
            ShellContentRecord::CandidateEnd(self.end),
        ]
    }
}

/// Body: transaction `u64`, Begin length `u32`, Chunk length `u32`, then the
/// Begin, Chunk and End payloads. The whole record stays within
/// [`SHELL_FILE_MAX_CANDIDATE_BYTES`].
pub fn encode_shell_file_candidate(
    header: ShellFileHeader,
    candidate: &ShellFileCandidate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Candidate)?;
    if !candidate.coherent() {
        return Err(ShellFilePayloadError::Identity);
    }
    let tx = candidate.transaction;
    let begin =
        encode_shell_content_value(&ShellContentRecord::CandidateBegin(candidate.begin.clone()))?;
    let chunk =
        encode_shell_content_value(&ShellContentRecord::CandidateChunk(candidate.chunk.clone()))?;
    let end = encode_shell_content_value(&ShellContentRecord::CandidateEnd(candidate.end.clone()))?;
    let mut body = Vec::with_capacity(16 + begin.len() + chunk.len() + end.len());
    body.extend(tx.raw().to_le_bytes());
    body.extend((begin.len() as u32).to_le_bytes());
    body.extend((chunk.len() as u32).to_le_bytes());
    body.extend_from_slice(&begin);
    body.extend_from_slice(&chunk);
    body.extend_from_slice(&end);
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
    let r = record(bytes, ShellFileKind::Candidate, 16)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let begin_len = u32_at(r.body, 8)? as usize;
    let chunk_len = u32_at(r.body, 12)? as usize;
    let chunk_at = 16usize
        .checked_add(begin_len)
        .ok_or(ShellFileCodecError::Length)?;
    let end_at = chunk_at
        .checked_add(chunk_len)
        .filter(|end_at| *end_at <= r.body.len())
        .ok_or(ShellFileCodecError::Length)?;
    let part =
        |kind, range: std::ops::Range<usize>| decode_shell_content_value(kind, &r.body[range]);
    let begin = part(ShellContentValueKind::CandidateBegin, 16..chunk_at)?;
    let chunk = part(ShellContentValueKind::CandidateChunk, chunk_at..end_at)?;
    let end = part(ShellContentValueKind::CandidateEnd, end_at..r.body.len())?;
    let (
        ShellContentRecord::CandidateBegin(begin),
        ShellContentRecord::CandidateChunk(chunk),
        ShellContentRecord::CandidateEnd(end),
    ) = (begin, chunk, end)
    else {
        return Err(ShellFileCodecError::Kind.into());
    };
    let candidate = ShellFileCandidate {
        transaction,
        begin,
        chunk,
        end,
    };
    if !candidate.coherent() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(candidate)
}
