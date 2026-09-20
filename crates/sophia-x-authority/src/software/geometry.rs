//! Turning geometry into spans.
//!
//! Portions derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes):
//! `crates/yserver/src/kms/render/stroke.rs` and
//! `crates/yserver/src/kms/backend.rs`, the chord-step arc approximation, the
//! dash walk, and the scanline polygon fill.
//!
//! Everything here is pure: geometry in, rectangles out. Nothing touches a
//! pixel. The caller feeds the rectangles to the ordinary fill, so every
//! primitive inherits the graphics context's clip, raster function and plane
//! mask without knowing they exist.
//!
//! One deliberate divergence from the source: the winding fill rule is
//! implemented here. yserver stores `fill_rule`, copies it, threads it through
//! its draw state, and then never reads it, so a self-intersecting polygon is
//! always even-odd there.

pub(crate) mod arc;
pub(crate) mod dash;
pub(crate) mod polygon;
