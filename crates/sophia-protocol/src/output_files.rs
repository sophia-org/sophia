//! Native output-role file records. This is the codec foundation for t253;
//! the export and its custody rules are not yet implemented or negotiated.
//! No socket envelope or socket codec is used here. Parsing checks byte shape;
//! the output owner checks capabilities, epochs, replay and topology validity.
mod controls;
mod envelope;
mod events;
mod proposal;
mod topology;

pub use controls::*;
pub use envelope::*;
pub use events::*;
pub use proposal::*;
pub use topology::*;

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
