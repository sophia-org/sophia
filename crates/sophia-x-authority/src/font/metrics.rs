//! Per-character and per-font metrics, and the rules that read them.
//!
//! Portions derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes):
//! `crates/yserver-protocol/src/x11/mod.rs:422-552`, the `CharInfo` /
//! `FontMetrics` shape and the `char_info` and `text_extents` rules.
//!
//! The semantics are the X server's, not an approximation of them. Two rules
//! decide everything here and both come from `dix`:
//!
//! - A character outside the font's matrix, or inside it with no glyph, is
//!   replaced by the font's default character. If the font has no usable
//!   default the character is **dropped entirely** -- nothing is drawn and the
//!   pen does not advance (`mi/mipolytext.c:75-104`, where `GetGlyphs` returns
//!   a compacted array and the width is summed over what survived).
//! - Extents accumulate left and right bearings against the width of every
//!   *preceding* character, so the order of the fold matters
//!   (`QueryTextExtents` in the protocol spec).

mod tests;

/// One character's metrics, in the wire's own field order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XCharInfo {
    pub left_side_bearing: i16,
    pub right_side_bearing: i16,
    pub character_width: i16,
    pub ascent: i16,
    pub descent: i16,
    pub attributes: u16,
}

impl XCharInfo {
    /// Whether this entry describes a glyph at all.
    ///
    /// All-zero metrics are the wire's way of saying a matrix cell is empty,
    /// so a font may carry the cell without carrying the character.
    pub const fn exists(&self) -> bool {
        self.left_side_bearing != 0
            || self.right_side_bearing != 0
            || self.character_width != 0
            || self.ascent != 0
            || self.descent != 0
    }
}

/// How a font's characters are addressed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFontEncoding {
    /// One byte selects the character; `byte1` must be zero.
    Linear8Bit,
    /// `byte1` is the row and `byte2` the column of a two-dimensional matrix.
    Matrix16Bit,
}

/// A font's global description: the matrix it covers and the bounds of what
/// is in it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XFontMetrics {
    pub min_bounds: XCharInfo,
    pub max_bounds: XCharInfo,
    /// Measured ink extents, which decide whether `QueryFont` must send a
    /// per-character array at all.
    pub ink_min_bounds: XCharInfo,
    pub ink_max_bounds: XCharInfo,
    pub min_char_or_byte2: u16,
    pub max_char_or_byte2: u16,
    pub min_byte1: u8,
    pub max_byte1: u8,
    pub default_char: u16,
    pub all_chars_exist: bool,
    pub draw_direction: u8,
    pub font_ascent: i16,
    pub font_descent: i16,
    /// Row-major over `min_byte1..=max_byte1` then
    /// `min_char_or_byte2..=max_char_or_byte2`.
    pub char_infos: Vec<XCharInfo>,
    /// Font properties as resolved atom-name and value pairs.
    pub properties: Vec<(String, u32, bool)>,
}

/// What `QueryTextExtents` answers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XTextExtents {
    pub draw_direction: u8,
    pub font_ascent: i16,
    pub font_descent: i16,
    pub overall_ascent: i16,
    pub overall_descent: i16,
    pub overall_width: i32,
    pub overall_left: i32,
    pub overall_right: i32,
}

impl XFontMetrics {
    pub const fn encoding(&self) -> XFontEncoding {
        if self.min_byte1 == 0 && self.max_byte1 == 0 {
            XFontEncoding::Linear8Bit
        } else {
            XFontEncoding::Matrix16Bit
        }
    }

    /// How many columns one matrix row holds.
    const fn row_len(&self) -> usize {
        if self.max_char_or_byte2 < self.min_char_or_byte2 {
            return 0;
        }
        (self.max_char_or_byte2 - self.min_char_or_byte2) as usize + 1
    }

    /// The number of cells the matrix declares, empty ones included.
    pub const fn matrix_len(&self) -> usize {
        if self.max_byte1 < self.min_byte1 {
            return 0;
        }
        self.row_len() * ((self.max_byte1 - self.min_byte1) as usize + 1)
    }

    /// The metrics of one character, by its two wire bytes.
    ///
    /// `None` for a character outside the matrix or for a cell the font left
    /// empty. Callers must apply the default-character rule themselves, since
    /// only they know whether they are measuring or drawing.
    pub fn char_info(&self, byte1: u8, byte2: u8) -> Option<&XCharInfo> {
        if byte1 < self.min_byte1 || byte1 > self.max_byte1 {
            return None;
        }
        let column = u16::from(byte2);
        if column < self.min_char_or_byte2 || column > self.max_char_or_byte2 {
            return None;
        }
        let row = usize::from(byte1 - self.min_byte1);
        let offset = usize::from(column - self.min_char_or_byte2);
        let entry = self.char_infos.get(row * self.row_len() + offset)?;
        entry.exists().then_some(entry)
    }

    /// The default character's metrics, if the font has a usable default.
    pub fn default_char_info(&self) -> Option<&XCharInfo> {
        let [byte1, byte2] = self.default_char.to_be_bytes();
        self.char_info(byte1, byte2)
    }

    /// The metrics actually used to draw `code`, after the default rule.
    ///
    /// `None` means the character contributes nothing at all: no ink and no
    /// advance.
    pub fn resolved_char_info(&self, code: u16) -> Option<&XCharInfo> {
        let [byte1, byte2] = code.to_be_bytes();
        self.char_info(byte1, byte2)
            .or_else(|| self.default_char_info())
    }

    /// Measure a run exactly as `QueryTextExtents` reports it.
    pub fn text_extents(&self, codes: &[u16]) -> XTextExtents {
        let mut extents = XTextExtents {
            draw_direction: self.draw_direction,
            font_ascent: self.font_ascent,
            font_descent: self.font_descent,
            ..XTextExtents::default()
        };
        let mut first = true;
        for code in codes.iter().copied() {
            let Some(info) = self.resolved_char_info(code) else {
                continue;
            };
            // Bearings are relative to the pen, which stands at the sum of
            // every preceding character's width. Folding the width in first
            // would shift this character's own bearings by its own advance.
            let left = extents
                .overall_width
                .saturating_add(i32::from(info.left_side_bearing));
            let right = extents
                .overall_width
                .saturating_add(i32::from(info.right_side_bearing));
            if first {
                extents.overall_ascent = info.ascent;
                extents.overall_descent = info.descent;
                extents.overall_left = left;
                extents.overall_right = right;
                first = false;
            } else {
                extents.overall_ascent = extents.overall_ascent.max(info.ascent);
                extents.overall_descent = extents.overall_descent.max(info.descent);
                extents.overall_left = extents.overall_left.min(left);
                extents.overall_right = extents.overall_right.max(right);
            }
            extents.overall_width = extents
                .overall_width
                .saturating_add(i32::from(info.character_width));
        }
        extents
    }

    /// Whether `QueryFont` must send a per-character array.
    ///
    /// The server sends one only when the ink bounds differ; identical bounds
    /// tell the client every character measures the same and the array would
    /// be pure redundancy (`dix/dispatch.c:1362-1370`).
    pub fn query_font_char_infos(&self) -> usize {
        if self.ink_min_bounds == self.ink_max_bounds {
            0
        } else {
            self.matrix_len()
        }
    }
}
