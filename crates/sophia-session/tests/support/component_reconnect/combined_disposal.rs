//! G3-5c Session controls for the walk's disposal distinctions (F3): Held
//! on Unknown before content reconciliation, Held or Failed on Unknown
//! after a same-generation end and on an exact Ended fact, and a same-grant
//! entry of another generation at the join. Each reads the runtime's own
//! candidate fact before relying on it.
//!
//! Labelled: control 13a holds back reconciliation by not running the
//! content service between the result and the walks; nothing is stood in.
//! The successful result turn collects the result, the owner's image
//! service settles it (`finish_frame_copy` takes the copy) and only then
//! does the row walk, so the fact there is Unknown, not Assembling. Held
//! while Assembling is not exercised by these controls; whether an error
//! or interruption path reaches it stays an open reachability question.
//! Controls 13b, 13c and 7b advance only the content candidate's clock,
//! through the test-only `advance_content_clock` seam, so the candidate's
//! own expiry ends it; the row's Instant is supplied separately and never
//! goes back.
use super::consume::LATER;
use super::*;
use crate::shell_component_connections::images::advance_content_clock;
use sophia_backend_live::CombinedRecompose;
use sophia_runtime::FrameCandidateState::{Ended, Pending, Unknown};

const FAILED: (u16, u16) = (3, ContentReason::RendererFailed as u16);
const TIMEOUT: (u16, u16) = (3, ContentReason::Timeout as u16);
/// Past any candidate's preparation deadline on the content clock.
const EXPIRY_MSEC: u64 = 600_000;

/// Candidate `n` ended by its own expiry: the content clock moves past its
/// deadline and one content service publishes the terminal. The peer then
/// reads it in the poll-then-observe order of `turns_at`, bounded by WAIT,
/// with no further owner or content service and no deadline refreshed.
fn expire_candidate(lane: &mut Lane, a: &mut Attempt, n: u64, label: &str) {
    advance_content_clock(EXPIRY_MSEC);
    lane.content(a, false);
    let deadline = Instant::now() + WAIT;
    while a.terminals(n).is_empty() {
        a.peer.poll_io().unwrap();
        observe_peer(a);
        assert!(
            Instant::now() < deadline,
            "{label}: the peer never read the expiry"
        );
        std::thread::yield_now();
    }
}

/// Candidate `n` Held and reconciled to Pending, then ended by its own
/// expiry on the advanced content clock, with the stream still open; the
/// job and the unchanged turn Instant used from here on.
fn held_then_expired(lane: &mut Lane, a: &mut Attempt, n: u64, label: &str) -> (u64, Instant) {
    let job = lane.held(a, n);
    let at = Instant::now();
    lane.turns_at(a, at, true, "the candidate's reconciliation", |lane, a| {
        lane.candidate(a, job) == Some(Pending)
    });
    expire_candidate(lane, a, n, label);
    assert_eq!(
        a.terminals(n),
        [TIMEOUT],
        "{label}: the candidate's own expiry, once"
    );
    assert_eq!(
        lane.candidate(a, job),
        Some(Unknown),
        "{label}: Unknown under the open stream"
    );
    (job, at)
}

/// Control 13a (rev 5): the result settled and reconciliation held back
/// before the deadline, so the fact is Unknown; the walks keep Held, then
/// reconciliation gives Pending and the result is a Match with no further
/// reserve. Rev 4 claimed Held while Assembling; this path cannot show it.
#[test]
fn held_waits_unknown_before_reconciliation_then_installs_without_a_second_render() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    let at = Instant::now();
    lane.turns_at(&mut a, at, false, "c13a: the Held result", |lane, a| {
        lane.states() == [(job, "held")]
            && lane.residency(a, 1) == FrameImageResidency::HeldForFrame
    });
    assert_eq!(
        lane.candidate(&a, job),
        Some(Unknown),
        "c13a: settled, reconciliation held back"
    );
    let reserves = lane.native.reserve_attempts();
    for _ in 0..3 {
        assert!(lane.turn_at(at).is_none());
    }
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "c13a: the walks keep Held while Unknown"
    );
    assert_eq!(lane.native.release_attempts(), 0, "c13a: nothing released");
    lane.content(&mut a, false);
    assert_eq!(
        lane.candidate(&a, job),
        Some(Pending),
        "c13a: reconciled once the content service runs"
    );
    lane.consume(&mut a, 1, at, None, true).unwrap();
    assert_eq!(lane.native.accepts(), 1, "c13a: the Match is accepted");
    assert_eq!(
        lane.native.reserve_attempts(),
        reserves,
        "c13a: no second render"
    );
    assert_eq!(lane.native.installs(), 0);
    assert!(lane.states().is_empty());
}

/// Control 13b: a same-generation end (the candidate's own expiry) leaves
/// the fact Unknown; Held waits until its deadline, then one release.
#[test]
fn held_on_a_same_generation_rejection_waits_unknown_until_its_deadline() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let (job, at) = held_then_expired(&mut lane, &mut a, 1, "c13b");
    assert!(lane.turn_at(at).is_none());
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "c13b: Held waits Unknown before its deadline"
    );
    assert_eq!(lane.native.release_attempts(), 0);
    let late = at + LATER;
    assert!(lane.turn_at(late).is_none());
    assert_eq!(
        lane.native.release_attempts(),
        1,
        "c13b: one release at its deadline"
    );
    assert!(lane.states().is_empty(), "c13b: the transfer removed it");
    assert!(lane.turn_at(late).is_none());
    assert_eq!(lane.native.release_attempts(), 1, "c13b: nothing more");
    assert_eq!(lane.native.renderer_releases(), 1);
    assert_eq!(a.terminals(1), [TIMEOUT], "c13b: no second outcome");
}

/// Control 13c: a later generation on the same stream makes the fact
/// Ended; Held is released at the next walk, with the turn Instant still
/// the one before the deadline.
#[test]
fn held_on_a_later_generation_releases_promptly_as_ended() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let (job, at) = held_then_expired(&mut lane, &mut a, 1, "c13c");
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    assert!(
        a.stream.has_credit(),
        "c13c: the expiry returned the credit"
    );
    lane.submit(&mut a, 2, PIXEL);
    lane.turns_at(&mut a, at, true, "c13c: the later generation", |lane, _| {
        lane.states().iter().all(|(held, _)| *held != job)
    });
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Ended),
        "c13c: the later generation ended it"
    );
    assert_eq!(
        lane.native.release_attempts(),
        1,
        "c13c: released promptly as Ended"
    );
    assert_eq!(lane.native.renderer_releases(), 1);
    assert_eq!(a.terminals(1), [TIMEOUT], "c13c: no second outcome");
}

/// Control 7b: a delivered failure (RenderFailed) whose bundle never comes,
/// then a later generation: Ended, so the entry is removed before its
/// deadline with no outcome from the row. (A live-grant Stopped stays an
/// explicit unexercised case.)
#[test]
fn a_failed_entry_without_a_bundle_retires_on_a_later_generation() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.rendered(&mut a, 1, StubReply::Failed, Instant::now(), "failed");
    let at = Instant::now();
    lane.turns_at(
        &mut a,
        at,
        true,
        "the candidate's reconciliation",
        |lane, a| lane.candidate(a, job) == Some(Pending),
    );
    expire_candidate(&mut lane, &mut a, 1, "c7b");
    assert_eq!(a.terminals(1), [TIMEOUT], "c7b: the candidate's own expiry");
    assert!(lane.turn_at(at).is_none());
    assert_eq!(
        lane.states(),
        [(job, "failed")],
        "c7b: Failed stays Unknown before its deadline"
    );
    assert_eq!(lane.candidate(&a, job), Some(Unknown));
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    assert!(a.stream.has_credit(), "c7b: the expiry returned the credit");
    lane.submit(&mut a, 2, PIXEL);
    lane.turns_at(&mut a, at, true, "c7b: the later generation", |lane, _| {
        lane.states().iter().all(|(held, _)| *held != job)
    });
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Ended),
        "c7b: the later generation ended it"
    );
    assert_eq!(
        a.terminals(1),
        [TIMEOUT],
        "c7b: no RendererFailed from the row"
    );
    assert_eq!(lane.native.release_attempts(), 0, "c7b: no lease");
}

/// Control 8, other generation: candidate 1 took its terminal while its
/// Release stays Debt (the cell Full); candidate 2 of the same grant and
/// output is admitted ordinarily while that cell is occupied, and its
/// bundle never joins the old entry.
#[test]
fn a_same_grant_entry_of_another_generation_never_joins() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let j1 = lane.held(&mut a, 1);
    lane.native
        .declare_classification(CombinedRecompose::CandidateFailure);
    lane.native.fill_renderer_queue();
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(a.terminals(1), [FAILED]);
    assert_eq!(lane.states(), [(j1, "debt")]);
    assert!(
        a.stream.has_credit(),
        "c8 generation: the terminal returned the credit"
    );
    lane.submit(&mut a, 2, PIXEL);
    lane.drive_until(&mut a, "candidate 2's ordinary copy", |lane, a| {
        lane.residency(a, 2) == FrameImageResidency::HeldForFrame
    });
    assert_eq!(
        lane.states(),
        [(j1, "debt")],
        "c8 generation: candidate 2 took no combined job"
    );
    let served = lane.consume(&mut a, 2, Instant::now(), None, true);
    assert!(
        served.is_ok(),
        "c8 generation: the old generation's entry never joins: {served:?}"
    );
    assert_eq!(lane.states(), [(j1, "debt")], "c8 generation: untouched");
    assert_eq!(lane.native.installs(), 1);
    assert_eq!(lane.native.classifies(), 1);
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 2)]);
}
