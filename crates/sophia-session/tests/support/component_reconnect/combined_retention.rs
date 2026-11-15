//! Gate 4 C3a: the R1 retention controls (REV5 5.4), labelled. Each row
//! declares its loss through the fixture, then reads the table's columns:
//! the retained state, the credit (the image owner's pending work), the
//! reporting destination, the stop effect and final quiescence's refusal.
//! Nothing here disposes of a retained entry by inference; the only
//! release is the walk's own, with native present, where the row says so.
//!
//! The table's five cases: rows 1-3 are here (native None with a Held
//! job; a replaced owner's NotFound debt; a result whose issuing facade is
//! gone at its settlement retry). Case 4, the worker lost after the
//! validator forwarded its job and before the worker served it, is
//! `driver::running::loss`. Case 5, the worker lost after its result is
//! buffered, is `driver::running`'s replacement control
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
fn quiesce(lane: &mut Lane, key: ComponentConnectionKey, mut turn: impl FnMut(&mut Lane)) {
    lane.h.owner.stop(key).unwrap();
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

/// R1 row 1, labelled: native None at pass time with a Held job. The pass
/// without native scanout is the session's own `poll` (its turn is
/// `turn_connected`) and the bar's ordinary content service; candidate 2
/// takes today's path to a pending presentation while the Held job is
/// untouched, with no row attempt, release, accept or install. Final
/// quiescence refuses the Held entry; one hooked walk after the ended grant
/// releases it and nothing else refuses.
#[test]
fn a_held_job_is_untouched_by_the_pass_without_native_and_refuses_final_quiescence() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    let before = native_counts(&lane.native);
    lane.submit(&mut a, 2, PIXEL);
    let deadline = Instant::now() + WAIT;
    while lane.pending(&a).is_empty() {
        a.peer.poll_io().unwrap();
        lane.h.owner.poll(65536).unwrap();
        assert_eq!(lane.serve_ordinary(&mut a), Ok(()), "R1 native None");
        assert!(
            Instant::now() < deadline,
            "candidate 2 never reached the ordinary path: {:?}",
            lane.states()
        );
        std::thread::yield_now();
    }
    assert_eq!(
        lane.states(),
        [(job, "held")],
        "R1 native None: the pass without native touched nothing in the row"
    );
    assert_eq!(
        native_counts(&lane.native),
        before,
        "R1 native None: no row attempt, release, accept, install or queue"
    );
    assert!(
        a.terminals(2).is_empty(),
        "candidate 2 awaits its presentation"
    );
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Connected,
        "R1 native None: nothing stops"
    );
    let key = a.key;
    drop(a);
    quiesce(&mut lane, key, |lane| {
        lane.h.owner.poll(65536).unwrap();
    });
    assert_eq!(lane.states(), [(job, "held")]);
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 1);
    refused(&mut lane, "R1 native None");
    assert_eq!(lane.states(), [(job, "held")], "refusal changed nothing");
    assert_eq!(native_counts(&lane.native), before);
    // With native present again, the walk releases the ended grant's Held
    // result, and the row was the only refusal.
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
