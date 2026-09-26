//! Passive data shared between Sophia processes.
//!
//! This crate deliberately has no compositor, X11, or IPC dependencies. It is
//! the executable form of the data model in `docs/dod.md`.

pub mod capacity;
pub mod cursor;
pub mod geometry;
pub mod ids;
pub mod ipc;
pub mod packets;
pub mod policy_behavior;
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
pub use presentation::*;
pub use table::*;
