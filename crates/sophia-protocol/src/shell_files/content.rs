use super::codec::u64_at;
use super::payload::*;
use super::*;
use crate::*;

pub fn encode_shell_file_limits(
    header: ShellFileHeader,
    limits: ContentLimits,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Limits)?;
    let record = ShellContentRecord::Limits(limits);
    let (_, body) = crate::ipc::encode_shell_content_payload(TransactionId::INVALID, &record)?;
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_limits(bytes: &[u8]) -> Result<ContentLimits, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::Limits, 0)?;
    let decoded = crate::ipc::decode_shell_content_payload(
        IpcMessageKind::ShellContentLimits,
        TransactionId::INVALID,
        r.body,
    )?;
    match decoded {
        ShellContentRecord::Limits(limits) => Ok(limits),
        _ => Err(ShellFileCodecError::Kind.into()),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellFileTransactionRecord {
    pub transaction: TransactionId,
    pub record: ShellContentRecord,
}

pub fn encode_shell_file_allocation_request(
    header: ShellFileHeader,
    tx_record: ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::AllocationRequest)?;
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    match tx_record.record {
        ShellContentRecord::AllocationRequest(_) => {}
        _ => return Err(ShellFileCodecError::Kind.into()),
    }
    let (_, payload) =
        crate::ipc::encode_shell_content_payload(tx_record.transaction, &tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(encode_shell_file_record(header, &body)?)
}

pub fn decode_shell_file_allocation_request(
    bytes: &[u8],
) -> Result<ShellFileTransactionRecord, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::AllocationRequest, 8)?;
    let tx = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !tx.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = crate::ipc::decode_shell_content_payload(
        IpcMessageKind::ShellContentAllocationRequest,
        tx,
        &r.body[8..],
    )?;
    match decoded {
        ShellContentRecord::AllocationRequest(_) => Ok(ShellFileTransactionRecord {
            transaction: tx,
            record: decoded,
        }),
        _ => Err(ShellFileCodecError::Kind.into()),
    }
}

pub fn encode_shell_file_allocation_result(
    header: ShellFileHeader,
    tx_record: ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::AllocationResult)?;
    let body = encode_shell_file_allocation_result_body(&tx_record)?;
    Ok(encode_shell_file_record(header, &body)?)
}

/// The journal supplies the event header; the body carries the correlation.
pub fn encode_shell_file_allocation_result_body(
    tx_record: &ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    match tx_record.record {
        ShellContentRecord::AllocationResult(_) => {}
        _ => return Err(ShellFileCodecError::Kind.into()),
    }
    let (_, payload) =
        crate::ipc::encode_shell_content_payload(tx_record.transaction, &tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode_shell_file_allocation_result(
    bytes: &[u8],
) -> Result<ShellFileTransactionRecord, ShellFilePayloadError> {
    let r = record(bytes, ShellFileKind::AllocationResult, 8)?;
    let tx = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !tx.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = crate::ipc::decode_shell_content_payload(
        IpcMessageKind::ShellContentAllocationResult,
        tx,
        &r.body[8..],
    )?;
    match decoded {
        ShellContentRecord::AllocationResult(_) => Ok(ShellFileTransactionRecord {
            transaction: tx,
            record: decoded,
        }),
        _ => Err(ShellFileCodecError::Kind.into()),
    }
}
