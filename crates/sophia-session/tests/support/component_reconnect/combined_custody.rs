//! G3-5d d-2b3a Session controls: running-core custody, Held/Unknown
//! against Ended (REV2 R3 as corrected by the d-2b3a reachability review).
//! Every owner turn is the split driver with native service, admission
//! included (b2's `admitted`). Each control starts from an already-rendered,
//! uncollected result (`rendered`): the render disposition is fixed and the
//! exported lease exists before its trigger.
//!
//! Two clocks are named where used: the admitted Instant deadline, which
//! only a turn's supplied Instant passes and which is never refreshed, and
//! the content clock, advanced only through `advance_content_clock`, and
//! only by the candidate timeout, so that the candidate's own deadline
//! passes and the resource transfer's does not (control (b)). Release has
//! two events, kept apart: the Transferred receipt, read from the release
//! counters in the turn whose walk enqueues the Release, and slot freedom,
//! `leased` back to 0 when the worker consumes it, waited for separately.
//! There is no worker ACK.
//!
//! Labelled: the trigger in each control is one owner or peer call between
//! turns (stop, a peer Close with one owner transport read, an owner
//! allocation invalidation); no control injects anything between a turn's
//! four calls, and none claims b2's uninterrupted T1 property.
use super::*;
use crate::shell_component_connections::images::{advance_content_clock, content_now_msec};
use sophia_runtime::FrameCandidateState::{Ended, Unknown};
use sophia_runtime::FrameCopyBinding;
use sophia_runtime::frame_credits::FrameStreamPhase;
use sophia_shell_client::FrameStreamState;

const TIMEOUT: (u16, u16) = (3, ContentReason::Timeout as u16);

impl Lane {
    /// The renderer's leased slots, as its worker publishes them.
    fn leased(&self) -> usize {
        self.native
            .slot_metrics(output().id)
            .expect("the head has a worker")
            .leased
    }

    /// (release attempts, Transferred receipts) so far.
    fn releases(&self) -> (usize, usize) {
        (
            self.native.release_attempts(),
            self.native.release_transfers(),
        )
    }

    /// Slot freedom: the worker consumed the enqueued Release, so no slot
    /// is leased. Bounded by WAIT, with no owner call.
    fn slot_freed(&self, label: &str) {
        let deadline = Instant::now() + WAIT;
        while self.leased() != 0 {
            assert!(
                Instant::now() < deadline,
                "{label}: the worker never consumed the Release"
            );
            std::thread::yield_now();
        }
    }

    /// One driver turn at `instant` with native service; its steps.
    fn quiet_driver_turn_at(&mut self, instant: Instant) -> (Vec<Step>, Vec<usize>) {
        let mut steps = Vec::new();
        let mut errors = Vec::new();
        let (turned, accepted) = self.driver_turn_at(&mut steps, instant, &mut |native| {
            if let Err(error) = native.service_native(output().id) {
                errors.push(error.to_string());
            }
        });
        let first = turned.expect("the turn completes");
        assert!(first.iter().all(Option::is_none), "{first:?}");
        assert!(errors.is_empty(), "native service: {errors:?}");
        (steps, accepted)
    }
}

/// Peer polls only, no owner call, until candidate `n` has a terminal.
fn peer_reads_terminal(a: &mut Attempt, n: u64, label: &str) {
    let deadline = Instant::now() + WAIT;
    while a.terminals(n).is_empty() {
        a.peer.poll_io().unwrap();
        observe_peer(a);
        assert!(
            Instant::now() < deadline,
            "{label}: the peer never read the terminal"
        );
        std::thread::yield_now();
    }
}

/// Candidate 1 admitted through b2's split-driver turns (`admitted`), then
/// its render result's ring, with no owner call after the turn that left
/// the job in flight: the result waits uncollected with its lease held.
fn rendered(lane: &mut Lane, wake: &sophia_wake::Wake, a: &mut Attempt) -> u64 {
    lane.submit(a, 1, PIXEL);
    let job = lane.admitted(a);
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the forwarded frame's render result rings the owner"
    );
    assert_eq!(lane.native.seen().len(), 1, "the seam read one occurrence");
    assert_eq!(lane.states(), [(job, "in flight")], "not yet collected");
    assert_eq!(lane.leased(), 1, "the exported result holds its slot");
    assert_eq!(lane.releases(), (0, 0));
    job
}

/// REV2 R3's revoke row. The grant is revoked after the result is published
/// and before any turn collects it. The next turn's images step collects
/// and delivers it into Held; content skips the revoked slot; the walk sees
/// the grant ended and releases Held in that same turn. Held exists only
/// between that turn's steps 1 and 3: no walk keeps it.
#[test]
fn a_grant_revoked_before_the_result_turn_releases_the_held_lease_in_that_turn() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    lane.h.owner.stop(a.key).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "stop collected nothing"
    );
    let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
    assert_eq!(
        steps,
        [Images, Walk, Native],
        "the revoked slot skips content and is still walked"
    );
    assert!(accepted.is_empty());
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)],
        "the images step collected the rendered result"
    );
    assert_eq!(
        lane.releases(),
        (1, 1),
        "the walk released the ended grant's Held lease: Transferred once"
    );
    assert!(lane.states().is_empty(), "the row is clear at the receipt");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    lane.slot_freed("revoke");
    assert_eq!(
        lane.quiet_driver_turn_at(Instant::now()).0,
        [Images, Walk, Native]
    );
    assert_eq!(lane.releases(), (1, 1), "nothing more");
}

/// REV2 R3's same-generation rejection row, as an expiry-induced rejection
/// on the content clock (not every possible rejection). The candidate's
/// deadline is its commit time plus the negotiated candidate timeout; the
/// resource transfer's is its reservation time plus the transfer timeout.
/// Both begin after `before`, so advancing the clock by exactly the
/// candidate timeout passes the first at the deciding turn, and the second
/// stays ahead while that turn ends before `before` plus the transfer
/// timeout, which is asserted. That turn's images step delivers the result
/// into Held, then settles the copy: the transfer completes, and
/// `finish_frame_copy` takes the copy gate with no failure, so its own
/// deadline check rejects the candidate with Timeout and content has no
/// bundle to join. With the stream open and the same generation the
/// candidate fact is Unknown, and the walk keeps Held before the admitted
/// Instant deadline. A later turn at an Instant past that deadline releases
/// it once. The binding is captured first, so the fact is read even if a
/// faulty walk removed the row.
#[test]
fn an_expired_candidate_keeps_held_unknown_until_the_deadline_then_releases() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let limits = lane
        .h
        .owner
        .with_service(a.key, |_, t| t.content_limits().cloned())
        .unwrap()
        .expect("negotiated limits");
    let (candidate, transfer) = (
        u64::from(limits.candidate_timeout_ms),
        u64::from(limits.transfer_timeout_ms),
    );
    assert!(candidate < transfer, "a window between the two deadlines");
    let before = content_now_msec();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let deadline = lane.deadline_of(job);
    advance_content_clock(candidate);
    let at = Instant::now();
    assert!(at < deadline, "the deciding turn precedes the deadline");
    let (steps, accepted) = lane.quiet_driver_turn_at(at);
    assert!(
        content_now_msec() < before + transfer,
        "precondition: the deciding turn ended before any transfer deadline"
    );
    assert_eq!(steps, TURN);
    assert_eq!(accepted, [0], "no bundle joined");
    peer_reads_terminal(&mut a, 1, "rejection");
    assert_eq!(
        a.terminals(1),
        [TIMEOUT],
        "the candidate's own expiry, once"
    );
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Unknown),
        "Unknown: same generation, stream open"
    );
    assert_eq!(a.stream.state(), FrameStreamState::Open);
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "Held kept before the deadline"
    );
    assert_eq!(lane.releases(), (0, 0), "nothing released");
    assert_eq!(lane.leased(), 1);
    let late = deadline + MARGIN;
    assert_eq!(lane.quiet_driver_turn_at(late).0, TURN);
    assert_eq!(lane.releases(), (1, 1), "one release at its deadline");
    assert!(lane.states().is_empty(), "the receipt removed it");
    lane.slot_freed("rejection");
    assert_eq!(lane.quiet_driver_turn_at(late).0, TURN);
    assert_eq!(lane.releases(), (1, 1), "nothing more");
    assert_eq!(a.terminals(1), [TIMEOUT], "no second outcome");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0);
}

/// REV2 R3's lost-allocation row, corrected: this lane's one allocation is
/// the frame stream's key allocation, so losing it closes the stream
/// (AllocationLost) and the fact is Ended, not Unknown. The original
/// open-stream prediction does not apply to this lane; a popout dismissal
/// is a different scenario, not covered here. The owner invalidates the
/// allocation after the result is published. In the result turn the images
/// step delivers the result into Held, then the owned copy's reconciliation
/// finds its description gone and stops it; content's
/// `reconcile_frame_streams` closes the stream for the missing context; the
/// walk releases Held as Ended in that turn. Held from a delivered Rendered
/// result and the later copy settlement are separate events.
///
/// Scope (d-2b3a publication disposition): this control is T1 owner-side
/// custody, then slot freedom. The candidate's exact terminal, its reason
/// and single publication, and its peer visibility are b3d obligations,
/// not asserted here; Ended alone does not fix the reason.
#[test]
fn a_lost_allocation_ends_the_candidate_and_releases_held_in_the_result_turn() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.invalidate_content_allocation(
                // Owner-side; clear of the lane's open (30) and commits (40 + n).
                TransactionId::from_raw(50),
                ALLOCATION,
                ContentReason::AllocationLost,
            )
        })
        .unwrap()
        .unwrap();
    let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
    assert_eq!(steps, TURN, "the grant is live: content runs");
    assert_eq!(accepted, [0], "no bundle joined");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)],
        "the result was collected before the allocation's loss was settled"
    );
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Ended),
        "the stream closed with its key allocation"
    );
    assert_eq!(
        lane.releases(),
        (1, 1),
        "the walk released Held as Ended: Transferred once"
    );
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0, "no install");
    lane.slot_freed("lost allocation");
}

/// REV2 R3's closed-stream row, corrected to a close-after-admission
/// lifecycle control: an explicit Close forbids new admissions and keeps
/// the admitted frame's lifecycle (credits.rs:155; frame_credits.rs:286-301),
/// and validation of the admitted assembly does not require Open, so the
/// previously admitted, unchanged-scene frame completes. The peer sends its
/// Close before the result turn. Before that turn the owner's stream is
/// still Open under the opening's transaction: the Close is read by the
/// turn's own owner input service (step 1) and admitted by its content
/// step, which then accepts the frame; native service submits it. The
/// owner-side witness is the exact stream's ledger fact after the turn,
/// Closing under the Close's transaction, read through a test-support
/// observer; the frame's Submitted fact alone would hold without a Close.
///
/// Scope (d-2b3a publication disposition): this control is T1 owner-side
/// lifecycle and custody. The peer's Closed record (transaction 31), its
/// Closing, no-credit and not-drained stream state, and any terminal are
/// b3d obligations, not asserted here.
#[test]
fn a_close_after_admission_keeps_the_admitted_frame_which_is_submitted() {
    const OPEN: TransactionId = TransactionId::from_raw(30);
    const CLOSE: TransactionId = TransactionId::from_raw(31);
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    a.stream.close(&mut a.peer, CLOSE).unwrap();
    a.peer.poll_io().unwrap();
    assert_eq!(
        lane.stream_fact(&a, binding),
        Some((FrameStreamPhase::Open, OPEN)),
        "no owner call has admitted the Close yet"
    );
    let t1 = lane.result_turn();
    assert_eq!(t1.steps, TURN);
    assert_eq!(t1.accepted, [1], "the admitted frame is accepted");
    assert_eq!(
        t1.status,
        Some(Submit::SubmittedWaitingForPageFlip),
        "native service submits it"
    );
    assert_eq!(
        lane.stream_fact(&a, binding),
        Some((FrameStreamPhase::Closing, CLOSE)),
        "T1 admitted the Close: the exact stream is Closing under its transaction"
    );
    assert!(lane.states().is_empty(), "accepted removes the entry");
    assert_eq!(lane.row().bindings().len(), 1, "no new admission");
    assert_eq!(lane.row().bindings()[0].job, job);
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "one pending presentation"
    );
    assert_eq!(lane.native.accepts(), 1);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert_eq!(lane.releases(), (0, 0), "the head holds the lease");
    assert_eq!(lane.leased(), 1);
}

impl Lane {
    /// The owner's fact for `binding`'s exact stream: its ledger phase and
    /// close-notice transaction (`content_frame_stream_fact`).
    fn stream_fact(
        &mut self,
        a: &Attempt,
        binding: FrameCopyBinding,
    ) -> Option<(FrameStreamPhase, TransactionId)> {
        self.h
            .owner
            .with_service(a.key, |_, t| t.content_frame_stream_fact(binding.key))
            .unwrap()
    }
}

// G3-5d d-2b3b1: running-core decline and in-slot failure.
#[path = "combined_decline.rs"]
mod decline;
