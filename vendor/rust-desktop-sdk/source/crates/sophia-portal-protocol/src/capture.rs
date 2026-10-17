use crate::RecordError;
pub const CAPTURE_BYTES: usize = 24;
pub const CAPTURE_STATUS_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureRequest {
    pub serial: u64,
    pub output: u64,
}
/// Produces one complete write. Serial ordering and admission belong to the client.
pub fn encode_capture(request: CaptureRequest) -> Result<[u8; CAPTURE_BYTES], RecordError> {
    if request.serial == 0 || request.output == 0 {
        return Err(RecordError);
    }
    let mut bytes = [0; CAPTURE_BYTES];
    for (chunk, value) in bytes
        .chunks_exact_mut(8)
        .zip([1, request.serial, request.output])
    {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureState {
    Idle,
    Queued,
    Capturing,
    Ready,
    Denied,
    Unavailable,
    Pressure,
    Cancelled,
}

/// Historical information, not a grant to fetch a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureStatus {
    pub serial: u64,
    pub output: u64,
    pub state: CaptureState,
}
pub fn decode_capture_status(bytes: &[u8]) -> Result<CaptureStatus, RecordError> {
    if bytes.len() != CAPTURE_STATUS_BYTES {
        return Err(RecordError);
    }
    let mut f = [0u64; 4];
    for (value, chunk) in f.iter_mut().zip(bytes.chunks_exact(8)) {
        *value = u64::from_le_bytes(chunk.try_into().map_err(|_| RecordError)?);
    }
    if f[0] != 1 {
        return Err(RecordError);
    }
    let state = match f[3] {
        0 => CaptureState::Idle,
        1 => CaptureState::Queued,
        2 => CaptureState::Capturing,
        3 => CaptureState::Ready,
        4 => CaptureState::Denied,
        5 => CaptureState::Unavailable,
        6 => CaptureState::Pressure,
        7 => CaptureState::Cancelled,
        _ => return Err(RecordError),
    };
    if if state == CaptureState::Idle {
        f[1] != 0 || f[2] != 0
    } else {
        f[1] == 0 || f[2] == 0
    } {
        return Err(RecordError);
    }
    Ok(CaptureStatus {
        serial: f[1],
        output: f[2],
        state,
    })
}
