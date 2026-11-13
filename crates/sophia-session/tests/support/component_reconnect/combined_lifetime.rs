//! G3-5c Session controls for REV3's lifetime table, through the controlled
//! walk (`turn_combined`) at a supplied turn Instant and the bar's real
//! content service. Release facts are counted apart: `release_attempts`,
//! `release_transfers` and `renderer_releases` (see the fixture).
//!
//! Clocks (F4). Once a control supplies a later Instant, every later turn
//! and consumption uses that same Instant or a later one; nothing goes back.
//! The content candidate's own clock is the real one unless a control
//! advances it through the labelled `advance_content_clock` seam, and each
//! control asserts the runtime's candidate fact it relies on.
use super::consume::LATER;
use super::*;
use crate::shell_component_connections::combined_row::Verdict;
use sophia_runtime::{FrameCandidateState, FrameCopyBinding};
use sophia_shell_client::RingOfferState;

const FAILED: (u16, u16) = (3, ContentReason::RendererFailed as u16);

impl Lane {
    /// The runtime's own fact for `job`'s candidate, read through `a`'s
    /// connection (`content_frame_candidate_state`).
    pub(super) fn candidate(&mut self, a: &Attempt, job: u64) -> Option<FrameCandidateState> {
        let binding = self
            .h
            .owner
            .processes
            .combined_row(0)
            .binding(job)
            .expect("the row holds the job");
        self.candidate_of(a, binding, job)
    }

    /// As `candidate`, for a binding the row no longer holds.
    pub(super) fn candidate_of(
        &mut self,
        a: &Attempt,
        binding: FrameCopyBinding,
        job: u64,
    ) -> Option<FrameCandidateState> {
        self.h
            .owner
            .with_service(a.key, |_, t| t.content_frame_candidate_state(binding, job))
            .unwrap()
    }

    /// Hooked turns at the one supplied `at` until `done`, with `a`'s
    /// content service only when `content`. Any slot failure panics.
    pub(super) fn turns_at(
        &mut self,
        a: &mut Attempt,
        at: Instant,
        content: bool,
        what: &str,
        mut done: impl FnMut(&mut Self, &Attempt) -> bool,
    ) {
        let deadline = Instant::now() + WAIT;
        while !done(self, a) {
            a.peer.poll_io().unwrap();
            if let Some(error) = self.turn_at(at) {
                panic!("{what}: slot failure {error:?}");
            }
            if content {
                self.content(a, false);
            } else {
                observe_peer(a);
            }
            assert!(
                Instant::now() < deadline,
                "{what} stalled: {:?}",
                self.states()
            );
            std::thread::yield_now();
        }
    }
}

/// The peer's ring offer states for candidate `n`'s resource.
pub(super) fn offers(a: &Attempt, n: u64) -> Vec<RingOfferState> {
    a.peer
        .image_adapter()
        .unwrap()
        .ring_offers()
        .into_iter()
        .filter(|offer| offer.resource == RESOURCE.id && offer.resource_generation == n)
        .map(|offer| offer.state)
        .collect()
}

/// Control 7a (C4): a Mismatch is delivered as Malformed, so no bundle can
/// come. Same grant, no later generation: Unknown. The entry stays before
/// its deadline and is removed at it, with no second outcome; the source's
/// exact SlotReleased is observed by the peer's producer, once.
#[test]
fn a_malformed_mismatch_entry_retires_without_a_second_outcome() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), StubReply::Mismatch);
    assert_eq!(lane.until(&mut a, "failed"), job);
    lane.drive_until(&mut a, "the candidate's rejection", |_, a| {
        !a.terminals(1).is_empty()
    });
    let late = Instant::now() + LATER;
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.states(),
        [(job, "failed")],
        "c7a: Failed stays while Unknown before its deadline"
    );
    assert_eq!(
        lane.candidate(&a, job),
        Some(FrameCandidateState::Unknown),
        "c7a: the candidate fact is Unknown"
    );
    assert!(lane.turn_at(late).is_none());
    assert!(lane.states().is_empty(), "c7a: retired at its deadline");
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 0);
    lane.turns_at(&mut a, late, true, "c7a: the source release", |_, a| {
        offers(a, 1) != [RingOfferState::Pending]
    });
    assert_eq!(
        offers(&a, 1),
        [RingOfferState::Refused],
        "c7a: SlotReleased once, for the refused source"
    );
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Malformed as u16)],
        "c7a: the Malformed outcome, once, and no RendererFailed"
    );
    assert_eq!((a.accepted, a.refused), (0, 1), "c7a: one ResourceStatus");
    assert_eq!(lane.native.release_attempts(), 0, "c7a: no lease");
}

/// Candidate `n` Held with its result validated, and the runtime's
/// candidate reconciled to Pending; the job.
fn held_pending(lane: &mut Lane, a: &mut Attempt, n: u64) -> u64 {
    let job = lane.held(a, n);
    lane.drive_until(a, "the candidate's reconciliation", |lane, a| {
        lane.candidate(a, job) == Some(FrameCandidateState::Pending)
    });
    job
}

/// Control 16 (R2-1): the walk expires a live candidate's Held result with
/// the Release cell Full; a later flush transfers it while the candidate is
/// still Pending; the bundle then takes the failure terminal. Every turn
/// after the expiry, and the consumption, use the one later Instant.
#[test]
fn an_expiry_verdict_survives_a_later_debt_transfer_and_fails_its_bundle() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = held_pending(&mut lane, &mut a, 1);
    let late = Instant::now() + LATER;
    lane.native.fill_renderer_queue();
    assert!(lane.turn_at(late).is_none());
    assert_eq!(lane.states(), [(job, "debt")]);
    assert_eq!(
        (lane.verdict(job), lane.candidate(&a, job)),
        (Some(Verdict::Expired), Some(FrameCandidateState::Pending)),
        "c16: the walk expired the live candidate"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn_at(late).is_none());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "failed")], Some(Verdict::Expired)),
        "c16: Failed(Expired) after the transfer"
    );
    assert_eq!(
        lane.candidate(&a, job),
        Some(FrameCandidateState::Pending),
        "c16: still Pending at the transfer"
    );
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 1);
    lane.consume(&mut a, 1, late, None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "c16: the expired candidate takes the failure terminal"
    );
    assert_eq!(lane.native.installs(), 0, "c16: no install");
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.classifies(), 0);
    assert!(lane.states().is_empty(), "c16: consumed");
    assert_eq!(lane.native.release_attempts(), 2, "release, then flush");
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(
        lane.native.renderer_releases(),
        1,
        "c16: one Release reached the renderer"
    );
}

/// Control 16's sibling: the walk's Release transfers at once.
#[test]
fn an_expiry_verdict_survives_an_immediate_transfer_and_fails_its_bundle() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = held_pending(&mut lane, &mut a, 1);
    let late = Instant::now() + LATER;
    assert!(lane.turn_at(late).is_none());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "failed")], Some(Verdict::Expired)),
        "c16 immediate: Failed(Expired) after the transfer"
    );
    assert_eq!(
        lane.candidate(&a, job),
        Some(FrameCandidateState::Pending),
        "c16 immediate: still Pending"
    );
    lane.consume(&mut a, 1, late, None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "c16 immediate: the failure terminal"
    );
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.release_attempts(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
}

/// Control 17 (R2-2): the grant and stream stay open and every turn is
/// before the admission deadline. With no native the decision is recorded
/// and the terminal issued; the next walk releases the Held lease.
#[test]
fn a_no_native_consumption_records_its_decision_and_the_next_walk_releases_it() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    let served = lane.consume(&mut a, 1, Instant::now(), None, false);
    assert_eq!(
        served,
        Ok(()),
        "c17: the no-native match never goes ordinary"
    );
    assert_eq!(a.terminals(1), [FAILED], "c17: one terminal, no install");
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert!(lane.pending(&a).is_empty());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "held")], Some(Verdict::Consumed)),
        "c17: the decision is recorded and the lease still Held"
    );
    assert_eq!(lane.native.release_attempts(), 0);
    lane.native.fill_renderer_queue();
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.native.release_attempts(),
        1,
        "c17: released at the next walk, before its deadline"
    );
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "debt")], Some(Verdict::Consumed)),
        "c17: Consumed Debt"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty(), "c17: the flush transferred");
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
    assert!(lane.turn().is_none());
    assert_eq!(lane.native.release_attempts(), 2, "c17: nothing more");
    lane.content(&mut a, false);
    assert_eq!(a.terminals(1), [FAILED], "c17: no second terminal");
}
