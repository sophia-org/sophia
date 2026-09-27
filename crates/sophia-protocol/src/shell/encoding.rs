//! Wire-neutral value encodings for the typed shell record model in
//! `crate::shell`.
//!
//! Each function encodes or decodes one record value: little-endian fields,
//! reserved padding, row counts and rows. Frames, message kinds, transfers
//! and transaction rules belong to the codecs that carry these values.

use crate::InvalidRecord;
use crate::byte_cursor::{Cursor, CursorError};

pub mod applications;
pub mod catalog_actions;
pub mod content;
pub mod indicators;
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

impl Wire for crate::OutputId {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.raw().put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(crate::OutputId::from_raw(u64::take(cursor)?))
    }
}

/// Reads a table length prefix, bounded by `maximum`. Shared by every whole
/// object/candidate encoding under this module.
pub(crate) fn table_count(cursor: &mut Cursor<'_>, maximum: usize) -> Result<usize, ValueError> {
    let value = usize::from(u16::take(cursor)?);
    if value > maximum {
        return Err(ValueError::CountTooLarge {
            count: value,
            max: maximum,
        });
    }
    Ok(value)
}

/// Reads exactly `count` rows of `T`, each row's own shape doing its own
/// bounds checking.
pub(crate) fn rows<T: Wire>(cursor: &mut Cursor<'_>, count: usize) -> Result<Vec<T>, ValueError> {
    (0..count).map(|_| T::take(cursor)).collect()
}

/// Writes a length-prefixed, zero-padded fixed-width text field: a `u16`
/// length, a reserved `u16`, then `max` bytes of UTF-8 text padded with
/// zeros. Every whole-object text field the launcher, catalog and indicator
/// file records carry uses this one shape, so every row in a table is the
/// same width and a decoder never trusts a length to find the next field.
/// Callers validate the text against its own bound before calling this: `put`
/// cannot fail, so the length must already be within `max`.
pub(crate) fn put_text_padded(bytes: &mut Vec<u8>, text: &str, max: usize) {
    (text.len() as u16).put(bytes);
    0u16.put(bytes);
    let mut padded = vec![0u8; max];
    padded[..text.len()].copy_from_slice(text.as_bytes());
    bytes.extend_from_slice(&padded);
}

pub(crate) fn take_text_padded(cursor: &mut Cursor<'_>, max: usize) -> Result<String, ValueError> {
    let len = usize::from(u16::take(cursor)?);
    reserved::<u16>(cursor)?;
    if len > max {
        return Err(ValueError::CountTooLarge { count: len, max });
    }
    let bytes = cursor.slice(max)?;
    if bytes[len..].iter().any(|b| *b != 0) {
        return Err(ValueError::ReservedNonZero(1));
    }
    std::str::from_utf8(&bytes[..len])
        .map(str::to_owned)
        .map_err(|_| ValueError::InvalidRecord("shell file text"))
}
