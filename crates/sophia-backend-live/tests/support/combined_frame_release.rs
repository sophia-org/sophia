//! The combined frame-job Release cell on the production facade (stage 4b).

use super::super::{
    CombinedReleaseCell, CombinedReleaseFlush, DiscardedWorkerLease, LiveRendererFrameSlotPool,
    LiveRendererFrameSlotToken, LiveRendererWorkerLeaseId, LiveRendererWorkerOutputKey,
    LiveRendererWorkerRequestId, WORKER_COMMAND_CAPACITY, WorkerCommand, WorkerOutcome,
    WorkerResult, frame_correlation,
};
use super::{correlated_facade, correlated_result, exported_outcome, mixed_job, submit_mixed_job};
use crate::{LiveRendererFrameSlotAcquire, LiveRendererFrameSlotRelease};
use std::sync::atomic::Ordering;

fn acquire(slots: &mut LiveRendererFrameSlotPool) -> LiveRendererFrameSlotToken {
    let LiveRendererFrameSlotAcquire::Acquired(token) = slots.try_acquire() else {
        panic!("fixture expected an available slot");
    };
    token
}

fn result(token: LiveRendererFrameSlotToken, lease: u64) -> WorkerResult {
    let mut outcome = exported_outcome();
    let WorkerOutcome::Exported {
        lease_id,
        slot_token,
        ..
    } = &mut outcome
    else {
        unreachable!();
    };
    *lease_id = LiveRendererWorkerLeaseId(lease);
    *slot_token = token;
    correlated_result(
        frame_correlation(&mixed_job(31), Some(LiveRendererWorkerRequestId(7))),
        outcome,
    )
}

fn assert_held(
    facade: &super::super::NativeGbmRendererWorker,
    lease: u64,
    token: LiveRendererFrameSlotToken,
) {
    let CombinedReleaseCell::Retained(release) = &facade.combined_release else {
        panic!("Release obligation must remain in its dedicated cell");
    };
    assert_eq!(release.output, facade.output);
    assert_eq!(release.lease_id, LiveRendererWorkerLeaseId(lease));
    assert_eq!(release.slot_token, token);
}

fn assert_release(
    command: WorkerCommand,
    output: LiveRendererWorkerOutputKey,
    lease: u64,
    token: LiveRendererFrameSlotToken,
) {
    assert!(
        matches!(command, WorkerCommand::Release { output: actual, lease_id, slot_token }
        if actual == output && lease_id == LiveRendererWorkerLeaseId(lease) && slot_token == token)
    );
}

#[test]
fn one_cell_reserves_before_retaining_and_reopens_only_after_enqueue() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    let original = result(token, 41);
    let original = facade
        .retain_combined_result(original)
        .expect("unreserved result is returned");
    assert!(facade.reserve_combined_release());
    assert!(
        !facade.reserve_combined_release(),
        "one combined reservation per facade"
    );
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::NoRelease
    );
    assert!(facade.retain_combined_result(original).is_none());
    assert!(
        !facade.reserve_combined_release(),
        "retained obligation still occupies capacity"
    );
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::Enqueued
    );
    assert_release(commands.try_recv().unwrap(), facade.output, 41, token);
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::NoRelease
    );
    assert!(commands.try_recv().is_err(), "no duplicate Release");
    assert!(facade.reserve_combined_release());
}

#[test]
fn foreign_and_non_exported_results_return_to_the_owner_untouched() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    let mut foreign = result(token, 42);
    foreign.output = LiveRendererWorkerOutputKey::from_raw(999);
    let returned = facade
        .retain_combined_result(foreign)
        .expect("foreign output must be returned");
    assert_eq!(returned.output, LiveRendererWorkerOutputKey::from_raw(999));
    assert!(
        matches!(returned.outcome, WorkerOutcome::Exported { lease_id, slot_token, .. }
        if lease_id == LiveRendererWorkerLeaseId(42) && slot_token == token)
    );

    let frame = mixed_job(99);
    let correlation = frame_correlation(&frame, Some(LiveRendererWorkerRequestId(8)));
    let deferred = correlated_result(correlation, WorkerOutcome::Deferred(frame));
    let returned = facade
        .retain_combined_result(deferred)
        .expect("Deferred frame must return to the owner");
    assert_eq!(returned.correlation, correlation);
    assert_eq!(returned.request_id, LiveRendererWorkerRequestId(8));
    let WorkerOutcome::Deferred(frame) = returned.outcome else {
        panic!("Deferred outcome was lost");
    };
    assert_eq!(
        frame_correlation(&frame, Some(returned.request_id)),
        correlation
    );

    let detail = super::super::LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable;
    let failed = correlated_result(correlation, WorkerOutcome::Failed(detail));
    let returned = facade
        .retain_combined_result(failed)
        .expect("Failed outcome must return to the owner");
    assert_eq!(returned.correlation, correlation);
    assert!(matches!(returned.outcome, WorkerOutcome::Failed(actual) if actual == detail));
    assert!(matches!(
        facade.combined_release,
        CombinedReleaseCell::Reserved
    ));
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::NoRelease
    );
    assert!(commands.try_recv().is_err());
}

#[test]
fn full_retains_both_dedicated_and_ordinary_release_obligations() {
    let (mut facade, commands, results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let combined = acquire(&mut slots);
    let ordinary = acquire(&mut slots);
    facade.discarded_release = Some(DiscardedWorkerLease {
        output: facade.output,
        lease_id: LiveRendererWorkerLeaseId(44),
        slot_token: ordinary,
    });
    assert!(facade.reserve_combined_release());
    results.send(result(combined, 43)).unwrap();
    let raw = facade.result_receiver.try_recv().unwrap();
    assert!(facade.retain_combined_result(raw).is_none());
    assert!(
        facade.result_receiver.try_recv().is_err(),
        "raw result drained once, without poll"
    );
    for _ in 0..WORKER_COMMAND_CAPACITY {
        facade
            .core
            .command_sender
            .try_send(WorkerCommand::Shutdown)
            .unwrap();
    }
    let failures = facade.core.release_enqueue_failures.load(Ordering::Relaxed);
    for _ in 0..2 {
        assert_eq!(facade.flush_combined_release(), CombinedReleaseFlush::Full);
        assert_held(&facade, 43, combined);
    }
    assert_eq!(
        facade.core.release_enqueue_failures.load(Ordering::Relaxed),
        failures,
        "dedicated retries do not change ordinary failure counts"
    );
    assert_eq!(facade.metrics().combined_release_enqueue_failures, 2);
    facade.flush_discarded_release();
    assert_eq!(
        facade.core.release_enqueue_failures.load(Ordering::Relaxed),
        failures + 1
    );
    let held = facade
        .discarded_release
        .as_ref()
        .expect("ordinary Full retains its lease");
    assert_eq!(held.lease_id, LiveRendererWorkerLeaseId(44));
    assert_eq!(held.slot_token, ordinary);
    for _ in 0..WORKER_COMMAND_CAPACITY {
        assert!(matches!(
            commands.try_recv().unwrap(),
            WorkerCommand::Shutdown
        ));
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::Enqueued
    );
    assert_release(commands.try_recv().unwrap(), facade.output, 43, combined);
    facade.flush_discarded_release();
    assert_release(commands.try_recv().unwrap(), facade.output, 44, ordinary);
    assert!(facade.discarded_release.is_none());
    assert_eq!(
        slots.leased(),
        2,
        "enqueue and command receipt free no slot"
    );
}

#[test]
fn disconnected_keeps_the_obligation_and_does_not_authorize_retirement() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    assert!(facade.retain_combined_result(result(token, 45)).is_none());
    drop(commands);
    for _ in 0..2 {
        assert_eq!(
            facade.flush_combined_release(),
            CombinedReleaseFlush::Disconnected
        );
        assert_held(&facade, 45, token);
        assert!(!facade.reserve_combined_release());
    }
    assert_eq!(
        facade.core.release_enqueue_failures.load(Ordering::Relaxed),
        0
    );
    assert_eq!(facade.metrics().combined_release_enqueue_failures, 2);
    assert_eq!(slots.leased(), 1);
}

#[test]
fn enqueue_does_not_allow_reuse_of_an_occupied_slot() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    let _second = acquire(&mut slots);
    let _third = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    assert!(facade.retain_combined_result(result(token, 46)).is_none());
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::Enqueued
    );
    assert_eq!(
        slots.try_acquire(),
        LiveRendererFrameSlotAcquire::Deferred,
        "queued Release is not slot freedom"
    );
    let command = commands.try_recv().unwrap();
    let WorkerCommand::Release { slot_token, .. } = command else {
        panic!("expected Release command");
    };
    assert_eq!(slot_token, token);
    assert_eq!(
        slots.try_acquire(),
        LiveRendererFrameSlotAcquire::Deferred,
        "receiving Release is not processing it"
    );
    // This is a supplied pool release, not run_worker processing a GPU buffer.
    assert_eq!(
        slots.release(slot_token),
        LiveRendererFrameSlotRelease::Released
    );
    let reused = acquire(&mut slots);
    assert_eq!(reused.slot_id(), token.slot_id());
    assert_ne!(reused.incarnation(), token.incarnation());
    assert_eq!(slots.release(token), LiveRendererFrameSlotRelease::Stale);
}

#[test]
fn occupied_cell_refuses_a_second_result_without_overwriting_the_first() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let first = acquire(&mut slots);
    let second = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    assert!(facade.retain_combined_result(result(first, 47)).is_none());
    let returned = facade
        .retain_combined_result(result(second, 48))
        .expect("occupied cell returns the second result");
    assert!(
        matches!(returned.outcome, WorkerOutcome::Exported { lease_id, slot_token, .. }
        if lease_id == LiveRendererWorkerLeaseId(48) && slot_token == second)
    );
    assert_held(&facade, 47, first);
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::Enqueued
    );
    assert_release(commands.try_recv().unwrap(), facade.output, 47, first);
    assert!(commands.try_recv().is_err());
}

#[test]
fn admission_refuses_busy_quarantined_and_detached_facades() {
    let (mut facade, commands, _results) = correlated_facade();
    submit_mixed_job(&mut facade, &commands, 51);
    assert!(
        !facade.reserve_combined_release(),
        "ordinary request is in flight"
    );
    facade.in_flight = None;
    facade.quarantined = true;
    assert!(
        !facade.reserve_combined_release(),
        "quarantined facade refuses admission"
    );
    facade.quarantined = false;
    facade.stalled = Some(super::super::StalledRequest {
        since: std::time::Instant::now(),
    });
    assert!(!facade.reserve_combined_release());
    facade.stalled = None;
    facade.claim.detach();
    assert!(!facade.reserve_combined_release());
    assert!(matches!(facade.combined_release, CombinedReleaseCell::Free));
}

#[test]
fn poll_flushes_the_combined_cell_while_an_ordinary_render_is_out() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    assert!(facade.retain_combined_result(result(token, 52)).is_none());
    submit_mixed_job(&mut facade, &commands, 53);
    assert!(facade.in_flight.is_some());
    assert!(commands.try_recv().is_err(), "nothing queued before poll");
    assert!(!matches!(facade.poll(), super::super::WorkerPoll::Idle));
    let release = commands
        .try_recv()
        .expect("busy poll enqueues the combined Release");
    assert_release(release, facade.output, 52, token);
    assert!(matches!(facade.combined_release, CombinedReleaseCell::Free));
    assert!(facade.in_flight.is_some(), "the ordinary render stays out");
}

#[test]
fn idle_poll_flushes_the_combined_cell_and_production_never_fills_it() {
    let (mut facade, commands, _results) = correlated_facade();
    assert!(matches!(facade.poll(), super::super::WorkerPoll::Idle));
    assert!(commands.try_recv().is_err(), "a Free cell sends nothing");
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    assert!(facade.reserve_combined_release());
    assert!(facade.retain_combined_result(result(token, 54)).is_none());
    assert!(matches!(facade.poll(), super::super::WorkerPoll::Idle));
    let release = commands
        .try_recv()
        .expect("idle poll enqueues the combined Release");
    assert_release(release, facade.output, 54, token);
    assert!(commands.try_recv().is_err());
}
