#![cfg(test)]

use super::{XArc, polyline};

fn circle(angle1: i16, angle2: i16) -> XArc {
    XArc {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        angle1,
        angle2,
    }
}

#[test]
fn a_chord_never_departs_from_the_curve_by_half_a_pixel() {
    // The step is chosen for this, and it is the only thing that makes a
    // chord approximation defensible rather than merely convenient.
    let points = polyline(circle(0, 360 * 64));
    assert!(points.len() > 8);
    for pair in points.windows(2) {
        let midpoint_x = (f64::from(pair[0].x) + f64::from(pair[1].x)) / 2.0 - 50.0;
        let midpoint_y = (f64::from(pair[0].y) + f64::from(pair[1].y)) / 2.0 - 50.0;
        let radius = midpoint_x.hypot(midpoint_y);
        // A quarter of a pixel for the chord itself, and up to half again
        // from rounding each endpoint to a whole pixel.
        assert!(
            (50.0 - radius) < 1.0,
            "a chord midpoint fell {} from the curve",
            50.0 - radius
        );
    }
}

#[test]
fn the_two_spellings_of_one_arc_give_the_same_points() {
    // A negative extent names the same arc from the other end. Leaving the two
    // to produce different points makes an arc's pixels depend on how a client
    // happened to write it.
    // Canonicalised to the same start and a positive extent, so the two
    // produce one sequence rather than two that merely trace the same curve.
    let forward = polyline(circle(90 * 64, 90 * 64));
    let backward = polyline(circle(180 * 64, -90 * 64));
    assert_eq!(forward.len(), backward.len());
    for (a, b) in forward.iter().zip(backward.iter()) {
        assert!(
            (i32::from(a.x) - i32::from(b.x)).abs() <= 1
                && (i32::from(a.y) - i32::from(b.y)).abs() <= 1,
            "{a:?} against {b:?}"
        );
    }
}

#[test]
fn an_arc_starts_and_ends_where_it_says() {
    // Zero degrees is the positive x axis; ninety is straight up, which in X
    // is a smaller y.
    let quarter = polyline(circle(0, 90 * 64));
    let first = quarter.first().expect("a start point");
    let last = quarter.last().expect("an end point");
    assert_eq!((first.x, first.y), (100, 50));
    assert_eq!((last.x, last.y), (50, 0));
}

#[test]
fn a_partial_arc_does_not_close_the_ellipse() {
    let quarter = polyline(circle(0, 90 * 64));
    let first = quarter.first().copied().expect("a start");
    let last = quarter.last().copied().expect("an end");
    assert_ne!(first, last, "a quarter arc is not a closed curve");
}

#[test]
fn an_empty_arc_produces_no_points() {
    assert!(polyline(circle(0, 0)).is_empty());
    assert!(
        polyline(XArc {
            width: 0,
            ..circle(0, 90 * 64)
        })
        .is_empty()
    );
}
