//! Native launcher (r7) file records: whole typed values, no transfer
//! shapes. Each single-payload record is its transaction ID followed by the
//! neutral native-launcher value; `NativeCandidate` is the transaction ID
//! followed by one whole [`NativeContentCandidate`] and `NativeInput` pads
//! its text to a fixed width, exactly like every other native launcher
//! event, instead of carrying it at variable length.
use super::codec::u64_at;
use super::payload::*;
use super::*;
use crate::shell::encoding::native_launcher::{
    ShellNativeLauncherValueKind, decode_native_content_candidate,
    decode_native_launcher_input_padded, decode_shell_native_launcher_value,
    encode_native_content_candidate, encode_native_launcher_input_padded,
    encode_shell_native_launcher_value,
};
use crate::*;

/// One native-launcher record under its transaction, carried as a
/// single-payload file record (every variant but `CandidateBegin` and
/// `CandidateChunk`, which the whole [`NativeContentCandidate`] replaces).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileNativeLauncherRecord {
    pub transaction: TransactionId,
    pub record: ShellNativeLauncherRecord,
}

fn carried(kind: ShellFileKind) -> Option<ShellNativeLauncherValueKind> {
    use ShellFileKind as K;
    use ShellNativeLauncherValueKind as V;
    Some(match kind {
        K::NativeOpening => V::Opening,
        K::NativeFocus => V::Focus,
        K::NativeFocusRevoked => V::FocusRevoked,
        K::NativeActivationOutcome => V::ActivationOutcome,
        K::NativeClosed => V::Closed,
        K::NativeAllocationRequest => V::AllocationRequest,
        K::NativeInputAck => V::InputAck,
        K::NativeActivate => V::Activate,
        _ => return None,
    })
}

/// The file kind that carries `record` as one single-payload transaction
/// record, if any (`Input` has its own padded body; `CandidateBegin` and
/// `CandidateChunk` only ever appear inside a whole `NativeCandidate`).
pub fn shell_file_native_launcher_kind(
    record: &ShellNativeLauncherRecord,
) -> Option<ShellFileKind> {
    use ShellFileKind as K;
    use ShellNativeLauncherRecord::*;
    Some(match record {
        Opening(_) => K::NativeOpening,
        Focus(_) => K::NativeFocus,
        FocusRevoked(_) => K::NativeFocusRevoked,
        ActivationOutcome(_) => K::NativeActivationOutcome,
        Closed(_) => K::NativeClosed,
        AllocationRequest(_) => K::NativeAllocationRequest,
        InputAck(_) => K::NativeInputAck,
        Activate(_) => K::NativeActivate,
        Input(_) | CandidateBegin(_) | CandidateChunk(_) => return None,
    })
}

pub fn encode_shell_file_native_launcher_transaction_body(
    tx_record: &ShellFileNativeLauncherRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellFilePayloadError> {
    let kind =
        shell_file_native_launcher_kind(&tx_record.record).ok_or(ShellFileCodecError::Kind)?;
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let payload = encode_shell_native_launcher_value(&tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok((kind, body))
}

pub fn encode_shell_file_native_launcher_transaction(
    header: ShellFileHeader,
    tx_record: &ShellFileNativeLauncherRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let (kind, body) = encode_shell_file_native_launcher_transaction_body(tx_record)?;
    header_kind(header, kind)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_native_launcher_transaction(
    bytes: &[u8],
    kind: ShellFileKind,
) -> Result<ShellFileNativeLauncherRecord, ShellFilePayloadError> {
    let value_kind = carried(kind).ok_or(ShellFileCodecError::Kind)?;
    let r = record(bytes, kind, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = decode_shell_native_launcher_value(value_kind, &r.body[8..])?;
    if shell_file_native_launcher_kind(&decoded) != Some(kind) {
        return Err(ShellFileCodecError::Kind.into());
    }
    Ok(ShellFileNativeLauncherRecord {
        transaction,
        record: decoded,
    })
}

pub fn encode_shell_file_native_input(
    header: ShellFileHeader,
    tx_record: &ShellFileNativeLauncherRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::NativeInput)?;
    let body = encode_shell_file_native_input_body(tx_record)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn encode_shell_file_native_input_body(
    tx_record: &ShellFileNativeLauncherRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let ShellNativeLauncherRecord::Input(value) = &tx_record.record else {
        return Err(ShellFileCodecError::Kind.into());
    };
    let payload = encode_native_launcher_input_padded(value)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode_shell_file_native_input(
    bytes: &[u8],
) -> Result<ShellFileNativeLauncherRecord, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::NativeInput, 8)?;
    let tx = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !tx.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let value = decode_native_launcher_input_padded(&r.body[8..])?;
    Ok(ShellFileNativeLauncherRecord {
        transaction: tx,
        record: ShellNativeLauncherRecord::Input(value),
    })
}

/// One whole native-launcher candidate under its transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileNativeCandidate {
    pub transaction: TransactionId,
    pub candidate: NativeContentCandidate,
}

/// Body: the transaction `u64`, then the whole native candidate value. The
/// whole record stays within [`SHELL_FILE_MAX_CANDIDATE_BYTES`], the same
/// cap the base `Candidate` record uses.
pub fn encode_shell_file_native_candidate(
    header: ShellFileHeader,
    value: &ShellFileNativeCandidate,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::NativeCandidate)?;
    if !value.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let candidate = encode_native_content_candidate(&value.candidate)?;
    let mut body = Vec::with_capacity(8 + candidate.len());
    body.extend(value.transaction.raw().to_le_bytes());
    body.extend_from_slice(&candidate);
    let bytes = encode_shell_file_record(header, &body)?;
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(bytes)
}

pub fn decode_shell_file_native_candidate(
    bytes: &[u8],
) -> Result<ShellFileNativeCandidate, ShellFilePayloadError> {
    if bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    let r = record(bytes, ShellFileKind::NativeCandidate, 8)?;
    let transaction = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileNativeCandidate {
        transaction,
        candidate: decode_native_content_candidate(&r.body[8..])?,
    })
}
