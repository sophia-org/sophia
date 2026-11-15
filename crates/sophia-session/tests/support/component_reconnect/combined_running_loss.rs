//! Gate 4 C3a: R1 case 4 (REV5 5.4), labelled: the renderer worker lost
//! after the validator forwarded its job and before the worker served it.
//! The ordering is produced by the running fixture's pausing gate, the
//! existing d-2a gate that pauses once on an empty route, and its
//! loss-and-resume operation: the admission is held before validation
//! (d-2a's checkpoint) until the worker has registered the route and paused
//! on it (`Waiting`); the hold is released and the validator forwards into
//! the paused worker's inbox (its iteration completes with no ordinary
//! completion); then the shutdown is requested, the pause resumed
//! (`PauseEnded` by `Released`) and the worker's exit awaited. The worker
//! serves nothing in the pass whose pause ended and reads the requested
//! shutdown at its next control check, so the forwarded job is dropped
//! unserved with its route: it never entered the gate and the seam read
//! nothing. Dropping the route's result sender notifies the owner wake
//! (`sophia_wake::SignalSender`, by source), a disconnect notification and
//! not a publication; the control observes the wake readable after the
//! exit, which is neither a count of notifications nor an attribution of a
//! coalesced wake to one source, and a wake is not a result. That no result
//! was buffered is shown by the producer boundaries above and by the next
//! collection, which meets the closed route and collects nothing.
//!
//! The owner's next images step finds the route closed (`NotConnected`,
//! reported once as the result-route error, the current attempt's failure)
//! and keeps the entry InFlight with its reservation; the credit stays
//! charged; later turns are silent and keep it; the walk never touches an
//! InFlight entry; no release, accept or install is inferred; and final
//! quiescence refuses, with the charged image work and the retained row the
//! guards that block. That hold is unbounded by construction (the gate-4
//! authority note's R1); its disposal is a later authority review. After
//! the loss only owner and shared-code steps run: no native service step,
//! whose failure would be the dead worker's, not this row's. Every wait is
//! bounded; a timeout, an unexpected gate fact, a buffered result or a
//! returned plan is a failed precondition of this producer, not another
//! schedule. The worker's slot metrics are stale after its exit and are not
//! read.
use super::*;

#[test]
fn a_worker_lost_after_forwarding_keeps_the_job_in_flight_and_refuses_final_quiescence() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut gate = lane.native.arm_pausing_render_gate(output().id, GATE_WAIT);
    let (mut hold, progress, readiness) = lane.hold_next();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.admitted(&mut a);
    assert!(
        hold.entered(WAIT),
        "1: the admission is held before validation"
    );
    assert_eq!(
        fact(&gate),
        CombinedGateFact::Waiting,
        "2: the route is registered and empty, and the worker paused on it"
    );
    let p0 = progress.value();
    hold.release();
    assert!(
        progress.wait_for(p0 + 1, WAIT),
        "3: the validation iteration completed"
    );
    assert!(
        !readiness.readable(),
        "3: no ordinary completion: the job was forwarded into the paused worker's inbox"
    );
    assert_eq!(
        gate.next(Duration::ZERO),
        None,
        "3: the pause has not ended and nothing entered the gate"
    );
    assert_eq!(lane.states(), [(job, "in flight")], "3: uncollected");
    lane.native.lose_renderer_worker_resuming(&mut gate, WAIT);
    assert_eq!(
        fact(&gate),
        CombinedGateFact::PauseEnded {
            by: CombinedGateRelease::Released
        },
        "4: the pause ended by the explicit resume, after the shutdown request"
    );
    assert_eq!(
        gate.next(Duration::ZERO),
        None,
        "4: the forwarded job never entered the gate: the exited worker served nothing"
    );
    assert!(
        wake.wait(Some(Instant::now())).unwrap(),
        "4: the owner wake rang for the route's closure (the result sender's drop notification, not a result)"
    );
    assert!(lane.native.seen().is_empty(), "4: the seam read nothing");
    assert!(lane.row().collected().is_empty(), "4: nothing collected");
    assert_eq!(lane.native.release_attempts(), 0);
    // 5. The first owner collection meets the closed route.
    let (first, steps) = lane.until_driver_failure(&mut a, false);
    assert_eq!(
        steps,
        [Images, FailService(0), Walk, Native],
        "5: the images step failed the slot before content"
    );
    let what = format!("combined job {job}: result route: not connected");
    assert!(
        first[0]
            .as_deref()
            .is_some_and(|error| error.contains(&what)),
        "5: the result-route error, once: {first:?}"
    );
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked,
        "5: the current attempt's failure stopped it"
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "5: the entry stays InFlight with its reservation"
    );
    assert!(
        lane.row().collected().is_empty(),
        "5: nothing collected: the closed route delivered no buffered result"
    );
    assert_ne!(
        lane.h.owner.processes.image_pending(0),
        0,
        "5: the credit stays charged"
    );
    drop(a);
    // 6. Later turns: silent, and the entry is kept.
    for _ in 0..3 {
        assert_eq!(
            lane.quiet_driver_turn(false),
            [Images, Walk, Native],
            "6: a later turn reports nothing"
        );
        assert_eq!(lane.states(), [(job, "in flight")], "6: kept");
    }
    assert!(
        lane.row().cleanup_diagnostics().is_empty(),
        "6: reported once, and not a diagnostic too"
    );
    assert_eq!(lane.native.release_attempts(), 0, "6: no release inferred");
    assert_eq!(lane.native.accepts(), 0, "6: no accept");
    assert_eq!(lane.native.installs(), 0, "6: no install");
    // 7. Final quiescence refuses: the charged image work and the retained
    // InFlight entry block; every other guard is clear.
    assert_ne!(
        lane.h.owner.processes.image_pending(0),
        0,
        "7: blocking guard: charged image work"
    );
    assert_eq!(
        lane.row().pending(),
        1,
        "7: blocking guard: the InFlight entry"
    );
    other_guards_clear(&lane);
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let Err(backend) = lane.h.owner.processes.finish_after_backend_drop(backend) else {
        panic!("7: final quiescence ignored the InFlight entry");
    };
    lane.h.backend = backend;
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "refusal changed nothing"
    );
    lane.trace_end(&[(job, "in flight")], &[]);
}
