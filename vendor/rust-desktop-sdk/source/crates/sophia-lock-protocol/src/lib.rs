//! The `sophia_lock_fs_v1` file contract a Sophia lock provider speaks over
//! 9P2000.L, as `spec/sophia-lock-files-v1.kdl` declares it: record
//! envelopes, the Limits and Lock objects, negotiation, uploads, candidates,
//! frame demands and the character-free entry and chord events. A provider
//! only renders; nothing here carries a secret or an unlock. The Sophia
//! session serves this contract with the same codec.

mod byte_cursor;
mod codec_error;
pub mod lock_files;

pub use codec_error::BinaryCodecError;
