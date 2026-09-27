//! Shell content wire codec. Encodes and decodes the typed records defined
//! in `crate::shell::content`; the byte shape of each record lives in
//! `crate::shell::encoding::content`, and validation lives with the types
//! themselves.

mod codec;

pub use codec::{decode_shell_content_frame, encode_shell_content_frame};
