//! The Sophia desktop shell's typed records and the `sophia_shell_fs_v1` file
//! contract that carries them over 9P2000.L.
//!
//! [`shell`] is the wire-neutral record model with its value encodings;
//! [`shell_files`] is the file contract's envelopes, records and objects, as
//! `spec/sophia-shell-files-v1.kdl` declares them. Neither names a socket
//! frame. The Sophia session serves this contract with the same types.

mod byte_cursor;
pub mod shell;
pub mod shell_files;

pub use shell::*;
pub use sophia_desktop_ids::{OutputId, TransactionId};
