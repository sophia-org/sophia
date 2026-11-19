//! Gate 4 C3a: the R1 retention controls (REV5 5.4), labelled. Each row
//! declares its loss through the fixture, then reads the table's columns:
//! the retained state, the credit (the image owner's pending work), the
//! reporting destination, the stop effect and final quiescence's refusal.
//! Nothing here disposes of a retained entry by inference; the only
//! release is the walk's own, with native present, where the row says so.
//!
//! The table's five cases: rows 1-3 are here (native None injected at pass
//! time after a Held producer, where the ordinary content pass is refused
//! the Held job's own candidate; a replaced owner's NotFound debt; a result
//! whose issuing facade is gone at its settlement retry). Case 4, the
//! worker lost after the validator forwarded its job and before the worker
//! served it, is `driver::running::loss`. Case 5, the worker lost after its
//! result is buffered, is `driver::running`'s replacement control
//! `an_unexpected_worker_loss_keeps_disconnected_debt_that_refuses_final_quiescence`,
//! as it stands.
use super::super::super::super::content::reconnect_fixture::adopt_lane_facts;
use super::*;
use crate::live_session::metadata_shell::ContentNative;
use sophia_backend_live::CombinedDeclaredHead;
use sophia_protocol::Rect;

/// The fixture's row-facing counts: release attempts, Transferred answers,
/// accepts, installs, ordinary queues, classifications.
fn native_counts(native: &CombinedNativeFixture) -> [usize; 6] {
    [
        native.release_attempts(),
        native.release_transfers(),
        native.accepts(),
        native.installs(),
        native.ordinary_queues(),
        native.classifies(),
    ]
}

/// The owner moved into final quiescence: `stop` of the attempt, then the
/// given turns until the slot's image work has ended.
fn quiesce(lane: &mut Lane, key: ComponentConnectionKey, turn: impl FnMut(&mut Lane)) {
    lane.h.owner.stop(key).unwrap();
    drain(lane, turn);
}

/// The given turns until the slot's image work has ended, bounded; the
/// attempt is already stopped.
fn drain(lane: &mut Lane, mut turn: impl FnMut(&mut Lane)) {
    let deadline = Instant::now() + WAIT;
    while lane.h.owner.processes.image_pending(0) != 0 {
        turn(lane);
        assert!(Instant::now() < deadline, "the image work did not end");
        std::thread::yield_now();
    }
}

/// `finish_after_backend_drop` refused; the owner it returned is put back.
fn refused(lane: &mut Lane, label: &str) {
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    let Err(backend) = lane.h.owner.processes.finish_after_backend_drop(backend) else {
        panic!("{label}: final quiescence ignored the retained entry");
    };
    lane.h.backend = backend;
}

impl Lane {
    /// One pass of the bar's real content service as the pass without
    /// native scanout serves it under CS: through `with_combined_service`,
    /// which lends the row beside the transport with the same checks as
    /// `with_service`, and `ContentNative::ordinary(None)`, so the lent row
    /// is unused, no `CombinedConsume` exists and no join runs: today's
    /// content service. The pass's poll is the session's own `poll`, the
    /// shared ordinary `finish_poll` path with `turn_connected`; neither
    /// the authenticated visit nor `service_components` itself runs here.
    fn serve_ordinary(&mut self, a: &mut Attempt) -> std::result::Result<(), String> {
        let root = Rect {
            x: 0,
            y: 0,
            width: output().size.width,
            height: output().size.height,
        };
        let Lane {
            h, runtime, scene, ..
        } = self;
        let served = h
            .owner
            .with_combined_service(a.key, |service, t, _row| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("bar required")
                };
                adopt_lane_facts(&mut bar.content, output());
                let served = bar
                    .service_content_on(
                        t,
                        runtime,
                        scene,
                        ContentNative::<CombinedNativeFixture>::ordinary(None),
                        &[output()],
                        &[(output().id, root)],
                        root,
                    )
                    .map_err(|error| error.to_string());
                t.poll_io().unwrap();
                served
            })
            .expect("the lane's bar is connected and ready");
        observe_peer(a);
        served
    }
}

/// R1 row 1, labelled: native None injected at pass time after a Held
/// producer. Not a demonstrated production transition: the owner loop's
/// `native_scanout` is `Some` from its assignment on, and this control
/// supplies no path from an admission with native present to a later pass
/// without it; the paused pass (presentation unavailable) keeps native
/// present and lends the row, a different condition that does not stand in
/// for this one. The pass is the production shape without native scanout:
/// the session's own `poll` (its turn is `turn_connected`) and the bar's
/// content service through `with_combined_service` with
/// `ContentNative::ordinary(None)`. The Held job's own candidate is still
/// the transport's next submission (`lane.held` renders and collects it and
/// consumes nothing), so the ordinary content path takes it, bypassing the
/// combined join, and the backend preflight refuses it without native
/// presentation: that refusal, exactly, is the outcome, not a row
/// result-route error and not a row cleanup diagnostic; the row records no
/// event, the job stays Held, the stream stays charged, nothing pends
/// presentation and candidate 1 has no terminal. The production pass maps
/// that failure to `fail_service` (component_service.rs, its Poll-error
/// branch); the control applies the same shared method to the current
/// attempt, which is not an execution of `service_components` itself, and
/// requires Revoked. The peer is then dropped and the slot drained by
/// bounded ordinary polling only, the Held job retained; with the image
/// work at 0 and every other guard shown clear, the Held entry alone
/// refuses final quiescence; one hooked walk with native present releases
/// the ended grant's result and nothing else refuses. The row's trace
/// (recorded events, b4's scope, not every reader call) is snapshotted
/// after the Held producer and must be unchanged until that cleanup.
#[test]
fn a_held_jobs_candidate_is_refused_by_the_pass_without_native_and_the_job_refuses_final_quiescence()
 {
    const REFUSAL: &str = "shell content candidate cannot be accepted without native presentation";
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    let old = a.key;
    let before = native_counts(&lane.native);
    let trace = lane.h.owner.processes.combined_row(0).trace().to_vec();
    assert!(
        !a.stream.has_credit(),
        "the Held job keeps the stream's credit"
    );
    assert!(
        lane.pending(&a).is_empty(),
        "nothing pends presentation before the pass"
    );
    assert!(
        a.terminals(1).is_empty(),
        "candidate 1 has no terminal before the pass"
    );
    // 1. The pass without native: the peer's poll_io, the session's own
    // poll (native None: image service and ordinary content, no walk) and
    // one bar content service pass, which meets candidate 1 on the ordinary
    // path and is refused.
    a.peer.poll_io().unwrap();
    lane.h.owner.poll(65536).unwrap();
    let served = lane.serve_ordinary(&mut a);
    assert_eq!(
        served,
        Err(REFUSAL.to_string()),
        "1: the pass without native is refused candidate 1"
    );
    let error = served.unwrap_err();
    assert_eq!(lane.states(), [(job, "held")], "1: the same Held job");
    assert_eq!(
        native_counts(&lane.native),
        before,
        "1: no row attempt, release, accept, install or queue"
    );
    assert!(
        lane.pending(&a).is_empty(),
        "1: no successful pending presentation"
    );
    assert!(
        a.terminals(1).is_empty(),
        "1: the refusal wrote no terminal for candidate 1"
    );
    assert!(!a.stream.has_credit(), "1: the stream is still charged");
    assert_eq!(
        lane.h.owner.processes.combined_row(0).trace(),
        trace.as_slice(),
        "1: the row recorded no event"
    );
    assert!(
        lane.h
            .owner
            .processes
            .combined_row(0)
            .cleanup_diagnostics()
            .is_empty(),
        "1: the refusal is not a row diagnostic"
    );
    assert_eq!(
        lane.h.owner.phase(old).unwrap(),
        ComponentConnectionPhase::Connected,
        "1: the refusal itself stops nothing"
    );
    // 2. The production Poll-error branch's effect, by the same shared
    // method.
    lane.h.owner.fail_service(old, &error).unwrap();
    assert_eq!(
        lane.h.owner.phase(old).unwrap(),
        ComponentConnectionPhase::Revoked,
        "2: the current attempt's failure stops it"
    );
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "2: retained through the stop"
    );
    assert_eq!(
        native_counts(&lane.native),
        before,
        "2: the stop touched nothing"
    );
    // 3. The drain: the peer dropped, then the session's own poll with
    // native None until the slot's image work is 0, bounded, the Held job
    // checked after every poll; then one more ordinary poll. No combined
    // walk and no native step runs.
    drop(a);
    drain(&mut lane, |lane| {
        lane.h.owner.poll(65536).unwrap();
        assert_eq!(
            lane.states(),
            [(job, "held")],
            "3: retained through the drain"
        );
    });
    lane.h.owner.poll(65536).unwrap();
    assert_eq!(
        lane.h.owner.processes.image_pending(0),
        0,
        "3: the image work ended"
    );
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "3: retained after the drain"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).pending(),
        1,
        "3: blocking guard: the Held entry"
    );
    assert_eq!(
        native_counts(&lane.native),
        before,
        "3: the drain touched nothing"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).trace(),
        trace.as_slice(),
        "3: the drain recorded no row event"
    );
    other_guards_clear(&lane);
    refused(&mut lane, "R1 native None");
    assert_eq!(lane.states(), [(job, "held")], "3: refusal changed nothing");
    assert_eq!(native_counts(&lane.native), before);
    assert_eq!(
        lane.h.owner.processes.combined_row(0).trace(),
        trace.as_slice(),
        "3: no row event before the explicit combined cleanup"
    );
    // 4. With native present again, the walk releases the ended grant's
    // Held result, and the row was the only refusal.
    assert!(lane.turn().is_none());
    assert!(
        lane.states().is_empty(),
        "the walk released the ended grant's result"
    );
    lane.trace_end(&[], &[]);
    assert_eq!(lane.native.release_transfers(), 1);
    let backend = std::mem::replace(
        &mut lane.h.backend,
        SessionContentFixture::new(&[output()]).unwrap(),
    );
    assert!(
        lane.h
            .owner
            .processes
            .finish_after_backend_drop(backend)
            .is_ok(),
        "R1 native None: with the result released nothing refuses"
    );
}

/// R1 row 2, labelled: the native owner replaced. The declined result's
/// release finds no issuer, the entry stays Debt(NotFound), every walk
/// keeps it, nothing is reported and nothing stops; with every other guard
/// of final quiescence shown clear, that debt refuses it.
#[test]
fn a_replaced_owners_debt_is_kept_by_every_walk_and_refuses_final_quiescence() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    lane.native
        .declare_head(output().id, CombinedDeclaredHead::OwnerReplaced);
    let served = lane.consume(&mut a, 1, Instant::now(), super::consume::MOVED, true);
    assert!(
        served.is_ok(),
        "R1 replaced: refused without error: {served:?}"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).debt(job),
        Some(CombinedRelease::NotFound),
        "R1 replaced: NotFound kept as Debt"
    );
    assert_eq!(a.terminals(1), [super::consume::FAILED]);
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Connected,
        "R1 replaced: nothing stops"
    );
    let attempts = lane.native.release_attempts();
    for _ in 0..3 {
        assert!(lane.turn().is_none(), "R1 replaced: no report");
        assert_eq!(
            lane.states(),
            [(job, "debt")],
            "R1 replaced: every walk keeps it"
        );
    }
    assert_eq!(
        lane.native.release_attempts(),
        attempts + 3,
        "R1 replaced: each walk retried the release"
    );
    assert_eq!(lane.native.release_transfers(), 0);
    let key = a.key;
    drop(a);
    quiesce(&mut lane, key, |lane| assert!(lane.turn().is_none()));
    assert_eq!(lane.states(), [(job, "debt")]);
    assert!(
        lane.h
            .owner
            .processes
            .combined_row(0)
            .cleanup_diagnostics()
            .is_empty(),
        "R1 replaced: no diagnostic either"
    );
    // Every other guard of final quiescence is clear.
    assert_eq!(lane.h.owner.processes.image_pending(0), 0, "no image work");
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
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 1);
    refused(&mut lane, "R1 replaced");
    assert_eq!(lane.states(), [(job, "debt")], "refusal changed nothing");
    lane.trace_end(&[(job, "debt")], &[]);
}

/// R1 row 3, labelled: the issuing facade gone at a settlement retry. The
/// result is taken and its first settlement refused by the declared
/// `Refused` fault (`a_settlement_refusal_keeps_the_result_and_reports_once`
/// is the contrast: with the facade still live, the spent-fault retry
/// settles to Held). Here the native owner is replaced between that
/// refusal and the retry. The Unsettled arm calls settle directly, without
/// a second take; with the fault spent, the route's issuer lookup finds no
/// facade for the old owner and refuses `RendererNotFound`, keeping the
/// boxed result and reservation Unsettled. `report` is once per job, so
/// the missing facade adds no second report: the one error is the first
/// refusal, the current attempt's failure, which `fail_service` ends; every
/// later retry is silent and keeps the entry; the credit stays charged;
/// final quiescence refuses on the retained row and the charged image work.
/// This is declared-fault preconditioning followed by the inspected
/// concrete route's actual missing-issuer lookup; it does not show that
/// the first refusal arises naturally in a native session.
#[test]
fn a_result_whose_facade_is_gone_at_its_retry_stays_unsettled_and_refuses_quiescence() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .declare_settle_fault(CombinedSettleFault::Refused);
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    let error = lane.until_failure(&mut a);
    assert!(
        row_error(&error, job, "the issuing facade refused its result"),
        "R1 facade gone: the first refusal is the one report: {error:?}"
    );
    assert_eq!(
        lane.states(),
        [(job, "unsettled")],
        "R1 facade gone: the result is taken and retained"
    );
    assert_ne!(
        lane.h.owner.processes.image_pending(0),
        0,
        "R1 facade gone: the credit stays charged"
    );
    lane.h.owner.fail_service(a.key, &error).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked,
        "R1 facade gone: the current attempt's failure stops it"
    );
    // The facade goes before the retry; the spent fault no longer answers.
    lane.native
        .declare_head(output().id, CombinedDeclaredHead::OwnerReplaced);
    let before = native_counts(&lane.native);
    drop(a);
    for _ in 0..3 {
        assert!(
            lane.turn().is_none(),
            "R1 facade gone: the missing facade is no second report"
        );
        assert_eq!(
            lane.states(),
            [(job, "unsettled")],
            "the retry's settlement found no issuing facade and kept the result"
        );
    }
    assert!(
        lane.h
            .owner
            .processes
            .combined_row(0)
            .cleanup_diagnostics()
            .is_empty(),
        "R1 facade gone: the once-reported error is not a diagnostic too"
    );
    assert_ne!(lane.h.owner.processes.image_pending(0), 0);
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 1);
    assert_eq!(
        native_counts(&lane.native),
        before,
        "R1 facade gone: no release, accept, install or queue"
    );
    // Final quiescence: the retained row and the charged image work block.
    assert!(
        (0..MAX_SHELL_COMPONENTS).all(|slot| lane.h.owner.processes.attempt(slot).is_none_or(
            |key| lane.h.owner.phase(key).unwrap() == ComponentConnectionPhase::Revoked
        )),
        "no attempt is active"
    );
    refused(&mut lane, "R1 facade gone");
    assert_eq!(
        lane.states(),
        [(job, "unsettled")],
        "refusal changed nothing"
    );
    lane.trace_end(&[(job, "unsettled")], &[]);
}
