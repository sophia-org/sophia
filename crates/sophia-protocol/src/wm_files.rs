//! Bounded binary objects for the WM file interface. These codecs own byte
//! shape only; Session still owns admission, phases, submission and settlement.

mod codec;
mod records;

pub use codec::*;
pub use records::*;
