#![cfg(test)]

//! The reader is exercised against files this module builds itself.
//!
//! A synthetic file pins the format exactly -- every byte is chosen here, so a
//! misread field fails on a known value rather than on whatever a host font
//! happened to contain. The builder covers the variations that actually differ
//! between real files: compressed and uncompressed metrics, both bit orders,
//! and row padding wider than the glyph.

use super::{X_PCF_MAX_BYTES, XPcfError, load};

const PCF_PROPERTIES: u32 = 1 << 0;
const PCF_METRICS: u32 = 1 << 2;
const PCF_BITMAPS: u32 = 1 << 3;
const PCF_BDF_ENCODINGS: u32 = 1 << 5;
const PCF_BDF_ACCELERATORS: u32 = 1 << 8;

#[derive(Clone, Copy)]
struct Shape {
    compressed_metrics: bool,
    msb_bits: bool,
    glyph_pad: usize,
}

/// One 6x13 glyph: a solid top row, then a left column.
fn glyph_rows() -> [u8; 13] {
    let mut rows = [0b0010_0000u8; 13];
    rows[0] = 0b1111_1100;
    rows
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Build a two-character font: cell 0 empty, cell 1 the glyph above.
///
/// Matrix rows 0..=0, columns 0..=1, default character 1.
fn build(shape: Shape) -> Vec<u8> {
    let format_word = |extra: u32| -> u32 {
        // Least significant byte order, the bit order and padding under test.
        extra | if shape.msb_bits { 1 << 3 } else { 0 } | (shape.glyph_pad.trailing_zeros())
    };

    // --- metrics table: one real glyph ---
    let mut metrics = Vec::new();
    if shape.compressed_metrics {
        push_u32(&mut metrics, format_word(1 << 8));
        push_u16(&mut metrics, 1);
        // lsb, rsb, width, ascent, descent -- each biased by 0x80.
        metrics.extend_from_slice(&[0x80, 0x86, 0x86, 0x8b, 0x82]);
    } else {
        push_u32(&mut metrics, format_word(0));
        push_u32(&mut metrics, 1);
        for value in [0i16, 6, 6, 11, 2] {
            metrics.extend_from_slice(&(value as u16).to_le_bytes());
        }
        push_u16(&mut metrics, 0);
    }

    // --- bitmap table ---
    let source_row = 6usize.div_ceil(shape.glyph_pad * 8) * shape.glyph_pad;
    let mut glyph_data = Vec::new();
    for row in glyph_rows() {
        let byte = if shape.msb_bits {
            row
        } else {
            row.reverse_bits()
        };
        glyph_data.push(byte);
        glyph_data.resize(glyph_data.len() + source_row - 1, 0);
    }
    let mut bitmaps = Vec::new();
    push_u32(&mut bitmaps, format_word(0));
    push_u32(&mut bitmaps, 1);
    push_u32(&mut bitmaps, 0);
    for pad in [1usize, 2, 4, 8] {
        let row = 6usize.div_ceil(pad * 8) * pad;
        push_u32(&mut bitmaps, u32::try_from(row * 13).unwrap());
    }
    bitmaps.extend_from_slice(&glyph_data);

    // --- accelerators, with ink bounds ---
    let mut accelerators = Vec::new();
    push_u32(&mut accelerators, format_word(1 << 8));
    accelerators.extend_from_slice(&[1, 1, 1, 1, 1, 1, 0, 0]);
    push_u32(&mut accelerators, 11);
    push_u32(&mut accelerators, 2);
    push_u32(&mut accelerators, 0);
    for bounds in [
        [0i16, 6, 6, 11, 2],
        [0, 6, 6, 11, 2],
        [0, 6, 6, 11, 2],
        [0, 6, 6, 11, 2],
    ] {
        for value in bounds {
            accelerators.extend_from_slice(&(value as u16).to_le_bytes());
        }
        push_u16(&mut accelerators, 0);
    }

    // --- encodings: cell 0 absent, cell 1 -> glyph 0 ---
    let mut encodings = Vec::new();
    push_u32(&mut encodings, format_word(0));
    push_u16(&mut encodings, 0); // min_char_or_byte2
    push_u16(&mut encodings, 1); // max_char_or_byte2
    push_u16(&mut encodings, 0); // min_byte1
    push_u16(&mut encodings, 0); // max_byte1
    push_u16(&mut encodings, 1); // default_char
    push_u16(&mut encodings, 0xffff);
    push_u16(&mut encodings, 0);

    let mut properties = Vec::new();
    push_u32(&mut properties, format_word(0));
    push_u32(&mut properties, 0);
    push_u32(&mut properties, 0);

    let sections = [
        (PCF_PROPERTIES, properties),
        (PCF_METRICS, metrics),
        (PCF_BITMAPS, bitmaps),
        (PCF_BDF_ENCODINGS, encodings),
        (PCF_BDF_ACCELERATORS, accelerators),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(b"\x01fcp");
    push_u32(&mut out, u32::try_from(sections.len()).unwrap());
    let mut offset = 8 + sections.len() * 16;
    let mut bodies = Vec::new();
    for (kind, body) in sections {
        // type, format, size, offset -- the directory repeats the table's own
        // leading format word.
        push_u32(&mut out, kind);
        out.extend_from_slice(&body[..4]);
        push_u32(&mut out, u32::try_from(body.len()).unwrap());
        push_u32(&mut out, u32::try_from(offset).unwrap());
        offset += body.len();
        bodies.push(body);
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

const PLAIN: Shape = Shape {
    compressed_metrics: false,
    msb_bits: true,
    glyph_pad: 1,
};

#[test]
fn a_face_loads_its_matrix_metrics_and_glyph() {
    let font = load(&build(PLAIN)).expect("synthetic font loads");
    assert_eq!(font.metrics.font_ascent, 11);
    assert_eq!(font.metrics.font_descent, 2);
    assert_eq!(font.metrics.max_byte1, 0);
    assert_eq!(font.metrics.max_char_or_byte2, 1);
    assert_eq!(font.metrics.default_char, 1);
    assert!(
        !font.metrics.all_chars_exist,
        "cell zero is absent, so the font must not claim every character"
    );
    let (info, glyph) = font.glyph(1).expect("cell one holds the glyph");
    assert_eq!(info.character_width, 6);
    assert_eq!((glyph.width, glyph.height), (6, 13));
    assert!(glyph.pixel(0, 0), "the top row is solid");
    assert!(glyph.pixel(5, 0));
    assert!(
        !glyph.pixel(0, 1),
        "below the top row only column two is set"
    );
    assert!(glyph.pixel(2, 1));
    assert!(
        !glyph.pixel(6, 0),
        "a pixel past the width is unset, not a panic"
    );
}

#[test]
fn an_absent_cell_falls_back_to_the_default_character() {
    let font = load(&build(PLAIN)).expect("synthetic font loads");
    let (_, absent) = font.glyph(0).expect("cell zero resolves to the default");
    let (_, default) = font.glyph(1).expect("the default itself");
    assert_eq!(absent, default);
    assert!(
        font.glyph(0x99).is_some(),
        "a code outside the matrix also takes the default"
    );
}

#[test]
fn compressed_metrics_read_the_same_face() {
    let plain = load(&build(PLAIN)).expect("plain");
    let compressed = load(&build(Shape {
        compressed_metrics: true,
        ..PLAIN
    }))
    .expect("compressed");
    // Attributes are the one field the compressed form cannot carry.
    assert_eq!(
        compressed.metrics.char_infos[1].character_width,
        plain.metrics.char_infos[1].character_width
    );
    assert_eq!(compressed.glyphs[1], plain.glyphs[1]);
}

#[test]
fn a_least_significant_bit_file_is_repacked_to_the_same_pixels() {
    // Real files differ in bit order and the drawing path must not know it.
    let msb = load(&build(PLAIN)).expect("msb");
    let lsb = load(&build(Shape {
        msb_bits: false,
        ..PLAIN
    }))
    .expect("lsb");
    assert_eq!(lsb.glyphs[1], msb.glyphs[1]);
}

#[test]
fn wider_row_padding_is_stripped() {
    // The host's fonts pad rows to four bytes for a six-pixel glyph; ours are
    // one byte wide, so the reader must drop the padding rather than read it
    // as pixels.
    for pad in [2usize, 4, 8] {
        let padded = load(&build(Shape {
            glyph_pad: pad,
            ..PLAIN
        }))
        .unwrap_or_else(|_| panic!("pad {pad} loads"));
        assert_eq!(
            padded.glyphs[1],
            load(&build(PLAIN)).unwrap().glyphs[1],
            "pad {pad}"
        );
        assert_eq!(padded.glyphs[1].row_bytes, 1);
    }
}

#[test]
fn a_file_that_is_not_a_font_is_refused_rather_than_read() {
    assert_eq!(load(b"not a font at all"), Err(XPcfError::Malformed));
    assert_eq!(load(&[]), Err(XPcfError::Malformed));
    assert_eq!(
        load(&vec![0u8; X_PCF_MAX_BYTES + 1]),
        Err(XPcfError::TooLarge)
    );
}

#[test]
fn a_truncated_file_is_refused_at_every_length() {
    // Every prefix of a valid file must fail cleanly. This is the property
    // that matters for a reader pointed at host files.
    let full = build(PLAIN);
    for length in 0..full.len() {
        let _ = load(&full[..length]);
    }
}

/// The ISO 8859-1 6x13 face as `bdftopcf` produced it. See the fixture README.
const REAL_6X13: &[u8] = include_bytes!("../../../../../tools/fixtures/fonts/6x13-iso8859-1.pcf");

#[test]
fn a_real_bdftopcf_face_reads_exactly() {
    // The synthetic files above prove the reader matches this test's idea of
    // the format. This one proves it matches the format: compressed metrics,
    // most significant byte and bit order, and rows padded to four bytes for a
    // six-pixel glyph.
    let font = load(REAL_6X13).expect("the host's 6x13 face loads");
    assert_eq!(font.metrics.font_ascent, 11);
    assert_eq!(font.metrics.font_descent, 2);
    assert_eq!(font.metrics.min_byte1, 0);
    assert_eq!(font.metrics.max_byte1, 0);
    assert_eq!(font.metrics.max_char_or_byte2, 255);
    assert_eq!(font.metrics.default_char, 0);
    assert_eq!(font.metrics.matrix_len(), 256);
    assert!(
        !font.metrics.all_chars_exist,
        "223 of the 256 cells are defined, so the control positions are absent"
    );
    let defined = (0..=255u16)
        .filter(|code| {
            let [byte1, byte2] = code.to_be_bytes();
            font.metrics.char_info(byte1, byte2).is_some()
        })
        .count();
    assert_eq!(defined, 223);

    // Ink bounds differ across the face, so QueryFont owes a per-character
    // array -- the branch a constant-metric font would never reach.
    assert_ne!(font.metrics.ink_min_bounds, font.metrics.ink_max_bounds);
    assert_eq!(font.metrics.query_font_char_infos(), 256);

    let (info, glyph) = font.glyph(u16::from(b'A')).expect("capital A");
    assert_eq!(
        (
            info.left_side_bearing,
            info.right_side_bearing,
            info.character_width
        ),
        (0, 6, 6)
    );
    assert_eq!((glyph.width, glyph.height, glyph.row_bytes), (6, 13, 1));
    // The letter, read off the rows the file actually stores.
    let drawn: Vec<String> = (0..13)
        .map(|row| {
            (0..6)
                .map(|column| if glyph.pixel(column, row) { '#' } else { '.' })
                .collect()
        })
        .collect();
    assert_eq!(
        drawn,
        [
            "......", "......", "..#...", ".#.#..", "#...#.", "#...#.", "#...#.", "#####.",
            "#...#.", "#...#.", "#...#.", "......", "......",
        ]
    );
    assert!(
        font.glyph(0xe9).is_some(),
        "e-acute is in the Latin-1 subset"
    );
}

#[test]
fn a_header_claiming_more_tables_than_the_file_holds_is_refused() {
    let mut corrupt = build(PLAIN);
    corrupt[4..8].copy_from_slice(&1_000u32.to_le_bytes());
    assert_eq!(load(&corrupt), Err(XPcfError::Malformed));
}
