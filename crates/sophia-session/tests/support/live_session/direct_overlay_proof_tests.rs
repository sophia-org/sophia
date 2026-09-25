//! The overlay proof control's rules.
//!
//! Kept beside the session's other tests rather than inside the module, so the
//! source-layout ledger stays a record of debt instead of gaining an exception.

use crate::live_session::direct_overlay_proof::{
    ACTIVATION_TICKS, DirectOverlayAction, DirectOverlayProof, FLIPS_BEFORE_ACTIVATION,
    overlay_projection,
};
use sophia_engine::CompositorDisplayCommand;
use sophia_protocol::{OutputId, Rect};

const OUTPUT: OutputId = OutputId::from_raw(1);

/// Off unless asked for. Every product session runs with this inert, so an
/// overlay appearing in one would be this control leaking into it.
#[test]
fn a_session_that_did_not_ask_never_activates() {
    let mut proof = DirectOverlayProof::new(false, ACTIVATION_TICKS);
    for _ in 0..1_000 {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_ACTIVATION * 10, Some(OUTPUT)),
            DirectOverlayAction::Idle
        );
    }
}

/// Counted, not timed. The transition being proven starts from a frame the
/// plane is scanning, so activating before one exists would prove the
/// composed path against a composed predecessor -- which is not the claim.
#[test]
fn activation_waits_for_flips_to_reach_glass() {
    let mut proof = DirectOverlayProof::new(true, ACTIVATION_TICKS);
    for flips in 0..FLIPS_BEFORE_ACTIVATION {
        assert_eq!(proof.tick(flips, Some(OUTPUT)), DirectOverlayAction::Idle);
    }
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(OUTPUT)),
        DirectOverlayAction::Activate(OUTPUT)
    );
}

/// No output carrying direct frames means nothing to overlay.
#[test]
fn activation_needs_an_output() {
    let mut proof = DirectOverlayProof::new(true, ACTIVATION_TICKS);
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION * 2, None),
        DirectOverlayAction::Idle
    );
}

/// Up for a bounded window, then down once -- and never again. A second
/// activation would open a second episode the verifier's brackets cannot
/// pair, and a withdrawal that never came would leave a bounded session
/// ending with the overlay still on glass.
#[test]
fn the_overlay_goes_up_once_and_comes_down_once() {
    let mut proof = DirectOverlayProof::new(true, ACTIVATION_TICKS);
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(OUTPUT)),
        DirectOverlayAction::Activate(OUTPUT)
    );

    let mut withdrawals = 0;
    let mut activations = 0;
    for _ in 0..(ACTIVATION_TICKS * 4) {
        match proof.tick(FLIPS_BEFORE_ACTIVATION * 2, Some(OUTPUT)) {
            DirectOverlayAction::Withdraw => withdrawals += 1,
            DirectOverlayAction::Activate(_) => activations += 1,
            DirectOverlayAction::Idle => {}
        }
    }
    assert_eq!(withdrawals, 1, "the overlay must come down exactly once");
    assert_eq!(activations, 0, "the overlay must not reopen");
}

/// The overlay paints. A frame carrying it cannot be scanned out directly,
/// and the whole proof rests on that being true of the command it emits.
#[test]
fn the_overlay_emits_a_command_that_requires_composition() {
    let head = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let overlay = overlay_projection(OUTPUT, 7, head);

    assert_eq!(overlay.output, OUTPUT);
    assert!(
        overlay.targets.is_empty(),
        "an interaction target would claim input this control cannot service"
    );
    let [CompositorDisplayCommand::Rect(rect)] = overlay.commands.as_slice() else {
        panic!("the overlay must emit exactly one painting rect")
    };
    assert!(rect.geometry.width > 0 && rect.geometry.height > 0);
    // Inside the head, so the operator sees it over the client rather than
    // off-screen where nothing would be proven visually.
    assert!(rect.geometry.x >= head.x && rect.geometry.y >= head.y);
    assert!(rect.geometry.x + rect.geometry.width <= head.x + head.width);
    assert!(rect.geometry.y + rect.geometry.height <= head.y + head.height);
}

/// The hold is a parameter, because a transition proof and a cost
/// measurement want different windows: the first wants the shortest one that
/// contains the transition, the second wants a composed population large
/// enough to have a distribution.
#[test]
fn the_overlay_holds_for_the_requested_number_of_ticks() {
    let output = OUTPUT;
    let mut proof = DirectOverlayProof::new(true, 3);
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
        DirectOverlayAction::Activate(output)
    );
    for _ in 0..2 {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
            DirectOverlayAction::Idle
        );
    }
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
        DirectOverlayAction::Withdraw,
        "a three-tick hold withdraws on the third tick after activating"
    );
}

/// A zero hold would open and close the overlay on the same tick, leaving a
/// window with no composed frame in it -- which the verifier reads as a
/// proof that never happened. Zero means "unspecified", so it takes the
/// default rather than degenerating.
#[test]
fn a_zero_hold_falls_back_to_the_default_window() {
    let output = OUTPUT;
    let mut proof = DirectOverlayProof::new(true, 0);
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
        DirectOverlayAction::Activate(output)
    );
    for _ in 0..(ACTIVATION_TICKS - 1) {
        assert_eq!(
            proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
            DirectOverlayAction::Idle
        );
    }
    assert_eq!(
        proof.tick(FLIPS_BEFORE_ACTIVATION, Some(output)),
        DirectOverlayAction::Withdraw
    );
}
