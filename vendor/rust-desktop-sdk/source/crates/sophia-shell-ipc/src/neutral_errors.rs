//! Conversions from the wire-neutral encoding errors into this codec's error.
//! They live here so neutral modules never name IPC types; they go with IPC.
use crate::InvalidRecord;
use crate::byte_cursor::CursorError;
use crate::shell::encoding::ValueError;

impl From<CursorError> for super::IpcCodecError {
    fn from(err: CursorError) -> Self {
        match err {
            CursorError::Truncated => super::IpcCodecError::Truncated,
            CursorError::TrailingBytes(remaining) => super::IpcCodecError::TrailingBytes(remaining),
        }
    }
}

impl From<InvalidRecord> for super::IpcCodecError {
    fn from(err: InvalidRecord) -> Self {
        super::IpcCodecError::InvalidRecord(err.0)
    }
}

impl From<ValueError> for super::IpcCodecError {
    fn from(err: ValueError) -> Self {
        match err {
            ValueError::Truncated => super::IpcCodecError::Truncated,
            ValueError::TrailingBytes(remaining) => super::IpcCodecError::TrailingBytes(remaining),
            ValueError::ReservedNonZero(word) => super::IpcCodecError::ReservedNonZero(word),
            ValueError::CountTooLarge { count, max } => {
                super::IpcCodecError::CountTooLarge { count, max }
            }
            ValueError::InvalidEnum { field, value } => {
                super::IpcCodecError::InvalidEnum { field, value }
            }
            ValueError::InvalidRecord(field) => super::IpcCodecError::InvalidRecord(field),
        }
    }
}
