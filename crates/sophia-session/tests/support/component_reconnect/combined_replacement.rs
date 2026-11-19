//! G3-5d d-2b3b2 Session controls: replacement and route loss on the running
//! core (REV2 R1a and R3, as dispositioned by the d-2b3b reachability review
//! and its row-6 outbox disposition). Every combined admission is b2's
//! `admitted`; every deciding owner turn is the split driver.
//!
//! Row 5: B's bundle meets the join while A's result is Held in the same
//! persistent slot. The join and collection records name the identities
//! stored at submission and collection; nothing is read from B's current
//! grant or the slot's current validator to tag A's result. The setup of
//! B's attempt and its ordinary copy is today's unhooked owner turns, as
//! the stub control has it, labelled at its loop.
//!
//! Row 6: replacement discards the old role's tracking. Stop leaves A's
//! Ready and its pending presentation; the next attempt's Ready replaces
//! it without inheriting it. Runtime custody (retired epoch, pending
//! revocation) and native custody (submitted head, leased slot) are read
//! separately and are not ended by either step. "Presentations end once"
//! stays held: discarded tracking is no terminal or native retirement.
//! Session nonempty-outbox clearing by disconnect is NOT ESTABLISHED: no
//! retained FIFO record was established in the traced unmodified
//! between-turn scenario, and no record is fabricated to reach one.
//!
//! Row 7: a labelled out-of-contract fault, unexpected renderer-worker loss
//! (`lose_renderer_worker`), after the result is already buffered. It is
//! no legal final shutdown and no retirement: final quiescence refuses.
//!
//! The SlotReleased disposal and its publication, every peer terminal and
//! the nonempty-outbox case are b3d obligations, beside P1-P3.
use super::*;
use crate::live_session::metadata_shell::content::reconnect_fixture::pending_presentations;

/// Slot 0's Ready key and its pending presentations.
type Tracking = Option<(
    ComponentConnectionKey,
    Vec<(ContentGrant, ContentOutputId, u64)>,
)>;
/// (head keeps submitted content, leased slots, (release attempts,
/// Transferred receipts), accepts, device counts).
type NativeCustody = (
    bool,
    usize,
    (usize, usize),
    usize,
    Option<(usize, usize, usize)>,
);

impl Lane {
    /// Slot 0's Ready, read directly: its exact key and the bar's pending
    /// presentations. Not `with_service`, which serves only a connected
    /// attempt; the caller compares the key, so a stale Ready cannot pass
    /// for a new one.
    fn ready_tracking(&self) -> Tracking {
        self.h.owner.ready[0].as_ref().map(|ready| {
            let ShellComponentService::Bar(bar) = &ready.service else {
                panic!("bar required")
            };
            (ready.key, pending_presentations(&bar.content))
        })
    }

    /// `a`'s next ordinary submission, as the content service reports it.
    fn next_submission(&mut self, a: &Attempt) -> Option<(ContentOutputId, u64)> {
        self.h
            .owner
            .with_service(a.key, |_, t| t.next_content_submission())
            .unwrap()
    }

    /// Native custody, read without an owner call: the head's submitted
    /// content, leased slots, (release attempts, Transferred receipts),
    /// accepts and device counts.
    fn native_custody(&self) -> NativeCustody {
        (
            self.native.head_observation(output().id).2,
            self.leased(),
            self.releases(),
            self.native.accepts(),
            self.native.device_counts(output().id),
        )
    }
}

/// Row 5. J1 renders under A, A stops, and B connects in the same slot with
/// its ordinary candidate 1 ready to submit while J1 is still uncollected.
/// One driver turn then collects J1 into Held (images), meets B's bundle
/// with the join while J1 is Held (content) and releases J1 for its ended
/// grant (walk). The join selects no entry: B's frame takes the ordinary
/// arm, one install entry and one pending presentation under B's grant.
/// Without the join's grant clause J1 would be selected; the join record
/// is asserted first, before anything that choice could change.
#[test]
fn a_replacement_bundle_meets_the_join_beside_the_old_held_result_and_goes_ordinary() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let serial = lane.validator().validator_serial();
    let j1 = rendered(&mut lane, &wake, &mut a);
    let old = a.key;
    let bindings = lane.row().bindings().to_vec();
    assert_eq!(bindings.len(), 1, "one admission");
    assert_eq!(
        (bindings[0].serial, bindings[0].job, bindings[0].grant),
        (serial, j1, old.grant),
        "J1 is bound at submission to this validator and A's grant"
    );
    lane.h.owner.stop(old).unwrap();
    assert_eq!(
        lane.h.owner.phase(old).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert_eq!(lane.states(), [(j1, "in flight")], "stop collected nothing");
    drop(a);
    // Labelled setup, today's unhooked turns as in the stub control. The
    // attempt's own loop and each iteration below are, in order: B's peer
    // poll, `plain_turn` (turn_connected: the unhooked image-owner service,
    // which collects no row result, with no walk and no native step), and
    // `content(b, false)` (resource and candidate service, no submission).
    // The loop ends only when B's copy is HeldForFrame and candidate 1 is
    // B's next submission.
    let mut b = lane.attempt();
    assert_eq!(b.key.slot, old.slot, "the same persistent slot");
    assert_ne!(b.key.grant, old.grant);
    assert_eq!(
        lane.validator().validator_serial(),
        serial,
        "the slot keeps its one validator, whose job numbers do not repeat"
    );
    lane.submit(&mut b, 1, PIXEL);
    let deadline = Instant::now() + WAIT;
    while lane.residency(&b, 1) != FrameImageResidency::HeldForFrame
        || lane.next_submission(&b) != Some((OUTPUT, 1))
    {
        b.peer.poll_io().unwrap();
        lane.plain_turn();
        lane.content(&mut b, false);
        assert!(Instant::now() < deadline, "B's ordinary copy stalled");
        std::thread::yield_now();
    }
    assert_eq!(
        lane.states(),
        [(j1, "in flight")],
        "J1 is still uncollected"
    );
    assert!(lane.row().collected().is_empty());
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.releases(), (0, 0));
    assert_eq!(lane.leased(), 1, "J1's exported result holds its slot");
    assert!(
        lane.native.ordinary_queue_ready(output().id),
        "precondition: the output's queue takes an ordinary entry"
    );
    assert!(
        Instant::now() + MARGIN < lane.deadline_of(j1),
        "precondition: J1's admitted deadline is ahead"
    );
    let before = lane.row().joins().len();
    let mut steps = Vec::new();
    let mut errors = Vec::new();
    let (turned, observed) =
        lane.driver_turn_observed(&mut steps, Instant::now(), None, &mut |native| {
            if let Err(error) = native.service_native(output().id) {
                errors.push(error.to_string());
            }
        });
    let joins: Vec<_> = lane.row().joins()[before..]
        .iter()
        .map(|join| {
            (
                join.grant,
                join.output,
                join.generation,
                join.present.clone(),
                join.matched,
            )
        })
        .collect();
    assert_eq!(
        joins,
        [(b.key.grant, OUTPUT, 1, vec![(j1, old.grant, "held")], None)],
        "B's bundle met the join while J1 was Held, and selected no entry"
    );
    let first = turned.expect("the turn completes");
    assert!(first.iter().all(Option::is_none), "{first:?}");
    assert!(errors.is_empty(), "native service: {errors:?}");
    assert_eq!(steps, TURN);
    let collections: Vec<_> = lane
        .row()
        .collections()
        .iter()
        .map(|c| (c.job, c.grant, c.serial, c.reserved, c.head, c.disposition))
        .collect();
    assert_eq!(
        collections,
        [(
            j1,
            old.grant,
            serial,
            j1,
            bindings[0].head,
            CombinedDisposition::Rendered
        )],
        "J1 collected under the identity stored at its submission"
    );
    assert_eq!(
        observed,
        [(0, (0, 0))],
        "the content arm accepted and released nothing"
    );
    assert_eq!(lane.native.accepts(), 0, "J1's render is never B's");
    assert_eq!(lane.native.installs(), 1, "B's one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1, "one retained queue");
    assert_eq!(
        lane.pending(&b),
        [(b.key.grant, OUTPUT, 1)],
        "one pending presentation, under B's grant"
    );
    assert!(lane.states().is_empty(), "the walk released J1");
    lane.trace_end(&[], &[]);
    assert_eq!(lane.releases(), (1, 1), "J1's one Release, Transferred");
    assert_eq!(
        lane.row().bindings(),
        bindings,
        "B's ordinary frame made no admission"
    );
    lane.slot_freed("J1 after replacement");
}

/// Row 6. T1 accepts and submits A's frame, leaving one pending
/// presentation in A's Ready. Stop and one labelled turn leave that exact
/// Ready and record; B's attempt then installs its own Ready, which holds
/// none of A's tracking. Runtime and native custody of A's frame are
/// unchanged throughout: nothing here ends A's presentation.
///
/// Labelled interrupted control: after T1 the native step is not served
/// (`serve_native == false`). No existing control services native again
/// after SubmittedWaitingForPageFlip, and a status outside the modelled
/// ones takes `take_failed_content`, a different native failure path; the
/// unchanged native facts hold only at this paused boundary.
#[test]
fn replacement_discards_the_old_role_tracking_and_inherits_none_of_it() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let t1 = lane.result_turn();
    assert_eq!(t1.steps, TURN);
    assert_eq!(t1.status, Some(Submit::SubmittedWaitingForPageFlip));
    assert_eq!(t1.accepted, [1]);
    assert!(lane.states().is_empty(), "accepted removes the entry");
    lane.trace_end(&[], &[]);
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)]
    );
    let old = a.key;
    let tracked = vec![(old.grant, OUTPUT, 1)];
    assert_eq!(lane.pending(&a), tracked, "one pending presentation");
    assert_eq!(
        lane.ready_tracking(),
        Some((old, tracked.clone())),
        "A's Ready holds it"
    );
    let custody = lane.native_custody();
    assert_eq!(
        custody,
        (true, 1, (0, 0), 1, Some((1, 1, 0))),
        "submitted head, one leased slot, no Release, one accept, one commit"
    );
    lane.h.owner.stop(old).unwrap();
    assert_eq!(
        lane.h.owner.phase(old).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    drop(a);
    let runtime = |lane: &Lane| {
        let accounting = lane.h.owner.accounting();
        (
            accounting.active_epochs,
            accounting.retired_epochs,
            accounting.candidates,
            lane.h.owner.pending_revocations(),
        )
    };
    assert_eq!(
        lane.ready_tracking(),
        Some((old, tracked.clone())),
        "stop keeps A's exact Ready and its pending presentation"
    );
    assert_eq!(
        runtime(&lane),
        (0, 1, 1, 1),
        "A's epoch retired, its candidate and revocation unsettled"
    );
    assert_eq!(
        lane.native_custody(),
        custody,
        "stop ends no native custody"
    );
    assert_eq!(lane.quiet_driver_turn(false), [Images, Walk, Native]);
    assert_eq!(
        lane.ready_tracking(),
        Some((old, tracked.clone())),
        "the turn keeps it too"
    );
    assert_eq!(runtime(&lane), (0, 1, 1, 1));
    assert_eq!(lane.native_custody(), custody);
    let b = lane.attempt();
    assert_eq!(b.key.slot, old.slot, "the same persistent slot");
    assert_ne!(b.key.grant, old.grant);
    assert_eq!(
        lane.ready_tracking(),
        Some((b.key, vec![])),
        "B's own Ready, with none of A's tracking"
    );
    assert!(lane.pending(&b).is_empty(), "B inherits no presentation");
    let (_, retired, candidates, revocations) = runtime(&lane);
    assert_eq!(
        (retired, candidates, revocations),
        (1, 1, 1),
        "A's runtime custody outlives its discarded tracking"
    );
    assert_eq!(
        lane.native_custody(),
        custody,
        "and so does its native custody"
    );
}

/// Row 7, labelled out-of-contract fault. J1's result is buffered (its ring
/// read) before the worker is lost; A is then stopped explicitly. The next
/// turn's images step still reads the buffered result into Held; the walk
/// releases it for the ended grant and finds the queue closed, so the row
/// keeps Debt(Disconnected), which each later walk retries and keeps. With
/// every other final-quiescence guard shown clear, that Debt is what
/// refuses `finish_after_backend_drop`, which returns the owner it was
/// given. The worker's slot metrics are stale after it exits and are not
/// read as slot freedom. Native is not served after the loss: the lost
/// worker's native failure is not this row.
#[test]
fn an_unexpected_worker_loss_keeps_disconnected_debt_that_refuses_final_quiescence() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    lane.native.lose_renderer_worker(WAIT);
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
    drop(a);
    assert_eq!(lane.quiet_driver_turn(false), [Images, Walk, Native]);
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)],
        "the buffered result was read after the worker exited"
    );
    assert_eq!(lane.states(), [(job, "debt")]);
    assert_eq!(
        lane.row().debt(job),
        Some(CombinedRelease::Disconnected),
        "the walk's Release found the worker gone"
    );
    assert_eq!(lane.releases(), (1, 0), "one attempt, no receipt");
    assert_eq!(lane.quiet_driver_turn(false), [Images, Walk, Native]);
    assert_eq!(lane.states(), [(job, "debt")], "the debt is kept");
    assert_eq!(lane.row().debt(job), Some(CombinedRelease::Disconnected));
    assert_eq!(lane.releases(), (2, 0), "the walk retried once more");
    // Every other guard of final quiescence is clear.
    assert_eq!(lane.h.owner.processes.image_pending(0), 0, "no image work");
    #[cfg(feature = "experimental-shell-capture")]
    assert_eq!(
        lane.h.owner.processes.connections().capture_work(),
        0,
        "no capture work"
    );
    assert!(
        (0..MAX_SHELL_COMPONENTS).all(|slot| lane.h.owner.processes.attempt(slot).is_none_or(
            |key| lane.h.owner.phase(key).unwrap() == ComponentConnectionPhase::Revoked
        )),
        "no attempt is active"
    );
    assert_eq!(
        lane.h.owner.accounting().active_epochs,
        0,
        "no active epoch"
    );
    assert_eq!(lane.row().pending(), 1, "the Debt is the row's one entry");
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let Err(backend) = lane.h.owner.processes.finish_after_backend_drop(backend) else {
        panic!("final quiescence ignored the Disconnected debt");
    };
    lane.h.backend = backend;
    assert_eq!(lane.states(), [(job, "debt")], "refusal changed nothing");
    lane.trace_end(&[(job, "debt")], &[]);
}
