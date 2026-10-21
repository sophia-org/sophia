//! Revision-specific additions keep the neutral content codec and legacy
//! entry points unchanged. In particular, reason 13 belongs to one record.
use super::*;
use crate::ShellContentRecord;

/// A receipt names the actual accepted candidate, including ImageBegin at r9.
/// This does not enable image submission in the legacy session client.
pub fn encode_shell_file_submitted_body_at_revision(
    value: ShellFileSubmitted,
    revision: u16,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    check_revision(revision)?;
    let kind = super::codec::kind_at_revision(value.candidate_kind as u16, revision)?;
    if shell_file_class(kind) != ShellFileClass::Candidate {
        return Err(ShellFileCodecError::Kind.into());
    }
    if value.submission_id == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&value.submission_id.to_le_bytes());
    bytes.extend_from_slice(&(kind as u16).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    Ok(bytes)
}

pub fn encode_shell_file_submitted_at_revision(
    header: ShellFileHeader,
    value: ShellFileSubmitted,
    revision: u16,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    super::payload::header_kind(header, ShellFileKind::Submitted)?;
    let body = encode_shell_file_submitted_body_at_revision(value, revision)?;
    Ok(encode_shell_file_record_at_revision(
        header, &body, revision,
    )?)
}

pub fn decode_shell_file_submitted_at_revision(
    bytes: &[u8],
    revision: u16,
) -> Result<ShellFileSubmitted, ShellFilePayloadError> {
    check_revision(revision)?;
    let record = decode_shell_file_record_at_revision(bytes, ShellFileClass::Event, revision)?;
    if record.header.kind != ShellFileKind::Submitted {
        return Err(ShellFileCodecError::Kind.into());
    }
    if record.body.len() != 16 {
        return Err(ShellFileCodecError::Length.into());
    }
    if record.body[10..].iter().any(|byte| *byte != 0) {
        return Err(ShellFileCodecError::Reserved.into());
    }
    let candidate_kind =
        super::codec::kind_at_revision(super::codec::u16_at(record.body, 8)?, revision)?;
    if shell_file_class(candidate_kind) != ShellFileClass::Candidate {
        return Err(ShellFileCodecError::Kind.into());
    }
    let submission_id = super::codec::u64_at(record.body, 0)?;
    if submission_id == 0 {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(ShellFileSubmitted {
        submission_id,
        candidate_kind,
    })
}

fn check_revision(revision: u16) -> Result<(), ShellFilePayloadError> {
    if !(1..=10).contains(&revision) {
        return Err(ShellFileCodecError::Version.into());
    }
    Ok(())
}

fn legacy_status(
    value: &ShellFileTransactionRecord,
    revision: u16,
) -> Result<(ShellFileTransactionRecord, bool), ShellFilePayloadError> {
    check_revision(revision)?;
    let ShellContentRecord::ResourceStatus(status) = &value.record else {
        return Err(ShellFileCodecError::Kind.into());
    };
    let extended = status.reason == 13;
    if extended && (revision != 9 || status.status != 3) {
        return Err(ShellFilePayloadError::Value);
    }
    // This variant has no heap payload. Reuse the complete legacy validation
    // after substituting only the new reason; the other seven reason fields
    // never reach this path.
    let mut status = status.clone();
    if extended {
        status.reason = 0;
    }
    Ok((
        ShellFileTransactionRecord {
            transaction: value.transaction,
            record: ShellContentRecord::ResourceStatus(status),
        },
        extended,
    ))
}

pub fn shell_file_resource_status_bytes_at_revision(
    header: ShellFileHeader,
    value: &ShellFileTransactionRecord,
    revision: u16,
) -> Result<usize, ShellFilePayloadError> {
    let (legacy, _) = legacy_status(value, revision)?;
    shell_file_content_bytes(header, &legacy)
}

pub fn encode_shell_file_resource_status_at_revision(
    header: ShellFileHeader,
    value: &ShellFileTransactionRecord,
    revision: u16,
) -> Result<Vec<u8>, ShellFilePayloadError> {
    let (legacy, extended) = legacy_status(value, revision)?;
    let mut bytes = encode_shell_file_resource_status(header, &legacy)?;
    if extended {
        bytes[74..76].copy_from_slice(&13_u16.to_le_bytes());
    }
    Ok(bytes)
}

pub fn decode_shell_file_resource_status_at_revision(
    bytes: &[u8],
    revision: u16,
) -> Result<ShellFileTransactionRecord, ShellFilePayloadError> {
    check_revision(revision)?;
    // Validate the envelope and exact body size before reading or copying.
    let record = decode_shell_file_record_at_revision(bytes, ShellFileClass::Event, revision)?;
    if record.header.kind != ShellFileKind::ResourceStatus {
        return Err(ShellFileCodecError::Kind.into());
    }
    if bytes.len() != 88 {
        return Err(ShellFileCodecError::Length.into());
    }
    if u16::from_le_bytes([bytes[74], bytes[75]]) != 13 {
        return decode_shell_file_resource_status(bytes);
    }
    if revision != 9 || u16::from_le_bytes([bytes[72], bytes[73]]) != 3 {
        return Err(ShellFilePayloadError::Value);
    }
    let mut legacy = [0_u8; 88];
    legacy.copy_from_slice(bytes);
    legacy[74..76].fill(0);
    let mut value = decode_shell_file_resource_status(&legacy)?;
    if let ShellContentRecord::ResourceStatus(status) = &mut value.record {
        status.reason = 13;
    }
    Ok(value)
}
