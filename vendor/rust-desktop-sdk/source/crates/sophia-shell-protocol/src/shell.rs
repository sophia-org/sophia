//! Wire-neutral typed shell protocol record model.
//!
//! The record modules below (`content`, `native_launcher`, `catalog_actions`,
//! ...) are plain data plus pure validation: no cursors, no `Wire` impls, no
//! frame message kinds. `encoding` holds their VALUE encodings — the little
//! endian cursor, the per-record byte shapes and the neutral
//! `encode_*_value`/`decode_*_value` entry points — which is likewise free
//! of frame message kinds and frame headers, so `crate::shell_files` can
//! encode these records without depending on the frame codec. The frame
//! codec depends on both; neither depends back on it, so the frame format
//! can be replaced without touching this module.

pub mod applications;
pub mod catalog_actions;
pub mod catalog_transaction;
pub mod content;
pub mod descriptor;
pub mod encoding;
pub mod hello;
pub mod indicators;
pub mod launcher;
pub mod metadata;
pub mod native_launcher;
pub mod reference;
pub mod tabs;

pub use applications::*;
pub use catalog_actions::*;
pub use catalog_transaction::*;
pub use content::*;
pub use descriptor::*;
pub use hello::*;
pub use indicators::*;
pub use launcher::*;
pub use metadata::*;
pub use native_launcher::*;
pub use reference::*;
pub use tabs::*;

/// A validation failure in the typed record model, independent of any codec.
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidRecord(pub &'static str);
