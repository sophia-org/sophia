//! Native launcher wire codec. Encodes and decodes the typed records defined
//! in `crate::shell::native_launcher`; the byte shape of each record lives
//! in `crate::shell::encoding::native_launcher`, and validation lives with
//! the types themselves.

mod codec;

pub use codec::{decode_shell_native_launcher_frame, encode_shell_native_launcher_frame};
