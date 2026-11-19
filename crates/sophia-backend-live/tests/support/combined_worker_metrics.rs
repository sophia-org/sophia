//! Worker accounting proposal 01 (Codex review 2026-11-16T12:45:49Z): the
//! combined Release-cell population, counted once per successful transition,
//! beside the unchanged ordinary `requests`, `completions` and `failures`.

use super::super::super::{LiveRendererCombinedWorkerMetrics as Counts, WorkerPoll};
use super::*;

fn counts(facade: &NativeGbmRendererWorker) -> Counts {
    facade.metrics().combined
}

/// The drained-capture conditions the gate checks: O1, C1-C4 and Q.
fn assert_drained(facade: &NativeGbmRendererWorker) {
    let metrics = facade.metrics();
    let c = metrics.combined;
    assert_eq!(c.admissions, c.cancels + c.holds, "C1 {c:?}");
    assert_eq!(c.holds, c.acceptances + c.give_ups, "C2 {c:?}");
    assert_eq!(c.give_ups, c.release_enqueues, "C3 {c:?}");
    assert_eq!(c.acceptances, c.completions, "C4 {c:?}");
    let ordinary = metrics
        .completions
        .checked_sub(c.completions)
        .expect("combined completions are part of the total");
    assert_eq!(metrics.requests, ordinary, "O1 {metrics:?}");
    assert_eq!((c.cells_not_free, c.receipts_pending), (0, 0), "Q {c:?}");
}

/// Reserves, renders and settles one exported frame: the result is Held. The
/// reply sender is returned so the facade's channel stays open, as in G3-4.
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
    (facade, commands, reply, issued, job)
}

fn expected(lease: u64) -> crate::LiveRendererFrameCorrelation {
    frame_correlation(&mixed_job(lease), Some(COMBINED_REQUEST))
}

#[test]
fn an_accepted_result_counts_once_as_acceptance_and_as_combined_completion() {
    let (mut facade, commands, _reply, issued, job) = held(81);
    assert_eq!(
        counts(&facade),
        Counts {
            admissions: 1,
            holds: 1,
            cells_not_free: 1,
            ..Counts::default()
        },
        "held, not yet accepted"
    );
    let before = facade.metrics();
    facade.accept_combined_held(&issued, job, expected(81), None);
    assert_eq!(
        counts(&facade),
        Counts {
            admissions: 1,
            holds: 1,
            acceptances: 1,
            ..Counts::default()
        },
        "accepted, not yet consumed"
    );
    let WorkerPoll::Exported(lease) = facade.poll() else {
        panic!("poll takes the accepted result as the reply");
    };
    let after = facade.metrics();
    assert_eq!(
        after.requests, before.requests,
        "an acceptance is not a request"
    );
    assert_eq!(
        after.completions,
        before.completions + 1,
        "the total counts the accepted reply"
    );
    assert_eq!(after.failures, before.failures);
    assert_eq!(after.combined.completions, 1);
    assert!(matches!(facade.poll(), WorkerPoll::Idle));
    assert_eq!(counts(&facade).completions, 1, "taken once, counted once");
    assert_drained(&facade);
    drop(lease);
    assert!(
        matches!(commands.try_recv(), Ok(WorkerCommand::Release { lease_id, .. })
            if lease_id == LiveRendererWorkerLeaseId(81)),
        "the accepted lease releases as an ordinary one does"
    );
}

#[test]
fn an_accepted_result_the_checks_refuse_is_a_failure_not_a_combined_completion() {
    let (mut facade, _commands, _reply, issued, job) = held(82);
    facade.accept_combined_held(&issued, job, expected(83), None);
    assert!(matches!(facade.poll(), WorkerPoll::Failed(_)));
    let metrics = facade.metrics();
    assert_eq!((metrics.completions, metrics.failures), (0, 1));
    assert_eq!(
        (metrics.combined.acceptances, metrics.combined.completions),
        (1, 0),
        "C4 exposes the refused reply"
    );
}

#[test]
fn a_give_up_behind_a_full_queue_counts_once_and_its_retries_only_as_failures() {
    let (mut facade, commands, _reply, issued, job) = held(84);
    for _ in 0..WORKER_COMMAND_CAPACITY {
        facade
            .core
            .command_sender
            .try_send(WorkerCommand::Shutdown)
            .unwrap();
    }
    assert_eq!(
        facade.release_combined_held(&issued, job),
        CombinedRelease::Pending
    );
    assert_eq!(
        facade.flush_combined_release_for(&issued, job),
        CombinedRelease::Pending
    );
    let c = counts(&facade);
    assert_eq!(
        (
            c.give_ups,
            c.release_enqueues,
            c.release_enqueue_failures,
            c.cells_not_free
        ),
        (1, 0, 2, 1),
        "one give-up, two refused sends, still retained: {c:?}"
    );
    for _ in 0..WORKER_COMMAND_CAPACITY {
        assert!(matches!(commands.try_recv(), Ok(WorkerCommand::Shutdown)));
    }
    assert!(
        matches!(facade.poll(), WorkerPoll::Idle),
        "an idle poll enqueues the retained Release"
    );
    let c = counts(&facade);
    assert_eq!(
        (
            c.give_ups,
            c.release_enqueues,
            c.cells_not_free,
            c.receipts_pending
        ),
        (1, 1, 0, 1),
        "enqueued, receipt unread: {c:?}"
    );
    assert_eq!(
        facade.flush_combined_release_for(&issued, job),
        CombinedRelease::Transferred
    );
    assert_eq!(
        counts(&facade).release_enqueues,
        1,
        "reading the receipt is not another enqueue"
    );
    assert_drained(&facade);
    assert!(
        matches!(commands.try_recv(), Ok(WorkerCommand::Release { lease_id, .. })
            if lease_id == LiveRendererWorkerLeaseId(84))
    );
}

#[test]
fn cancels_count_freed_reservations_and_a_refused_hold_counts_nothing() {
    let (mut facade, commands, _reply) = correlated_facade();
    let (first, _inbox) = facade.reserve_combined_frame().expect("Free reserves");
    assert!(matches!(
        commands.try_recv(),
        Ok(WorkerCommand::RegisterCombined(_))
    ));
    assert!(facade.cancel_combined_frame(&first));
    assert!(
        !facade.cancel_combined_frame(&first),
        "a cancel is spent once"
    );
    assert_eq!(
        counts(&facade),
        Counts {
            admissions: 1,
            cancels: 1,
            ..Counts::default()
        }
    );
    let ticket = facade
        .reserve_combined_release()
        .expect("a cancelled reservation frees the cell");
    let mut foreign = exported(85, 85);
    foreign.output = LiveRendererWorkerOutputKey::from_raw(999);
    assert!(facade.hold_combined_result(ticket, 1, foreign).is_some());
    let c = counts(&facade);
    assert_eq!(
        c,
        Counts {
            admissions: 2,
            cancels: 1,
            cells_not_free: 1,
            ..Counts::default()
        },
        "a refused hold is neither a hold nor a cancel"
    );
    assert_ne!(
        c.admissions,
        c.cancels + c.holds,
        "C1 exposes the unresolved reservation"
    );
    assert!(facade.cancel_combined_release(ticket));
    assert_drained(&facade);
}

#[test]
fn a_render_that_exported_nothing_frees_its_reservation_as_a_cancel() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let (issued, inbox) = facade
        .reserve_combined_frame()
        .expect("an idle facade reserves");
    let WorkerCommand::RegisterCombined(registration) = commands.recv().unwrap() else {
        panic!("first combined use registers the route");
    };
    let mut route = registration
        .route_for(facade.output(), &facade.claim)
        .expect("route for the live claim");
    let reservation = submit(&mut validator, inbox, 86, Instant::now() + WAIT);
    route.serve(Instant::now(), |_, pixels, _| {
        RenderAttempt::Done(CombinedOutcome::Rendered(rendered(86)), pixels)
    });
    let result = facade.take_combined_result().unwrap();
    assert!(
        facade
            .settle_combined_frame(&issued, &mut validator, reservation, result)
            .is_ok()
    );
    assert_eq!(
        counts(&facade),
        Counts {
            admissions: 1,
            cancels: 1,
            ..Counts::default()
        }
    );
    assert_drained(&facade);
}

#[test]
fn an_ordinary_render_leaves_the_combined_population_at_zero() {
    let (mut facade, commands, results) = correlated_facade();
    let (correlation, _) = super::super::submit_mixed_job(&mut facade, &commands, 87);
    let mut ordinary = correlated_result(correlation, exported_outcome());
    ordinary.output = facade.output;
    results.send(ordinary).unwrap();
    assert!(matches!(facade.poll(), WorkerPoll::Exported(_)));
    let metrics = facade.metrics();
    assert_eq!((metrics.requests, metrics.completions), (1, 1));
    assert_eq!(metrics.combined, Counts::default());
    assert_drained(&facade);
}

struct NoDevice;

impl crate::RenderDeviceDiscoveryBackend for NoDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> std::io::Result<std::fs::File> {
        Err(std::io::Error::other("no render device in this test"))
    }
}

/// Within one owner an exporter's facade is attached once and never replaced,
/// so the owner's sum over its exporters covers every facade it had.
#[test]
fn an_exporter_keeps_its_first_facade_so_its_counts_survive_a_second_attach() {
    let first = device_free_core(9_501);
    let second = device_free_core(9_502);
    let mut exporter = crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(NoDevice);
    exporter.set_output(LiveRendererWorkerOutputKey::from_raw(951));
    exporter.attach_shared_worker(&first);
    let (issued, _inbox) = exporter
        .reserve_combined_frame()
        .expect("the attached facade reserves");
    assert_eq!(exporter.cancel_combined_frame(&issued), Some(true));
    exporter.attach_shared_worker(&second);
    assert!(
        exporter.issued_combined(&issued),
        "the first facade remains"
    );
    let c = exporter
        .worker_metrics()
        .expect("a worker exporter")
        .combined;
    assert_eq!(
        (c.admissions, c.cancels),
        (1, 1),
        "its counts survive: {c:?}"
    );
}
