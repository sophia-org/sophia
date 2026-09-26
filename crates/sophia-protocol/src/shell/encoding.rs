//! Wire-neutral value encodings for the typed shell record model in
//! `crate::shell`.
//!
//! Everything here encodes or decodes ONE record's byte body: little-endian
//! integers, reserved padding, table counts, and per-variant record shapes.
//! It never touches frame headers, the frame codec's message-kind enum,
//! multi-frame transfer assembly, or transaction-id rules — those stay with
//! the frame codec, which maps its own message kinds onto the neutral
//! `*ValueKind` enums here and wraps [`ValueError`] into its own error type
//! at the boundary (see the bridge `impl` at the bottom of this file).

use crate::InvalidRecord;
use crate::byte_cursor::{Cursor, CursorError};

pub mod catalog_actions;
pub mod content;
pub mod native_launcher;

/// A value-encoding failure, independent of frames, message kinds or
/// transaction-id rules. The frame codec maps this into the identical
/// error value every existing caller already expects (see the bridge
/// `impl` below).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueError {
    Truncated,
    TrailingBytes(usize),
    ReservedNonZero(u32),
    CountTooLarge { count: usize, max: usize },
    InvalidEnum { field: &'static str, value: u32 },
    InvalidRecord(&'static str),
}

impl From<CursorError> for ValueError {
    fn from(err: CursorError) -> Self {
        match err {
            CursorError::Truncated => ValueError::Truncated,
            CursorError::TrailingBytes(remaining) => ValueError::TrailingBytes(remaining),
        }
    }
}

impl From<InvalidRecord> for ValueError {
    fn from(err: InvalidRecord) -> Self {
        ValueError::InvalidRecord(err.0)
    }
}

impl From<ValueError> for crate::IpcCodecError {
    fn from(err: ValueError) -> Self {
        match err {
            ValueError::Truncated => crate::IpcCodecError::Truncated,
            ValueError::TrailingBytes(remaining) => crate::IpcCodecError::TrailingBytes(remaining),
            ValueError::ReservedNonZero(word) => crate::IpcCodecError::ReservedNonZero(word),
            ValueError::CountTooLarge { count, max } => {
                crate::IpcCodecError::CountTooLarge { count, max }
            }
            ValueError::InvalidEnum { field, value } => {
                crate::IpcCodecError::InvalidEnum { field, value }
            }
            ValueError::InvalidRecord(field) => crate::IpcCodecError::InvalidRecord(field),
        }
    }
}

/// One record family's field-level wire shape: how to write its bytes and
/// how to read them back. No frame, kind or transaction knowledge lives
/// here.
pub(crate) trait Wire: Sized {
    fn put(&self, bytes: &mut Vec<u8>);
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError>;
}

macro_rules! integer {
    ($ty:ty, $read:ident) => {
        impl Wire for $ty {
            fn put(&self, bytes: &mut Vec<u8>) {
                bytes.extend_from_slice(&self.to_le_bytes());
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
                Ok(cursor.$read()? as Self)
            }
        }
    };
}
integer!(u16, u16);
integer!(u32, u32);
integer!(u64, u64);
integer!(i16, u16);
integer!(i32, i32);

/// Reserved fields exist only on the wire, never as mutable record state.
pub(crate) fn reserved<T: Wire + Default + PartialEq>(
    cursor: &mut Cursor<'_>,
) -> Result<(), ValueError> {
    if T::take(cursor)? != T::default() {
        return Err(ValueError::ReservedNonZero(1));
    }
    Ok(())
}

macro_rules! fields {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        impl Wire for $name {
            fn put(&self, bytes: &mut Vec<u8>) {
                $(self.$field.put(bytes);)*
            }
            fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
                Ok(Self { $($field: <$ty>::take(cursor)?),* })
            }
        }
    };
}

pub(crate) use fields;
