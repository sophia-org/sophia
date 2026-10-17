use super::records::*;

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], WmFileCodecError> {
    let end = offset.checked_add(N).ok_or(WmFileCodecError::Length)?;
    bytes
        .get(offset..end)
        .and_then(|field| field.try_into().ok())
        .ok_or(WmFileCodecError::Length)
}

pub(super) fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, WmFileCodecError> {
    Ok(u16::from_le_bytes(field(bytes, offset)?))
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, WmFileCodecError> {
    Ok(u32::from_le_bytes(field(bytes, offset)?))
}

pub(super) fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, WmFileCodecError> {
    Ok(u64::from_le_bytes(field(bytes, offset)?))
}

pub(super) fn kind(value: u16) -> Result<WmFileKind, WmFileCodecError> {
    Ok(match value {
        1 => WmFileKind::Limits,
        2 => WmFileKind::Snapshot,
        16 => WmFileKind::Negotiated,
        17 => WmFileKind::Submitted,
        18 => WmFileKind::ProfilePrepare,
        19 => WmFileKind::ProfileActivate,
        20 => WmFileKind::ProfileRollback,
        21 => WmFileKind::ConfigurationOutcome,
        22 => WmFileKind::Cycle,
        23 => WmFileKind::ProjectionOutcome,
        24 => WmFileKind::SessionOperationOutcome,
        25 => WmFileKind::PresentationReceipt,
        256 => WmFileKind::Negotiate,
        257 => WmFileKind::ProfilePrepared,
        258 => WmFileKind::ProfileActive,
        259 => WmFileKind::ProfileRolledBack,
        260 => WmFileKind::Configuration,
        261 => WmFileKind::Dirty,
        262 => WmFileKind::Projection,
        263 => WmFileKind::SessionOperation,
        _ => return Err(WmFileCodecError::Kind),
    })
}

pub fn wm_file_class(kind: WmFileKind) -> WmFileClass {
    match kind {
        WmFileKind::Limits | WmFileKind::Snapshot => WmFileClass::Object,
        WmFileKind::Negotiate
        | WmFileKind::ProfilePrepared
        | WmFileKind::ProfileActive
        | WmFileKind::ProfileRolledBack
        | WmFileKind::Configuration
        | WmFileKind::Dirty
        | WmFileKind::Projection
        | WmFileKind::SessionOperation => WmFileClass::Candidate,
        WmFileKind::Negotiated
        | WmFileKind::Submitted
        | WmFileKind::ProfilePrepare
        | WmFileKind::ProfileActivate
        | WmFileKind::ProfileRollback
        | WmFileKind::ConfigurationOutcome
        | WmFileKind::Cycle
        | WmFileKind::ProjectionOutcome
        | WmFileKind::SessionOperationOutcome
        | WmFileKind::PresentationReceipt => WmFileClass::Event,
    }
}

pub(super) fn validate_header(header: WmFileHeader) -> Result<(), WmFileCodecError> {
    let valid = header.connection_epoch != 0
        && match wm_file_class(header.kind) {
            WmFileClass::Object => header.submission_id == 0 && header.sequence == 0,
            WmFileClass::Candidate => header.submission_id != 0 && header.sequence == 0,
            WmFileClass::Event => header.submission_id == 0 && header.sequence != 0,
        };
    valid.then_some(()).ok_or(WmFileCodecError::Identity)
}

/// Decodes one complete object. Fragment assembly and admitted-epoch matching
/// belong to the file owner, and body semantics to the corresponding codec.
pub fn decode_wm_file_record(
    bytes: &[u8],
    expected: WmFileClass,
) -> Result<WmFileRecord<'_>, WmFileCodecError> {
    if !(WM_FILE_HEADER_BYTES..=WM_FILE_MAX_BYTES).contains(&bytes.len())
        || usize::try_from(u32_at(bytes, 0)?).ok() != Some(bytes.len())
    {
        return Err(WmFileCodecError::Length);
    }
    if u16_at(bytes, 4)? != WM_FILE_API_VERSION {
        return Err(WmFileCodecError::Version);
    }
    let header = WmFileHeader {
        kind: kind(u16_at(bytes, 6)?)?,
        connection_epoch: u64_at(bytes, 8)?,
        submission_id: u64_at(bytes, 16)?,
        sequence: u64_at(bytes, 24)?,
    };
    validate_header(header)?;
    if wm_file_class(header.kind) != expected {
        return Err(WmFileCodecError::Class);
    }
    Ok(WmFileRecord {
        header,
        body: &bytes[WM_FILE_HEADER_BYTES..],
    })
}

pub fn encode_wm_file_record(
    header: WmFileHeader,
    body: &[u8],
) -> Result<Vec<u8>, WmFileCodecError> {
    validate_header(header)?;
    let size = WM_FILE_HEADER_BYTES
        .checked_add(body.len())
        .filter(|size| *size <= WM_FILE_MAX_BYTES)
        .ok_or(WmFileCodecError::Length)?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend(
        u32::try_from(size)
            .map_err(|_| WmFileCodecError::Length)?
            .to_le_bytes(),
    );
    bytes.extend(WM_FILE_API_VERSION.to_le_bytes());
    bytes.extend((header.kind as u16).to_le_bytes());
    bytes.extend(header.connection_epoch.to_le_bytes());
    bytes.extend(header.submission_id.to_le_bytes());
    bytes.extend(header.sequence.to_le_bytes());
    bytes.extend(body);
    Ok(bytes)
}

/// Splits a complete section block without copying its row data. This checks
/// envelope counts/order/lengths only, never the context-specific row grammar.
pub fn decode_wm_file_sections(
    bytes: &[u8],
    section_count: u16,
) -> Result<Vec<WmFileSection<'_>>, WmFileCodecError> {
    let count = usize::from(section_count);
    if count > WM_FILE_MAX_SECTIONS || bytes.len() > WM_FILE_MAX_BYTES {
        return Err(WmFileCodecError::Sections);
    }
    let mut sections = Vec::with_capacity(count);
    let mut remaining = bytes;
    let mut previous = 0;
    for _ in 0..count {
        if remaining.len() < WM_FILE_SECTION_HEADER_BYTES {
            return Err(WmFileCodecError::Length);
        }
        let kind = u16_at(remaining, 0)?;
        if kind <= previous {
            return Err(WmFileCodecError::Sections);
        }
        if u16_at(remaining, 2)? != 0 || u32_at(remaining, 12)? != 0 {
            return Err(WmFileCodecError::Reserved);
        }
        let rows = u32_at(remaining, 4)?;
        let size = usize::try_from(u32_at(remaining, 8)?).map_err(|_| WmFileCodecError::Length)?;
        if rows == 0 || size == 0 || size > remaining.len() - WM_FILE_SECTION_HEADER_BYTES {
            return Err(WmFileCodecError::Length);
        }
        let end = WM_FILE_SECTION_HEADER_BYTES + size;
        sections.push(WmFileSection {
            kind,
            count: rows,
            bytes: &remaining[WM_FILE_SECTION_HEADER_BYTES..end],
        });
        remaining = &remaining[end..];
        previous = kind;
    }
    if !remaining.is_empty() {
        return Err(WmFileCodecError::Length);
    }
    Ok(sections)
}

pub fn encode_wm_file_sections(
    sections: &[WmFileSection<'_>],
) -> Result<Vec<u8>, WmFileCodecError> {
    if sections.len() > WM_FILE_MAX_SECTIONS {
        return Err(WmFileCodecError::Sections);
    }
    let mut size = 0usize;
    let mut previous = 0;
    for section in sections {
        if section.kind <= previous || section.count == 0 || section.bytes.is_empty() {
            return Err(WmFileCodecError::Sections);
        }
        size = size
            .checked_add(WM_FILE_SECTION_HEADER_BYTES)
            .and_then(|size| size.checked_add(section.bytes.len()))
            .filter(|size| *size <= WM_FILE_MAX_BYTES)
            .ok_or(WmFileCodecError::Length)?;
        previous = section.kind;
    }
    let mut bytes = Vec::with_capacity(size);
    for section in sections {
        bytes.extend(section.kind.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(section.count.to_le_bytes());
        bytes.extend(
            u32::try_from(section.bytes.len())
                .map_err(|_| WmFileCodecError::Length)?
                .to_le_bytes(),
        );
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(section.bytes);
    }
    Ok(bytes)
}

pub fn decode_wm_file_submit(bytes: &[u8]) -> Result<WmFileSubmit, WmFileCodecError> {
    if bytes.len() != WM_FILE_SUBMIT_BYTES {
        return Err(WmFileCodecError::Length);
    }
    if u32_at(bytes, 20)? != 0 {
        return Err(WmFileCodecError::Reserved);
    }
    let submit = WmFileSubmit {
        connection_epoch: u64_at(bytes, 0)?,
        submission_id: u64_at(bytes, 8)?,
        candidate_bytes: u32_at(bytes, 16)?,
    };
    if submit.connection_epoch == 0 || submit.submission_id == 0 {
        return Err(WmFileCodecError::Identity);
    }
    let size = usize::try_from(submit.candidate_bytes).map_err(|_| WmFileCodecError::Length)?;
    if !(WM_FILE_HEADER_BYTES..=WM_FILE_MAX_BYTES).contains(&size) {
        return Err(WmFileCodecError::Length);
    }
    Ok(submit)
}

pub fn decode_wm_file_ack(bytes: &[u8]) -> Result<WmFileAck, WmFileCodecError> {
    if bytes.len() != WM_FILE_ACK_BYTES {
        return Err(WmFileCodecError::Length);
    }
    let ack = WmFileAck {
        connection_epoch: u64_at(bytes, 0)?,
        sequence: u64_at(bytes, 8)?,
    };
    if ack.connection_epoch == 0 || ack.sequence == 0 {
        return Err(WmFileCodecError::Identity);
    }
    Ok(ack)
}
