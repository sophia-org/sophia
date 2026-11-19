//! G3-5c Session controls: the combined arm of `submit_bundle`, entered
//! through the bar's real content service with the slot's row lent
//! (`with_combined_service` → `service_content_on` → `service_on` →
//! `service_with_catalog` → `submit_bundle`). The bar adopts the output facts
//! the lane published itself; nothing else of the content path is stood in.
//!
//! Declared states are labelled where they are used: a declared class
//! (controls 3, 4) and a declared head (control 9) prove the arm's
//! disposition of that class or refusal, not that Session state reaches it.
//! Release facts are counted apart: `release_attempts` (calls), and
//! `release_transfers` (Transferred answers), and `renderer_releases`
//! (Release commands the renderer drained). One is not evidence of another.
use super::super::super::super::content::reconnect_fixture::{
    adopt_lane_facts, pending_presentations,
};
use super::*;
use crate::live_session::metadata_shell::ContentNative;
use crate::shell_component_connections::combined_row::{CombinedConsume, Verdict};
use sophia_backend_live::{CombinedAcceptRefusal, CombinedDeclaredHead, CombinedRecompose};
use sophia_protocol::{ContentGrant, Point, Rect};

/// Far past any admission's deadline.
pub(super) const LATER: Duration = Duration::from_secs(3600);
/// A plane cursor moved from the turn's (None): real scene churn.
pub(super) const MOVED: Option<Point> = Some(Point { x: 10.0, y: 4.0 });
pub(super) const FAILED: (u16, u16) = (3, ContentReason::RendererFailed as u16);

impl Lane {
    /// One pass of the bar's real content service with the row lent, at
    /// `at`, with the plane cursor at `plane`, and the fixture as the native
    /// only when `native`.
    fn serve_at(
        &mut self,
        a: &mut Attempt,
        at: Instant,
        plane: Option<Point>,
        native: bool,
    ) -> std::result::Result<(), String> {
        let (cursor, plane) = turn_cursor(true, plane);
        let root = Rect {
            x: 0,
            y: 0,
            width: output().size.width,
            height: output().size.height,
        };
        #[cfg(feature = "diagnostic-combined-witness")]
        let witness = PassSlot {
            pass: self.witness_pass(),
            slot: 0,
        };
        let Lane {
            h,
            native: fixture,
            runtime,
            scene,
            ..
        } = self;
        let served = h
            .owner
            .with_combined_service(a.key, |service, t, row| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("bar required")
                };
                adopt_lane_facts(&mut bar.content, output());
                let served = bar
                    .service_content_on(
                        t,
                        runtime,
                        scene,
                        ContentNative {
                            native: native.then_some(&mut *fixture),
                            combined: Some(CombinedConsume {
                                row,
                                cursor,
                                plane,
                                instant: at,
                                #[cfg(feature = "diagnostic-combined-witness")]
                                witness,
                            }),
                        },
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

    /// Serves until candidate `n`'s bundle met the arm: an error, a frame
    /// outcome for `n`, or a pending presentation. No owner turn runs in
    /// between, so the row is seen as the arm left it.
    pub(super) fn consume(
        &mut self,
        a: &mut Attempt,
        n: u64,
        at: Instant,
        plane: Option<Point>,
        native: bool,
    ) -> std::result::Result<(), String> {
        let deadline = Instant::now() + WAIT;
        loop {
            a.peer.poll_io().unwrap();
            self.serve_at(a, at, plane, native)?;
            if !a.terminals(n).is_empty() || !self.pending(a).is_empty() {
                return Ok(());
            }
            assert!(
                Instant::now() < deadline,
                "candidate {n} never met the arm: {:?}",
                self.states()
            );
            std::thread::yield_now();
        }
    }

    /// The bar's pending presentations.
    pub(super) fn pending(&mut self, a: &Attempt) -> Vec<(ContentGrant, ContentOutputId, u64)> {
        self.h
            .owner
            .with_service(a.key, |service, _| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("bar required")
                };
                pending_presentations(&bar.content)
            })
            .unwrap()
    }

    /// Candidate `n` rendered by `reply` at `at`, collected into the row in
    /// `state`, with its owner-side validation result; the job.
    pub(super) fn rendered(
        &mut self,
        a: &mut Attempt,
        n: u64,
        reply: StubReply,
        at: Instant,
        state: &'static str,
    ) -> u64 {
        self.submit(a, n, PIXEL);
        let job = self.until(a, "in flight");
        self.native.serve_combined(output().id, at, reply);
        assert_eq!(self.until(a, state), job);
        self.drive_until(a, "the owner-side validation result", |lane, a| {
            lane.residency(a, n) == FrameImageResidency::HeldForFrame
        });
        job
    }

    /// Candidate `n` exported and Held, its result validated; the job.
    pub(super) fn held(&mut self, a: &mut Attempt, n: u64) -> u64 {
        self.rendered(a, n, exported(), Instant::now(), "held")
    }

    pub(super) fn verdict(&self, job: u64) -> Option<Verdict> {
        self.h.owner.processes.combined_row(0).verdict(job)
    }
}

/// Control 1 (and 13a's natural window: the walks between Held and the
/// validation result kept it, with no further reserve).
#[test]
fn a_matching_result_is_accepted_in_submit_bundle_with_prepared_and_pending() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    let reserves = lane.native.reserve_attempts();
    let before = lane.native.head_observation(output().id);
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(lane.native.classifies(), 1, "c1: one recomposition");
    assert_eq!(
        lane.native.accepts(),
        1,
        "c1: the matching result is accepted"
    );
    assert_eq!(lane.native.installs(), 0, "c1: no ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 0, "c1: no ordinary queue");
    assert_eq!(
        lane.native.reserve_attempts(),
        reserves,
        "c1: no second render"
    );
    assert_eq!(lane.native.release_attempts(), 0, "c1: nothing released");
    assert!(lane.states().is_empty(), "c1: accepted removes the entry");
    lane.trace_end(&[], &[]);
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)], "c1: pending");
    assert!(
        a.terminals(1).is_empty(),
        "c1: no terminal before presentation"
    );
    let after = lane.native.head_observation(output().id);
    assert!(
        !before.1 && after.1,
        "c1: the plan's frame is the head's rendering content"
    );
}

/// Control 2: a real scene change (the plane cursor moved).
#[test]
fn scene_churn_releases_the_result_and_enters_conventionally_once() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    lane.consume(&mut a, 1, Instant::now(), MOVED, true)
        .unwrap();
    assert_eq!(lane.native.accepts(), 0, "c2: churn is never accepted");
    assert_eq!(
        lane.native.release_attempts(),
        1,
        "c2: the churned result is released"
    );
    assert_eq!(lane.native.classifies(), 1, "c2: one recomposition");
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
    assert!(
        lane.states().is_empty(),
        "c2: the transfer removed the entry"
    );
    lane.trace_end(&[], &[]);
    assert_eq!(lane.native.installs(), 1, "c2: one conventional entry");
    assert_eq!(lane.native.ordinary_queues(), 1, "c2: one retained queue");
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    assert!(a.terminals(1).is_empty());
    let reserves = lane.native.reserve_attempts();
    let moved = Some(Point { x: 20.0, y: 4.0 });
    lane.serve_at(&mut a, Instant::now(), moved, true).unwrap();
    assert_eq!(lane.native.installs(), 1, "c2: no second entry");
    assert_eq!(lane.native.ordinary_queues(), 1, "c2: no second queue");
    assert_eq!(lane.native.reserve_attempts(), reserves, "c2: no reserve");
    assert_eq!(lane.native.classifies(), 1);
}

/// Controls 3 and 4. Declared class (labelled): with the authority check
/// first, a moved target or owner is refused before any classification
/// (control 9); the backend's scene controls produce each class from real
/// composition.
fn a_declared_class_takes_the_failure_terminal(class: CombinedRecompose, label: &str) {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    lane.native.declare_classification(class);
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "{label}: the failure terminal, once"
    );
    assert_eq!(lane.native.release_attempts(), 1, "{label}: released once");
    assert!(lane.states().is_empty(), "{label}: transferred");
    lane.trace_end(&[], &[]);
    assert!(lane.pending(&a).is_empty(), "{label}: no Prepared");
    assert_eq!(lane.native.installs(), 0, "{label}: no install entry");
    assert_eq!(lane.native.ordinary_queues(), 0, "{label}: no queue");
    assert_eq!(lane.native.accepts(), 0, "{label}: no accept");
}

#[test]
fn a_candidate_failure_ends_in_the_renderer_failure_terminal() {
    a_declared_class_takes_the_failure_terminal(CombinedRecompose::CandidateFailure, "c3");
}

#[test]
fn a_topology_change_ends_in_the_renderer_failure_terminal() {
    a_declared_class_takes_the_failure_terminal(CombinedRecompose::TopologyChange, "c4");
}

/// Control 5. Declared fault (labelled), after the authority check passed.
/// The stop and backoff that follow are the callers' (d).
#[test]
fn an_unclassified_recomposition_error_is_returned_first_and_releases_once() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    lane.native
        .declare_classify_fault("a declared recomposition fault");
    let served = lane.consume(&mut a, 1, Instant::now(), None, true);
    assert_eq!(
        served,
        Err("a declared recomposition fault".to_string()),
        "c5: the first error is returned"
    );
    assert_eq!(lane.native.classifies(), 1);
    assert_eq!(lane.native.release_attempts(), 1, "c5: released once");
    assert!(lane.states().is_empty());
    lane.trace_end(&[], &[]);
    assert!(a.terminals(1).is_empty(), "c5: no terminal");
    assert!(lane.pending(&a).is_empty(), "c5: no Prepared");
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
}

/// Control 6, RenderFailed and Expired. Stopped is reached in the lane
/// only by an attempt stop, whose grant then ends (b's control).
#[test]
fn a_render_failed_or_expired_result_takes_the_terminal_once() {
    for (reply, at, label) in [
        (StubReply::Failed, Instant::now(), "c6 render failed"),
        (exported(), Instant::now() + LATER, "c6 expired"),
    ] {
        let mut lane = Lane::new();
        let mut a = lane.attempt();
        lane.rendered(&mut a, 1, reply, at, "failed");
        lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
        assert_eq!(a.terminals(1), [FAILED], "{label}: one terminal");
        assert!(
            lane.states().is_empty(),
            "{label}: consumed_failed removes the entry"
        );
        lane.trace_end(&[], &[]);
        assert_eq!(lane.native.release_attempts(), 0, "{label}: no lease");
        assert_eq!(lane.native.classifies(), 0);
        assert_eq!(lane.native.installs(), 0);
        assert_eq!(lane.native.ordinary_queues(), 0);
        assert!(lane.pending(&a).is_empty());
    }
}

/// Control 8, absent entry: an unrelated reservation holds the cell, so
/// the frame is refused a renderer and its bundle has no row entry.
#[test]
fn a_bundle_with_no_row_entry_takes_the_ordinary_arm_unchanged() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let (_unrelated, _inbox) =
        ShellContentNative::reserve_combined_frame(&mut lane.native, output().id)
            .expect("the unrelated reservation holds the cell");
    lane.submit(&mut a, 1, PIXEL);
    lane.drive_until(&mut a, "the ordinary copy", |lane, a| {
        lane.residency(a, 1) == FrameImageResidency::HeldForFrame
    });
    assert!(lane.states().is_empty());
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(lane.native.installs(), 1, "c8 absent: ordinary arm");
    assert_eq!(lane.native.ordinary_queues(), 1);
    assert_eq!(lane.native.classifies(), 0);
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    assert!(
        lane.trace_no_admission() >= 1,
        "c8 absent: the ordinary join"
    );
}

/// Control 8, old grant: A's entry is Debt under A's grant; B's bundle for
/// the same output, generation and allocation never joins it.
#[test]
fn an_old_grants_entry_never_joins_a_new_bundle() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let j1 = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    lane.native.fill_renderer_queue();
    assert!(lane.turn().is_none());
    assert_eq!(lane.states(), [(j1, "debt")]);
    let mut b = lane.attempt();
    lane.submit(&mut b, 1, PIXEL);
    lane.drive_until(&mut b, "B's ordinary copy", |lane, b| {
        lane.residency(b, 1) == FrameImageResidency::HeldForFrame
    });
    let served = lane.consume(&mut b, 1, Instant::now(), None, true);
    assert!(
        served.is_ok(),
        "c8 old grant: the old entry never joins: {served:?}"
    );
    assert_eq!(lane.states(), [(j1, "debt")], "c8 old grant: untouched");
    lane.trace_end(&[(j1, "debt")], &[]);
    assert_eq!(lane.native.installs(), 1);
    assert_eq!(lane.native.ordinary_queues(), 1);
    assert_eq!(lane.native.classifies(), 0);
}

/// Control 9 (C1): real scene churn together with a declared head state
/// (labelled), each a native refusal at the authority check.
#[test]
fn scene_churn_with_a_busy_head_or_a_protected_retirement_takes_the_failure_terminal() {
    for (state, label) in [
        (CombinedDeclaredHead::PendingOrdinary, "c9 busy head"),
        (CombinedDeclaredHead::RetirementOwed, "c9 retirement owed"),
        (CombinedDeclaredHead::OwnerReplaced, "c9 owner replaced"),
    ] {
        let mut lane = Lane::new();
        let mut a = lane.attempt();
        let job = lane.held(&mut a, 1);
        lane.native.declare_head(output().id, state);
        let before = lane.native.head_observation(output().id);
        let served = lane.consume(&mut a, 1, Instant::now(), MOVED, true);
        assert!(served.is_ok(), "{label}: refused without error: {served:?}");
        assert_eq!(
            lane.native.classifies(),
            0,
            "{label}: authority before the scene"
        );
        assert_eq!(
            lane.native.release_attempts(),
            1,
            "{label}: the refused result is released"
        );
        assert_eq!(a.terminals(1), [FAILED], "{label}: the failure terminal");
        assert_eq!(
            lane.native.head_observation(output().id),
            before,
            "{label}: native state unchanged"
        );
        assert_eq!(lane.native.installs(), 0, "{label}: no install entry");
        assert_eq!(lane.native.ordinary_queues(), 0, "{label}: no queue");
        assert_eq!(lane.native.accepts(), 0);
        assert!(lane.pending(&a).is_empty());
        // The declined result's one release: through the issuing exporter
        // it transfers and the entry goes; with the native owner replaced
        // no exporter issues the head any more, the release is NotFound and
        // the entry stays Debt, the custody no release is inferred from.
        // Transfers are command ownership, not slot freedom.
        let replaced = state == CombinedDeclaredHead::OwnerReplaced;
        assert_eq!(
            lane.native.release_transfers(),
            usize::from(!replaced),
            "{label}: the release transferred unless the owner was replaced"
        );
        assert_eq!(
            lane.h.owner.processes.combined_row(0).debt(job),
            replaced.then_some(CombinedRelease::NotFound),
            "{label}: NotFound kept as Debt only for the replaced owner"
        );
        let retained: &[(u64, &str)] = if replaced { &[(job, "debt")] } else { &[] };
        lane.trace_end(retained, &[]);
    }
}

/// G3-5d d-0: a capture acquired after reserve. The consumption check
/// refuses before any classification, so the result takes the baseline
/// failure terminal and is released, and nothing enters conventionally. The
/// capture is the exporter's, left by a discard through real exporter calls;
/// this fixture has no CaptureOwner endpoint (the backend's accept controls
/// read one). d-2b3e form 3: the fixture's own record of its check names
/// the guard that answered, CaptureActive; the Session's disposition alone
/// could not.
fn a_capture_after_reserve_takes_the_failure_terminal(plane: Option<Point>, label: &str) {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    let (checks, _) = lane.native.check_observation();
    lane.native.capture_left_by_discard(output().id);
    let before = lane.native.head_observation(output().id);
    let served = lane.consume(&mut a, 1, Instant::now(), plane, true);
    assert!(served.is_ok(), "{label}: refused without error: {served:?}");
    assert_eq!(
        lane.native.check_observation(),
        (checks + 1, Some(Err(CombinedAcceptRefusal::CaptureActive))),
        "{label}: one check, answered by the capture guard"
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
}

#[test]
fn a_capture_acquired_after_reserve_refuses_an_otherwise_matching_result() {
    a_capture_after_reserve_takes_the_failure_terminal(None, "d0 capture, match");
}

/// The moved plane is control 2's real scene change.
#[test]
fn a_capture_acquired_after_reserve_refuses_a_scene_churn_result() {
    a_capture_after_reserve_takes_the_failure_terminal(MOVED, "d0 capture, churn");
}

/// Control 11: a real setter refusal (native presentation quiesced). The
/// error is the ordinary setter's own text for that state.
#[test]
fn a_setter_refusal_returns_the_ordinary_error_and_releases() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    lane.runtime
        .suspend_revoked_native_scanout(&[output()])
        .unwrap();
    let served = lane.consume(&mut a, 1, Instant::now(), None, true);
    assert_eq!(
        served,
        Err(
            "shell content candidate cannot be accepted while native presentation is quiesced"
                .to_string()
        ),
        "c11: the setter's own error"
    );
    assert_eq!(lane.native.release_attempts(), 1, "c11: released once");
    assert!(a.terminals(1).is_empty(), "c11: no terminal");
    assert_eq!(lane.native.classifies(), 0);
    lane.trace_end(&[], &[]);
}

/// Control 12: CandidateFailure (declared, labelled) with the Release cell
/// Full. The terminal is issued, the entry stays Debt through stop and
/// reconnect, and the next free flush removes it once.
#[test]
fn a_full_release_cell_keeps_debt_through_the_terminal_and_reconnect() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    lane.native
        .declare_classification(CombinedRecompose::CandidateFailure);
    lane.native.fill_renderer_queue();
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(a.terminals(1), [FAILED], "c12: the terminal is issued");
    assert_eq!(
        lane.states(),
        [(job, "debt")],
        "c12: Debt survives the terminal"
    );
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    let _b = lane.attempt();
    assert_eq!(
        lane.states(),
        [(job, "debt")],
        "c12: Debt survives reconnect"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty(), "c12: the free flush transferred");
    lane.trace_end(&[], &[]);
    assert_eq!(lane.native.release_attempts(), 2);
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
}

/// Control 15 (C3): rendered and Held before its deadline, consumed after
/// it while the candidate is still submittable.
#[test]
fn a_result_consumed_after_its_deadline_is_declined_with_the_failure_terminal() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.held(&mut a, 1);
    let at = Instant::now() + LATER;
    lane.consume(&mut a, 1, at, None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "c15: the deadline gates consumption"
    );
    assert_eq!(lane.native.classifies(), 0, "c15: nothing classified");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert_eq!(lane.native.release_attempts(), 1);
    assert!(lane.states().is_empty());
    lane.trace_end(&[], &[]);
    assert!(lane.pending(&a).is_empty(), "c15: no Prepared");
}
