//! G3-5d d-2b3e: the Session capture forms, mapped onto the d-0 controls
//! without renaming them. Which guard answered is the fixture's own record
//! of its pre-classification check (`check_observation`): None when none
//! ran, `Some(Err(refusal))` when one refused. The Session's disposition
//! cannot tell the forms apart; only that record does. Form 1 (a capture
//! before reserve) and form 2 (an ordinary frame queued after reserve with
//! its capture requested) are here; form 3 (the discarded frame's capture)
//! is the two d-0 rows in combined_consume.rs. Every state here is the
//! existing stub fixture composition; the capture is the exporter's own
//! flags set through its real calls, never a CaptureOwner endpoint or a
//! completed capture.
use super::consume::{FAILED, MOVED};
use super::*;
use sophia_backend_live::CombinedAcceptRefusal;
use sophia_protocol::Point;

/// Form 1: a capture present before admission. The composition is
/// ineligible, so nothing is reserved and no row entry exists; the frame
/// takes today's ordinary path and installs as a queued ordinary entry (not
/// a render or a flip). No consumption check runs.
#[test]
fn a_capture_before_reserve_declines_to_the_ordinary_path() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.native.capture_left_by_discard(output().id);
    assert!(
        lane.native.capture_pending(output().id),
        "form 1: the capture is present before admission"
    );
    assert_eq!(lane.native.check_observation(), (0, None));
    lane.submit(&mut a, 1, PIXEL);
    lane.drive_until(&mut a, "the ordinary copy", |lane, a| {
        lane.residency(a, 1) == FrameImageResidency::HeldForFrame
    });
    assert_eq!(
        lane.native.composes(),
        1,
        "form 1: composed once and found ineligible"
    );
    assert_eq!(
        lane.native.reserve_attempts(),
        0,
        "form 1: nothing reserved"
    );
    assert!(lane.states().is_empty(), "form 1: no row entry");
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(
        lane.native.check_observation(),
        (0, None),
        "form 1: no consumption check ran"
    );
    assert_eq!(
        lane.native.installs(),
        1,
        "form 1: the ordinary install entry"
    );
    assert_eq!(
        lane.native.ordinary_queues(),
        1,
        "form 1: queued, not rendered"
    );
    assert_eq!(lane.native.classifies(), 0, "form 1: no classification");
    assert_eq!(lane.native.accepts(), 0, "form 1: no combined accept");
    assert_eq!(
        lane.native.release_attempts(),
        0,
        "form 1: nothing to release"
    );
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "form 1: pending"
    );
    assert!(a.terminals(1).is_empty(), "form 1: no terminal");
    assert!(
        lane.native.capture_pending(output().id),
        "form 1: the capture flags stay"
    );
    assert!(lane.trace_no_admission() >= 1, "form 1: the ordinary join");
}

/// Form 2: an ordinary frame queued after reserve with its capture
/// requested, the declared fixture state. The exporter's own refusal,
/// OrdinaryPending, answers before the final CaptureActive guard: the result
/// takes the baseline failure terminal before any classification, and the
/// queued frame and its capture are both still there afterwards.
fn an_ordinary_frame_queued_with_a_capture_takes_the_failure_terminal(
    plane: Option<Point>,
    label: &str,
) {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    let (checks, _) = lane.native.check_observation();
    lane.native.capture_queued_on_pending_frame(output().id);
    assert!(
        lane.native.pending_mixed_frame(output().id),
        "{label}: queued"
    );
    assert!(
        lane.native.capture_pending(output().id),
        "{label}: capture requested"
    );
    let before = lane.native.head_observation(output().id);
    let served = lane.consume(&mut a, 1, Instant::now(), plane, true);
    assert!(served.is_ok(), "{label}: refused without error: {served:?}");
    assert_eq!(
        lane.native.check_observation(),
        (
            checks + 1,
            Some(Err(CombinedAcceptRefusal::OrdinaryPending))
        ),
        "{label}: one check, the queued frame answered before the capture"
    );
    assert_eq!(a.terminals(1), [FAILED], "{label}: the failure terminal");
    assert_eq!(lane.native.installs(), 0, "{label}: no install entry");
    assert_eq!(lane.native.ordinary_queues(), 0, "{label}: no queue");
    assert_eq!(lane.native.accepts(), 0, "{label}: no accept");
    assert_eq!(lane.native.classifies(), 0, "{label}: before the scene");
    assert_eq!(lane.native.release_attempts(), 1, "{label}: released");
    assert_eq!(lane.native.release_transfers(), 1, "{label}: transferred");
    assert!(lane.states().is_empty(), "{label}: the entry is gone");
    lane.trace_end(&[], &[]);
    assert!(lane.pending(&a).is_empty(), "{label}: no Prepared");
    assert_eq!(
        lane.native.head_observation(output().id),
        before,
        "{label}: native state unchanged"
    );
    assert!(
        lane.native.pending_mixed_frame(output().id),
        "{label}: still queued"
    );
    assert!(
        lane.native.capture_pending(output().id),
        "{label}: capture still present"
    );
}

#[test]
fn an_ordinary_frame_queued_with_a_capture_refuses_an_otherwise_matching_result() {
    an_ordinary_frame_queued_with_a_capture_takes_the_failure_terminal(None, "form 2, match");
}

/// The moved plane is control 2's real scene change.
#[test]
fn an_ordinary_frame_queued_with_a_capture_refuses_a_scene_churn_result() {
    an_ordinary_frame_queued_with_a_capture_takes_the_failure_terminal(MOVED, "form 2, churn");
}
