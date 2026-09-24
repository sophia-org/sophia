#![cfg(test)]
//! Wide lines against the protocol's own geometry: a wide line is the
//! polygon its width sweeps, and a pixel is drawn when its centre lies
//! inside. Each case is small enough to work out by hand.

use super::{XInkedSpans, XSpan, polyline, rectangles, segments};
use crate::{
    X_CAP_BUTT, X_CAP_PROJECTING, X_CAP_ROUND, X_JOIN_BEVEL, X_JOIN_MITER, X_JOIN_ROUND,
    X_LINE_ON_OFF_DASH, XGraphicsContextValues, XPoint,
};
use sophia_protocol::Rect;
use std::collections::BTreeSet;

fn point(x: i16, y: i16) -> XPoint {
    XPoint { x, y }
}

fn gc(width: u16, cap: u8, join: u8) -> XGraphicsContextValues {
    XGraphicsContextValues {
        foreground: 1,
        background: 2,
        line_width: width,
        cap_style: cap,
        join_style: join,
        ..XGraphicsContextValues::default()
    }
}

/// Every pixel the spans cover, whatever their batch.
fn pixels(spans: &XInkedSpans) -> BTreeSet<(i32, i32)> {
    spans
        .iter()
        .flat_map(|(_, spans)| spans)
        .flat_map(|span| (span.x..span.x + span.width).map(move |x| (x, span.y)))
        .collect()
}

fn block(x: std::ops::Range<i32>, y: std::ops::Range<i32>) -> BTreeSet<(i32, i32)> {
    y.flat_map(|y| x.clone().map(move |x| (x, y))).collect()
}

#[test]
fn a_butt_line_is_the_rectangle_its_width_sweeps() {
    // From (2, 5) to (10, 5), four wide: y from 3 to 7, x from 2 to 10.
    let drawn = pixels(&polyline(
        &[point(2, 5), point(10, 5)],
        &gc(4, X_CAP_BUTT, X_JOIN_MITER),
    ));
    assert_eq!(drawn, block(2..10, 3..7));
    let drawn = pixels(&polyline(
        &[point(5, 2), point(5, 10)],
        &gc(4, X_CAP_BUTT, X_JOIN_MITER),
    ));
    assert_eq!(drawn, block(3..7, 2..10));
}

#[test]
fn a_projecting_cap_extends_the_line_by_half_its_width() {
    let drawn = pixels(&polyline(
        &[point(2, 5), point(10, 5)],
        &gc(4, X_CAP_PROJECTING, X_JOIN_MITER),
    ));
    assert_eq!(drawn, block(0..12, 3..7));
}

#[test]
fn a_round_cap_adds_a_half_disc_and_no_corner() {
    let drawn = pixels(&polyline(
        &[point(10, 10), point(30, 10)],
        &gc(10, X_CAP_ROUND, X_JOIN_MITER),
    ));
    let butt = block(10..30, 5..15);
    assert!(drawn.is_superset(&butt), "the body is all there");
    // The disc of radius 5 about (10, 10) holds (6, 10) but not the corner
    // of the square a projecting cap would have drawn.
    assert!(drawn.contains(&(6, 9)));
    assert!(!drawn.contains(&(5, 5)) && !drawn.contains(&(34, 14)));
}

#[test]
fn a_miter_fills_the_outer_corner_that_a_bevel_and_a_round_join_cut() {
    // A right angle at (10, 2), four wide. The outer corner of the two
    // bodies is the square x 10..12, y 0..2; its far pixel (11, 0) has its
    // centre on the miter's side of the bevel and outside the round join.
    let path = [point(2, 2), point(10, 2), point(10, 10)];
    let corner = (11, 0);
    let miter = pixels(&polyline(&path, &gc(4, X_CAP_BUTT, X_JOIN_MITER)));
    let bevel = pixels(&polyline(&path, &gc(4, X_CAP_BUTT, X_JOIN_BEVEL)));
    let round = pixels(&polyline(&path, &gc(4, X_CAP_BUTT, X_JOIN_ROUND)));
    assert!(miter.contains(&corner));
    assert!(!bevel.contains(&corner));
    assert!(!round.contains(&corner));
    // Everything else is the two bodies, which every join shares.
    let bodies: BTreeSet<_> = block(2..10, 0..4)
        .union(&block(8..12, 2..10))
        .copied()
        .collect();
    for joined in [&miter, &bevel, &round] {
        assert!(joined.is_superset(&bodies));
    }
}

#[test]
fn a_raster_function_that_counts_touches_gets_each_pixel_once() {
    // Under GXxor a pixel painted twice is unpainted. The join overlaps
    // both bodies, so the spans must be their union, one batch, no overlap.
    let mut xor = gc(4, X_CAP_BUTT, X_JOIN_MITER);
    xor.function = 6;
    let spans = polyline(&[point(2, 2), point(10, 2), point(10, 10)], &xor);
    assert_eq!(spans.len(), 1, "one union for the foreground");
    let mut rows: Vec<&XSpan> = spans[0].1.iter().collect();
    rows.sort_by_key(|span| (span.y, span.x));
    for pair in rows.windows(2) {
        assert!(
            pair[0].y != pair[1].y || pair[0].x + pair[0].width < pair[1].x,
            "overlapping spans {pair:?}"
        );
    }
}

#[test]
fn an_on_off_dash_draws_only_its_on_runs() {
    let mut dashed = gc(2, X_CAP_BUTT, X_JOIN_MITER);
    dashed.line_style = X_LINE_ON_OFF_DASH;
    dashed.dashes = vec![4, 4];
    let drawn = pixels(&polyline(&[point(0, 5), point(16, 5)], &dashed));
    let on: BTreeSet<_> = block(0..4, 4..6)
        .union(&block(8..12, 4..6))
        .copied()
        .collect();
    assert_eq!(drawn, on);
}

#[test]
fn segments_are_capped_separately_and_rectangles_close_on_themselves() {
    let gc = gc(2, X_CAP_PROJECTING, X_JOIN_MITER);
    let drawn = pixels(&segments(&[(point(4, 4), point(8, 4))], &gc));
    assert_eq!(drawn, block(3..9, 3..5));
    // A solid mitred outline is its four bands: the outside of a 10x6 box
    // at (5, 5), two wide, less its inside.
    let outline = pixels(&rectangles(
        &[Rect {
            x: 5,
            y: 5,
            width: 10,
            height: 6,
        }],
        &gc,
    ));
    let outside = block(4..16, 4..12);
    let inside = block(6..14, 6..10);
    assert_eq!(outline, outside.difference(&inside).copied().collect());
}
