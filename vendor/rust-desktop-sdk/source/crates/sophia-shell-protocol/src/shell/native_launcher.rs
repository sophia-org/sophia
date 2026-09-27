//! Wire-neutral typed model for the revision-7 native launcher records.
//!
//! Codec support is not role admission. Runtime must grant the
//! native-launcher capability explicitly and retain the exact catalog,
//! presented binding and issued event before authorizing effects.

mod records;
pub(crate) mod validation;

pub use records::*;

pub const SOPHIA_SHELL_NATIVE_LAUNCHER_REVISION: u16 = 7;
pub const SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER: u64 = 1 << 11;
pub const SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES: usize = 256;
