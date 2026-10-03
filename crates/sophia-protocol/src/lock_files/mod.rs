//! Native lock-provider file records for t294, bound to
//! `protocol/sophia-lock-files-v1.kdl`. Parsing checks byte shape; the lock
//! owner checks capabilities, epochs, allocations, chords and budgets. These
//! records carry no characters and no unlock authority in either direction.
mod bodies;
mod envelope;
mod transfer;

pub use bodies::*;
pub use envelope::*;
pub use transfer::*;

use crate::BinaryCodecError;
use crate::byte_cursor::Cursor;

fn invalid(field: &'static str) -> BinaryCodecError {
    BinaryCodecError::InvalidRecord(field)
}

fn reserved(cursor: &mut Cursor<'_>, bytes: usize) -> Result<(), BinaryCodecError> {
    if cursor.slice(bytes)?.iter().any(|byte| *byte != 0) {
        return Err(invalid("reserved"));
    }
    Ok(())
}

fn zero_u16(cursor: &mut Cursor<'_>) -> Result<(), BinaryCodecError> {
    reserved(cursor, 2)
}

fn nonzero(value: u64, field: &'static str) -> Result<u64, BinaryCodecError> {
    if value == 0 {
        return Err(invalid(field));
    }
    Ok(value)
}

fn range(value: u64, low: u64, high: u64, field: &'static str) -> Result<(), BinaryCodecError> {
    (low..=high)
        .contains(&value)
        .then_some(())
        .ok_or_else(|| invalid(field))
}
