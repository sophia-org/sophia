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
