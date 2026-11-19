//! G3-4: the facade's half of the combined accept (AUTHORITY-STEP3 F2, R5).
//! A held result is acceptable only by its own admission on an idle facade,
//! and once accepted `poll()` takes it as the reply, through the ordinary
//! checks, exactly once.

use super::super::super::{CombinedAcceptRefusal, StalledRequest, WorkerPoll};
use super::*;

/// A held exported result for lease `lease` on a fresh facade.
fn held(
    lease: u64,
) -> (
    NativeGbmRendererWorker,
    Receiver<WorkerCommand>,
    sophia_wake::SignalSender<WorkerResult>,
    CombinedFacade,
    u64,
) {
    let (mut facade, commands, reply) = correlated_facade();
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let (issued, reservation, result) =
        exported_result(&mut facade, &commands, &mut validator, lease);
    let job = reservation.job();
    assert!(
        facade
            .settle_combined_frame(&issued, &mut validator, reservation, result)
            .is_ok()
    );
    assert_held(&facade, job, lease, "settlement holds the exported result");
    (facade, commands, reply, issued, job)
}

/// What the owner expects for `exported(lease, lease)`.
fn expected(lease: u64) -> crate::LiveRendererFrameCorrelation {
    frame_correlation(&mixed_job(lease), Some(COMBINED_REQUEST))
}

#[test]
fn a_held_result_is_refused_on_a_busy_facade_or_for_another_job_and_stays_held() {
    let (mut facade, commands, _reply, issued, job) = held(71);
    assert_eq!(facade.combined_accept_refusal(&issued, job), None);
    facade.quarantined = true;
    assert_eq!(
        facade.combined_accept_refusal(&issued, job),
        Some(CombinedAcceptRefusal::Quarantined)
    );
    facade.quarantined = false;
    facade.stalled = Some(StalledRequest {
        since: Instant::now(),
    });
    assert_eq!(
        facade.combined_accept_refusal(&issued, job),
        Some(CombinedAcceptRefusal::Stalled)
    );
    facade.stalled = None;
    assert_eq!(
        facade.combined_accept_refusal(&issued, job.wrapping_add(1)),
        Some(CombinedAcceptRefusal::NotHeld)
    );
    assert_held(&facade, job, 71, "no refusal touched the held result");
    // Last: the ordinary reply never comes in this test.
    let _ = super::super::submit_mixed_job(&mut facade, &commands, 72);
    assert_eq!(
        facade.combined_accept_refusal(&issued, job),
        Some(CombinedAcceptRefusal::OrdinaryInFlight)
    );
    assert_held(&facade, job, 71, "an ordinary render leaves it held");
    assert_eq!(
        facade.release_combined_held(&issued, job),
        CombinedRelease::Transferred,
        "a refused result stays for ordinary disposal"
    );
}

/// Codex 4c-4 F1 again: the same job number under another ticket is not this
/// admission.
#[test]
fn an_old_ticket_with_the_same_job_cannot_accept_its_successor() {
    let (mut facade, commands, _reply) = correlated_facade();
    let (old, inbox) = facade.reserve_combined_frame().expect("Free reserves");
    let WorkerCommand::RegisterCombined(registration) = commands.recv().unwrap() else {
        panic!("first combined use registers the route");
    };
    let mut route = registration
        .route_for(facade.output(), &facade.claim)
        .expect("route for the live claim");
    let mut first = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = submit(&mut first, inbox, 73, Instant::now() + WAIT);
    route.serve(Instant::now(), |_, pixels, _| {
        RenderAttempt::Done(CombinedOutcome::Rendered(exported(73, 73)), pixels)
    });
    let result = facade.take_combined_frame(&old).unwrap().unwrap();
    let job = reservation.job();
    assert!(
        facade
            .settle_combined_frame(&old, &mut first, reservation, result)
            .is_ok()
    );
    assert_eq!(
        facade.release_combined_held(&old, job),
        CombinedRelease::Transferred
    );
    let (current, inbox) = facade.reserve_combined_frame().unwrap();
    let mut second = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = submit(&mut second, inbox, 74, Instant::now() + WAIT);
    assert_eq!(reservation.job(), job, "separate validators repeat jobs");
    route.serve(Instant::now(), |_, pixels, _| {
        RenderAttempt::Done(CombinedOutcome::Rendered(exported(74, 74)), pixels)
    });
    let result = facade.take_combined_frame(&current).unwrap().unwrap();
    assert!(
        facade
            .settle_combined_frame(&current, &mut second, reservation, result)
            .is_ok()
    );
    assert_eq!(
        facade.combined_accept_refusal(&old, job),
        Some(CombinedAcceptRefusal::NotHeld),
        "an old ticket names the same job but not this admission"
    );
    assert_eq!(facade.combined_accept_refusal(&current, job), None);
    assert_held(&facade, job, 74, "the old ticket changed nothing");
}

#[test]
fn poll_takes_an_accepted_result_as_its_reply_exactly_once() {
    let (mut facade, commands, _reply, issued, job) = held(75);
    facade.accept_combined_held(&issued, job, expected(75), None);
    assert!(
        matches!(facade.combined_release, CombinedReleaseCell::Free),
        "the accepted result left the cell"
    );
    assert!(facade.in_flight(), "the accepted result is in flight");
    assert_eq!(facade.in_flight_correlation(), Some(expected(75)));
    assert!(commands.try_recv().is_err(), "acceptance queued no render");
    let WorkerPoll::Exported(lease) = facade.poll() else {
        panic!("poll takes the accepted result as the reply");
    };
    assert_eq!(lease.lease_id, LiveRendererWorkerLeaseId(75));
    assert_eq!(lease.correlation(), expected(75));
    assert!(!facade.in_flight());
    assert!(
        matches!(facade.poll(), WorkerPoll::Idle),
        "the accepted result is taken once"
    );
    assert!(commands.try_recv().is_err());
    drop(lease);
    assert!(
        matches!(commands.try_recv(), Ok(WorkerCommand::Release { lease_id, .. })
            if lease_id == LiveRendererWorkerLeaseId(75)),
        "the lease releases as an ordinary one does"
    );
}

#[test]
fn an_accepted_result_that_does_not_match_its_expected_correlation_is_quarantined() {
    let (mut facade, commands, _reply, issued, job) = held(76);
    facade.accept_combined_held(&issued, job, expected(77), None);
    assert!(
        matches!(
            facade.poll(),
            WorkerPoll::Failed(LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
        ),
        "the ordinary correlation check refuses the accepted result"
    );
    assert!(facade.quarantined);
    assert!(
        matches!(commands.try_recv(), Ok(WorkerCommand::Release { lease_id, .. })
            if lease_id == LiveRendererWorkerLeaseId(76)),
        "the refused lease is released, not leaked"
    );
}
