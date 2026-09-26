//! Shell content wire codec. Encodes and decodes the typed records defined
//! in `crate::shell::content`; validation lives with those types.

mod codec;
pub(super) mod fields;

pub use codec::{decode_shell_content_frame, encode_shell_content_frame};
pub(crate) use codec::{decode_shell_content_payload, encode_shell_content_payload};
