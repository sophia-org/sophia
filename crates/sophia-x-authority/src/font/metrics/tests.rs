#![cfg(test)]

//! The two rules that decide what a run measures: the default-character
//! substitution and the bearing fold.

use super::{XCharInfo, XFontEncoding, XFontMetrics};

fn cell(width: i16) -> XCharInfo {
    XCharInfo {
        left_side_bearing: 0,
        right_side_bearing: width,
        character_width: width,
        ascent: 11,
        descent: 2,
        attributes: 0,
    }
}

/// A 2x2 matrix over rows 0..=1, columns 0..=1, with one empty cell.
fn matrix() -> XFontMetrics {
    XFontMetrics {
        min_bounds: cell(6),
        max_bounds: cell(6),
        ink_min_bounds: cell(6),
        ink_max_bounds: cell(6),
        min_char_or_byte2: 0,
        max_char_or_byte2: 1,
        min_byte1: 0,
        max_byte1: 1,
        default_char: 0x0001,
        all_chars_exist: false,
        draw_direction: 0,
        font_ascent: 11,
        font_descent: 2,
        char_infos: vec![
            XCharInfo::default(), // (0,0) empty
            cell(6),              // (0,1) the default character
            cell(6),              // (1,0)
            cell(4),              // (1,1) a narrower glyph
        ],
        properties: Vec::new(),
    }
}

#[test]
fn a_matrix_cell_is_addressed_by_its_two_bytes() {
    let metrics = matrix();
    assert_eq!(metrics.encoding(), XFontEncoding::Matrix16Bit);
    assert_eq!(
        metrics.char_info(1, 1).map(|info| info.character_width),
        Some(4)
    );
    assert_eq!(
        metrics.char_info(1, 0).map(|info| info.character_width),
        Some(6)
    );
    assert!(
        metrics.char_info(0, 0).is_none(),
        "an empty cell holds no glyph"
    );
    assert!(metrics.char_info(2, 0).is_none(), "outside the matrix");
    assert!(metrics.char_info(0, 9).is_none(), "outside the row");
}

#[test]
fn a_missing_character_takes_the_default_and_a_missing_default_takes_nothing() {
    let metrics = matrix();
    // 0x0000 is an empty cell, so it resolves to the default at 0x0001.
    assert_eq!(
        metrics
            .resolved_char_info(0x0000)
            .map(|info| info.character_width),
        Some(6)
    );
    let mut without_default = matrix();
    without_default.default_char = 0x0000; // itself empty, so unusable
    assert!(
        without_default.resolved_char_info(0x0000).is_none(),
        "a font with no usable default drops the character entirely"
    );
}

#[test]
fn a_dropped_character_does_not_advance_the_pen() {
    // The X server compacts missing characters out of the glyph array, so
    // they contribute neither ink nor width. A run of three unknowns beside
    // one known glyph must measure exactly that one glyph.
    let mut metrics = matrix();
    metrics.default_char = 0x0000;
    let extents = metrics.text_extents(&[0x0000, 0x0101, 0x0000]);
    assert_eq!(extents.overall_width, 4);
    assert_eq!(extents.overall_left, 0);
    assert_eq!(extents.overall_right, 4);
}

#[test]
fn bearings_fold_against_the_preceding_width_not_the_running_total() {
    // Two 6-wide glyphs: the second's right bearing is measured from the pen
    // after the first, so the run is 12 wide and ends at 12 -- not 18, which
    // is what folding the width in before the bearing would give.
    let metrics = matrix();
    let extents = metrics.text_extents(&[0x0100, 0x0100]);
    assert_eq!(extents.overall_width, 12);
    assert_eq!(extents.overall_right, 12);
    assert_eq!(extents.overall_left, 0);
    assert_eq!(extents.overall_ascent, 11);
    assert_eq!(extents.overall_descent, 2);
    assert_eq!(
        extents.font_ascent, 11,
        "the font's own ascent, not the run's"
    );
}

#[test]
fn an_empty_run_measures_nothing_but_still_reports_the_font() {
    let metrics = matrix();
    let extents = metrics.text_extents(&[]);
    assert_eq!(extents.overall_width, 0);
    assert_eq!(extents.overall_ascent, 0);
    assert_eq!(extents.font_ascent, 11);
    assert_eq!(extents.font_descent, 2);
}

#[test]
fn constant_ink_bounds_suppress_the_per_character_array() {
    // The reply carries 65,536 entries for a full 16-bit matrix, so a font
    // whose characters all measure alike must not send one.
    let mut metrics = matrix();
    assert_eq!(metrics.query_font_char_infos(), 0);
    metrics.ink_max_bounds = cell(9);
    assert_eq!(metrics.query_font_char_infos(), 4);
    assert_eq!(metrics.matrix_len(), 4);
}
