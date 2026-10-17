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
pub mod descriptor;
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
    fn put(&self, bytes: &mut impl crate::shell::encoding::ByteSink);
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError>;
}

pub(crate) trait ValidatedWire: Wire {
    fn validate_wire(&self) -> Result<(), ValueError>;
    fn validated_bytes(&self) -> Result<usize, ValueError> {
        self.validate_wire()?;
        let mut count = ByteCount::new();
        self.put(&mut count);
        count.finish()
    }
}

/// The same field writer either emits bytes or counts them. Counting never
/// allocates storage or maintains a second description of a record's layout.
pub(crate) trait ByteSink {
    fn extend_from_slice(&mut self, bytes: &[u8]);
    fn zeros(&mut self, count: usize);
}

impl ByteSink for Vec<u8> {
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }
    fn zeros(&mut self, count: usize) {
        self.resize(self.len() + count, 0);
    }
}

pub(crate) struct ByteCount(Option<usize>);
impl ByteCount {
    pub(crate) fn new() -> Self {
        Self(Some(0))
    }
    pub(crate) fn finish(self) -> Result<usize, ValueError> {
        self.0
            .ok_or(ValueError::InvalidRecord("encoded length overflow"))
    }
}
impl ByteSink for ByteCount {
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.zeros(bytes.len());
    }
    fn zeros(&mut self, count: usize) {
        self.0 = self.0.and_then(|length| length.checked_add(count));
    }
}

macro_rules! integer {
    ($ty:ty, $read:ident) => {
        impl Wire for $ty {
            fn put(&self, bytes: &mut impl crate::shell::encoding::ByteSink) {
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
            fn put(&self, bytes: &mut impl crate::shell::encoding::ByteSink) {
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
    fn put(&self, bytes: &mut impl crate::shell::encoding::ByteSink) {
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
/// Takes `count` rows into a vector of exactly that capacity (collecting a
/// fallible iterator would grow it by doubling instead). Callers bound
/// `count` first, so decoded memory follows the declared counts exactly.
pub(crate) fn rows<T: Wire>(cursor: &mut Cursor<'_>, count: usize) -> Result<Vec<T>, ValueError> {
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        rows.push(T::take(cursor)?);
    }
    Ok(rows)
}

/// Writes a length-prefixed, zero-padded fixed-width text field: a `u16`
/// length, a reserved `u16`, then `max` bytes of UTF-8 text padded with
/// zeros. Every whole-object text field the launcher, catalog and indicator
/// file records carry uses this one shape, so every row in a table is the
/// same width and a decoder never trusts a length to find the next field.
/// Callers validate the text against its own bound before calling this: `put`
/// cannot fail, so the length must already be within `max`.
pub(crate) fn put_text_padded(bytes: &mut impl ByteSink, text: &str, max: usize) {
    (text.len() as u16).put(bytes);
    0u16.put(bytes);
    bytes.extend_from_slice(text.as_bytes());
    bytes.zeros(max - text.len());
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
