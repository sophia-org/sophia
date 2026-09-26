use super::records::*;

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], ShellFileCodecError> {
    let end = offset.checked_add(N).ok_or(ShellFileCodecError::Length)?;
    bytes
        .get(offset..end)
        .and_then(|field| field.try_into().ok())
        .ok_or(ShellFileCodecError::Length)
}

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, ShellFileCodecError> {
    Ok(u16::from_le_bytes(field(bytes, offset)?))
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ShellFileCodecError> {
    Ok(u32::from_le_bytes(field(bytes, offset)?))
}

pub(super) fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, ShellFileCodecError> {
    Ok(u64::from_le_bytes(field(bytes, offset)?))
}

pub fn shell_file_class(kind: ShellFileKind) -> ShellFileClass {
    match kind {
        ShellFileKind::Limits | ShellFileKind::Outputs => ShellFileClass::Object,
        ShellFileKind::Negotiate
        | ShellFileKind::AllocationRequest
        | ShellFileKind::ResourceBegin
        | ShellFileKind::ResourceEnd
        | ShellFileKind::ResourceCancel
        | ShellFileKind::ResourceRetire
        | ShellFileKind::Candidate
        | ShellFileKind::FrameDemand
        | ShellFileKind::FrameDemandCancel
        | ShellFileKind::ActionAck => ShellFileClass::Candidate,
        ShellFileKind::Negotiated
        | ShellFileKind::Refused
        | ShellFileKind::Submitted
        | ShellFileKind::ObjectPublished
        | ShellFileKind::AllocationResult
        | ShellFileKind::ResourceStatus
        | ShellFileKind::ResourceReleased
        | ShellFileKind::CandidateOutcome
        | ShellFileKind::FramePermit
        | ShellFileKind::Action => ShellFileClass::Event,
    }
}

pub(super) fn kind(value: u16) -> Result<ShellFileKind, ShellFileCodecError> {
    Ok(match value {
        1 => ShellFileKind::Limits,
        2 => ShellFileKind::Outputs,
        16 => ShellFileKind::Negotiated,
        17 => ShellFileKind::Refused,
        18 => ShellFileKind::Submitted,
        19 => ShellFileKind::ObjectPublished,
        32 => ShellFileKind::AllocationResult,
        33 => ShellFileKind::ResourceStatus,
        34 => ShellFileKind::ResourceReleased,
        35 => ShellFileKind::CandidateOutcome,
        36 => ShellFileKind::FramePermit,
        37 => ShellFileKind::Action,
        256 => ShellFileKind::Negotiate,
        257 => ShellFileKind::AllocationRequest,
        258 => ShellFileKind::ResourceBegin,
        259 => ShellFileKind::ResourceEnd,
        260 => ShellFileKind::ResourceCancel,
        261 => ShellFileKind::ResourceRetire,
        262 => ShellFileKind::Candidate,
        263 => ShellFileKind::FrameDemand,
        264 => ShellFileKind::FrameDemandCancel,
        265 => ShellFileKind::ActionAck,
        _ => return Err(ShellFileCodecError::Kind),
    })
}

pub(super) fn validate_header(header: ShellFileHeader) -> Result<(), ShellFileCodecError> {
    let valid = header.connection_epoch != 0
        && match shell_file_class(header.kind) {
            ShellFileClass::Object => header.submission_id == 0 && header.sequence == 0,
            ShellFileClass::Candidate => header.submission_id != 0 && header.sequence == 0,
            ShellFileClass::Event => header.submission_id == 0 && header.sequence != 0,
        };
    valid.then_some(()).ok_or(ShellFileCodecError::Identity)
}

pub fn decode_shell_file_record(
    bytes: &[u8],
    expected: ShellFileClass,
) -> Result<ShellFileRecord<'_>, ShellFileCodecError> {
    let size = bytes.len();
    if size < SHELL_FILE_HEADER_BYTES {
        return Err(ShellFileCodecError::Length);
    }
    let declared_size = usize::try_from(u32_at(bytes, 0)?).unwrap_or(0);
    if declared_size != size {
        return Err(ShellFileCodecError::Length);
    }
    if u16_at(bytes, 4)? != SHELL_FILE_API_VERSION {
        return Err(ShellFileCodecError::Version);
    }
    let header = ShellFileHeader {
        kind: kind(u16_at(bytes, 6)?)?,
        connection_epoch: u64_at(bytes, 8)?,
        submission_id: u64_at(bytes, 16)?,
        sequence: u64_at(bytes, 24)?,
    };
    validate_header(header)?;
    let class = shell_file_class(header.kind);
    if class != expected {
        return Err(ShellFileCodecError::Class);
    }
    let max_size = if class == ShellFileClass::Object {
        SHELL_FILE_MAX_OBJECT_BYTES
    } else {
        SHELL_FILE_MAX_TRANSACTION_BYTES
    };
    if size > max_size {
        return Err(ShellFileCodecError::Length);
    }
    Ok(ShellFileRecord {
        header,
        body: &bytes[SHELL_FILE_HEADER_BYTES..],
    })
}

pub fn encode_shell_file_record(
    header: ShellFileHeader,
    body: &[u8],
) -> Result<Vec<u8>, ShellFileCodecError> {
    validate_header(header)?;
    let size = SHELL_FILE_HEADER_BYTES
        .checked_add(body.len())
        .ok_or(ShellFileCodecError::Length)?;
    let class = shell_file_class(header.kind);
    let max_size = if class == ShellFileClass::Object {
        SHELL_FILE_MAX_OBJECT_BYTES
    } else {
        SHELL_FILE_MAX_TRANSACTION_BYTES
    };
    if size > max_size {
        return Err(ShellFileCodecError::Length);
    }
    let mut bytes = Vec::with_capacity(size);
    bytes.extend(
        u32::try_from(size)
            .map_err(|_| ShellFileCodecError::Length)?
            .to_le_bytes(),
    );
    bytes.extend(SHELL_FILE_API_VERSION.to_le_bytes());
    bytes.extend((header.kind as u16).to_le_bytes());
    bytes.extend(header.connection_epoch.to_le_bytes());
    bytes.extend(header.submission_id.to_le_bytes());
    bytes.extend(header.sequence.to_le_bytes());
    bytes.extend(body);
    Ok(bytes)
}

pub fn decode_shell_file_submit(bytes: &[u8]) -> Result<ShellFileSubmit, ShellFileCodecError> {
    if bytes.len() != SHELL_FILE_SUBMIT_BYTES {
        return Err(ShellFileCodecError::Length);
    }
    if u32_at(bytes, 20)? != 0 {
        return Err(ShellFileCodecError::Reserved);
    }
    let submit = ShellFileSubmit {
        connection_epoch: u64_at(bytes, 0)?,
        submission_id: u64_at(bytes, 8)?,
        candidate_bytes: u32_at(bytes, 16)?,
    };
    if submit.connection_epoch == 0 || submit.submission_id == 0 {
        return Err(ShellFileCodecError::Identity);
    }
    let size = usize::try_from(submit.candidate_bytes).map_err(|_| ShellFileCodecError::Length)?;
    if !(SHELL_FILE_HEADER_BYTES..=SHELL_FILE_MAX_TRANSACTION_BYTES).contains(&size) {
        return Err(ShellFileCodecError::Length);
    }
    Ok(submit)
}

pub fn encode_shell_file_submit(submit: ShellFileSubmit) -> Result<Vec<u8>, ShellFileCodecError> {
    if submit.connection_epoch == 0 || submit.submission_id == 0 {
        return Err(ShellFileCodecError::Identity);
    }
    let size = usize::try_from(submit.candidate_bytes).map_err(|_| ShellFileCodecError::Length)?;
    if !(SHELL_FILE_HEADER_BYTES..=SHELL_FILE_MAX_TRANSACTION_BYTES).contains(&size) {
        return Err(ShellFileCodecError::Length);
    }
    let mut bytes = Vec::with_capacity(SHELL_FILE_SUBMIT_BYTES);
    bytes.extend(submit.connection_epoch.to_le_bytes());
    bytes.extend(submit.submission_id.to_le_bytes());
    bytes.extend(submit.candidate_bytes.to_le_bytes());
    bytes.extend([0; 4]);
    Ok(bytes)
}

pub fn decode_shell_file_ack(bytes: &[u8]) -> Result<ShellFileAck, ShellFileCodecError> {
    if bytes.len() != SHELL_FILE_ACK_BYTES {
        return Err(ShellFileCodecError::Length);
    }
    let ack = ShellFileAck {
        connection_epoch: u64_at(bytes, 0)?,
        sequence: u64_at(bytes, 8)?,
    };
    if ack.connection_epoch == 0 || ack.sequence == 0 {
        return Err(ShellFileCodecError::Identity);
    }
    Ok(ack)
}

pub fn encode_shell_file_ack(ack: ShellFileAck) -> Result<Vec<u8>, ShellFileCodecError> {
    if ack.connection_epoch == 0 || ack.sequence == 0 {
        return Err(ShellFileCodecError::Identity);
    }
    let mut bytes = Vec::with_capacity(SHELL_FILE_ACK_BYTES);
    bytes.extend(ack.connection_epoch.to_le_bytes());
    bytes.extend(ack.sequence.to_le_bytes());
    Ok(bytes)
}
