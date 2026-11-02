//! Clock-backed checks of the opt-in frame-cost timers; no device or transport claim.
use std::sync::Mutex;

use crate::frame_cost::state::{Fault, accumulate, bump};
use crate::frame_cost::{
    Reason, Role, Stage, begin_block, drain, enable, measure, register_thread, snapshot_thread,
};

// Generation, flag and queue are process-wide; run these serially.
static SERIAL: Mutex<()> = Mutex::new(());

fn spin() -> u64 {
    (0..200_000u64).fold(0, |acc, i| acc.wrapping_mul(31).wrapping_add(i))
}

fn fresh_block() -> u64 {
    enable(false);
    let generation = begin_block();
    drain();
    generation
}

/// This thread's tid; other test threads exit concurrently and publish Drop
/// snapshots, so drained results are matched by tid or by registered role.
fn my_tid() -> u32 {
    snapshot_thread().expect("thread state").tid
}

fn drain_tid(tid: u32) -> Vec<crate::frame_cost::Snapshot> {
    drain().into_iter().filter(|s| s.tid == tid).collect()
}

#[test]
fn disabled_timers_count_entries_and_keep_the_outer_window() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_block();
    assert_eq!(measure(Stage::Admission, spin), spin());
    let snapshot = snapshot_thread().expect("thread state");
    assert_eq!(snapshot.role, Role::Owner);
    assert_eq!(snapshot.reason, Reason::Explicit);
    assert_eq!((snapshot.disabled_calls, snapshot.enabled_calls), (1, 0));
    assert_eq!(snapshot.stages[Stage::Admission as usize].calls, 0);
    assert!(snapshot.cpu_now_nsec >= snapshot.cpu_first_nsec);
    assert!(snapshot.mono_now_nsec >= snapshot.mono_first_nsec);
    assert!(!snapshot.invalid);
}

#[test]
fn enabled_timers_accumulate_nested_inclusive_scopes_and_record_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_block();
    enable(true);
    measure(Stage::Transport, || measure(Stage::Pixels, spin));
    measure(Stage::Pixels, spin);
    enable(false);
    let snapshot = snapshot_thread().expect("thread state");
    let transport = snapshot.stages[Stage::Transport as usize];
    let pixels = snapshot.stages[Stage::Pixels as usize];
    assert_eq!(snapshot.enabled_calls, 3);
    assert_eq!((transport.calls, pixels.calls), (1, 2));
    assert!(transport.elapsed_nsec > 0 && transport.cpu_nsec <= transport.elapsed_nsec + 1_000_000);
    assert!(pixels.max_elapsed_nsec <= pixels.elapsed_nsec);
    assert!(transport.max_cpu_nsec <= transport.cpu_nsec);
    assert!(!snapshot.invalid);
    let lines = snapshot.records();
    assert_eq!(
        lines.len(),
        Stage::COUNT + crate::frame_cost::Event::COUNT + 1
    );
    assert!(lines[0].starts_with("sophia_frame_cost schema=2 role=owner tid="));
    assert!(lines[0].contains(" kind=summary reason=explicit "));
    assert!(lines[0].ends_with(" invalid=0 sample_period=1 sample_phase=0"));
    assert!(
        lines[1 + Stage::Pixels as usize]
            .contains(" kind=stage stage=pixels entries=2 calls=2 aborted=0 ")
    );
    assert!(lines.iter().all(|l| l.len() < 512));
}

#[test]
fn begin_block_publishes_the_previous_generation_and_resets() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let first = fresh_block();
    let tid = my_tid();
    enable(true);
    measure(Stage::Copy, spin);
    enable(false);
    let second = begin_block();
    assert_eq!(second, first + 1);
    let published = drain_tid(tid);
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].reason, Reason::Block);
    assert_eq!(published[0].generation, first);
    assert_eq!(published[0].stages[Stage::Copy as usize].calls, 1);
    assert!(!published[0].invalid);
    let fresh = snapshot_thread().expect("thread state");
    assert_eq!(fresh.generation, second);
    assert_eq!(fresh.enabled_calls, 0);
    assert_eq!(fresh.stages[Stage::Copy as usize].calls, 0);
}

#[test]
fn unwinding_scope_is_recorded_as_aborted() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_block();
    enable(true);
    let outcome = std::panic::catch_unwind(|| {
        measure(Stage::Validation, || {
            spin();
            panic!("validation unwinds");
        })
    });
    enable(false);
    assert!(outcome.is_err());
    let cost = snapshot_thread().expect("thread state").stages[Stage::Validation as usize];
    assert_eq!((cost.calls, cost.aborted), (1, 1));
    assert!(cost.elapsed_nsec > 0);
}

#[test]
fn generation_change_inside_an_open_scope_voids_both_generations() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let first = fresh_block();
    let tid = my_tid();
    enable(true);
    measure(Stage::OwnerService, || {
        begin_block();
    });
    enable(false);
    let published = drain_tid(tid);
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].generation, first);
    assert!(published[0].invalid);
    let current = snapshot_thread().expect("thread state");
    assert_eq!(current.generation, first + 1);
    assert!(current.invalid);
    assert_eq!(current.stages[Stage::OwnerService as usize].calls, 1);
    assert_eq!(
        current.stages[Stage::OwnerService as usize].cpu_nsec,
        0,
        "clocks are not merged"
    );
}

#[test]
fn worker_and_peer_threads_publish_on_generation_change_and_exit() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let first = fresh_block();
    enable(true);
    let worker = std::thread::spawn(move || {
        register_thread(Role::Worker);
        measure(Stage::Validation, spin);
        begin_block();
        measure(Stage::Validation, spin);
    });
    worker.join().expect("worker");
    let peer = std::thread::spawn(|| {
        register_thread(Role::Peer);
        snapshot_thread().expect("peer state")
    });
    let peer = peer.join().expect("peer");
    enable(false);
    let published = drain();
    let workers: Vec<_> = published
        .iter()
        .filter(|s| s.role == Role::Worker)
        .collect();
    assert_eq!(workers.len(), 2, "one roll plus one drop");
    assert_eq!(
        (workers[0].reason, workers[0].generation),
        (Reason::Block, first)
    );
    assert_eq!(
        (workers[1].reason, workers[1].generation),
        (Reason::Drop, first + 1)
    );
    assert!(
        workers
            .iter()
            .all(|s| s.stages[Stage::Validation as usize].calls == 1)
    );
    assert!(workers.iter().all(|s| s.tid != 0 && !s.invalid));
    assert_eq!(peer.role, Role::Peer);
    assert_eq!(peer.enabled_calls, 0);
    assert!(
        published
            .iter()
            .any(|s| s.role == Role::Peer && s.reason == Reason::Drop)
    );
    assert!(workers[0].tid != peer.tid && workers[0].thread_sequence != peer.thread_sequence);
}

#[test]
fn generation_change_elsewhere_while_a_scope_is_open_voids_it_at_close() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let first = fresh_block();
    let tid = my_tid();
    enable(true);
    measure(Stage::Admission, || {
        spin();
        std::thread::spawn(begin_block)
            .join()
            .expect("other thread");
    });
    enable(false);
    assert!(drain_tid(tid).is_empty(), "owner has not rolled yet");
    // The peek observes the new generation and rolls the voided one out.
    let current = snapshot_thread().expect("thread state");
    let voided = drain_tid(tid);
    assert_eq!(voided.len(), 1);
    assert_eq!(
        (voided[0].generation, voided[0].reason),
        (first, Reason::Block)
    );
    assert!(voided[0].invalid);
    assert_eq!(voided[0].stages[Stage::Admission as usize].calls, 1);
    assert_eq!(voided[0].stages[Stage::Admission as usize].cpu_nsec, 0);
    assert_eq!(current.generation, first + 1);
    assert!(
        !current.invalid,
        "the scope closed before this thread rolled"
    );
    assert_eq!(current.stages[Stage::Admission as usize].calls, 0);
}

#[test]
fn thread_exit_reports_under_the_generation_it_last_observed() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let first = fresh_block();
    enable(true);
    let (done, finished) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        register_thread(Role::Worker);
        measure(Stage::Copy, spin);
        done.send(()).expect("owner waits");
        released.recv().expect("owner releases");
    });
    finished.recv().expect("worker finished its job");
    let second = begin_block();
    release.send(()).expect("worker waits");
    worker.join().expect("worker");
    enable(false);
    let published = drain();
    let exit = published
        .iter()
        .find(|s| s.role == Role::Worker && s.reason == Reason::Drop)
        .expect("worker exit snapshot");
    assert_eq!(exit.generation, first);
    assert_ne!(exit.generation, second);
    assert_eq!(exit.stages[Stage::Copy as usize].calls, 1);
    assert!(!exit.invalid);
    assert_eq!(
        published.iter().filter(|s| s.role == Role::Worker).count(),
        1
    );
}

#[test]
fn bump_saturates_and_voids_the_block() {
    let (mut counter, mut invalid) = (u64::MAX - 1, false);
    bump(&mut counter, &mut invalid);
    assert_eq!((counter, invalid), (u64::MAX, false));
    bump(&mut counter, &mut invalid);
    assert_eq!((counter, invalid), (u64::MAX, true));
}

#[test]
fn accumulate_reports_regression_and_saturation() {
    let (mut total, mut max) = (0, 0);
    assert_eq!(accumulate(10, 25, &mut total, &mut max), None);
    assert_eq!((total, max), (15, 15));
    assert_eq!(
        accumulate(30, 20, &mut total, &mut max),
        Some(Fault::Regression)
    );
    assert_eq!((total, max), (15, 15));
    total = u64::MAX - 1;
    assert_eq!(
        accumulate(0, 5, &mut total, &mut max),
        Some(Fault::Saturation)
    );
    assert_eq!(total, u64::MAX);
}

#[test]
fn sparse_timing_counts_every_entry_without_scaling_sample_clocks() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    enable(false);
    crate::frame_cost::begin_sampled_block(std::num::NonZeroU32::new(8).unwrap(), 3);
    for _ in 0..16 {
        measure(Stage::Transport, || ());
    }
    let disabled = snapshot_thread().unwrap();
    assert_eq!(disabled.stages[Stage::Transport as usize].entries, 16);
    assert_eq!(disabled.stages[Stage::Transport as usize].calls, 0);
    enable(true);
    for value in 0..128 {
        assert_eq!(measure(Stage::Transport, || value + 1), value + 1);
    }
    enable(false);
    let snapshot = snapshot_thread().unwrap();
    let stage = snapshot.stages[Stage::Transport as usize];
    assert_eq!((snapshot.sample_period, snapshot.sample_phase), (8, 3));
    assert_eq!((snapshot.enabled_calls, snapshot.disabled_calls), (128, 16));
    assert_eq!((stage.entries, stage.calls), (144, 16));
    assert!(stage.cpu_nsec <= snapshot.cpu_now_nsec - snapshot.cpu_first_nsec);
    assert!(!snapshot.invalid);
}

#[test]
fn sampling_phases_select_disjoint_stage_ordinals() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for period in [1, 3, 8, 17, 64, 256] {
        let mut selected = std::collections::BTreeSet::new();
        for phase in 0..period {
            crate::frame_cost::begin_sampled_block(
                std::num::NonZeroU32::new(period).unwrap(),
                phase,
            );
            enable(true);
            let mut previous = 0;
            for ordinal in 0..2 * period {
                measure(Stage::Copy, || ());
                let snapshot = snapshot_thread().unwrap();
                let calls = snapshot.stages[Stage::Copy as usize].calls;
                if calls != previous {
                    assert_eq!(ordinal % period, phase);
                    assert!(selected.insert(ordinal));
                }
                previous = calls;
            }
            assert_eq!(previous, 2);
        }
        assert_eq!(selected, (0..2 * period).collect());
    }
    enable(false);
}

#[test]
fn worker_latches_sampling_on_roll_and_keeps_it_through_exit() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_block();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        register_thread(Role::Worker);
        let tid = my_tid();
        done_tx.send(tid).unwrap();
        next_rx.recv().unwrap();
        for _ in 0..20 {
            measure(Stage::Copy, || ());
        }
        done_tx.send(tid).unwrap();
        next_rx.recv().unwrap();
    });
    let tid = done_rx.recv().unwrap();
    let generation =
        crate::frame_cost::begin_sampled_block(std::num::NonZeroU32::new(8).unwrap(), 3);
    enable(true);
    next_tx.send(()).unwrap();
    done_rx.recv().unwrap();
    enable(false);
    begin_block();
    next_tx.send(()).unwrap();
    worker.join().unwrap();
    let snapshots = drain_tid(tid);
    let measured = snapshots
        .iter()
        .find(|s| s.generation == generation)
        .unwrap();
    assert_eq!(measured.reason, Reason::Drop);
    assert_eq!((measured.sample_period, measured.sample_phase), (8, 3));
    assert_eq!(measured.enabled_calls, 20);
    assert_eq!(measured.stages[Stage::Copy as usize].entries, 20);
    assert_eq!(measured.stages[Stage::Copy as usize].calls, 3);
    assert!(!measured.invalid);
    assert_eq!(
        snapshots
            .iter()
            .filter(|s| s.generation == generation)
            .count(),
        1
    );
}

#[test]
fn outcome_counts_are_independent_of_timer_mode_and_reset_with_generation() {
    use crate::frame_cost::{Event, count};
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    fresh_block();
    count(Event::JournalPending);
    count(Event::WireFallback);
    enable(true);
    count(Event::JournalReady);
    count(Event::JournalReady);
    enable(false);
    count(Event::JournalRefused);
    count(Event::WireObserved);
    let snapshot = snapshot_thread().unwrap();
    assert_eq!(snapshot.events, [2, 1, 1, 1, 1]);
    assert_eq!((snapshot.enabled_calls, snapshot.disabled_calls), (0, 0));
    assert!(
        snapshot
            .stages
            .iter()
            .all(|stage| stage.entries == 0 && stage.calls == 0)
    );
    assert!(!snapshot.invalid);
    assert!(
        snapshot
            .records()
            .iter()
            .any(|line| line.ends_with("kind=count event=journal_ready count=2"))
    );
    begin_block();
    assert_eq!(snapshot_thread().unwrap().events, [0; Event::COUNT]);
}
