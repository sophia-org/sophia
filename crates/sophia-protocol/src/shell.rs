//! Wire-neutral typed shell IPC record model.
//!
//! Everything under this module is plain data plus pure validation: no
//! cursors, no `Wire` impls, no `IpcMessageKind`. The codec in `crate::ipc`
//! depends on these types; nothing here depends back on the codec, so the
//! codec can be replaced without touching this module.

pub mod applications;
pub mod catalog_actions;
pub mod catalog_transaction;
pub mod content;
pub mod hello;
pub mod indicators;
pub mod native_launcher;

pub use applications::*;
pub use catalog_actions::*;
pub use catalog_transaction::*;
pub use content::*;
pub use hello::*;
pub use indicators::*;
pub use native_launcher::*;

/// A validation failure in the typed record model, independent of any codec.
///
/// The wire codec maps this into its own error type at the boundary; nothing
/// in this module needs to know that mapping exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRecord(pub &'static str);

impl From<InvalidRecord> for crate::IpcCodecError {
    fn from(err: InvalidRecord) -> Self {
        crate::IpcCodecError::InvalidRecord(err.0)
    }
}
