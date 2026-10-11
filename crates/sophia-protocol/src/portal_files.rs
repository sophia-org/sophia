//! Portal transfer file records for t315, bound to
//! `protocol/sophia-portal-files-v1.kdl`. Parsing checks byte shape and the
//! value rules the contract states; the portal owner checks admission, the
//! admitted namespace pair, lifecycle, custody and budgets. A record grants
//! nothing: requester permission bits and the requested decision are absent
//! from the wire, and the owner derives both.
mod bodies;
mod envelope;
mod records;

pub use bodies::*;
pub use envelope::*;
pub use records::*;

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
