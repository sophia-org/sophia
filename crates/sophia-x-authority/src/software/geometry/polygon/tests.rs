#![cfg(test)]

use super::{convex, general as fill};
use crate::XPoint;

fn point(x: i16, y: i16) -> XPoint {
    XPoint { x, y }
}

fn covers(spans: &[sophia_protocol::Rect], x: i32, y: i32) -> bool {
    spans
        .iter()
        .any(|span| span.y == y && x >= span.x && x < span.x + span.width)
}

#[test]
fn a_square_fills_its_interior_and_stops_at_its_edges() {
    let square = [point(0, 0), point(4, 0), point(4, 4), point(0, 4)];
    let spans = fill(&square, false);
    assert!(covers(&spans, 0, 0));
    assert!(covers(&spans, 3, 3));
    assert!(
        !covers(&spans, 0, 4),
        "spans are half open in y, so abutting shapes neither gap nor overlap"
    );
    assert!(!covers(&spans, 4, 0), "nor overlap in x");
}

#[test]
fn a_triangle_narrows_as_it_rises() {
    let triangle = [point(0, 0), point(8, 0), point(4, 8)];
    let spans = fill(&triangle, false);
    let width_at = |row: i32| -> i32 {
        spans
            .iter()
            .filter(|span| span.y == row)
            .map(|span| span.width)
            .sum()
    };
    assert!(
        width_at(0) > width_at(4),
        "the base is wider than the middle"
    );
    assert!(
        width_at(4) > width_at(7),
        "and the middle wider than the tip"
    );
}

#[test]
fn the_two_rules_differ_exactly_where_a_polygon_crosses_itself() {
    // A five-pointed star drawn as one self-intersecting path. Even-odd leaves
    // the middle hollow; winding fills it. This is the whole reason the rule
    // is a graphics context component, and yserver never reads it.
    let star = [
        point(50, 0),
        point(20, 90),
        point(95, 35),
        point(5, 35),
        point(80, 90),
    ];
    let even_odd = fill(&star, false);
    let winding = fill(&star, true);
    assert!(
        !covers(&even_odd, 50, 45),
        "even-odd leaves the centre of a star hollow"
    );
    assert!(covers(&winding, 50, 45), "winding fills it");
    // Outside the crossing the two agree.
    assert_eq!(covers(&even_odd, 50, 10), covers(&winding, 50, 10));
}

#[test]
fn a_degenerate_polygon_fills_nothing() {
    assert!(fill(&[], false).is_empty());
    assert!(fill(&[point(0, 0)], false).is_empty());
    assert!(fill(&[point(0, 0), point(4, 4)], false).is_empty());
    // A zero-height polygon has no scanline to sample.
    assert!(fill(&[point(0, 0), point(4, 0), point(8, 0)], false).is_empty());
}

/// The pixels a fill covers, row by row.
fn rows(spans: &[sophia_protocol::Rect]) -> Vec<(i32, i32, i32)> {
    let mut rows: Vec<_> = spans
        .iter()
        .map(|span| (span.y, span.x, span.x + span.width))
        .collect();
    rows.sort_unstable();
    rows
}

#[test]
fn a_pixel_is_inside_when_its_centre_is_and_centres_are_integral() {
    // (0, 0), (10, 0), (0, 3): the slanted edge crosses row 1 at x = 6.67
    // and row 2 at x = 3.33, so centres 0..=6 and 0..=3 are inside. Sampling
    // the middle of each row instead, as the filler before `mi`'s did, gives
    // 0..=4 and 0..=1.
    let triangle = [point(0, 0), point(10, 0), point(0, 3)];
    assert_eq!(
        rows(&fill(&triangle, false)),
        vec![(0, 0, 10), (1, 0, 7), (2, 0, 4)]
    );
}

#[test]
fn a_centre_on_an_edge_counts_only_with_the_interior_to_its_right() {
    // Both triangles have an edge through centres (1, 1) and (2, 2). Where
    // the interior lies right of it the centre is drawn; left of it, not.
    let right_of = [point(0, 0), point(4, 0), point(4, 4)];
    assert_eq!(
        rows(&fill(&right_of, false)),
        vec![(0, 0, 4), (1, 1, 4), (2, 2, 4), (3, 3, 4)]
    );
    let left_of = [point(0, 0), point(4, 0), point(0, 4)];
    assert_eq!(
        rows(&fill(&left_of, false)),
        vec![(0, 0, 4), (1, 0, 3), (2, 0, 2), (3, 0, 1)]
    );
}

#[test]
fn the_convex_filler_agrees_on_a_convex_polygon_and_survives_a_false_claim() {
    let pentagon = [
        point(10, 0),
        point(19, 7),
        point(15, 18),
        point(4, 18),
        point(0, 7),
    ];
    assert_eq!(rows(&convex(&pentagon)), rows(&fill(&pentagon, false)));
    // `mi` leaves a false claim of convexity undefined. Whatever it draws,
    // the walk must end and stay inside the polygon's bounds.
    let star = [
        point(50, 0),
        point(20, 90),
        point(95, 35),
        point(5, 35),
        point(80, 90),
    ];
    for span in convex(&star) {
        assert!(span.x >= 5 && span.x + span.width <= 95, "{span:?}");
        assert!((0..90).contains(&span.y), "{span:?}");
    }
}
