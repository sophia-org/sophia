//! Turning geometry into spans.
//!
//! Portions derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes):
//! `crates/yserver/src/kms/render/stroke.rs` and
//! `crates/yserver/src/kms/backend.rs`, the chord-step arc approximation that
//! density replay still strokes. The polygon fill, filled arcs, wide lines,
//! thin lines and arcs, and wide arcs are ports of the X server's `mi`
//! (`polygon.rs`, `fill_arc.rs`, `wide_line.rs`, `zero_line.rs`,
//! `wide_arc.rs`).
//!
//! Everything here is pure: geometry in, rectangles out. Nothing touches a
//! pixel. The caller feeds the rectangles to the ordinary fill, so every
//! primitive inherits the graphics context's clip, raster function and plane
//! mask without knowing they exist.
//!
//! yserver stores `fill_rule` and never reads it, so its polygons are always
//! even-odd; `mi`'s filler, and so this one, honours both rules.

pub(crate) mod arc;
pub(crate) mod fill_arc;
pub(crate) mod polygon;
pub(crate) mod wide_arc;
pub(crate) mod wide_line;
pub(crate) mod zero_line;
