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

/// The `outputs` object body: the facts' transaction ID followed by the
/// existing OutputFacts payload. Bounded by the 1 KiB outputs cap.
pub fn encode_shell_file_outputs(
    header: ShellFileHeader,
    tx_record: &ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    header_kind(header, ShellFileKind::Outputs)?;
    let body = encode_shell_file_outputs_body(tx_record)?;
    let bytes = encode_shell_file_record(header, &body)?;
    if bytes.len() > SHELL_FILE_OUTPUTS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(bytes)
}

pub fn encode_shell_file_outputs_body(
    tx_record: &ShellFileTransactionRecord,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if !tx_record.transaction.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let ShellContentRecord::OutputFacts(_) = &tx_record.record else {
        return Err(ShellFileCodecError::Kind.into());
    };
    let (_, payload) =
        crate::ipc::encode_shell_content_payload(tx_record.transaction, &tx_record.record)?;
    let mut body = Vec::with_capacity(8 + payload.len());
    body.extend(tx_record.transaction.raw().to_le_bytes());
    body.extend_from_slice(&payload);
    if SHELL_FILE_HEADER_BYTES + body.len() > SHELL_FILE_OUTPUTS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(body)
}

pub fn decode_shell_file_outputs(
    bytes: &[u8],
) -> Result<ShellFileTransactionRecord, ShellFilePayloadError> {
    if bytes.len() > SHELL_FILE_OUTPUTS_MAX_BYTES {
        return Err(ShellFileCodecError::Length.into());
    }
    let r = record(bytes, ShellFileKind::Outputs, 8)?;
    let tx = TransactionId::from_raw(u64_at(r.body, 0)?);
    if !tx.is_valid() {
        return Err(ShellFilePayloadError::Identity);
    }
    let decoded = crate::ipc::decode_shell_content_payload(
        IpcMessageKind::ShellContentOutputFacts,
        tx,
        &r.body[8..],
    )?;
    Ok(ShellFileTransactionRecord {
        transaction: tx,
        record: decoded,
    })
}

/// Names the object a publication made current: its kind, the domain
/// generation it carries and the qid a pin of it reports through getattr.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellFileObjectPublished {
    pub object: ShellFileKind,
    pub generation: u64,
    pub qid: u64,
}

pub fn encode_shell_file_object_published_body(
    value: ShellFileObjectPublished,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    if shell_file_class(value.object) != ShellFileClass::Object {
        return Err(ShellFileCodecError::Kind.into());
    }
    if value.qid == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    let mut body = Vec::with_capacity(24);
    body.extend((value.object as u16).to_le_bytes());
    body.extend([0; 6]);
    body.extend(value.generation.to_le_bytes());
    body.extend(value.qid.to_le_bytes());
    Ok(body)
}

pub fn decode_shell_file_object_published(
    bytes: &[u8],
) -> Result<ShellFileObjectPublished, ShellFilePayloadError> {
    let r = fixed_record(bytes, ShellFileKind::ObjectPublished, 24)?;
    reserved(&r.body[2..8])?;
    let value = ShellFileObjectPublished {
        object: super::codec::kind(super::codec::u16_at(r.body, 0)?)?,
        generation: u64_at(r.body, 8)?,
        qid: u64_at(r.body, 16)?,
    };
    if shell_file_class(value.object) != ShellFileClass::Object {
        return Err(ShellFileCodecError::Kind.into());
    }
    if value.qid == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(value)
}
