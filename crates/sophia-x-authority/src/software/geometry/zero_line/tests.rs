#![cfg(test)]
//! Thin lines and arcs by the rules `mi` keeps: a line's pixels do not
//! depend on the direction it is drawn in, a joint is drawn once, NotLast
//! drops only the polyline's last pixel, and an arc stays on its box.

use super::{arcs, polyline};
use crate::XPoint;
use crate::software::geometry::arc::XArc;
use std::collections::BTreeSet;

fn point(x: i16, y: i16) -> XPoint {
    XPoint { x, y }
}

fn set(pixels: &[(i32, i32)]) -> BTreeSet<(i32, i32)> {
    pixels.iter().copied().collect()
}

#[test]
fn a_line_covers_the_same_pixels_in_either_direction() {
    // The zero-line bias exists for this: a tie in the error term rounds so
    // that both directions agree.
    for (x, y) in [(7, 3), (3, 7), (8, 4), (-6, 2), (5, -5), (-4, -9)] {
        let forward = polyline(&[point(0, 0), point(x, y)], false);
        let backward = polyline(&[point(x, y), point(0, 0)], false);
        assert_eq!(set(&forward), set(&backward), "to ({x}, {y})");
        let major = i32::from(x.abs().max(y.abs())) + 1;
        assert_eq!(
            forward.len() as i32,
            major,
            "one pixel per step on the major axis"
        );
    }
}

#[test]
fn a_joint_is_drawn_once_and_not_last_drops_only_the_end() {
    let path = [point(0, 0), point(3, 0), point(3, 3)];
    let drawn = polyline(&path, false);
    assert_eq!(
        drawn.len(),
        set(&drawn).len(),
        "no pixel twice, so XOR is safe"
    );
    assert_eq!(
        set(&drawn),
        set(&[(0, 0), (1, 0), (2, 0), (3, 0), (3, 1), (3, 2), (3, 3)])
    );
    let not_last = polyline(&path, true);
    assert_eq!(set(&not_last), set(&drawn[..drawn.len() - 1]));
}

#[test]
fn a_closed_path_does_not_draw_its_start_twice_but_a_point_is_drawn() {
    let square = [
        point(0, 0),
        point(2, 0),
        point(2, 2),
        point(0, 2),
        point(0, 0),
    ];
    let drawn = polyline(&square, false);
    assert_eq!(drawn.len(), 8);
    assert_eq!(set(&drawn).len(), 8);
    assert_eq!(polyline(&[point(5, 5), point(5, 5)], false), vec![(5, 5)]);
    assert!(polyline(&[point(5, 5)], false).is_empty());
}

fn arc(width: u16, height: u16, angle1: i16, angle2: i16) -> XArc {
    XArc {
        x: 0,
        y: 0,
        width,
        height,
        angle1,
        angle2,
    }
}

#[test]
fn a_circle_touches_each_side_of_its_box_and_no_corner() {
    // An arc's box spans [x, x + width] by [y, y + height], one pixel wider
    // than a filled arc's.
    for size in [10u16, 11, 30] {
        let drawn = set(&arcs(&[arc(size, size, 0, 360 * 64)]));
        let s = i32::from(size);
        for (x, y) in &drawn {
            assert!(
                (0..=s).contains(x) && (0..=s).contains(y),
                "{size}: ({x}, {y})"
            );
        }
        assert!(drawn.iter().any(|(x, _)| *x == 0) && drawn.iter().any(|(x, _)| *x == s));
        assert!(drawn.iter().any(|(_, y)| *y == 0) && drawn.iter().any(|(_, y)| *y == s));
        for corner in [(0, 0), (s, 0), (0, s), (s, s)] {
            assert!(!drawn.contains(&corner), "{size}: corner {corner:?}");
        }
        assert!(!drawn.contains(&(s / 2, s / 2)), "a ring, not a disc");
    }
}

#[test]
fn a_quarter_arc_stays_in_its_quadrant() {
    // 0 to 90 degrees runs from 3 o'clock to 12 o'clock: the upper right.
    let drawn = set(&arcs(&[arc(40, 30, 0, 90 * 64)]));
    assert!(!drawn.is_empty());
    for (x, y) in &drawn {
        assert!(*x >= 20 && *y <= 15, "({x}, {y})");
    }
    assert!(arcs(&[arc(0, 0, 0, 360 * 64)]).is_empty());
}

fn dashed_gc(style: u8, dashes: &[u8], offset: u16, cap: u8) -> crate::XGraphicsContextValues {
    crate::XGraphicsContextValues {
        line_style: style,
        dashes: dashes.to_vec(),
        dash_offset: offset,
        cap_style: cap,
        foreground: 1,
        background: 2,
        ..crate::XGraphicsContextValues::default()
    }
}

fn inked(spans: crate::software::geometry::wide_line::XInkedSpans) -> Vec<(u32, (i32, i32))> {
    spans
        .into_iter()
        .flat_map(|(pixel, spans)| spans.into_iter().map(move |span| (pixel, (span.x, span.y))))
        .collect()
}

/// `fbZeroLine` steps the dash once per pixel and runs the offset on from
/// segment to segment; only the last segment draws its end point. With
/// dashes 2 on, 1 off: the first segment's four pixels are on, on, off, on,
/// and the second picks up four pixels into the pattern.
#[test]
fn a_dashed_thin_polyline_carries_its_phase_across_the_joint() {
    let gc = dashed_gc(crate::X_LINE_ON_OFF_DASH, &[2, 1], 0, crate::X_CAP_BUTT);
    let got = inked(super::dash::polyline(
        &[point(0, 0), point(4, 0), point(4, 4)],
        &gc,
    ));
    let want: Vec<(u32, (i32, i32))> = [(0, 0), (1, 0), (3, 0), (4, 0), (4, 2), (4, 3)]
        .into_iter()
        .map(|p| (1, p))
        .collect();
    assert_eq!(got, want);
}

/// A double dash paints the off dashes in the background, and the offset
/// shifts the pattern: starting one pixel in, the first pixel is the second
/// of the first dash.
#[test]
fn a_double_dashed_thin_line_paints_its_gaps_in_the_background() {
    let gc = dashed_gc(crate::X_LINE_DOUBLE_DASH, &[2, 1], 1, crate::X_CAP_BUTT);
    let got = inked(super::dash::polyline(&[point(0, 0), point(5, 0)], &gc));
    assert_eq!(
        got,
        vec![
            (1, (0, 0)),
            (2, (1, 0)),
            (1, (2, 0)),
            (1, (3, 0)),
            (2, (4, 0)),
            (1, (5, 0))
        ]
    );
}

/// An odd dash list is kept twice over, so on and off swap on each pass.
#[test]
fn an_odd_dash_list_alternates_its_phase_each_pass() {
    let gc = dashed_gc(crate::X_LINE_ON_OFF_DASH, &[1], 0, crate::X_CAP_BUTT);
    let got = inked(super::dash::polyline(&[point(0, 0), point(5, 0)], &gc));
    let xs: Vec<i32> = got.into_iter().map(|(_, (x, _))| x).collect();
    assert_eq!(xs, vec![0, 2, 4]);
}
