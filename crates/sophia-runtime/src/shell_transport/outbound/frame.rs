use super::ShellTransportError;
use sophia_protocol::shell_files::*;

fn header(record: &ShellFrameRecord) -> Result<ShellFileHeader, ShellTransportError> {
    let grant = match record {
        ShellFrameRecord::Opened { key, .. } | ShellFrameRecord::Closed { key, .. } => key.grant,
        ShellFrameRecord::Refused { grant, .. } => *grant,
        ShellFrameRecord::Outcome { outcome, .. } => outcome.grant,
        _ => return Err(ShellTransportError::WrongContentRecord),
    };
    // The journal assigns the real sequence when it takes this body.
    Ok(ShellFileHeader {
        kind: record.kind(),
        connection_epoch: grant.connection_epoch,
        submission_id: 0,
        sequence: 1,
    })
}

pub(super) fn body_len(record: &ShellFrameRecord) -> Result<usize, ShellTransportError> {
    shell_frame_record_bytes(header(record)?, record, SHELL_FRAME_REVISION)
        .map(|bytes| bytes - SHELL_FILE_HEADER_BYTES)
        .map_err(|_| ShellTransportError::WrongContentRecord)
}

pub(super) fn native(
    record: &ShellFrameRecord,
) -> Result<(ShellFileKind, Vec<u8>), ShellTransportError> {
    let mut bytes = encode_shell_frame_record(header(record)?, record, SHELL_FRAME_REVISION)
        .map_err(|_| ShellTransportError::WrongContentRecord)?;
    bytes.drain(..SHELL_FILE_HEADER_BYTES);
    Ok((record.kind(), bytes))
}
