//! G3-5d d-2a: the renderer gate on a running worker (design REV3 section 1).
//! Each control arms the gate for one output, waits for the worker's
//! acknowledgement, then forwards a real validated frame through a facade's
//! reservation. The gate's facts say which job it held and what `serve` read
//! after the hold; the result the worker sends home says what the renderer
//! decided. Expired and Stopped are read only from that result.

use super::super::super::super::service::{GateFact, GateHandle, GateRelease, combined_gate};
use super::*;
use sophia_runtime::ValidatedIsbPixels;

/// Arms the worker's gate for `output` and waits for its acknowledgement, so
/// nothing admitted afterwards can pass the gate unseen.
fn arm(
    core: &NativeGbmRendererWorkerCore,
    output: LiveRendererWorkerOutputKey,
    wait: Duration,
) -> GateHandle {
    let (gate, handle) = combined_gate(output, wait);
    assert!(
        core.command_sender
            .send(WorkerCommand::InstallCombinedGate(gate))
            .is_ok(),
        "the running worker takes the gate"
    );
    assert_eq!(
        handle.next(WAIT),
        Some(GateFact::Armed),
        "the worker acknowledged the gate before any admission"
    );
    handle
}

/// One frame reserved, validated and forwarded; nothing waits for its result.
fn admit(
    output: &mut Output,
    generation: u64,
    deadline: Instant,
) -> (CombinedFacade, CombinedReservation) {
    let (issued, inbox) = output
        .facade
        .reserve_combined_frame()
        .expect("an idle facade reserves");
    let reservation = output.validator.reserve_combined(8, deadline).unwrap();
    let _owner_plan = output
        .validator
        .submit_combined(&reservation, pixels(), plan(generation, deadline), inbox)
        .ok()
        .expect("combined submission");
    (issued, reservation)
}

fn entered(handle: &GateHandle, why: &str) -> (u64, Instant, bool) {
    match handle.next(WAIT) {
        Some(GateFact::Entered {
            job,
            deadline,
            stopped,
        }) => (job, deadline, stopped),
        other => panic!("{why}: {other:?}"),
    }
}

struct Released {
    job: u64,
    now: Instant,
    deadline: Instant,
    stopped: bool,
    by: GateRelease,
}

fn released(handle: &GateHandle, why: &str) -> Released {
    match handle.next(WAIT) {
        Some(GateFact::Released {
            job,
            now,
            deadline,
            stopped,
            by,
        }) => Released {
            job,
            now,
            deadline,
            stopped,
            by,
        },
        other => panic!("{why}: {other:?}"),
    }
}

/// Settles the result's credit with its own reservation and answers the
/// pixels that came home.
fn settle(
    output: &mut Output,
    issued: &CombinedFacade,
    reservation: CombinedReservation,
    result: CombinedRenderResult,
) -> ValidatedIsbPixels {
    let pixels = output
        .facade
        .settle_combined_frame(issued, &mut output.validator, reservation, result)
        .ok()
        .expect("the frame's own reservation settles");
    assert_eq!(output.validator.pending(), 0, "the credit settled once");
    pixels
}

#[test]
fn a_gated_job_is_held_after_forwarding_and_served_with_a_fresh_instant() {
    let (core, _control, probe) = seam_core(9_520, &[]);
    let mut output = Output::attach(&core, 960);
    let mut handle = arm(&core, output.facade.output(), WAIT);
    let deadline = Instant::now() + WAIT;
    let (issued, reservation) = admit(&mut output, 30, deadline);
    let (job, held_deadline, stopped) = entered(&handle, "the forwarded job reaches the gate");
    assert_eq!(job, reservation.job(), "the gate holds the admitted job");
    assert!(reservation.was_forwarded(), "only a forwarded job is held");
    assert_eq!(
        held_deadline, deadline,
        "the job keeps its admitted deadline"
    );
    assert!(!stopped);
    assert_eq!(
        (
            probe.calls.load(Ordering::SeqCst),
            output.slots().acquisitions
        ),
        (0, 0),
        "nothing renders and no slot is taken while the job is held"
    );
    assert!(
        matches!(
            output.facade.take_combined_result(),
            Err(TryRecvError::Empty)
        ),
        "no result leaves a held job"
    );
    let before = Instant::now();
    handle.release();
    let after = released(&handle, "the hold ends on release");
    assert_eq!(
        (after.job, after.by, after.stopped),
        (job, GateRelease::Released, false),
        "the control's own release ended the hold"
    );
    assert!(
        after.now >= before && after.now < after.deadline,
        "serve reads an instant taken after the hold"
    );
    let result = result_after_wake(&output.facade, &output.owner);
    let CombinedOutcome::Rendered(rendered) = &result.outcome else {
        panic!("the held job renders once released");
    };
    assert!(exported(&rendered.outcome).is_ok());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    settle(&mut output, &issued, reservation, result);
    // One job per arming: the next frame passes without a hold.
    let (_issued, _job, outcome) = output.render(31, None);
    assert!(outcome.is_ok(), "a disarmed gate holds nothing");
    assert_eq!(
        handle.next(Duration::from_millis(100)),
        None,
        "the gate held one job"
    );
}

#[test]
fn a_job_whose_deadline_passes_at_the_gate_comes_home_expired_unrendered() {
    let (core, _control, probe) = seam_core(9_521, &[]);
    let mut output = Output::attach(&core, 961);
    let mut handle = arm(&core, output.facade.output(), WAIT);
    let deadline = Instant::now() + Duration::from_millis(500);
    let (issued, reservation) = admit(&mut output, 32, deadline);
    let (job, held_deadline, stopped) = entered(
        &handle,
        "setup: the job was validated and forwarded inside its deadline",
    );
    assert_eq!(
        (job, held_deadline, stopped),
        (reservation.job(), deadline, false)
    );
    let end = Instant::now() + WAIT;
    while Instant::now() < deadline {
        assert!(Instant::now() < end, "the deadline passes");
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.release();
    let after = released(&handle, "the hold ends on release");
    assert_eq!((after.job, after.by), (job, GateRelease::Released));
    assert!(
        after.now >= after.deadline && !after.stopped,
        "serve reads an instant at or past the deadline, with the job unstopped"
    );
    let result = result_after_wake(&output.facade, &output.owner);
    assert!(
        matches!(result.outcome, CombinedOutcome::Expired),
        "the renderer decided Expired after the gate"
    );
    assert_eq!(
        (
            probe.calls.load(Ordering::SeqCst),
            output.slots().acquisitions,
            output.slots().leased
        ),
        (0, 0, 0),
        "an expired job takes no slot and renders nothing"
    );
    let pixels = settle(&mut output, &issued, reservation, result);
    assert_eq!(pixels.bytes().len(), 8, "the pixels come home unbound");
}

#[test]
fn a_job_stopped_at_the_gate_comes_home_stopped_unrendered() {
    let (core, _control, probe) = seam_core(9_522, &[]);
    let mut output = Output::attach(&core, 962);
    let mut handle = arm(&core, output.facade.output(), WAIT);
    let (issued, reservation) = admit(&mut output, 33, Instant::now() + WAIT);
    let (job, _, stopped) = entered(&handle, "the forwarded job reaches the gate");
    assert!(!stopped, "the job arrives unstopped");
    assert!(
        output.validator.cancel(job),
        "the credit is still pending, so the stop reaches the job's flag"
    );
    handle.release();
    let after = released(&handle, "the hold ends on release");
    assert_eq!(
        (after.job, after.by, after.stopped),
        (job, GateRelease::Released, true),
        "serve reads the stop the owner set while the job was held"
    );
    let result = result_after_wake(&output.facade, &output.owner);
    assert!(
        matches!(result.outcome, CombinedOutcome::Stopped),
        "the renderer decided Stopped after the gate"
    );
    assert_eq!(
        (
            probe.calls.load(Ordering::SeqCst),
            output.slots().acquisitions,
            output.slots().leased
        ),
        (0, 0, 0),
        "a stopped job takes no slot and renders nothing"
    );
    let pixels = settle(&mut output, &issued, reservation, result);
    assert_eq!(pixels.bytes().len(), 8, "the pixels come home unbound");
}

#[test]
fn a_disconnected_release_ends_the_hold_and_the_job_is_served() {
    let (core, _control, probe) = seam_core(9_523, &[]);
    let mut output = Output::attach(&core, 963);
    let mut handle = arm(&core, output.facade.output(), WAIT);
    let (issued, reservation) = admit(&mut output, 34, Instant::now() + WAIT);
    let (job, _, _) = entered(&handle, "the forwarded job reaches the gate");
    handle.disconnect();
    let after = released(&handle, "the hold ends on disconnect");
    assert_eq!(
        (after.job, after.by),
        (job, GateRelease::Disconnected),
        "a dropped release is a disconnect, not a release"
    );
    let result = result_after_wake(&output.facade, &output.owner);
    assert!(
        matches!(&result.outcome, CombinedOutcome::Rendered(_)),
        "the worker serves the job after a disconnect"
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    settle(&mut output, &issued, reservation, result);
}

#[test]
fn a_dropped_gate_handle_never_stalls_the_worker() {
    let (core, _control, _probe) = seam_core(9_524, &[]);
    let mut output = Output::attach(&core, 964);
    let handle = arm(&core, output.facade.output(), WAIT);
    let (issued, reservation) = admit(&mut output, 35, Instant::now() + WAIT);
    entered(&handle, "the forwarded job reaches the gate");
    drop(handle);
    let result = result_after_wake(&output.facade, &output.owner);
    assert!(
        matches!(&result.outcome, CombinedOutcome::Rendered(_)),
        "the worker serves the job once the handle is gone"
    );
    settle(&mut output, &issued, reservation, result);
}

#[test]
fn a_missing_release_ends_the_hold_as_timed_out() {
    let (core, _control, _probe) = seam_core(9_525, &[]);
    let mut output = Output::attach(&core, 965);
    let handle = arm(&core, output.facade.output(), Duration::from_millis(50));
    let (issued, reservation) = admit(&mut output, 36, Instant::now() + WAIT);
    let (job, _, _) = entered(&handle, "the forwarded job reaches the gate");
    let after = released(&handle, "the hold ends at its bound");
    assert_eq!(
        (after.job, after.by),
        (job, GateRelease::TimedOut),
        "a missing release is reported as a timeout, never as a release"
    );
    let result = result_after_wake(&output.facade, &output.owner);
    assert!(matches!(&result.outcome, CombinedOutcome::Rendered(_)));
    settle(&mut output, &issued, reservation, result);
}

#[test]
fn a_gate_armed_for_another_output_holds_nothing_here() {
    let (core, _control, _probe) = seam_core(9_526, &[]);
    let mut here = Output::attach(&core, 966);
    let mut there = Output::attach(&core, 967);
    let mut handle = arm(&core, there.facade.output(), WAIT);
    let (_issued, _job, outcome) = here.render(37, None);
    assert!(outcome.is_ok(), "this output's frame renders");
    assert_eq!(
        handle.next(Duration::from_millis(100)),
        None,
        "a gate armed for another output holds nothing here"
    );
    let (issued, reservation) = admit(&mut there, 38, Instant::now() + WAIT);
    let (job, _, _) = entered(&handle, "the gated output's job reaches the gate");
    assert_eq!(job, reservation.job());
    handle.release();
    released(&handle, "the hold ends on release");
    let result = result_after_wake(&there.facade, &there.owner);
    assert!(matches!(&result.outcome, CombinedOutcome::Rendered(_)));
    settle(&mut there, &issued, reservation, result);
}

#[test]
fn shutdown_while_gated_ends_the_worker_after_the_hold() {
    let (core, control, _probe) = seam_core(9_527, &[]);
    let mut output = Output::attach(&core, 968);
    let mut handle = arm(&core, output.facade.output(), WAIT);
    let (_issued, _reservation) = admit(&mut output, 39, Instant::now() + WAIT);
    entered(&handle, "the forwarded job reaches the gate");
    control.shutdown();
    handle.release();
    released(&handle, "the hold ends on release");
    let end = Instant::now() + WAIT;
    let mut thread = core._thread.lock().unwrap();
    while !thread.poll_join().unwrap() {
        assert!(
            Instant::now() < end,
            "shutdown ends the worker once the hold ends"
        );
        std::thread::yield_now();
    }
}
