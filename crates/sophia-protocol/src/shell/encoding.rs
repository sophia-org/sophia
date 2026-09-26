//! Wire-neutral value encodings for the typed shell record model in
//! `crate::shell`.
//!
//! Each function encodes or decodes one record value: little-endian fields,
//! reserved padding, row counts and rows. Frames, message kinds, transfers
//! and transaction rules belong to the codecs that carry these values.

use crate::InvalidRecord;
use crate::byte_cursor::{Cursor, CursorError};

pub mod catalog_actions;
pub mod content;
pub mod native_launcher;

/// A value-encoding failure, independent of any codec that carries values.
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
