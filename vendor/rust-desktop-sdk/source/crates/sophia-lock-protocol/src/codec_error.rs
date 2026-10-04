//! Structural and value errors shared by binary record codecs. Transport
//! admission, sequencing and custody errors belong to their respective owners.
use crate::byte_cursor::CursorError;

/// A binary envelope, record or scalar failed validation. Variants describe
/// byte shape and values, without choosing a socket or file transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BinaryCodecError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u16),
    UnknownMessageKind(u16),
    PayloadTooLarge(usize),
    ReservedNonZero(u32),
    TrailingBytes(usize),
    CountTooLarge {
        count: usize,
        max: usize,
    },
    TextTooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
    FieldTooLarge {
        field: &'static str,
        len: usize,
        max: usize,
    },
    InvalidTransaction(u64),
    InvalidProfileIdentity(&'static str),
    InvalidUtf8 {
        field: &'static str,
    },
    InvalidEnum {
        field: &'static str,
        value: u32,
    },
    InvalidBool {
        field: &'static str,
        value: u8,
    },
    InvalidRecord(&'static str),
}

impl From<CursorError> for BinaryCodecError {
    fn from(err: CursorError) -> Self {
        match err {
            CursorError::Truncated => BinaryCodecError::Truncated,
            CursorError::TrailingBytes(remaining) => BinaryCodecError::TrailingBytes(remaining),
        }
    }
}
