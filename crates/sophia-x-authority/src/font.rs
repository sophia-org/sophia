// This bitmap is derived from X.Org's public-domain misc 6x13 ISO-8859-1 font.
// Upstream license: "Public domain font. Share and enjoy."

pub const X_FIXED_6X13_WIDTH: i32 = 6;
pub const X_FIXED_6X13_ASCENT: i32 = 11;
pub const X_FIXED_6X13_DESCENT: i32 = 2;
pub const X_FIXED_6X13_HEIGHT: i32 = X_FIXED_6X13_ASCENT + X_FIXED_6X13_DESCENT;
pub const X_FIXED_6X13_CANONICAL_NAME: &str =
    "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
/// The same face under the Unicode registry.
///
/// An XLFD's last two fields are its charset registry and encoding, not a
/// different typeface: `iso8859-1` and `iso10646-1` name one 6x13 bitmap
/// indexed two ways. xterm asks for this spelling whenever it is in UTF-8
/// mode, which on a UTF-8 locale is by default, so refusing it refuses the
/// terminal rather than the encoding.
///
/// Accepting it is not a claim to cover the Unicode repertoire. Sophia
/// rasterizes one fixed face either way, and a glyph outside it falls back
/// exactly as it already does under the Latin-1 name.
pub const X_FIXED_6X13_UNICODE_NAME: &str =
    "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso10646-1";

mod fixed_6x13;
pub mod metrics;
pub mod pcf;

use fixed_6x13::X_FIXED_6X13_GLYPHS;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum XFontFace {
    #[default]
    Fixed6x13,
}

impl XFontFace {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        name.eq_ignore_ascii_case("fixed")
            .then_some(Self::Fixed6x13)
            // The core cursor font is accepted for CreateGlyphCursor. Cursor
            // glyphs are authority metadata today and are not text-rasterized.
            .or_else(|| {
                name.eq_ignore_ascii_case("cursor")
                    .then_some(Self::Fixed6x13)
            })
            // xterm opens the standard X.Org `nil2` compatibility face for
            // its tiny-font and icon slots even when `-fn 6x13` selects the
            // terminal face. Sophia does not expose a font-menu raster path,
            // so retaining the fixed face here preserves FONTABLE lifetime
            // without introducing host-font dependence.
            .or_else(|| name.eq_ignore_ascii_case("nil2").then_some(Self::Fixed6x13))
            .or_else(|| name.eq_ignore_ascii_case("6x13").then_some(Self::Fixed6x13))
            .or_else(|| {
                name.eq_ignore_ascii_case(X_FIXED_6X13_CANONICAL_NAME)
                    .then_some(Self::Fixed6x13)
            })
            .or_else(|| {
                name.eq_ignore_ascii_case(X_FIXED_6X13_UNICODE_NAME)
                    .then_some(Self::Fixed6x13)
            })
    }

    pub(crate) const fn width(self) -> i32 {
        X_FIXED_6X13_WIDTH
    }

    pub(crate) const fn ascent(self) -> i32 {
        X_FIXED_6X13_ASCENT
    }

    pub(crate) const fn descent(self) -> i32 {
        X_FIXED_6X13_DESCENT
    }

    pub(crate) fn glyph_rows(self, byte: u8) -> [u8; 13] {
        X_FIXED_6X13_GLYPHS[usize::from(byte)]
    }
}

pub fn x_fixed_glyph_rows(byte: u8) -> [u8; 13] {
    XFontFace::Fixed6x13.glyph_rows(byte)
}
