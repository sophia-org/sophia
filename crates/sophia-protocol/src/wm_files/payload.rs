//! Shared typed-file byte checks; no transport state or policy authority.
use super::codec::validate_header;
use super::codec::{u32_at, u64_at};
use super::*;
use crate::IpcCodecError;
use crate::{OutputId, SurfaceId};

impl From<WmFileCodecError> for WmFilePayloadError {
    fn from(error: WmFileCodecError) -> Self {
        Self::Envelope(error)
    }
}

impl From<IpcCodecError> for WmFilePayloadError {
    fn from(error: IpcCodecError) -> Self {
        Self::Records(error)
    }
}

pub(super) fn require_capabilities(selected: u64, required: u64) -> Result<(), WmFilePayloadError> {
    let missing = required & !selected;
    if missing == 0 {
        Ok(())
    } else {
        Err(WmFilePayloadError::Capabilities { missing })
    }
}

pub(super) fn header_kind(
    header: WmFileHeader,
    kind: WmFileKind,
) -> Result<(), WmFilePayloadError> {
    validate_header(header)?;
    if header.kind != kind {
        return Err(WmFileCodecError::Kind.into());
    }
    Ok(())
}

pub(super) fn record(
    bytes: &[u8],
    kind: WmFileKind,
    prefix: usize,
) -> Result<WmFileRecord<'_>, WmFilePayloadError> {
    let record = decode_wm_file_record(bytes, wm_file_class(kind))?;
    header_kind(record.header, kind)?;
    if record.body.len() < prefix {
        return Err(WmFileCodecError::Length.into());
    }
    Ok(record)
}

pub(super) fn reserved(bytes: &[u8]) -> Result<(), WmFilePayloadError> {
    if bytes.iter().any(|value| *value != 0) {
        return Err(WmFileCodecError::Reserved.into());
    }
    Ok(())
}

pub(super) fn identity(values: &[u64]) -> Result<(), WmFilePayloadError> {
    if values.contains(&0) {
        return Err(WmFilePayloadError::Identity);
    }
    Ok(())
}

pub(super) fn same_epoch(header: WmFileHeader, epoch: u64) -> Result<(), WmFilePayloadError> {
    if header.connection_epoch != epoch {
        return Err(WmFilePayloadError::Identity);
    }
    Ok(())
}

pub(super) fn fixed_record(
    bytes: &[u8],
    kind: WmFileKind,
    size: usize,
) -> Result<WmFileRecord<'_>, WmFilePayloadError> {
    let value = record(bytes, kind, size)?;
    if value.body.len() != size {
        return Err(WmFileCodecError::Length.into());
    }
    Ok(value)
}

pub(super) fn surface(bytes: &[u8], at: usize) -> Result<SurfaceId, WmFilePayloadError> {
    Ok(SurfaceId::new(u32_at(bytes, at)?, u32_at(bytes, at + 4)?))
}

pub(super) fn optional_surface(
    bytes: &[u8],
    at: usize,
) -> Result<Option<SurfaceId>, WmFilePayloadError> {
    let value = surface(bytes, at)?;
    Ok(if value.index() == 0 && value.generation() == 0 {
        None
    } else {
        Some(value)
    })
}

pub(super) fn push_surface(bytes: &mut Vec<u8>, value: Option<SurfaceId>) {
    bytes.extend(value.map_or(0, |v| v.index()).to_le_bytes());
    bytes.extend(value.map_or(0, |v| v.generation()).to_le_bytes());
}

pub(super) fn outputs(bytes: &[u8], count: u16) -> Result<Vec<OutputId>, WmFilePayloadError> {
    let count = usize::from(count);
    if count == 0 || count > crate::POLICY_MAX_OUTPUTS || count.checked_mul(8) != Some(bytes.len())
    {
        return Err(WmFileCodecError::Length.into());
    }
    (0..count)
        .map(|i| Ok(OutputId::from_raw(u64_at(bytes, i * 8)?)))
        .collect()
}
