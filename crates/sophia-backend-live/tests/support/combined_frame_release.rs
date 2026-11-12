//! The combined frame-job Release cell on the production facade (stage 4b),
//! with the 4c-4 Held state, admission ticket and Release receipt.

use super::super::{
    CombinedRelease, CombinedReleaseCell, CombinedReleaseFlush, DiscardedWorkerLease,
    LiveRendererFrameSlotPool, LiveRendererFrameSlotToken, LiveRendererWorkerLeaseId,
    LiveRendererWorkerOutputKey, LiveRendererWorkerRequestId, WORKER_COMMAND_CAPACITY,
    WorkerCommand, WorkerOutcome, WorkerResult, frame_correlation,
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
    job: u64,
    lease: u64,
    token: LiveRendererFrameSlotToken,
) {
    let CombinedReleaseCell::Retained {
        job: held,
        lease: release,
    } = &facade.combined_release
    else {
        panic!("Release obligation must remain in its dedicated cell");
    };
    assert_eq!(*held, job);
    assert_eq!(release.output, facade.output);
    assert_eq!(release.lease_id, LiveRendererWorkerLeaseId(lease));
    assert_eq!(release.slot_token, token);
}

/// Reserves and holds one exported result as the owner row will.
fn hold(facade: &mut super::super::NativeGbmRendererWorker, job: u64, result: WorkerResult) {
    let ticket = facade
        .reserve_combined_release()
        .expect("a free facade reserves");
    assert!(facade.hold_combined_result(ticket, job, result).is_none());
}

/// Fills the command queue so the next Release enqueue sees `Full`.
fn fill(facade: &super::super::NativeGbmRendererWorker) {
    for _ in 0..WORKER_COMMAND_CAPACITY {
        facade
            .core
            .command_sender
            .try_send(WorkerCommand::Shutdown)
            .unwrap();
    }
}

fn drain(commands: &std::sync::mpsc::Receiver<WorkerCommand>) {
    for _ in 0..WORKER_COMMAND_CAPACITY {
        assert!(matches!(
            commands.try_recv().unwrap(),
            WorkerCommand::Shutdown
        ));
    }
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
fn one_cell_reserves_holds_and_reopens_only_after_the_receipt_is_read() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    let original = facade
        .hold_combined_result(1, 9, result(token, 41))
        .expect("an unreserved result is returned");
    let ticket = facade.reserve_combined_release().expect("Free reserves");
    assert!(
        facade.reserve_combined_release().is_none(),
        "one combined reservation per facade"
    );
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::NoRelease
    );
    assert!(facade.hold_combined_result(ticket, 9, original).is_none());
    assert!(
        matches!(
            facade.combined_release,
            CombinedReleaseCell::Held { job: 9, .. }
        ),
        "an exported result is held, not released"
    );
    assert_eq!(
        facade.flush_combined_release(),
        CombinedReleaseFlush::NoRelease,
        "a held result is not a Release"
    );
    assert!(commands.try_recv().is_err());
    assert!(facade.reserve_combined_release().is_none());
    assert_eq!(facade.release_combined_held(8), CombinedRelease::Mismatch);
    assert!(
        matches!(
            facade.combined_release,
            CombinedReleaseCell::Held { job: 9, .. }
        ),
        "another job cannot give this result up"
    );
    assert_eq!(
        facade.release_combined_held(9),
        CombinedRelease::Transferred
    );
    assert_release(commands.try_recv().unwrap(), facade.output, 41, token);
    assert_eq!(
        facade.flush_combined_release_for(9),
        CombinedRelease::Mismatch,
        "the receipt is read once"
    );
    assert!(commands.try_recv().is_err(), "no duplicate Release");
    assert!(facade.reserve_combined_release().is_some());
}

#[test]
fn foreign_stale_and_non_exported_results_return_to_the_owner_untouched() {
    let (mut facade, commands, _results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    let ticket = facade.reserve_combined_release().unwrap();
    let mut foreign = result(token, 42);
    foreign.output = LiveRendererWorkerOutputKey::from_raw(999);
    let returned = facade
        .hold_combined_result(ticket, 1, foreign)
        .expect("foreign output must be returned");
    assert_eq!(returned.output, LiveRendererWorkerOutputKey::from_raw(999));
    assert!(
        matches!(returned.outcome, WorkerOutcome::Exported { lease_id, slot_token, .. }
        if lease_id == LiveRendererWorkerLeaseId(42) && slot_token == token)
    );
    let stale = facade
        .hold_combined_result(ticket.wrapping_add(1), 1, result(token, 43))
        .expect("another admission's ticket must be returned");
    assert!(
        matches!(stale.outcome, WorkerOutcome::Exported { lease_id, .. }
        if lease_id == LiveRendererWorkerLeaseId(43))
    );

    let frame = mixed_job(99);
    let correlation = frame_correlation(&frame, Some(LiveRendererWorkerRequestId(8)));
    let deferred = correlated_result(correlation, WorkerOutcome::Deferred(frame));
    let returned = facade
        .hold_combined_result(ticket, 1, deferred)
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
        .hold_combined_result(ticket, 1, failed)
        .expect("Failed outcome must return to the owner");
    assert_eq!(returned.correlation, correlation);
    assert!(matches!(returned.outcome, WorkerOutcome::Failed(actual) if actual == detail));
    assert!(matches!(
        facade.combined_release,
        CombinedReleaseCell::Reserved(reserved) if reserved == ticket
    ));
    assert!(
        !facade.cancel_combined_release(ticket.wrapping_add(1)),
        "a stale ticket cannot free this admission"
    );
    assert!(facade.cancel_combined_release(ticket));
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
    results.send(result(combined, 43)).unwrap();
    let raw = facade.result_receiver.try_recv().unwrap();
    hold(&mut facade, 3, raw);
    assert!(
        facade.result_receiver.try_recv().is_err(),
        "raw result drained once, without poll"
    );
    fill(&facade);
    let failures = facade.core.release_enqueue_failures.load(Ordering::Relaxed);
    assert_eq!(facade.release_combined_held(3), CombinedRelease::Pending);
    assert_held(&facade, 3, 43, combined);
    assert_eq!(
        facade.flush_combined_release_for(3),
        CombinedRelease::Pending
    );
    assert_held(&facade, 3, 43, combined);
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
    drain(&commands);
    assert!(commands.try_recv().is_err());
    assert_eq!(
        facade.flush_combined_release_for(3),
        CombinedRelease::Transferred
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
    hold(&mut facade, 5, result(token, 45));
    drop(commands);
    assert_eq!(
        facade.release_combined_held(5),
        CombinedRelease::Disconnected
    );
    assert_held(&facade, 5, 45, token);
    assert_eq!(
        facade.flush_combined_release_for(5),
        CombinedRelease::Disconnected
    );
    assert_held(&facade, 5, 45, token);
    assert!(facade.reserve_combined_release().is_none());
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
    hold(&mut facade, 6, result(token, 46));
    assert_eq!(
        facade.release_combined_held(6),
        CombinedRelease::Transferred
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
    let ticket = facade.reserve_combined_release().unwrap();
    assert!(
        facade
            .hold_combined_result(ticket, 7, result(first, 47))
            .is_none()
    );
    let returned = facade
        .hold_combined_result(ticket, 7, result(second, 48))
        .expect("occupied cell returns the second result");
    assert!(
        matches!(returned.outcome, WorkerOutcome::Exported { lease_id, slot_token, .. }
        if lease_id == LiveRendererWorkerLeaseId(48) && slot_token == second)
    );
    assert!(
        !facade.cancel_combined_release(ticket),
        "a held result is not an unused reservation"
    );
    assert_eq!(
        facade.release_combined_held(7),
        CombinedRelease::Transferred
    );
    assert_release(commands.try_recv().unwrap(), facade.output, 47, first);
    assert!(commands.try_recv().is_err());
}

#[test]
fn admission_refuses_busy_quarantined_and_detached_facades() {
    let (mut facade, commands, _results) = correlated_facade();
    submit_mixed_job(&mut facade, &commands, 51);
    assert!(
        facade.reserve_combined_release().is_none(),
        "ordinary request is in flight"
    );
    facade.in_flight = None;
    facade.quarantined = true;
    assert!(
        facade.reserve_combined_release().is_none(),
        "quarantined facade refuses admission"
    );
    facade.quarantined = false;
    facade.stalled = Some(super::super::StalledRequest {
        since: std::time::Instant::now(),
    });
    assert!(facade.reserve_combined_release().is_none());
    facade.stalled = None;
    facade.claim.detach();
    assert!(facade.reserve_combined_release().is_none());
    assert!(matches!(facade.combined_release, CombinedReleaseCell::Free));
}

/// AUTHORITY-STEP3.md revision 5: an ordinary poll enqueues the Release that
/// maintenance found Full, and maintenance then reads that receipt.
#[test]
fn poll_enqueues_a_full_release_and_maintenance_reads_its_receipt() {
    let (mut facade, commands, results) = correlated_facade();
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    hold(&mut facade, 52, result(token, 52));
    fill(&facade);
    assert_eq!(facade.release_combined_held(52), CombinedRelease::Pending);
    drain(&commands);
    let (correlation, _) = submit_mixed_job(&mut facade, &commands, 53);
    let mut ordinary = correlated_result(correlation, exported_outcome());
    ordinary.output = facade.output;
    results.send(ordinary).unwrap();
    let delivered = facade.poll();
    let release = commands
        .try_recv()
        .expect("the ordinary poll enqueues the combined Release");
    assert_release(release, facade.output, 52, token);
    assert!(
        matches!(delivered, super::super::WorkerPoll::Exported(_)),
        "the ordinary result is delivered normally"
    );
    assert!(facade.in_flight.is_none(), "poll took its own result only");
    assert!(
        facade.reserve_combined_release().is_none(),
        "no successor before the receipt is read"
    );
    assert_eq!(
        facade.flush_combined_release_for(51),
        CombinedRelease::Mismatch,
        "another job's receipt is not this one's"
    );
    assert_eq!(
        facade.flush_combined_release_for(52),
        CombinedRelease::Transferred
    );
    assert!(
        !commands
            .try_iter()
            .any(|command| matches!(command, WorkerCommand::Release { .. })),
        "exactly one Release"
    );
    assert!(facade.reserve_combined_release().is_some());
}

#[test]
fn idle_poll_flushes_the_combined_cell_and_production_never_fills_it() {
    let (mut facade, commands, _results) = correlated_facade();
    assert!(matches!(facade.poll(), super::super::WorkerPoll::Idle));
    assert!(commands.try_recv().is_err(), "a Free cell sends nothing");
    let mut slots = LiveRendererFrameSlotPool::new();
    let token = acquire(&mut slots);
    hold(&mut facade, 54, result(token, 54));
    assert!(matches!(facade.poll(), super::super::WorkerPoll::Idle));
    assert!(
        commands.try_recv().is_err(),
        "poll never gives up a held result"
    );
    fill(&facade);
    assert_eq!(facade.release_combined_held(54), CombinedRelease::Pending);
    drain(&commands);
    assert!(matches!(facade.poll(), super::super::WorkerPoll::Idle));
    let release = commands
        .try_recv()
        .expect("idle poll enqueues the combined Release");
    assert_release(release, facade.output, 54, token);
    assert!(commands.try_recv().is_err());
    assert_eq!(
        facade.flush_combined_release_for(54),
        CombinedRelease::Transferred
    );
}
