#![cfg(test)]
//! Wide arcs by the protocol's geometry: a wide arc is the area its width
//! sweeps along the ellipse, and a pixel is drawn when its centre is inside.

use super::poly_arc;
use crate::software::geometry::arc::XArc;
use crate::{X_LINE_DOUBLE_DASH, X_LINE_ON_OFF_DASH, XGraphicsContextValues};
use std::collections::BTreeSet;

fn gc(width: u16) -> XGraphicsContextValues {
    XGraphicsContextValues {
        foreground: 1,
        background: 2,
        line_width: width,
        ..XGraphicsContextValues::default()
    }
}

fn arc(x: i16, y: i16, size: u16, angle1: i16, angle2: i16) -> XArc {
    XArc {
        x,
        y,
        width: size,
        height: size,
        angle1,
        angle2,
    }
}

fn pixels(
    batches: &[(u32, Vec<crate::software::geometry::wide_line::XSpan>)],
) -> BTreeSet<(i32, i32)> {
    batches
        .iter()
        .flat_map(|(_, spans)| spans)
        .flat_map(|span| (span.x..span.x + span.width).map(move |x| (x, span.y)))
        .collect()
}

#[test]
fn a_wide_circle_is_the_ring_its_width_sweeps() {
    // Centre (30, 30), radius 20, line width 6: centres at a distance well
    // inside 17..23 are drawn, well outside it are not.
    let drawn = pixels(&poly_arc(&[arc(10, 10, 40, 0, 360 * 64)], &gc(6)));
    for y in 0..60 {
        for x in 0..60 {
            let d = f64::from(x - 30).hypot(f64::from(y - 30));
            if (17.6..=22.4).contains(&d) {
                assert!(drawn.contains(&(x, y)), "({x}, {y}) at {d:.2}");
            }
            if !(16.4..=23.6).contains(&d) {
                assert!(!drawn.contains(&(x, y)), "({x}, {y}) at {d:.2}");
            }
        }
    }
}

#[test]
fn a_wide_quarter_arc_stays_in_its_quadrant_with_butt_ends() {
    // 0 to 90 degrees: the upper-right quarter, ending square on the axes.
    let drawn = pixels(&poly_arc(&[arc(10, 10, 40, 0, 90 * 64)], &gc(6)));
    assert!(!drawn.is_empty());
    for (x, y) in &drawn {
        assert!(*x >= 30 && *y <= 30, "({x}, {y})");
    }
}

#[test]
fn a_double_dash_draws_its_background_before_its_foreground() {
    let mut dashed = gc(4);
    dashed.line_style = X_LINE_DOUBLE_DASH;
    dashed.dashes = vec![10, 10];
    let batches = poly_arc(&[arc(10, 10, 60, 0, 360 * 64)], &dashed);
    let first_fg = batches.iter().position(|(pixel, _)| *pixel == 1);
    let last_bg = batches.iter().rposition(|(pixel, _)| *pixel == 2);
    assert!(first_fg.is_some() && last_bg.is_some());
    assert!(last_bg < first_fg, "background phase first");
    let mut on_off = dashed.clone();
    on_off.line_style = X_LINE_ON_OFF_DASH;
    let on = pixels(&poly_arc(&[arc(10, 10, 60, 0, 360 * 64)], &on_off));
    let whole = pixels(&poly_arc(&[arc(10, 10, 60, 0, 360 * 64)], &gc(4)));
    assert!(on.len() < whole.len() && on.is_subset(&whole));
}

#[test]
fn each_rendered_group_paints_a_pixel_once() {
    // Two arcs meeting end to end join; under GXxor a doubled pixel would
    // vanish, so no batch may cover a pixel twice.
    let mut xor = gc(8);
    xor.function = 6;
    let batches = poly_arc(
        &[
            arc(10, 10, 40, 0, 90 * 64),
            arc(10, 10, 40, 90 * 64, 90 * 64),
        ],
        &xor,
    );
    for (_, spans) in &batches {
        let mut seen = BTreeSet::new();
        for span in spans {
            for x in span.x..span.x + span.width {
                assert!(seen.insert((x, span.y)), "({x}, {}) twice", span.y);
            }
        }
    }
}

#[test]
fn an_on_off_dash_that_ends_off_draws_without_a_second_phase() {
    // mi reads a phase-1 count that an on/off dash does not have; the port
    // must neither panic nor draw from it.
    let mut dashed = gc(1);
    dashed.line_style = X_LINE_ON_OFF_DASH;
    dashed.dashes = vec![3, 17];
    for angle2 in [45 * 64, 90 * 64, 200 * 64, 360 * 64] {
        let drawn = pixels(&poly_arc(
            &[arc(0, 0, 40, 0, angle2), arc(0, 0, 40, angle2, 30 * 64)],
            &dashed,
        ));
        assert!(!drawn.is_empty());
    }
}
