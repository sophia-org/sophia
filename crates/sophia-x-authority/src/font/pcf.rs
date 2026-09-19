//! A bounded reader for X11 Portable Compiled Font files.
//!
//! Written here rather than borrowed: yserver reads only a PCF's accelerator
//! and encoding headers and leaves the glyphs to FreeType, and the whole point
//! of this reader is that Sophia does not link a font library. The format is
//! taken from the X.Org `pcf.h` layout and from `libXfont2`'s `bitmap/pcfread.c`
//! behaviour.
//!
//! Every field is read through a checked accessor, so a corrupt or hostile
//! file yields `None` rather than a panic or an out-of-bounds read. The caller
//! supplies already-decompressed bytes; this module never opens a file.
//!
//! A PCF is a 16-byte-per-entry table directory over a handful of tables. Each
//! table begins with its own format word, and that word -- not the file --
//! decides the byte order, bit order and row padding of everything after it.

mod tests;

use super::metrics::{XCharInfo, XFontMetrics};

/// The largest file this reader will parse.
///
/// The host's widest face, 10x20 with five thousand glyphs, is under half a
/// megabyte uncompressed. Eight gives room for a larger face without letting a
/// crafted header ask for an unbounded allocation.
pub const X_PCF_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The most glyphs one face may define.
///
/// A full two-byte matrix is 65,536 cells; a font that claims more is
/// malformed rather than large.
pub const X_PCF_MAX_GLYPHS: usize = 65_536;

const PCF_ACCELERATORS: u32 = 1 << 1;
const PCF_METRICS: u32 = 1 << 2;
const PCF_BITMAPS: u32 = 1 << 3;
const PCF_BDF_ENCODINGS: u32 = 1 << 5;
const PCF_BDF_ACCELERATORS: u32 = 1 << 8;

const PCF_GLYPH_PAD_MASK: u32 = 3;
const PCF_BYTE_MASK: u32 = 1 << 2;
const PCF_BIT_MASK: u32 = 1 << 3;
const PCF_ACCEL_W_INKBOUNDS: u32 = 1 << 8;
const PCF_COMPRESSED_METRICS: u32 = 1 << 8;

/// One glyph's bitmap, repacked into this authority's own row form.
///
/// Rows are top to bottom, each `row_bytes` wide, most significant bit
/// leftmost, whatever the file's own bit and byte order were. Repacking at
/// load time keeps every drawing path free of format questions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XGlyph {
    pub width: u16,
    pub height: u16,
    pub row_bytes: u16,
    pub rows: Vec<u8>,
}

impl XGlyph {
    /// Whether the pixel at `(x, y)` is set. Out-of-range is unset.
    pub fn pixel(&self, x: u16, y: u16) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let offset = usize::from(y) * usize::from(self.row_bytes) + usize::from(x / 8);
        self.rows
            .get(offset)
            .is_some_and(|byte| byte & (0x80 >> (x % 8)) != 0)
    }
}

/// A face as loaded: what it measures and what it draws.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XLoadedFont {
    pub metrics: XFontMetrics,
    /// Parallel to `metrics.char_infos`; an absent cell holds an empty glyph.
    pub glyphs: Vec<XGlyph>,
}

impl XLoadedFont {
    /// The glyph drawn for `code`, after the default-character rule.
    pub fn glyph(&self, code: u16) -> Option<(&XCharInfo, &XGlyph)> {
        let index = self.glyph_index(code)?;
        let info = self.metrics.char_infos.get(index)?;
        if !info.exists() {
            return None;
        }
        Some((info, self.glyphs.get(index)?))
    }

    fn glyph_index(&self, code: u16) -> Option<usize> {
        let direct = self.matrix_index(code);
        if direct.is_some_and(|index| {
            self.metrics
                .char_infos
                .get(index)
                .is_some_and(super::metrics::XCharInfo::exists)
        }) {
            return direct;
        }
        self.matrix_index(self.metrics.default_char)
    }

    fn matrix_index(&self, code: u16) -> Option<usize> {
        let [byte1, byte2] = code.to_be_bytes();
        let metrics = &self.metrics;
        if byte1 < metrics.min_byte1 || byte1 > metrics.max_byte1 {
            return None;
        }
        let column = u16::from(byte2);
        if column < metrics.min_char_or_byte2 || column > metrics.max_char_or_byte2 {
            return None;
        }
        let row_len = usize::from(metrics.max_char_or_byte2 - metrics.min_char_or_byte2) + 1;
        Some(
            usize::from(byte1 - metrics.min_byte1) * row_len
                + usize::from(column - metrics.min_char_or_byte2),
        )
    }

    /// Bytes this face holds, for the cache's accounting.
    pub fn retained_bytes(&self) -> usize {
        self.metrics.char_infos.len() * size_of::<XCharInfo>()
            + self
                .glyphs
                .iter()
                .map(|glyph| glyph.rows.len())
                .sum::<usize>()
    }
}

/// Why a file could not be read as a font.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPcfError {
    TooLarge,
    Malformed,
    UnsupportedMatrix,
}

/// A table's own byte order and glyph padding, taken from its format word.
#[derive(Clone, Copy, Debug)]
struct TableFormat {
    format: u32,
}

impl TableFormat {
    const fn msb_bytes(self) -> bool {
        self.format & PCF_BYTE_MASK != 0
    }

    const fn msb_bits(self) -> bool {
        self.format & PCF_BIT_MASK != 0
    }

    const fn glyph_pad(self) -> usize {
        1 << (self.format & PCF_GLYPH_PAD_MASK)
    }

    fn u16_at(self, bytes: &[u8], offset: usize) -> Option<u16> {
        let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(if self.msb_bytes() {
            u16::from_be_bytes(raw)
        } else {
            u16::from_le_bytes(raw)
        })
    }

    fn i16_at(self, bytes: &[u8], offset: usize) -> Option<i16> {
        self.u16_at(bytes, offset).map(|value| value as i16)
    }

    fn u32_at(self, bytes: &[u8], offset: usize) -> Option<u32> {
        let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(if self.msb_bytes() {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        })
    }

    fn i32_at(self, bytes: &[u8], offset: usize) -> Option<i32> {
        self.u32_at(bytes, offset).map(|value| value as i32)
    }
}

/// One entry of the file's table directory.
#[derive(Clone, Copy, Debug)]
struct TableEntry {
    kind: u32,
    offset: usize,
    size: usize,
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

/// Locate a table and read its format word.
fn table<'a>(bytes: &'a [u8], tables: &[TableEntry], kind: u32) -> Option<(TableFormat, &'a [u8])> {
    let entry = tables.iter().find(|entry| entry.kind == kind)?;
    let body = bytes.get(entry.offset..entry.offset.checked_add(entry.size)?)?;
    // The format word itself is always least significant byte first; only what
    // follows it obeys the order the word declares.
    let format = TableFormat {
        format: le_u32(body, 0)?,
    };
    Some((format, body))
}

/// Read one 12-byte uncompressed metric.
fn uncompressed_metric(format: TableFormat, bytes: &[u8], offset: usize) -> Option<XCharInfo> {
    Some(XCharInfo {
        left_side_bearing: format.i16_at(bytes, offset)?,
        right_side_bearing: format.i16_at(bytes, offset + 2)?,
        character_width: format.i16_at(bytes, offset + 4)?,
        ascent: format.i16_at(bytes, offset + 6)?,
        descent: format.i16_at(bytes, offset + 8)?,
        attributes: format.u16_at(bytes, offset + 10)?,
    })
}

/// Read one 5-byte compressed metric. Each field is biased by 0x80.
fn compressed_metric(bytes: &[u8], offset: usize) -> Option<XCharInfo> {
    let raw = bytes.get(offset..offset + 5)?;
    let field = |index: usize| i16::from(raw[index]) - 0x80;
    Some(XCharInfo {
        left_side_bearing: field(0),
        right_side_bearing: field(1),
        character_width: field(2),
        ascent: field(3),
        descent: field(4),
        attributes: 0,
    })
}

/// Read a metrics table in whichever of its two encodings it uses.
fn read_metrics(format: TableFormat, body: &[u8]) -> Option<Vec<XCharInfo>> {
    let compressed = format.format & PCF_COMPRESSED_METRICS != 0;
    let (count, mut offset) = if compressed {
        (usize::from(format.u16_at(body, 4)?), 6)
    } else {
        (usize::try_from(format.u32_at(body, 4)?).ok()?, 8)
    };
    if count > X_PCF_MAX_GLYPHS {
        return None;
    }
    let mut metrics = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = if compressed {
            compressed_metric(body, offset)?
        } else {
            uncompressed_metric(format, body, offset)?
        };
        metrics.push(entry);
        offset += if compressed { 5 } else { 12 };
    }
    Some(metrics)
}

/// Reverse a byte's bits, for files that store glyph rows least significant
/// bit first.
const fn reverse_bits(byte: u8) -> u8 {
    byte.reverse_bits()
}

/// Read the bitmap table and repack every glyph into MSB-first rows.
fn read_bitmaps(format: TableFormat, body: &[u8], metrics: &[XCharInfo]) -> Option<Vec<XGlyph>> {
    let count = usize::try_from(format.u32_at(body, 4)?).ok()?;
    if count != metrics.len() || count > X_PCF_MAX_GLYPHS {
        return None;
    }
    let offsets_at = 8;
    let sizes_at = offsets_at + count * 4;
    let data_at = sizes_at + 16;
    // Four candidate sizes are stored, one per padding; the format word says
    // which one this file actually used.
    let data_len = usize::try_from(format.u32_at(
        body,
        sizes_at + (format.format & PCF_GLYPH_PAD_MASK) as usize * 4,
    )?)
    .ok()?;
    let data = body.get(data_at..data_at.checked_add(data_len)?)?;
    let pad = format.glyph_pad();
    let mut glyphs = Vec::with_capacity(count);
    for (index, info) in metrics.iter().enumerate() {
        let start = usize::try_from(format.u32_at(body, offsets_at + index * 4)?).ok()?;
        let width = info
            .right_side_bearing
            .saturating_sub(info.left_side_bearing)
            .max(0);
        let height = info.ascent.saturating_add(info.descent).max(0);
        let width = u16::try_from(width).unwrap_or(0);
        let height = u16::try_from(height).unwrap_or(0);
        if width == 0 || height == 0 {
            glyphs.push(XGlyph::default());
            continue;
        }
        let row_bytes = usize::from(width).div_ceil(8);
        // The file's rows are padded to the declared unit; ours are not.
        let source_row = usize::from(width).div_ceil(pad * 8) * pad;
        let mut rows = Vec::with_capacity(row_bytes * usize::from(height));
        for row in 0..usize::from(height) {
            let at = start.checked_add(row * source_row)?;
            let source = data.get(at..at.checked_add(source_row)?)?;
            for column in 0..row_bytes {
                let byte = source.get(column).copied().unwrap_or(0);
                rows.push(if format.msb_bits() {
                    byte
                } else {
                    reverse_bits(byte)
                });
            }
        }
        glyphs.push(XGlyph {
            width,
            height,
            row_bytes: u16::try_from(row_bytes).unwrap_or(0),
            rows,
        });
    }
    Some(glyphs)
}

/// The accelerator table's font-wide facts.
struct Accelerators {
    font_ascent: i16,
    font_descent: i16,
    draw_direction: u8,
    min_bounds: XCharInfo,
    max_bounds: XCharInfo,
    ink_bounds: Option<(XCharInfo, XCharInfo)>,
}

fn read_accelerators(format: TableFormat, body: &[u8]) -> Option<Accelerators> {
    let draw_direction = *body.get(10)?;
    let font_ascent = i16::try_from(format.i32_at(body, 12)?).unwrap_or(0);
    let font_descent = i16::try_from(format.i32_at(body, 16)?).unwrap_or(0);
    let min_bounds = uncompressed_metric(format, body, 24)?;
    let max_bounds = uncompressed_metric(format, body, 36)?;
    let ink_bounds = if format.format & PCF_ACCEL_W_INKBOUNDS != 0 {
        Some((
            uncompressed_metric(format, body, 48)?,
            uncompressed_metric(format, body, 60)?,
        ))
    } else {
        None
    };
    Some(Accelerators {
        font_ascent,
        font_descent,
        draw_direction,
        min_bounds,
        max_bounds,
        ink_bounds,
    })
}

/// Parse a decompressed PCF file into a face.
pub fn load(bytes: &[u8]) -> Result<XLoadedFont, XPcfError> {
    if bytes.len() > X_PCF_MAX_BYTES {
        return Err(XPcfError::TooLarge);
    }
    parse(bytes).ok_or(XPcfError::Malformed)
}

fn parse(bytes: &[u8]) -> Option<XLoadedFont> {
    if bytes.get(0..4)? != b"\x01fcp" {
        return None;
    }
    let table_count = usize::try_from(le_u32(bytes, 4)?).ok()?;
    if table_count > 64 {
        return None;
    }
    let mut tables = Vec::with_capacity(table_count);
    for index in 0..table_count {
        let at = 8 + index * 16;
        // A directory entry is type, format, size, offset -- the format word
        // sits between the type and the size, and reading past it lands one
        // field short of the data.
        tables.push(TableEntry {
            kind: le_u32(bytes, at)?,
            size: usize::try_from(le_u32(bytes, at + 8)?).ok()?,
            offset: usize::try_from(le_u32(bytes, at + 12)?).ok()?,
        });
    }

    let (metrics_format, metrics_body) = table(bytes, &tables, PCF_METRICS)?;
    let glyph_metrics = read_metrics(metrics_format, metrics_body)?;
    let (bitmap_format, bitmap_body) = table(bytes, &tables, PCF_BITMAPS)?;
    let glyph_bitmaps = read_bitmaps(bitmap_format, bitmap_body, &glyph_metrics)?;

    // Prefer the BDF accelerators, which carry the ink bounds a real bdftopcf
    // measured; fall back to the plain table when a file has only that.
    let accelerators = table(bytes, &tables, PCF_BDF_ACCELERATORS)
        .or_else(|| table(bytes, &tables, PCF_ACCELERATORS))
        .and_then(|(format, body)| read_accelerators(format, body))?;

    let (encoding_format, encoding_body) = table(bytes, &tables, PCF_BDF_ENCODINGS)?;
    let min_char_or_byte2 = encoding_format.u16_at(encoding_body, 4)?;
    let max_char_or_byte2 = encoding_format.u16_at(encoding_body, 6)?;
    let min_byte1 = encoding_format.u16_at(encoding_body, 8)?;
    let max_byte1 = encoding_format.u16_at(encoding_body, 10)?;
    let default_char = encoding_format.u16_at(encoding_body, 12)?;
    if max_char_or_byte2 < min_char_or_byte2 || max_byte1 < min_byte1 || max_byte1 > 0xff {
        return None;
    }
    let row_len = usize::from(max_char_or_byte2 - min_char_or_byte2) + 1;
    let rows = usize::from(max_byte1 - min_byte1) + 1;
    let cells = row_len.checked_mul(rows)?;
    if cells > X_PCF_MAX_GLYPHS {
        return None;
    }

    // The encoding table maps every matrix cell to a glyph index, or to
    // 0xffff for a cell the font does not define.
    let mut char_infos = vec![XCharInfo::default(); cells];
    let mut cell_glyphs = vec![XGlyph::default(); cells];
    let mut all_exist = true;
    let mut ink_min: Option<XCharInfo> = None;
    let mut ink_max: Option<XCharInfo> = None;
    for cell in 0..cells {
        let index = encoding_format.u16_at(encoding_body, 14 + cell * 2)?;
        if index == 0xffff {
            all_exist = false;
            continue;
        }
        let index = usize::from(index);
        let Some(info) = glyph_metrics.get(index) else {
            all_exist = false;
            continue;
        };
        char_infos[cell] = *info;
        if let Some(glyph) = glyph_bitmaps.get(index) {
            cell_glyphs[cell] = glyph.clone();
        }
        ink_min = Some(match ink_min {
            None => *info,
            Some(current) => XCharInfo {
                left_side_bearing: current.left_side_bearing.min(info.left_side_bearing),
                right_side_bearing: current.right_side_bearing.min(info.right_side_bearing),
                character_width: current.character_width.min(info.character_width),
                ascent: current.ascent.min(info.ascent),
                descent: current.descent.min(info.descent),
                attributes: 0,
            },
        });
        ink_max = Some(match ink_max {
            None => *info,
            Some(current) => XCharInfo {
                left_side_bearing: current.left_side_bearing.max(info.left_side_bearing),
                right_side_bearing: current.right_side_bearing.max(info.right_side_bearing),
                character_width: current.character_width.max(info.character_width),
                ascent: current.ascent.max(info.ascent),
                descent: current.descent.max(info.descent),
                attributes: 0,
            },
        });
    }

    let (ink_min_bounds, ink_max_bounds) = accelerators.ink_bounds.unwrap_or((
        ink_min.unwrap_or(accelerators.min_bounds),
        ink_max.unwrap_or(accelerators.max_bounds),
    ));

    Some(XLoadedFont {
        metrics: XFontMetrics {
            min_bounds: accelerators.min_bounds,
            max_bounds: accelerators.max_bounds,
            ink_min_bounds,
            ink_max_bounds,
            min_char_or_byte2,
            max_char_or_byte2,
            min_byte1: u8::try_from(min_byte1).ok()?,
            max_byte1: u8::try_from(max_byte1).ok()?,
            default_char,
            all_chars_exist: all_exist,
            draw_direction: u8::from(accelerators.draw_direction != 0),
            font_ascent: accelerators.font_ascent,
            font_descent: accelerators.font_descent,
            char_infos,
            properties: Vec::new(),
        },
        glyphs: cell_glyphs,
    })
}
