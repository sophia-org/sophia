use crate::RecordError;
/// Largest version-1 api record, including a 20-digit epoch and newline.
pub const API_MAX_BYTES: usize = 101;
pub const STATUS_MAX_BYTES: usize = 43;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestCapability {
    Unavailable,
    OutputCaptureV1,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Api {
    pub epoch: u64,
    pub request: RequestCapability,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferState {
    Absent,
    Ready,
    Revoked,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub request: RequestCapability,
    pub transfer: TransferState,
}
fn capability(s: &str) -> Result<RequestCapability, RecordError> {
    match s {
        "unavailable" => Ok(RequestCapability::Unavailable),
        "output-capture-v1" => Ok(RequestCapability::OutputCaptureV1),
        _ => Err(RecordError),
    }
}
/// Exact version-1 snapshot. No allocation or authority; the caller establishes EOF.
pub fn decode_api(bytes: &[u8]) -> Result<Api, RecordError> {
    if bytes.len() > API_MAX_BYTES {
        return Err(RecordError);
    }
    let s = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let s = s
        .strip_prefix("sophia-portal-client version=1 epoch=")
        .and_then(|s| s.strip_suffix(" fd_transfer=none\n"))
        .ok_or(RecordError)?;
    let (epoch, request) = s.split_once(" request=").ok_or(RecordError)?;
    if epoch.is_empty() || epoch.starts_with('0') || !epoch.bytes().all(|b| b.is_ascii_digit()) {
        return Err(RecordError);
    }
    Ok(Api {
        epoch: epoch.parse().map_err(|_| RecordError)?,
        request: capability(request)?,
    })
}
/// Historical snapshot, not authorization to read transfer/frame objects.
pub fn decode_status(bytes: &[u8]) -> Result<Status, RecordError> {
    if bytes.len() > STATUS_MAX_BYTES {
        return Err(RecordError);
    }
    let s = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let s = s
        .strip_prefix("request=")
        .and_then(|s| s.strip_suffix('\n'))
        .ok_or(RecordError)?;
    let (request, transfer) = s.split_once(" transfer=").ok_or(RecordError)?;
    let transfer = match transfer {
        "absent" => TransferState::Absent,
        "ready" => TransferState::Ready,
        "revoked" => TransferState::Revoked,
        _ => return Err(RecordError),
    };
    Ok(Status {
        request: capability(request)?,
        transfer,
    })
}
