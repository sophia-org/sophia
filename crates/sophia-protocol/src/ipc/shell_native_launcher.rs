//! Native launcher wire codec. Encodes and decodes the typed records defined
//! in `crate::shell::native_launcher`; validation lives with those types.

mod codec;
mod fields;

pub use codec::{decode_shell_native_launcher_frame, encode_shell_native_launcher_frame};
