//! Typed development-time conformance and promotion support.
//!
//! Production crates never depend on this crate. It consumes their public
//! records and vocabulary so development tools can validate sessions without
//! duplicating production authority or parsing schemas in shell.
//!
//! The package also builds Sophia's conformance hosts and the desktop profile
//! probe as binaries, so external integration repositories can install them
//! from a pinned revision. Their sources remain the examples of
//! `sophia-runtime` and `sophia-config`, reached through `../` target paths.
//! Installation therefore works only from an exact git checkout (for example
//! `cargo install --git <sophia> --rev <sha> --locked sophia-conformance --bin
//! <host>`); the package cannot be published or used through `cargo vendor`,
//! whose versioned directory layout breaks those paths.

pub mod direct_scanout;
pub mod direct_scanout_archive;
pub mod direct_scanout_cost;
pub mod direct_scanout_cursor;
pub mod direct_scanout_overlay;
pub mod private_instance;
pub mod profile;
pub mod record;
