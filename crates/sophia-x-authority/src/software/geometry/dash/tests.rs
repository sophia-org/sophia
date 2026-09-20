#![cfg(test)]

use super::{XDashState, split};
use crate::XPoint;

fn point(x: i16, y: i16) -> XPoint {
    XPoint { x, y }
}

#[test]
fn a_solid_pattern_returns_the_line_whole() {
    // So a caller needs no special case for an undashed line.
    let mut state = XDashState::new(&[], 0);
    let runs = split(point(0, 0), point(40, 0), &[], &mut state);
    assert_eq!(runs.len(), 1);
    assert!(runs[0].on);
    assert_eq!((runs[0].from, runs[0].to), (point(0, 0), point(40, 0)));
}

#[test]
fn a_pattern_alternates_on_and_off_starting_on() {
    let dashes = [4u8, 4];
    let mut state = XDashState::new(&dashes, 0);
    let runs = split(point(0, 0), point(16, 0), &dashes, &mut state);
    assert_eq!(runs.len(), 4);
    assert_eq!(
        runs.iter().map(|run| run.on).collect::<Vec<_>>(),
        [true, false, true, false]
    );
    assert_eq!(runs[0].from.x, 0);
    assert_eq!(runs[0].to.x, 4);
    assert_eq!(runs[1].to.x, 8);
}

#[test]
fn an_offset_starts_part_way_into_the_pattern() {
    let dashes = [4u8, 4];
    // Two units into the first on run: it has two left before the off run.
    let mut state = XDashState::new(&dashes, 2);
    let runs = split(point(0, 0), point(8, 0), &dashes, &mut state);
    assert!(runs[0].on);
    assert_eq!(
        runs[0].to.x, 2,
        "the first run is the remainder of the on run"
    );
    assert!(!runs[1].on);

    // Six units in lands inside the off run.
    let mut state = XDashState::new(&dashes, 6);
    let runs = split(point(0, 0), point(8, 0), &dashes, &mut state);
    assert!(!runs[0].on);
}

#[test]
fn an_offset_past_the_pattern_wraps() {
    let dashes = [4u8, 4];
    let wrapped = XDashState::new(&dashes, 10);
    let direct = XDashState::new(&dashes, 2);
    let mut a = wrapped;
    let mut b = direct;
    assert_eq!(
        split(point(0, 0), point(8, 0), &dashes, &mut a),
        split(point(0, 0), point(8, 0), &dashes, &mut b)
    );
}

#[test]
fn the_pattern_continues_across_a_corner_rather_than_restarting() {
    // A dashed rectangle outline should be dashed evenly around its corners;
    // restarting at every vertex puts a dash at each one and reads as four
    // separate lines.
    let dashes = [4u8, 4];
    let mut state = XDashState::new(&dashes, 0);
    let first = split(point(0, 0), point(6, 0), &dashes, &mut state);
    let second = split(point(6, 0), point(6, 6), &dashes, &mut state);
    assert!(!first.last().expect("a run").on || first.len() > 1);
    assert!(
        !second[0].on,
        "the second segment resumes in the off run the first ended in"
    );
}

#[test]
fn a_degenerate_segment_still_produces_one_run() {
    let dashes = [4u8, 4];
    let mut state = XDashState::new(&dashes, 0);
    let runs = split(point(5, 5), point(5, 5), &dashes, &mut state);
    assert_eq!(runs.len(), 1);
}
