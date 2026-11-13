//! G3-5d d-2b2 Session controls: the running core end to end through the
//! split driver (REV2 R1/R3/R7, REV3 sections 1-4). Every owner turn is
//! `combined_owner_turn` with native service; d-1's running worker renders
//! through the deterministic seam and leases the result.
//!
//! Admission (T0) is the driver turn that leaves the row's one job in
//! flight; the result turn (T1) is the next driver turn. The validator's
//! hold, progress counter, serial and stop flag are d-2a's test-support
//! handles, reached through the slot's image owner; the renderer gate is
//! d-2a's, armed through the running fixture. The row records each
//! admission's binding where it is submitted and each collected
//! disposition where it reads it; nothing is reconstructed from later
//! state.
//!
//! The lane's negotiated validation timeout is the role default, 2 s; no
//! control changes it (REV3 named 500 ms and 10 s; see the README). Waits
//! for a deadline sleep on the admitted deadline itself.
//!
//! While the renderer gate holds a job the whole worker thread waits (REV3
//! section 1). The only owner calls made while gated are the withdrawal
//! turns of the Stopped control, each recorded and asserted to be one
//! driver turn [Images, Content(0), Walk, Native] that collected nothing.
use super::*;
use crate::shell_component_connections::combined_row::CombinedRow;
use rustix::event::{PollFd, PollFlags};
use sophia_backend_live::{
    CombinedDisposition, CombinedGateFact, CombinedGateRelease, CombinedRenderGate,
    CombinedRenderPlan, LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus as Submit,
};
use sophia_runtime::{CheckpointHandle, ValidatorProgress, checkpoint};
use std::os::fd::OwnedFd;

/// A hold ends at the latest after this, as `TimedOut`, never a pass.
const GATE_WAIT: Duration = Duration::from_secs(10);
/// REV3 section 2's positive deadline margin at release.
const MARGIN: Duration = Duration::from_secs(1);
const TURN: [Step; 4] = [Images, Content(0), Walk, Native];

/// One result turn: its steps, the accept count after each content call,
/// and native service's submission status.
struct ResultTurn {
    steps: Vec<Step>,
    accepted: Vec<usize>,
    status: Option<Submit>,
}

/// The validator's readiness, duplicated before release so that probing it
/// is no owner call. Polled at timeout 0; never drained.
struct Readiness(OwnedFd);

impl Readiness {
    fn readable(&self) -> bool {
        let mut fds = [PollFd::new(&self.0, PollFlags::IN)];
        sophia_wake::wait(&mut fds, Some(Instant::now())).unwrap();
        fds[0].revents().contains(PollFlags::IN)
    }
}

impl Lane {
    fn validator(&mut self) -> &mut sophia_runtime::IsbValidationWorker<CombinedRenderPlan> {
        self.h
            .owner
            .processes
            .combined_validator(0)
            .expect("slot 0 has an image owner")
    }

    /// Arms the validator's pre-validation hold for the next admission.
    fn hold_next(&mut self) -> (CheckpointHandle, ValidatorProgress, Readiness) {
        let (hold, handle) = checkpoint();
        let validator = self.validator();
        validator.arm_next_combined_checkpoint(hold);
        let progress = validator.validator_progress();
        let readiness = Readiness(validator.readiness().try_clone_to_owned().unwrap());
        (handle, progress, readiness)
    }

    fn arm_gate(&self) -> CombinedRenderGate {
        self.native.arm_render_gate(output().id, GATE_WAIT)
    }

    /// Driver turns with `a`'s peer polled until the row holds exactly one
    /// job, in flight. The last is T0; the job.
    fn admitted(&mut self, a: &mut Attempt) -> u64 {
        let deadline = Instant::now() + WAIT;
        loop {
            a.peer.poll_io().unwrap();
            assert_eq!(self.quiet_driver_turn(true), TURN);
            observe_peer(a);
            if let [(job, "in flight")] = self.states().as_slice() {
                return *job;
            }
            assert!(
                Instant::now() < deadline,
                "no admission: {:?}",
                self.states()
            );
            std::thread::yield_now();
        }
    }

    /// One driver turn with native service's status kept.
    fn result_turn(&mut self) -> ResultTurn {
        let mut steps = Vec::new();
        let mut status = None;
        let mut errors = Vec::new();
        let (turned, accepted) = self.driver_turn_with(&mut steps, &mut |native| match native
            .service_native(output().id)
        {
            Ok(submit) => status = submit,
            Err(error) => errors.push(error.to_string()),
        });
        let first = turned.expect("the turn completes");
        assert!(first.iter().all(Option::is_none), "{first:?}");
        assert!(errors.is_empty(), "native service: {errors:?}");
        ResultTurn {
            steps,
            accepted,
            status,
        }
    }

    fn row(&self) -> &CombinedRow {
        self.h.owner.processes.combined_row(0)
    }

    fn deadline_of(&self, job: u64) -> Instant {
        self.row()
            .admitted_deadline(job)
            .expect("the row holds the admitted job")
    }

    fn acquisitions(&self) -> usize {
        self.native
            .slot_metrics(output().id)
            .expect("the head has a worker")
            .acquisitions
    }

    /// Driver turns until the row and the slot's image work are empty and
    /// candidate `n` has a terminal; the grant stays live throughout.
    fn settled(&mut self, a: &mut Attempt, n: u64) {
        let deadline = Instant::now() + WAIT;
        while !self.states().is_empty()
            || self.h.owner.processes.image_pending(0) != 0
            || a.terminals(n).is_empty()
        {
            a.peer.poll_io().unwrap();
            assert_eq!(self.quiet_driver_turn(true), TURN);
            observe_peer(a);
            assert_eq!(
                self.h.owner.phase(a.key).unwrap(),
                ComponentConnectionPhase::Connected,
                "the grant stays live"
            );
            assert!(
                Instant::now() < deadline,
                "not settled: {:?}",
                self.states()
            );
            std::thread::yield_now();
        }
    }

    /// The facade's cell is free again: a later reservation succeeds.
    fn reserve_again(&mut self) {
        let (head, _inbox) =
            ShellContentNative::reserve_combined_frame(&mut self.native, output().id)
                .expect("the cancelled reservation freed the cell");
        assert!(matches!(
            ShellContentNative::cancel_combined_frame(&mut self.native, &head),
            Ok(true)
        ));
    }
}

/// Sleeps until `Instant::now() >= deadline`; no owner call.
fn sleep_past(deadline: Instant) {
    while Instant::now() < deadline {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
    }
}

/// The gate's next fact, which must come within `WAIT`.
fn fact(gate: &CombinedRenderGate) -> CombinedGateFact {
    gate.next(WAIT).expect("the gate reports within its wait")
}

/// REV2 R3's result-turn row (MD3, running MD4, R7 binding). T0 admits;
/// the result rings the owner; T1 collects it in its images step, accepts
/// it in its content step and submits it in native service.
#[test]
fn an_unchanged_scene_is_accepted_and_submitted_in_the_result_turn() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let serial = lane.validator().validator_serial();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    let bindings = lane.row().bindings().to_vec();
    assert_eq!(bindings.len(), 1, "one admission, bound at submitted");
    assert_eq!(
        (bindings[0].serial, bindings[0].job, bindings[0].grant),
        (serial, job, a.key.grant),
        "the binding names this validator, this job and this grant"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the render result rings the owner"
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "T0 left it uncollected"
    );
    let t1 = lane.result_turn();
    assert_eq!(
        t1.steps, TURN,
        "MD4: content accepts before the walk, native last"
    );
    assert_eq!(
        t1.status,
        Some(Submit::SubmittedWaitingForPageFlip),
        "MD3: after T1 the accepted frame is Submitted"
    );
    assert_eq!(t1.accepted, [1], "T1's content step accepted the result");
    assert_eq!(
        lane.native.device_counts(output().id),
        Some((1, 1, 0)),
        "one framebuffer, one commit, no test commit"
    );
    assert_eq!(lane.native.accepts(), 1);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert!(lane.states().is_empty(), "accepted removes the entry");
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "one pending presentation"
    );
    assert!(
        lane.native.head_observation(output().id).2,
        "the head keeps the accepted content as submitted"
    );
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)]
    );
    assert_eq!(lane.row().bindings(), bindings, "no later binding");
}

/// REV3 section 2 (MD6). The admission is held before validation; after T0
/// the baseline is read, the margin checked and the hold released. Between
/// release and the decisive probe only the progress wait runs.
#[test]
fn the_normal_path_has_no_intermediate_validation_return() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let (mut hold, progress, readiness) = lane.hold_next();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    assert!(
        hold.entered(WAIT),
        "the admission is held before validation"
    );
    let p0 = progress.value();
    assert_eq!(lane.row().bindings().len(), 1, "exactly one admission");
    assert_eq!(lane.validator().pending(), 1, "the validator holds one job");
    assert!(!readiness.readable(), "nothing is readable while held");
    assert!(
        Instant::now() + MARGIN < lane.deadline_of(job),
        "a positive deadline margin at release"
    );
    hold.release();
    assert!(
        progress.wait_for(p0 + 1, WAIT),
        "the forwarded job's iteration completes"
    );
    assert!(
        !readiness.readable(),
        "readiness stays quiet after a forwarded job"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the render result rings the owner"
    );
    let t1 = lane.result_turn();
    assert_eq!(t1.steps, TURN);
    assert_eq!(t1.status, Some(Submit::SubmittedWaitingForPageFlip));
    assert_eq!(t1.accepted, [1]);
}

/// REV3 section 3, interrupted. The admission is held before validation
/// until its admitted deadline has passed, with no owner call, then
/// released: the validator times it out and returns it through the
/// ordinary completion path (its notification is readable at the next loop
/// head). With no owner call, no stop flag was set, so that completion is
/// the validator's own Timeout. The renderer gate, armed too, never enters.
#[test]
fn a_job_held_past_its_deadline_before_validation_times_out_in_the_validator() {
    let (mut lane, _wake) = Lane::running();
    let mut a = lane.attempt();
    let gate = lane.arm_gate();
    let (mut hold, progress, readiness) = lane.hold_next();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    assert!(
        hold.entered(WAIT),
        "the admission is held before validation"
    );
    let p0 = progress.value();
    sleep_past(lane.deadline_of(job));
    hold.release();
    assert!(progress.wait_for(p0 + 1, WAIT), "the iteration completes");
    assert!(
        readiness.readable(),
        "the validator's Timeout came through the ordinary completion path"
    );
    lane.settled(&mut a, 1);
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Timeout as u16)],
        "the validation-failure terminal, once"
    );
    assert_eq!(
        gate.next(Duration::ZERO),
        None,
        "the renderer never entered"
    );
    assert!(lane.row().collected().is_empty(), "no renderer result");
    assert_eq!(
        lane.row().unconfirmed_cancels(),
        0,
        "the facade cancel held"
    );
    assert_eq!(lane.acquisitions(), 0);
    assert!(lane.native.seen().is_empty());
    assert_eq!(lane.native.accepts(), 0);
    lane.reserve_again();
}

/// REV3 section 3, interrupted. The admission is held before validation;
/// the peer cancels its offer; driver turns run until the store stop has
/// reached the validator's stop flag; then the hold is released and the
/// validator cancels the job through the ordinary completion path.
#[test]
fn a_withdrawal_before_validation_is_cancelled_in_the_validator() {
    let (mut lane, _wake) = Lane::running();
    let mut a = lane.attempt();
    let gate = lane.arm_gate();
    let (mut hold, progress, readiness) = lane.hold_next();
    let offer = lane.submit_at(&mut a, 1, PIXEL, &[0]);
    let job = lane.admitted(&mut a);
    assert!(
        hold.entered(WAIT),
        "the admission is held before validation"
    );
    a.peer.image_cancel(offer, Instant::now()).unwrap();
    let turns = lane.withdrawal(&mut a, job, None);
    let p0 = progress.value();
    hold.release();
    assert!(progress.wait_for(p0 + 1, WAIT), "the iteration completes");
    assert!(
        readiness.readable(),
        "the validator's Cancelled came through the ordinary completion path"
    );
    lane.settled(&mut a, 1);
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Cancelled as u16)],
        "the withdrawal's terminal, once, after {turns} withdrawal turns"
    );
    assert_eq!(
        gate.next(Duration::ZERO),
        None,
        "the renderer never entered"
    );
    assert!(lane.row().collected().is_empty(), "no renderer result");
    assert_eq!(
        lane.row().unconfirmed_cancels(),
        0,
        "the facade cancel held"
    );
    assert_eq!(lane.acquisitions(), 0);
    assert!(lane.native.seen().is_empty());
    lane.reserve_again();
}

impl Lane {
    /// The withdrawal turns (REV3 section 4's gated owner-call list): driver
    /// turns, each [Images, Content(0), Walk, Native], until the validator's
    /// stop flag for `job` is set. Each collected nothing; with a gate, none
    /// let it release. Their count.
    fn withdrawal(
        &mut self,
        a: &mut Attempt,
        job: u64,
        gate: Option<&CombinedRenderGate>,
    ) -> usize {
        a.peer.poll_io().unwrap();
        let deadline = Instant::now() + WAIT;
        let mut turns = 0;
        while self.validator().cancel_requested(job) != Some(true) {
            assert_eq!(self.quiet_driver_turn(true), TURN, "a withdrawal turn");
            turns += 1;
            observe_peer(a);
            assert_eq!(self.states(), [(job, "in flight")], "collected nothing");
            if let Some(gate) = gate {
                assert_eq!(gate.next(Duration::ZERO), None, "still gated");
            }
            assert!(Instant::now() < deadline, "the stop never reached the job");
            a.peer.poll_io().unwrap();
        }
        turns
    }
}

/// REV3 section 4 (MD13). The gate holds the forwarded job until its
/// admitted deadline has passed, with no owner call; the renderer then
/// decides Expired before any slot.
#[test]
fn a_job_expired_after_validation_comes_home_expired_from_the_renderer() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut gate = lane.arm_gate();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    let admitted = lane.deadline_of(job);
    assert_eq!(lane.row().bindings()[0].job, job);
    let CombinedGateFact::Entered {
        job: entered,
        deadline,
        stopped,
    } = fact(&gate)
    else {
        panic!("1: the forwarded job entered the gate")
    };
    assert_eq!(
        (entered, deadline, stopped),
        (job, admitted, false),
        "1: Entered names the bound job with its admitted deadline, unstopped"
    );
    sleep_past(deadline);
    gate.release();
    let CombinedGateFact::Released {
        job: released,
        now,
        stopped,
        by,
        ..
    } = fact(&gate)
    else {
        panic!("3: the hold ended")
    };
    assert_eq!(
        by,
        CombinedGateRelease::Released,
        "3: released by the control"
    );
    assert!(
        released == job && now >= deadline && !stopped,
        "3: serve reads the job past its deadline, unstopped"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the renderer's result rings the owner"
    );
    assert_eq!(lane.result_turn().steps, TURN);
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Expired)],
        "4: the renderer decided Expired"
    );
    let slots = lane.native.slot_metrics(output().id).unwrap();
    assert_eq!(
        (slots.acquisitions, slots.history_invalidations),
        (0, 0),
        "5: no slot taken, nothing invalidated"
    );
    assert!(lane.native.seen().is_empty(), "5: nothing bound or read");
    lane.settled(&mut a, 1);
    assert_eq!(
        a.terminals(1).len(),
        1,
        "one terminal: {:?}",
        a.terminals(1)
    );
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.release_attempts(), 0, "no lease, no debt");
}

/// REV3 section 4 (MD12), live-grant Stopped. The gate holds the forwarded
/// job; the peer cancels its offer; the withdrawal turns run while gated
/// until the store stop reaches the job's stop flag; then the gate releases
/// and the renderer decides Stopped before any slot. The grant stays live.
#[test]
fn a_withdrawal_after_validation_comes_home_stopped_from_the_renderer() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut gate = lane.arm_gate();
    let offer = lane.submit_at(&mut a, 1, PIXEL, &[0]);
    let job = lane.admitted(&mut a);
    assert_eq!(lane.row().bindings()[0].job, job);
    let CombinedGateFact::Entered {
        job: entered,
        stopped,
        ..
    } = fact(&gate)
    else {
        panic!("1: the forwarded job entered the gate")
    };
    assert_eq!(
        (entered, stopped),
        (job, false),
        "1: the bound job, unstopped"
    );
    a.peer.image_cancel(offer, Instant::now()).unwrap();
    let turns = lane.withdrawal(&mut a, job, Some(&gate));
    gate.release();
    let CombinedGateFact::Released {
        job: released,
        stopped,
        by,
        ..
    } = fact(&gate)
    else {
        panic!("3: the hold ended")
    };
    assert_eq!(
        by,
        CombinedGateRelease::Released,
        "3: released by the control"
    );
    assert!(
        released == job && stopped,
        "3: serve reads the job stopped, after {turns} withdrawal turns"
    );
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the renderer's result rings the owner"
    );
    assert_eq!(lane.result_turn().steps, TURN);
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Stopped)],
        "4: the renderer decided Stopped"
    );
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Connected,
        "the grant is live"
    );
    let kept = lane.states();
    assert_eq!(lane.acquisitions(), 0, "5: no slot taken");
    assert!(lane.native.seen().is_empty(), "5: nothing bound or read");
    lane.settled(&mut a, 1);
    assert_eq!(
        a.terminals(1).len(),
        1,
        "exactly one candidate terminal: {:?}; after collection the row held {kept:?}",
        a.terminals(1)
    );
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.release_attempts(), 0, "no lease, no debt");
}

/// REV2 R3's repeated-resource row (MD5): two placements of one resource
/// render as two reads of the same validated pixels, in one slot, and the
/// frame is accepted and submitted in the result turn.
#[test]
fn a_repeated_resource_frame_is_accepted_with_every_occurrence_rendered() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.submit_at(&mut a, 1, PIXEL, &[0, 5]);
    lane.admitted(&mut a);
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the render result rings the owner"
    );
    let seen = lane.native.seen();
    assert!(
        seen.len() == 2
            && seen[0].index < seen[1].index
            && (seen[0].handle, seen[0].checksum) == (seen[1].handle, seen[1].checksum),
        "one read of the same pixels per occurrence, in order: {seen:?}"
    );
    assert_eq!(lane.acquisitions(), 1, "one slot");
    let t1 = lane.result_turn();
    assert_eq!(t1.status, Some(Submit::SubmittedWaitingForPageFlip));
    assert_eq!(t1.accepted, [1]);
}

// G3-5d d-2b3a: running-core custody, Held/Unknown against Ended.
#[path = "combined_custody.rs"]
mod custody;
