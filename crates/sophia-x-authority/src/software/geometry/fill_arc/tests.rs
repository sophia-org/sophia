#![cfg(test)]
//! Filled arcs against the protocol's rule: a pixel is drawn when its centre,
//! on integral coordinates, lies inside the ellipse and, for a partial arc,
//! inside its pie slice or chord.

use super::{XArc, fill};
use std::collections::BTreeSet;

fn arc(x: i16, y: i16, width: u16, height: u16, angle1: i16, angle2: i16) -> XArc {
    XArc {
        x,
        y,
        width,
        height,
        angle1,
        angle2,
    }
}

fn pixels(arcs: &[XArc], pie_slice: bool) -> BTreeSet<(i32, i32)> {
    fill(arcs, pie_slice)
        .iter()
        .flat_map(|span| (span.x..span.x + span.width).map(move |x| (x, span.y)))
        .collect()
}

/// The pixels whose centres are strictly inside the ellipse inscribed in the
/// arc's box, whose boundary runs through the box's edges.
fn strictly_inside(x: i32, y: i32, width: i32, height: i32) -> BTreeSet<(i32, i32)> {
    let (cx, cy) = (
        f64::from(x) + f64::from(width) / 2.0,
        f64::from(y) + f64::from(height) / 2.0,
    );
    let (rx, ry) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    (y..y + height)
        .flat_map(|py| (x..x + width).map(move |px| (px, py)))
        .filter(|(px, py)| {
            let dx = (f64::from(*px) - cx) / rx;
            let dy = (f64::from(*py) - cy) / ry;
            dx * dx + dy * dy < 1.0 - 1e-9
        })
        .collect()
}

#[test]
fn a_full_ellipse_holds_every_centre_inside_it_and_none_outside_its_box() {
    for (width, height) in [(20, 20), (21, 21), (30, 12), (13, 27), (900, 40)] {
        let drawn = pixels(&[arc(3, 5, width, height, 0, 360 * 64)], true);
        let inside = strictly_inside(3, 5, i32::from(width), i32::from(height));
        assert!(
            drawn.is_superset(&inside),
            "{width}x{height} misses a centre inside it"
        );
        for (x, y) in &drawn {
            assert!(
                (3..3 + i32::from(width)).contains(x) && (5..5 + i32::from(height)).contains(y)
            );
        }
    }
}

#[test]
fn a_pie_slice_is_a_wedge_and_a_chord_is_cut_straight_across() {
    // The quarter from 3 o'clock to 12 o'clock of a disc in (0, 0, 40, 40):
    // as a pie slice it is the upper-right quarter; as a chord, the part of
    // that quarter beyond the line from (40, 20) to (20, 0).
    let quarter = arc(0, 0, 40, 40, 0, 90 * 64);
    let pie = pixels(&[quarter], true);
    let chord = pixels(&[quarter], false);
    assert!(pie.contains(&(25, 15)) && pie.contains(&(35, 18)) && pie.contains(&(22, 3)));
    assert!(
        !pie.contains(&(15, 15)) && !pie.contains(&(25, 25)),
        "nothing left of or below the centre"
    );
    assert!(
        chord.contains(&(35, 8)) && !chord.contains(&(25, 15)),
        "the chord cuts off the wedge's middle"
    );
    assert!(chord.is_subset(&pie));
}

#[test]
fn a_full_turn_ignores_the_arc_mode_and_an_empty_arc_draws_nothing() {
    let disc = arc(0, 0, 16, 16, 0, 360 * 64);
    assert_eq!(pixels(&[disc], true), pixels(&[disc], false));
    // Angles past a full turn are a full turn.
    assert_eq!(
        pixels(&[arc(0, 0, 16, 16, 0, 500 * 64)], true),
        pixels(&[disc], true)
    );
    for empty in [
        arc(0, 0, 16, 16, 0, 0),
        arc(0, 0, 0, 16, 0, 360 * 64),
        arc(0, 0, 1, 15, 0, 360 * 64),
    ] {
        assert!(fill(&[empty], true).is_empty(), "{empty:?}");
    }
}

#[test]
fn opposite_sweeps_over_the_same_span_fill_the_same_pixels() {
    // (angle1, +extent) and (angle1 + extent, -extent) name one arc.
    let forward = pixels(&[arc(0, 0, 50, 30, 30 * 64, 100 * 64)], true);
    let backward = pixels(&[arc(0, 0, 50, 30, 130 * 64, -100 * 64)], true);
    assert_eq!(forward, backward);
}
