#![cfg(test)]

//! The built-in face must draw exactly what it drew before it became a loaded
//! font, because the pixel proofs are pinned to it.

use super::{fixed_6x13, provides};
use crate::font::fixed_6x13::X_FIXED_6X13_GLYPHS;

#[test]
fn the_built_in_face_draws_the_table_it_carries() {
    // The table packs six pixels into the low bits of a byte; a loaded glyph
    // is most significant bit leftmost. Getting that conversion wrong shifts
    // every glyph two pixels and is invisible until something reads pixels.
    let font = fixed_6x13();
    for (code, rows) in X_FIXED_6X13_GLYPHS.iter().enumerate() {
        let code = u16::try_from(code).expect("the table is 256 entries");
        let (_, glyph) = font.glyph(code).expect("every built-in character exists");
        for (row, bits) in rows.iter().copied().enumerate() {
            let row = u16::try_from(row).expect("thirteen rows");
            for column in 0..6u16 {
                let expected = bits & (1 << (5 - column)) != 0;
                assert_eq!(
                    glyph.pixel(column, row),
                    expected,
                    "character {code:#04x} row {row} column {column}"
                );
            }
        }
    }
}

#[test]
fn capital_a_is_the_letter() {
    let font = fixed_6x13();
    let (_, glyph) = font.glyph(u16::from(b'A')).expect("capital A");
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
}

#[test]
fn the_face_describes_itself_as_single_byte() {
    // This is what tells a client to draw with the 8-bit requests. Claiming a
    // two-byte matrix here is what made xterm paint Latin-1 glyphs for
    // Unicode text.
    let font = fixed_6x13();
    assert_eq!(font.metrics.min_byte1, 0);
    assert_eq!(font.metrics.max_byte1, 0);
    assert_eq!(font.metrics.max_char_or_byte2, 255);
    assert!(font.metrics.all_chars_exist);
    assert_eq!(font.metrics.query_font_char_infos(), 0, "constant metrics");
    assert_eq!(
        font.metrics
            .text_extents(&[b'A'.into(), b'B'.into()])
            .overall_width,
        12
    );
}

#[test]
fn the_compatibility_names_all_resolve_here() {
    for name in ["fixed", "6x13", "cursor", "nil2", "FIXED"] {
        assert!(provides(name), "{name}");
    }
    assert!(!provides("helvetica"));
}
