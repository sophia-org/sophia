//! Bounded binary objects for the WM file interface. These codecs own byte
//! shape only; Session still owns admission, phases, submission and settlement.

mod arrays;
mod codec;
mod records;

pub use arrays::*;
pub use codec::*;
pub use records::*;
