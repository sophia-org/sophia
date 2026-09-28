//! Passive data shared between Sophia processes.
//!
//! This crate deliberately has no compositor, X11, or IPC dependencies. It is
//! the executable form of the data model in `docs/dod.md`.

mod byte_cursor;
pub mod capacity;
mod codec_error;
pub mod cursor;
pub mod geometry;
pub mod ids;
pub mod inspection;
pub mod ipc;
pub mod packets;
pub mod policy_behavior;
pub mod policy_profile;
pub mod policy_scalars;
pub mod presentation;
pub mod table;
pub mod wm_files;
pub mod wm_records;
pub mod wm_rows;

pub use capacity::*;
pub use codec_error::BinaryCodecError;
pub use cursor::*;
pub use geometry::*;
pub use ids::*;
pub use ipc::*;
// Preserve the retiring socket facade's error type while the SDK exposes
// the same structural validators independently of a wire.
pub use ipc::{
    validate_shell_launcher_candidate, validate_shell_reference_candidate,
    validate_shell_shortcut_catalog, validate_shell_tab_snapshot,
};
pub use packets::*;
pub use policy_behavior::*;
pub use policy_profile::*;
pub use policy_scalars::*;
pub use presentation::*;
pub use shell::*;
// The shell record model and its file contract are the Rust desktop SDK's
// `sophia-shell-protocol`, which clients build against without Sophia.
pub use sophia_shell_protocol::{shell, shell_files};
pub use table::*;
pub use wm_records::*;
pub use wm_rows::*;
