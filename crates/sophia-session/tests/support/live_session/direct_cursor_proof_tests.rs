//! The cursor proof's rules.

use crate::live_session::direct_cursor_proof::{
    DirectCursorAction, DirectCursorProof, FLIPS_BEFORE_MOTION, MOVES, TICKS_PER_MOVE,
    cursor_position,
};
use sophia_protocol::{OutputId, Rect};

const OUTPUT: OutputId = OutputId::from_raw(1);

const HEAD: Rect = Rect {
    x: 0,
    y: 0,
    width: 2560,
    height: 1440,
};

fn run_to_completion(proof: &mut DirectCursorProof) -> (Vec<usize>, Option<usize>) {
    let mut steps = Vec::new();
    let mut finished = None;
    for _ in 0..(MOVES as u32 * TICKS_PER_MOVE * 2) {
        match proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)) {
            DirectCursorAction::Move { step, .. } => steps.push(step),
            DirectCursorAction::Finished { moves } => {
                finished = Some(moves);
                break;
            }
            DirectCursorAction::Idle => {}
        }
    }
    (steps, finished)
}

#[test]
fn a_session_that_did_not_ask_never_moves_the_cursor() {
    let mut proof = DirectCursorProof::new(false);
    for _ in 0..1_000 {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
            DirectCursorAction::Idle
        );
    }
}

/// Motion starts only once frames are actually reaching the plane directly.
/// Moving before that would prove a cursor rides over composed frames, which
/// nobody doubted.
#[test]
fn motion_waits_for_direct_flips() {
    let mut proof = DirectCursorProof::new(true);
    for flips in 0..FLIPS_BEFORE_MOTION {
        assert_eq!(proof.tick(flips, Some(OUTPUT)), DirectCursorAction::Idle);
    }
    assert_eq!(
        proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
        DirectCursorAction::Move {
            output: OUTPUT,
            step: 0
        }
    );
}

/// And only while there is an output flipping directly to ride over.
#[test]
fn motion_waits_for_an_output() {
    let mut proof = DirectCursorProof::new(true);
    assert_eq!(
        proof.tick(FLIPS_BEFORE_MOTION, None),
        DirectCursorAction::Idle
    );
}

/// Every step is visited once, in order, and the proof then stops for good.
#[test]
fn the_cursor_visits_each_position_once_and_stops() {
    let mut proof = DirectCursorProof::new(true);
    let (steps, finished) = run_to_completion(&mut proof);
    assert_eq!(steps, (0..MOVES).collect::<Vec<_>>());
    assert_eq!(finished, Some(MOVES));
    for _ in 0..100 {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
            DirectCursorAction::Idle,
            "a finished proof stays finished"
        );
    }
}

/// Moves are spread across ticks. A dozen updates in a dozen consecutive
/// ticks would coalesce into one ioctl and say nothing about a cursor moving
/// across many flips.
#[test]
fn moves_are_spread_across_ticks() {
    let mut proof = DirectCursorProof::new(true);
    assert!(matches!(
        proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
        DirectCursorAction::Move { .. }
    ));
    for tick in 0..(TICKS_PER_MOVE - 1) {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
            DirectCursorAction::Idle,
            "tick {tick} between moves must be quiet"
        );
    }
    assert!(matches!(
        proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
        DirectCursorAction::Move { .. }
    ));
}

/// Direct scanout stopping stalls the proof rather than letting it report
/// motion over frames that were composed.
#[test]
fn motion_stalls_when_direct_scanout_stops() {
    let mut proof = DirectCursorProof::new(true);
    assert!(matches!(
        proof.tick(FLIPS_BEFORE_MOTION, Some(OUTPUT)),
        DirectCursorAction::Move { .. }
    ));
    for _ in 0..(TICKS_PER_MOVE * 4) {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_MOTION, None),
            DirectCursorAction::Idle
        );
    }
}

/// The path stays inside the head and actually moves.
#[test]
fn the_path_sweeps_within_the_head() {
    let first = cursor_position(HEAD, 0);
    let last = cursor_position(HEAD, MOVES - 1);
    assert!(last.x > first.x, "the cursor has to actually move");
    for step in 0..MOVES {
        let position = cursor_position(HEAD, step);
        assert!(
            position.x >= f64::from(HEAD.x)
                && position.x < f64::from(HEAD.x.saturating_add(HEAD.width)),
            "step {step} left the head horizontally: {position:?}"
        );
        assert!(
            position.y >= f64::from(HEAD.y)
                && position.y < f64::from(HEAD.y.saturating_add(HEAD.height)),
            "step {step} left the head vertically: {position:?}"
        );
    }
}
