use super::codec::validate_header;
use super::*;
use crate::IpcCodecError;

impl From<ShellFileCodecError> for ShellFilePayloadError {
    fn from(error: ShellFileCodecError) -> Self {
        Self::Envelope(error)
    }
}

impl From<IpcCodecError> for ShellFilePayloadError {
    fn from(error: IpcCodecError) -> Self {
        Self::Records(error)
    }
}

pub(super) fn header_kind(
    header: ShellFileHeader,
    kind: ShellFileKind,
) -> Result<(), ShellFilePayloadError> {
    validate_header(header)?;
    if header.kind != kind {
        return Err(ShellFileCodecError::Kind.into());
    }
    Ok(())
}

pub(super) fn record(
    bytes: &[u8],
    kind: ShellFileKind,
    prefix: usize,
) -> Result<ShellFileRecord<'_>, ShellFilePayloadError> {
    let record = decode_shell_file_record(bytes, shell_file_class(kind))?;
    header_kind(record.header, kind)?;
    if record.body.len() < prefix {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(record)
}

pub(super) fn reserved(bytes: &[u8]) -> Result<(), ShellFilePayloadError> {
    if bytes.iter().any(|value| *value != 0) {
        return Err(ShellFileCodecError::Reserved.into());
    }
    Ok(())
}

pub(super) fn same_epoch(header: ShellFileHeader, epoch: u64) -> Result<(), ShellFilePayloadError> {
    if header.connection_epoch != epoch {
        return Err(ShellFilePayloadError::Identity);
    }
    Ok(())
}

pub(super) fn fixed_record(
    bytes: &[u8],
    kind: ShellFileKind,
    size: usize,
) -> Result<ShellFileRecord<'_>, ShellFilePayloadError> {
    let value = record(bytes, kind, size)?;
    if value.body.len() != size {
        return Err(ShellFileCodecError::Length.into());
    }
    Ok(value)
}
