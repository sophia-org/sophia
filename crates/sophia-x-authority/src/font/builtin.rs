//! The faces this authority carries itself.
//!
//! The `built-ins` element is the last one searched and the only one that is
//! always present. It exists so the authority serves text with no font
//! directory configured at all, and so the pixel proofs have a face that
//! cannot change underneath them when a host package is updated.
//!
//! XLibre's libXfont embeds `fixed` and `cursor` the same way and for the same
//! reason.

use super::fixed_6x13::X_FIXED_6X13_GLYPHS;
use super::metrics::{XCharInfo, XFontMetrics};
use super::pcf::{XGlyph, XLoadedFont};
use super::{
    X_FIXED_6X13_ASCENT, X_FIXED_6X13_CANONICAL_NAME, X_FIXED_6X13_DESCENT,
    X_FIXED_6X13_UNICODE_NAME, X_FIXED_6X13_WIDTH,
};

/// The names the built-in element answers to.
///
/// `cursor` and `nil2` are lifecycle-only compatibility names that real xterm
/// opens at startup; they retain the fixed face so a FONTABLE exists without
/// importing a host dependency. Cursor glyph shapes are authority metadata and
/// are not text-rasterised.
pub const X_BUILTIN_FONT_NAMES: &[&str] = &[
    "fixed",
    "6x13",
    "cursor",
    "nil2",
    X_FIXED_6X13_CANONICAL_NAME,
    X_FIXED_6X13_UNICODE_NAME,
];

/// Whether the built-in element publishes this name.
pub fn provides(name: &str) -> bool {
    X_BUILTIN_FONT_NAMES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

/// The built-in 6x13 face as a loaded font.
///
/// A single-byte linear face: `min_byte1` and `max_byte1` are both zero, so a
/// client is told plainly that this face is indexed by one byte. That is what
/// it is, and claiming a two-byte matrix here is precisely the mistake that
/// made xterm draw Latin-1 glyphs for Unicode text.
pub fn fixed_6x13() -> XLoadedFont {
    let bounds = XCharInfo {
        left_side_bearing: 0,
        right_side_bearing: i16::try_from(X_FIXED_6X13_WIDTH).unwrap_or(6),
        character_width: i16::try_from(X_FIXED_6X13_WIDTH).unwrap_or(6),
        ascent: i16::try_from(X_FIXED_6X13_ASCENT).unwrap_or(11),
        descent: i16::try_from(X_FIXED_6X13_DESCENT).unwrap_or(2),
        attributes: 0,
    };
    let width = u16::try_from(X_FIXED_6X13_WIDTH).unwrap_or(6);
    let height = u16::try_from(X_FIXED_6X13_ASCENT + X_FIXED_6X13_DESCENT).unwrap_or(13);
    let glyphs = X_FIXED_6X13_GLYPHS
        .iter()
        .map(|rows| XGlyph {
            width,
            height,
            row_bytes: 1,
            rows: rows.to_vec(),
        })
        .collect();
    XLoadedFont {
        metrics: XFontMetrics {
            min_bounds: bounds,
            max_bounds: bounds,
            // Every character measures alike, which tells QueryFont it owes no
            // per-character array.
            ink_min_bounds: bounds,
            ink_max_bounds: bounds,
            min_char_or_byte2: 0,
            max_char_or_byte2: 255,
            min_byte1: 0,
            max_byte1: 0,
            default_char: 0,
            all_chars_exist: true,
            draw_direction: 0,
            font_ascent: bounds.ascent,
            font_descent: bounds.descent,
            char_infos: vec![bounds; 256],
            properties: Vec::new(),
        },
        glyphs,
    }
}
