//! Passive data shared between Sophia processes.
//!
//! This crate deliberately has no compositor, X11, or IPC dependencies. It is
//! the executable form of the data model in `docs/dod.md`.

mod byte_cursor;
pub mod capacity;
pub mod cursor;
pub mod geometry;
pub mod ids;
pub mod inspection;
pub mod ipc;
pub mod packets;
pub mod policy_behavior;
pub mod policy_profile;
pub mod presentation;
pub mod table;
pub mod wm_files;

pub use capacity::*;
pub use cursor::*;
pub use geometry::*;
pub use ids::*;
pub use ipc::*;
pub use packets::*;
pub use policy_behavior::*;
pub use policy_profile::*;
pub use presentation::*;
pub use shell::*;
// The shell record model and its file contract are the Rust desktop SDK's
// `sophia-shell-protocol`, which clients build against without Sophia.
pub use sophia_shell_protocol::{shell, shell_files};
pub use table::*;
