//! Typed development-time conformance and promotion support.
//!
//! Production crates never depend on this crate. It consumes their public
//! records and vocabulary so development tools can validate sessions without
//! duplicating production authority or parsing schemas in shell.

pub mod desktop_comparison;
pub mod direct_scanout;
pub mod direct_scanout_archive;
pub mod direct_scanout_cost;
pub mod direct_scanout_cursor;
pub mod direct_scanout_gate;
pub mod direct_scanout_overlay;
pub mod private_instance;
pub mod profile;

pub mod dock;
pub mod panel;
