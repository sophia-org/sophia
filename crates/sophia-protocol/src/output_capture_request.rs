//! Passive output-only request records. Decoding never creates a grant.
use crate::OutputId;

pub const CAPTURE_REQUEST_LEN: usize = 24;
pub const CAPTURE_STATUS_LEN: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputCaptureRequest {
    pub serial: u64,
    pub output: OutputId,
}
impl OutputCaptureRequest {
    pub fn encode(self) -> Option<[u8; CAPTURE_REQUEST_LEN]> {
        if self.serial == 0 || !self.output.is_valid() {
            return None;
        }
        let mut bytes = [0; CAPTURE_REQUEST_LEN];
        for (value, bytes) in [1, self.serial, self.output.raw()]
            .into_iter()
            .zip(bytes.chunks_exact_mut(8))
        {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        Some(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CAPTURE_REQUEST_LEN {
            return None;
        }
        let word = |start| u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap());
        if word(0) != 1 {
            return None;
        }
        let request = Self {
            serial: word(8),
            output: OutputId::from_raw(word(16)),
        };
        request.encode()?;
        Some(request)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u64)]
pub enum OutputCaptureState {
    Idle = 0,
    Queued = 1,
    Capturing = 2,
    Ready = 3,
    Denied = 4,
    Unavailable = 5,
    Pressure = 6,
    Cancelled = 7,
}
impl OutputCaptureState {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Idle | Self::Queued | Self::Capturing)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputCaptureStatus {
    pub request: Option<OutputCaptureRequest>,
    pub state: OutputCaptureState,
}
impl OutputCaptureStatus {
    pub fn encode(self) -> Option<[u8; CAPTURE_STATUS_LEN]> {
        let mut bytes = [0; CAPTURE_STATUS_LEN];
        match self.request {
            None if self.state == OutputCaptureState::Idle => {
                bytes[..8].copy_from_slice(&1u64.to_le_bytes())
            }
            Some(request) if self.state != OutputCaptureState::Idle => {
                bytes[..24].copy_from_slice(&request.encode()?)
            }
            _ => return None,
        }
        bytes[24..].copy_from_slice(&(self.state as u64).to_le_bytes());
        Some(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CAPTURE_STATUS_LEN {
            return None;
        }
        let state = match u64::from_le_bytes(bytes[24..].try_into().ok()?) {
            0 => OutputCaptureState::Idle,
            1 => OutputCaptureState::Queued,
            2 => OutputCaptureState::Capturing,
            3 => OutputCaptureState::Ready,
            4 => OutputCaptureState::Denied,
            5 => OutputCaptureState::Unavailable,
            6 => OutputCaptureState::Pressure,
            7 => OutputCaptureState::Cancelled,
            _ => return None,
        };
        let status = Self {
            request: if state == OutputCaptureState::Idle {
                None
            } else {
                Some(OutputCaptureRequest::decode(&bytes[..24])?)
            },
            state,
        };
        (status.encode()?.as_slice() == bytes).then_some(status)
    }
}
