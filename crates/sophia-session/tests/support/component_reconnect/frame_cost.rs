//! Real owned-ring and 9P traffic; only native completion is simulated.
use super::*;
use sophia_runtime::frame_cost::{self as cost, Role, Snapshot, Stage, measure};
#[path = "frame_cost/peer.rs"]
mod peer;
use peer::{PeerTurn, ThreadLink, peer_frames};
use std::sync::mpsc;
static ALONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const RESOURCE_IDS: u64 = 6;

fn descriptor() -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }
}

// A bounded resource ledger keeps long runs below the lifetime identity cap.
fn resource(frame: u64) -> ContentResourceId {
    assert!(frame > 0);
    ContentResourceId {
        id: (frame - 1) % RESOURCE_IDS + 1,
        generation: (frame - 1) / RESOURCE_IDS + 1,
    }
}

#[derive(Clone, Copy)]
struct TimerSettings {
    enabled: bool,
    period: u32,
    phase: u32,
}

impl TimerSettings {
    fn begin(self) -> u64 {
        let generation = cost::begin_sampled_block(
            std::num::NonZeroU32::new(self.period).expect("nonzero sampling period"),
            self.phase,
        );
        cost::enable(self.enabled);
        generation
    }
}

struct BlockResult {
    snapshots: Vec<Snapshot>,
    visits: u64,
    fill_wire_turns: u64,
    owner_cpu_nsec: u64,
    owner_elapsed_nsec: u64,
}

fn outer_clocks() -> (u64, u64) {
    use rustix::time::{ClockId, clock_gettime};
    let nsec = |clock| {
        let time = clock_gettime(clock);
        u64::try_from(time.tv_sec)
            .unwrap()
            .checked_mul(1_000_000_000)
            .unwrap()
            .checked_add(u64::try_from(time.tv_nsec).unwrap())
            .unwrap()
    };
    (nsec(ClockId::ThreadCPUTime), nsec(ClockId::Monotonic))
}

fn exercise(height: u32, warmup: u64, measured: u64, timers: TimerSettings) -> BlockResult {
    assert!(warmup.is_multiple_of(2) && measured.is_multiple_of(2));
    cost::enable(false);
    cost::begin_block();
    cost::drain();
    let frames = warmup + measured;
    let mut h = Harness::new();
    h.backend = SessionContentFixture::new(&[descriptor()]).unwrap();
    let (key, mut peer) = wire::connect_with_slots(&mut h.owner, 0, 2);
    h.owner
        .with_service(key, |service, transport| {
            wire::allocate_size(transport, &mut peer, 1920, 1080, height);
            let ShellComponentService::Bar(bar) = service else {
                panic!("bar required")
            };
            crate::live_session::metadata_shell::content::reconnect_fixture::prime_outputs(
                &mut bar.content,
                descriptor(),
            );
        })
        .unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (start_tx, start_rx) = mpsc::channel();
    let (pixels_tx, pixels_rx) = mpsc::channel();
    let mut link = ThreadLink {
        ready: ready_tx,
        start: start_rx,
        pixels: pixels_rx,
    };
    let worker =
        std::thread::spawn(move || peer_frames(peer, key.grant, height, frames, warmup, &mut link));
    let mut completed = 0;
    let mut visits = 0;
    let mut fill_wire_turns = 0;
    let mut generation = None;
    let mut outer_start = None;
    let scene = sophia_backend_live::LiveProductionCpuScene::new(descriptor().size);
    let output = descriptor();
    let root = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    let mut transaction_id = 100_000;
    let mut transaction = || {
        transaction_id += 1;
        Ok(TransactionId::from_raw(transaction_id))
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    while !worker.is_finished() {
        if let Ok(turn) = ready_rx.try_recv() {
            match turn {
                PeerTurn::Measure => {
                    assert!(generation.is_none());
                    assert_eq!(completed, warmup);
                    generation = Some(timers.begin());
                    outer_start = Some(outer_clocks());
                    start_tx.send(()).unwrap();
                }
                PeerTurn::Fill => {
                    start_tx.send(()).unwrap();
                    let filled_deadline = Instant::now() + Duration::from_secs(10);
                    loop {
                        match ready_rx.try_recv() {
                            Ok(PeerTurn::Filled) => break,
                            Ok(turn) => panic!("unexpected fill response: {turn:?}"),
                            Err(mpsc::TryRecvError::Disconnected) => {
                                panic!("peer disconnected during fill")
                            }
                            Err(mpsc::TryRecvError::Empty) => {}
                        }
                        // Pump 9P only: no worker reservation or release may reuse slot 0 yet.
                        h.owner
                            .with_service(key, |_, transport| {
                                transport.service_owner_turn().unwrap()
                            })
                            .unwrap();
                        if generation.is_some() {
                            fill_wire_turns += 1;
                        }
                        assert!(Instant::now() < filled_deadline, "paired fill timed out");
                        let mut fds = h.owner.poll_fds();
                        rustix::event::poll(
                            &mut fds,
                            Some(&rustix::event::Timespec {
                                tv_sec: 0,
                                tv_nsec: 1_000_000,
                            }),
                        )
                        .unwrap();
                    }
                    start_tx.send(()).unwrap();
                }
                PeerTurn::Filled => panic!("fill completion without begin"),
            }
        }
        if generation.is_some() {
            visits += 1;
        }
        assert!(
            h.owner
                .processes
                .turn_connected()
                .iter()
                .all(Option::is_none)
        );
        h.owner
            .with_service(key, |service, transport| {
                let ShellComponentService::Bar(bar) = service else {
                    panic!("bar required")
                };
                let inputs = measure(Stage::OwnerService, || {
                    bar.content.service_until_submission(
                        transport,
                        h.backend.runtime_mut(),
                        &scene,
                        None,
                        &[output],
                        &mut transaction,
                        None,
                    )
                })
                .unwrap()
                .expect("fixture grant remains live");
                while let Some(bundle) = bar
                    .content
                    .next_submission(transport, &inputs, None)
                    .unwrap()
                {
                    let generation = bundle.candidate_generation;
                    let pixels = bundle.resource(resource(generation)).unwrap().clone();
                    pixels_tx.send(pixels).unwrap();
                    bar.content
                        .submit_bundle_to(
                            transport,
                            &[output],
                            &[(output.id, root)],
                            root,
                            bundle,
                            sophia_backend_live::LiveShellContentLayer::Shell,
                            inputs.now,
                            |frame, layer| h.backend.admit(frame, layer),
                        )
                        .unwrap();
                    measure(Stage::FixtureTarget, || {
                        h.backend.simulate_submit(output.id);
                        h.backend.simulate_completion(output.id);
                    });
                    assert!(
                        bar.observe_presentation(transport, h.backend.runtime())
                            .unwrap()
                    );
                    completed += 1;
                }
                transport.service_content_resources(inputs.now).unwrap();
                transport.poll_io().unwrap();
            })
            .unwrap();
        assert!(Instant::now() < deadline, "frame driver timed out");
        // Bound fixture control-channel latency; worker and wire readiness wake sooner.
        let mut fds = h.owner.poll_fds();
        rustix::event::poll(
            &mut fds,
            Some(&rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000,
            }),
        )
        .unwrap();
    }
    let peer = worker.join().unwrap();
    assert_eq!(completed, frames);
    let (cpu_end, mono_end) = outer_clocks();
    let (cpu_start, mono_start) = outer_start.expect("outer measurement boundary was reached");
    let owner_cpu_nsec = cpu_end.checked_sub(cpu_start).expect("CPU clock regressed");
    let owner_elapsed_nsec = mono_end
        .checked_sub(mono_start)
        .expect("monotonic clock regressed");
    assert!(visits >= measured);
    let owner = cost::snapshot_thread();
    assert_eq!(owner.is_some(), cfg!(feature = "diagnostic-frame-cost"));
    let generation = generation.expect("measurement boundary was reached");
    cost::enable(false);
    cost::begin_block();
    h.backend.teardown();
    drop(peer);
    h.owner.stop(key).unwrap();
    drop(h);
    let Some(owner) = owner else {
        assert!(cost::drain().is_empty());
        return BlockResult {
            snapshots: Vec::new(),
            visits,
            fill_wire_turns,
            owner_cpu_nsec,
            owner_elapsed_nsec,
        };
    };
    assert_eq!(owner.generation, generation);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut snapshots = vec![owner];
    loop {
        snapshots.extend(
            cost::drain().into_iter().filter(|snapshot| {
                snapshot.generation == generation && snapshot.role != Role::Owner
            }),
        );
        if snapshots
            .iter()
            .any(|snapshot| snapshot.role == Role::Worker)
        {
            break;
        }
        assert!(Instant::now() < deadline, "worker accounting did not close");
        std::thread::yield_now();
    }
    assert_eq!(
        snapshots.iter().filter(|s| s.role == Role::Owner).count(),
        1
    );
    assert_eq!(snapshots.iter().filter(|s| s.role == Role::Peer).count(), 1);
    let peer = snapshots.iter().find(|s| s.role == Role::Peer).unwrap();
    assert_eq!((peer.enabled_calls, peer.disabled_calls), (0, 0));
    assert!(snapshots.iter().all(|s| !s.invalid));
    assert!(
        snapshots
            .iter()
            .all(|s| s.stages.iter().all(|stage| stage.aborted == 0))
    );
    let worker = snapshots.iter().find(|s| s.role == Role::Worker).unwrap();
    assert_eq!(
        snapshots.iter().filter(|s| s.role == Role::Worker).count(),
        1
    );
    for snapshot in [&snapshots[0], worker] {
        assert_eq!(
            (snapshot.sample_period, snapshot.sample_phase),
            (timers.period, timers.phase)
        );
        let entries: u64 = snapshot.stages.iter().map(|stage| stage.entries).sum();
        assert!(entries > 0);
        assert_eq!(
            snapshot.enabled_calls,
            if timers.enabled { entries } else { 0 }
        );
        assert_eq!(
            snapshot.disabled_calls,
            if timers.enabled { 0 } else { entries }
        );
        for stage in snapshot.stages {
            let samples = if timers.enabled && stage.entries > u64::from(timers.phase) {
                1 + (stage.entries - 1 - u64::from(timers.phase)) / u64::from(timers.period)
            } else {
                0
            };
            assert_eq!(stage.calls, samples);
            if samples == 0 {
                assert_eq!(
                    (
                        stage.cpu_nsec,
                        stage.elapsed_nsec,
                        stage.max_cpu_nsec,
                        stage.max_elapsed_nsec
                    ),
                    (0, 0, 0, 0)
                );
            }
        }
    }
    for stage in [Stage::Validation, Stage::Copy, Stage::Pixels] {
        assert_eq!(
            worker.stages[stage as usize].entries,
            measured,
            "{} jobs",
            stage.name()
        );
    }
    let entries = |stage: Stage| snapshots[0].stages[stage as usize].entries;
    assert_eq!(entries(Stage::Projection), measured * 2);
    assert_eq!(entries(Stage::Logging), measured * 2);
    assert_eq!(entries(Stage::SourceRelease), measured);
    assert!(entries(Stage::Reserve) >= measured);
    assert_eq!(entries(Stage::Reserve), entries(Stage::Authority));
    assert!(entries(Stage::Collect) >= measured);
    assert_eq!(entries(Stage::Completion), visits + measured);
    assert!(entries(Stage::Transport) >= visits);
    assert_eq!(entries(Stage::Submission), measured);
    assert_eq!(entries(Stage::OwnerService), visits);
    assert_eq!(entries(Stage::FixtureTarget), measured);
    assert_eq!(
        snapshots[0].events.iter().sum::<u64>(),
        entries(Stage::JournalRead)
    );
    assert_eq!(snapshots[0].events[cost::Event::JournalRefused as usize], 0);
    assert!(worker.events.iter().all(|count| *count == 0));
    assert!(peer.events.iter().all(|count| *count == 0));
    assert!(visits >= measured);
    BlockResult {
        snapshots,
        visits,
        fill_wire_turns,
        owner_cpu_nsec,
        owner_elapsed_nsec,
    }
}

#[test]
#[ignore = "process-global timers; run alone with devices hidden"]
fn frame_attribution_owned_ring_smoke() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    crate::output::install(crate::output::SessionOutput::new(|_| {}, |_| {})).unwrap();
    exercise(
        24,
        6,
        12,
        TimerSettings {
            enabled: true,
            period: 256,
            phase: 193,
        },
    );
    for (height, enabled, phase) in [(24, true, 0), (512, false, 17), (24, true, 33)] {
        exercise(
            height,
            6,
            12,
            TimerSettings {
                enabled,
                period: 64,
                phase,
            },
        );
    }
}

#[test]
#[ignore = "counter versus compile-time stub reference; run exact release binaries in ABBA order"]
fn frame_attribution_counter_reference() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    crate::output::install(crate::output::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    for height in [24, 512] {
        for block in 0..4 {
            let BlockResult {
                snapshots,
                visits,
                fill_wire_turns,
                owner_cpu_nsec,
                owner_elapsed_nsec,
            } = exercise(
                height,
                120,
                8000,
                TimerSettings {
                    enabled: false,
                    period: 256,
                    phase: 0,
                },
            );
            println!(
                "sophia_frame_reference schema=1 height={height} block={block} counters={} timed=0 warmup=120 completed=8000 visits={visits} fill_wire_turns={fill_wire_turns} ring_slots=2 alternating_slots=1 resource_ids={RESOURCE_IDS} owner_cpu_nsec={owner_cpu_nsec} owner_elapsed_nsec={owner_elapsed_nsec}",
                u8::from(cfg!(feature = "diagnostic-frame-cost")),
            );
            for snapshot in snapshots {
                for record in snapshot.records() {
                    println!("{record}");
                }
            }
        }
    }
}

#[cfg(feature = "diagnostic-frame-cost")]
fn timer_controls(period: u32) {
    for (block, enabled) in [false, true, true, false].into_iter().enumerate() {
        let generation = TimerSettings {
            enabled,
            period,
            phase: 0,
        }
        .begin();
        for _ in 0..2000 {
            measure(Stage::OwnerService, || std::hint::black_box(()));
        }
        let snapshot = cost::snapshot_thread().unwrap();
        assert!(!snapshot.invalid);
        assert_eq!(snapshot.enabled_calls, if enabled { 2000 } else { 0 });
        assert_eq!(snapshot.disabled_calls, if enabled { 0 } else { 2000 });
        println!(
            "sophia_frame_timer schema=2 block={block} enabled={} iterations=2000 generation={generation} sample_period={period} sample_phase=0",
            u8::from(enabled)
        );
        for record in snapshot.records() {
            println!("{record}");
        }
    }
    cost::enable(false);
    cost::begin_block();
    cost::drain();
}

#[test]
#[cfg(feature = "diagnostic-frame-cost")]
#[ignore = "release attribution; run alone with devices hidden and stdout saved"]
fn frame_attribution_measure() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    timer_controls(1);
    crate::output::install(crate::output::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    for height in [24, 512] {
        for (block, enabled) in [false, true, true, false].into_iter().enumerate() {
            let BlockResult {
                snapshots,
                visits,
                fill_wire_turns,
                ..
            } = exercise(
                height,
                120,
                2000,
                TimerSettings {
                    enabled,
                    period: 1,
                    phase: 0,
                },
            );
            println!(
                "sophia_frame_block schema=2 height={height} block={block} enabled={} warmup=120 completed=2000 visits={visits} fill_wire_turns={fill_wire_turns} ring_slots=2 alternating_slots=1 resource_ids={RESOURCE_IDS} sample_period=1 sample_phase=0 generation={}",
                u8::from(enabled),
                snapshots[0].generation
            );
            for snapshot in snapshots {
                for record in snapshot.records() {
                    println!("{record}");
                }
            }
        }
    }
}

#[test]
#[cfg(feature = "diagnostic-frame-cost")]
#[ignore = "sparse release attribution; run alone with devices hidden and stdout saved"]
fn frame_attribution_sparse_measure() {
    sparse_measure(64, 2000, [0, 17, 33, 49]);
}

#[test]
#[cfg(feature = "diagnostic-frame-cost")]
#[ignore = "sparse long-block attribution; run alone with devices hidden and stdout saved"]
fn frame_attribution_sparse_long_measure() {
    sparse_measure(256, 8000, [0, 65, 129, 193]);
}

#[cfg(feature = "diagnostic-frame-cost")]
fn sparse_measure(period: u32, measured: u64, phases: [u32; 4]) {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    timer_controls(period);
    crate::output::install(crate::output::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    for height in [24, 512] {
        for (block, (enabled, phase)) in [
            (false, phases[0]),
            (true, phases[0]),
            (true, phases[1]),
            (false, phases[1]),
            (false, phases[2]),
            (true, phases[2]),
            (true, phases[3]),
            (false, phases[3]),
        ]
        .into_iter()
        .enumerate()
        {
            let timers = TimerSettings {
                enabled,
                period,
                phase,
            };
            let BlockResult {
                snapshots,
                visits,
                fill_wire_turns,
                ..
            } = exercise(height, 120, measured, timers);
            println!(
                "sophia_frame_block schema=2 height={height} block={block} enabled={} warmup=120 completed={measured} visits={visits} fill_wire_turns={fill_wire_turns} ring_slots=2 alternating_slots=1 resource_ids={RESOURCE_IDS} sample_period={period} sample_phase={phase} generation={}",
                u8::from(enabled),
                snapshots[0].generation
            );
            for snapshot in snapshots {
                for record in snapshot.records() {
                    println!("{record}");
                }
            }
        }
    }
}
