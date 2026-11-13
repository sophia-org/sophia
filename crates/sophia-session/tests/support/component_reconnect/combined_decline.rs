//! G3-5d d-2b3b1 Session controls: running-core decline and in-slot failure
//! (REV2 R3 as dispositioned by the d-2b3b reachability review). Every owner
//! turn is the split driver with native service, admission included (b2's
//! `admitted`).
//!
//! Two controls start from a labelled fault declared before submission. The
//! running seam's one-shot draw failure fails after bind (row 1). A one-shot
//! plan mismatch widens the first placeholder of the real plan, so the
//! worker's admission check disagrees with the job's real description
//! (row 2). Two controls start from an already-rendered, uncollected result
//! (`rendered`) and decline it in the content arm: a real scene change, the
//! hardware cursor plane moved (row 3), and the arm's own admitted-deadline
//! check C3 (row 4).
//!
//! A release made by the content arm is read at that call's boundary, before
//! the walk (`driver_turn_observed`): the walk releases a Consumed Held entry
//! by itself, so final counters alone cannot show that the arm declined.
//!
//! These are owner-side controls. The failure, Malformed and C3 terminals,
//! the candidate's resource status and their publication and peer
//! visibility are b3d obligations, beside P1-P3, and are not asserted here.
//! Clearing a non-empty slot history after a failed draw stays backend
//! evidence (combined_frame_seam.rs). The combined path publishes no damage
//! metrics, so Session cannot observe it, and no Session control here claims
//! it.
use super::*;
use crate::shell_component_connections::images::content_now_msec;
use sophia_runtime::FrameCandidateState::{Assembling, Unknown};

/// The real scene change: the hardware cursor plane moved, as in the stub
/// churn control.
const MOVED: Option<Point> = Some(Point { x: 10.0, y: 4.0 });
/// The content step's observation in a turn where the arm made no accept
/// and no release.
const NOTHING: [Observed; 1] = [(0, (0, 0))];
/// The content step's observation in a turn where the arm declined once:
/// one release attempt, answered Transferred, before the walk.
const DECLINED: [Observed; 1] = [(0, (1, 1))];

impl Lane {
    /// One driver turn at `instant` with the plane at `plane` and native
    /// service; its steps and the content-boundary observations.
    fn observed_turn(
        &mut self,
        instant: Instant,
        plane: Option<Point>,
    ) -> (Vec<Step>, Vec<Observed>) {
        let mut steps = Vec::new();
        let mut errors = Vec::new();
        let (turned, observed) =
            self.driver_turn_observed(&mut steps, instant, plane, &mut |native| {
                if let Err(error) = native.service_native(output().id) {
                    errors.push(error.to_string());
                }
            });
        let first = turned.expect("the turn completes");
        assert!(first.iter().all(Option::is_none), "{first:?}");
        assert!(errors.is_empty(), "native service: {errors:?}");
        (steps, observed)
    }

    /// The negotiated candidate timeout, in content-clock milliseconds.
    fn candidate_timeout(&mut self, a: &Attempt) -> u64 {
        let limits = self
            .h
            .owner
            .with_service(a.key, |_, t| t.content_limits().cloned())
            .unwrap()
            .expect("negotiated limits");
        u64::from(limits.candidate_timeout_ms)
    }
}

/// Candidate 1 admitted through b2's split-driver turns with a labelled
/// fault already declared, then its result's ring, with no owner call after
/// the turn that left the job in flight. The worker took one slot for it;
/// nothing is released.
fn faulted(lane: &mut Lane, wake: &sophia_wake::Wake, a: &mut Attempt) -> u64 {
    lane.submit(a, 1, PIXEL);
    let job = lane.admitted(a);
    assert!(
        wake.wait(Some(Instant::now() + WAIT)).unwrap(),
        "the faulted job's result rings the owner"
    );
    assert_eq!(lane.states(), [(job, "in flight")], "not yet collected");
    assert_eq!(lane.acquisitions(), 1, "the worker took a slot for it");
    assert_eq!(lane.releases(), (0, 0));
    job
}

/// REV2 R3's render-failure row, labelled fault; renamed because Session
/// cannot observe the slot invalidation (module doc). The draw fails after
/// bind: the worker frees the slot it took before the result comes home,
/// and leases nothing. T1 collects RenderFailed; content takes the failure
/// path and consumes the entry; nothing is held, released, accepted or
/// installed.
#[test]
fn a_render_failure_in_its_slot_frees_the_slot_and_takes_the_failure_path() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_render_failure();
    let job = faulted(&mut lane, &wake, &mut a);
    assert_eq!(
        lane.leased(),
        0,
        "the failed draw freed its slot before delivery"
    );
    assert!(
        !lane.native.render_failure_armed(),
        "the declared draw failure was taken, once"
    );
    assert_eq!(
        lane.native.seen().len(),
        1,
        "the seam was entered with the bound frame"
    );
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "content neither accepted nor released");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::RenderFailed)]
    );
    assert!(
        lane.states().is_empty(),
        "the failure path consumed the entry"
    );
    assert_eq!(lane.releases(), (0, 0), "nothing was leased to release");
    assert_eq!(lane.leased(), 0);
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert_eq!(lane.native.ordinary_queues(), 0);
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "nothing later");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::RenderFailed)],
        "collected once"
    );
    assert_eq!(lane.acquisitions(), 1, "no second render");
    assert_eq!(lane.native.seen().len(), 1);
}

/// REV2 R3's description-mismatch row, labelled fault, with the trigger the
/// reachability review corrected: the plan, not the exported descriptor.
/// The worker takes a slot, finds the widened placeholder disagreeing with
/// the job's admitted description, frees the slot unbound and never calls
/// the seam (CombinedOutcome::Mismatch). T1 collects Mismatch and the owner
/// answers the copy Malformed, which removes the candidate's assembly: the
/// fact is Unknown (same generation, stream open), so the walk keeps the
/// Failed entry until the original admitted deadline, then removes it.
#[test]
fn a_plan_description_mismatch_is_malformed_and_frees_the_slot_unbound() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_plan_mismatch();
    let job = faulted(&mut lane, &wake, &mut a);
    assert_eq!(
        lane.native.plan_mismatches(),
        (1, 0),
        "the declared mismatch widened one real placeholder, once"
    );
    assert!(
        lane.native.seen().is_empty(),
        "no seam call: nothing was bound or rendered"
    );
    assert_eq!(lane.leased(), 0, "the mismatch freed its slot unbound");
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let deadline = lane.deadline_of(job);
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "no bundle met the failed entry");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Mismatch)]
    );
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Unknown),
        "the Malformed copy removed the assembly: same generation, stream open"
    );
    assert_eq!(
        lane.states(),
        [(job, "failed")],
        "the walk keeps the Failed entry on Unknown before its deadline"
    );
    assert_eq!(lane.releases(), (0, 0), "no lease, no Release");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert_eq!(lane.quiet_driver_turn_at(deadline + MARGIN).0, TURN);
    assert!(
        lane.states().is_empty(),
        "past its deadline the walk removed it"
    );
    assert_eq!(lane.releases(), (0, 0));
    assert_eq!(lane.acquisitions(), 1, "no later render");
    assert!(lane.native.seen().is_empty());
    assert_eq!(lane.native.plan_mismatches(), (1, 0));
}

/// REV2 R3's scene-churn row on the running core. T1 moves the hardware
/// cursor plane, a real scene change read by the ordinary classifier.
/// Content classifies SceneChurn, declines (Transferred at its own call)
/// and makes one ordinary install entry, queued on the fixture's target.
/// That is one queued entry, not a conventional render or a presentation.
#[test]
fn scene_churn_releases_the_result_and_makes_one_ordinary_entry() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    assert!(
        lane.native.ordinary_queue_ready(output().id),
        "precondition: the output's queue takes an ordinary entry"
    );
    assert_eq!(lane.native.classifies(), 0);
    let (steps, observed) = lane.observed_turn(Instant::now(), MOVED);
    assert_eq!(steps, TURN);
    assert_eq!(
        observed, DECLINED,
        "the content arm declined the churned result before the walk"
    );
    assert_eq!(lane.native.classifies(), 1, "one recomposition");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)]
    );
    assert!(lane.states().is_empty(), "the receipt removed it");
    assert_eq!(lane.releases(), (1, 1), "the walk released nothing more");
    assert_eq!(lane.native.accepts(), 0, "churn is never accepted");
    assert_eq!(lane.native.installs(), 1, "one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1, "one retained queue");
    assert_eq!(
        lane.pending(&a),
        [(a.key.grant, OUTPUT, 1)],
        "one pending presentation for the ordinary entry"
    );
    lane.slot_freed("churn");
}

/// REV2 R3's idle-output row, as dispositioned: no idle decline class
/// exists, and the real arm decline that queues no render is C3, the
/// admission's own deadline. The result is collected in a turn whose
/// Instant is past the admitted deadline. Collection precedes the image
/// owner's expiry, which skips a job with a result, so the result reaches
/// the arm; the content clock is not advanced, so the candidate's own
/// deadline cannot stand in for C3. This is late consumption declined
/// without another render, not idle detection.
#[test]
fn a_result_collected_past_its_admitted_deadline_is_declined_without_a_later_render() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let candidate = lane.candidate_timeout(&a);
    let before = content_now_msec();
    let job = rendered(&mut lane, &wake, &mut a);
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    assert_eq!(
        lane.candidate_of(&a, binding, job),
        Some(Assembling),
        "precondition: the candidate is live, its copy outstanding"
    );
    let late = lane.deadline_of(job) + MARGIN;
    let (steps, observed) = lane.observed_turn(late, None);
    assert!(
        content_now_msec() < before + candidate,
        "precondition: the deciding turn ended before the candidate's own deadline"
    );
    assert_eq!(steps, TURN);
    assert_eq!(
        observed, DECLINED,
        "C3 declined at the content call, before the walk"
    );
    assert_eq!(
        lane.native.classifies(),
        0,
        "C3 precedes authority and classification"
    );
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Rendered)]
    );
    assert!(lane.states().is_empty(), "the receipt removed it");
    assert_eq!(lane.releases(), (1, 1), "the walk released nothing more");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.installs(), 0, "no ordinary install");
    assert_eq!(lane.native.ordinary_queues(), 0, "no render queued");
    lane.slot_freed("C3");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "nothing more");
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.native.ordinary_queues(), 0, "no later render queued");
    assert_eq!(lane.acquisitions(), 1, "no later render");
}
