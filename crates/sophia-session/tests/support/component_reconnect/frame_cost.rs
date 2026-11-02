//! Real owned-ring and 9P traffic; only native completion is simulated.
use super::*;
use sophia_runtime::frame_cost::{self as cost, Role, Snapshot, Stage, measure};
use sophia_shell_client::{ImageAdapterError, RingOfferState, RingUpload};
use std::sync::mpsc;
static ALONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

fn next(peer: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some((_, record)) = peer.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "peer observation timed out");
        std::thread::yield_now();
    }
}

#[derive(Debug)]
enum PeerTurn {
    Measure,
    Fill,
    Filled,
}

fn upload_pair(
    peer: &mut ShellConnection,
    first: u64,
    height: u32,
    ready: &mpsc::Sender<PeerTurn>,
    start: &mpsc::Receiver<()>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while peer.image_adapter().unwrap().channel()
        != Some(sophia_shell_client::ImageChannelState::Ready)
    {
        peer.poll_io().unwrap();
        assert!(Instant::now() < deadline, "image channel not ready");
        std::thread::yield_now();
    }
    // Serve control-file writes but defer image jobs until both slots are filled.
    ready.send(PeerTurn::Fill).unwrap();
    start.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut offers = [0; 2];
    for (slot, offer) in offers.iter_mut().enumerate() {
        let generation = first + slot as u64;
        *offer = loop {
            match peer.image_upload_ring_fill(
                RingUpload {
                    resource: generation,
                    resource_generation: 1,
                    output: 1,
                    output_generation: 1,
                    allocation: 1,
                    allocation_generation: 1,
                    topology_generation: 1,
                    width: 1920,
                    height,
                },
                1920 * height as usize * 4,
                Instant::now(),
                |bytes| {
                    for pixel in bytes.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&[
                            if generation.is_multiple_of(2) { 32 } else { 64 },
                            0,
                            0,
                            255,
                        ]);
                    }
                },
            ) {
                Ok(offer) => break offer,
                Err(ImageAdapterError::Again) => {
                    peer.poll_io().unwrap();
                }
                Err(error) => panic!("paired ring fill refused: {error:?}"),
            }
            assert!(Instant::now() < deadline, "paired control write timed out");
            std::thread::yield_now();
        };
        assert_eq!(peer.image_ring_offer(*offer).unwrap().slot, slot as u32);
    }
    ready.send(PeerTurn::Filled).unwrap();
    start.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut accepted = [false; 2];
    loop {
        if let Some((_, record)) = peer.poll_content().unwrap() {
            let ShellContentRecord::ResourceStatus(status) = record else {
                panic!("unexpected {record:?}")
            };
            let index = usize::try_from(status.resource.id.checked_sub(first).unwrap()).unwrap();
            assert!(index < 2 && !accepted[index]);
            assert_eq!(status.status, 2);
            accepted[index] = true;
        }
        if accepted.iter().all(|accepted| *accepted)
            && offers.iter().all(|offer| {
                peer.image_ring_offer(*offer).unwrap().state == RingOfferState::Settled
            })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "paired upload settlement timed out"
        );
        std::thread::yield_now();
    }
    for offer in offers {
        peer.image_forget(offer).unwrap();
    }
}

fn peer_frames(
    mut peer: ShellConnection,
    grant: ContentGrant,
    height: u32,
    frames: u64,
    warmup: u64,
    ready: mpsc::Sender<PeerTurn>,
    start: mpsc::Receiver<()>,
    pixels: mpsc::Receiver<sophia_runtime::ContentResourceLease>,
) -> ShellConnection {
    for generation in 1..=frames {
        if generation == warmup + 1 {
            ready.send(PeerTurn::Measure).unwrap();
            start.recv_timeout(Duration::from_secs(10)).unwrap();
            cost::register_thread(Role::Peer);
        }
        if generation % 2 == 1 {
            upload_pair(&mut peer, generation, height, &ready, &start);
        }
        peer.send_content(
            TransactionId::from_raw(generation * 10),
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id: generation,
                reason: 1,
            }),
        )
        .unwrap();
        let ShellContentRecord::FramePermit(permit) = next(&mut peer) else {
            panic!("permit required")
        };
        assert_eq!(permit.demand_id, generation);
        let records = [
            ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                grant,
                output: OUTPUT,
                candidate_generation: generation,
                facts_generation: 1,
                pacing_permit: permit.permit_id,
                interaction_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
            ShellContentRecord::CandidateChunk(ContentCandidateChunk {
                grant,
                candidate_generation: generation,
                chunk_ordinal: 0,
                surfaces: vec![ContentSurface {
                    allocation: ALLOCATION,
                    scale_generation: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: height,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: ContentResourceId {
                        id: generation,
                        generation: 1,
                    },
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![],
            }),
            ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                grant,
                candidate_generation: generation,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
        ];
        peer.enqueue_content_group_tracked(TransactionId::from_raw(generation * 10 + 1), &records)
            .unwrap();
        for kind in [1, 2] {
            let ShellContentRecord::CandidateOutcome(outcome) = next(&mut peer) else {
                panic!("outcome required")
            };
            assert_eq!(
                (outcome.candidate_generation, outcome.kind),
                (generation, kind)
            );
        }
        // The oracle is peer work; release its extra lease before retirement.
        let lease = pixels.recv_timeout(Duration::from_secs(10)).unwrap();
        let bytes = lease.cpu_bytes().unwrap();
        assert_eq!(lease.description().resource.id, generation);
        assert_eq!(bytes.len(), 1920 * height as usize * 4);
        let expected = [
            if generation.is_multiple_of(2) { 32 } else { 64 },
            0,
            0,
            255,
        ];
        assert!(bytes.chunks_exact(4).all(|pixel| pixel == expected));
        drop(lease);
        // Retirement is explicit; the backend must have released the predecessor.
        if generation > 1 {
            peer.send_content(
                TransactionId::from_raw(generation * 10 + 2),
                &ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant,
                    resource: ContentResourceId {
                        id: generation - 1,
                        generation: 1,
                    },
                }),
            )
            .unwrap();
            let ShellContentRecord::ResourceReleased(released) = next(&mut peer) else {
                panic!("release required")
            };
            assert_eq!(released.resource.id, generation - 1);
        }
    }
    peer
}

fn exercise(height: u32, warmup: u64, measured: u64, enabled: bool) -> (Vec<Snapshot>, u64, u64) {
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
    let worker = std::thread::spawn(move || {
        peer_frames(
            peer, key.grant, height, frames, warmup, ready_tx, start_rx, pixels_rx,
        )
    });
    let mut completed = 0;
    let mut visits = 0;
    let mut fill_wire_turns = 0;
    let mut generation = None;
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
                    generation = Some(cost::begin_block());
                    cost::enable(enabled);
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
                    let pixels = bundle
                        .resource(ContentResourceId {
                            id: generation,
                            generation: 1,
                        })
                        .unwrap()
                        .clone();
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
    let owner = cost::snapshot_thread().unwrap();
    let generation = generation.expect("measurement boundary was reached");
    assert_eq!(owner.generation, generation);
    cost::enable(false);
    cost::begin_block();
    h.backend.teardown();
    drop(peer);
    h.owner.stop(key).unwrap();
    drop(h);
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
        if enabled {
            assert!(snapshot.enabled_calls > 0);
            assert_eq!(snapshot.disabled_calls, 0);
        } else {
            assert_eq!(snapshot.enabled_calls, 0);
            assert!(snapshot.disabled_calls > 0);
            assert!(
                snapshot
                    .stages
                    .iter()
                    .all(|stage| *stage == cost::StageCost::default())
            );
        }
    }
    if enabled {
        for stage in [Stage::Validation, Stage::Copy, Stage::Pixels] {
            assert_eq!(
                worker.stages[stage as usize].calls,
                measured,
                "{} job count",
                stage.name()
            );
        }
        assert_eq!(
            snapshots[0].stages[Stage::Projection as usize].calls,
            measured * 2
        );
        assert_eq!(
            snapshots[0].stages[Stage::SourceRelease as usize].calls,
            measured
        );
    }
    if enabled {
        for stage in [Stage::Reserve, Stage::Authority] {
            assert!(
                snapshots[0].stages[stage as usize].calls >= measured,
                "{} count",
                stage.name()
            );
        }
        assert!(snapshots[0].stages[Stage::Collect as usize].calls >= measured);
        assert_eq!(
            snapshots[0].stages[Stage::Completion as usize].calls,
            visits + measured
        );
        assert!(snapshots[0].stages[Stage::Transport as usize].calls >= visits);
        assert_eq!(
            snapshots[0].stages[Stage::Submission as usize].calls,
            measured
        );
        assert_eq!(
            snapshots[0].stages[Stage::OwnerService as usize].calls,
            visits
        );
        assert_eq!(
            snapshots[0].stages[Stage::FixtureTarget as usize].calls,
            measured
        );
    }
    assert!(visits >= measured);
    (snapshots, visits, fill_wire_turns)
}

#[test]
#[ignore = "process-global timers; run alone with devices hidden"]
fn frame_attribution_owned_ring_smoke() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    exercise(24, 2, 4, true);
    exercise(512, 2, 4, false);
}

fn timer_controls() {
    for (block, enabled) in [false, true, true, false].into_iter().enumerate() {
        cost::enable(enabled);
        let generation = cost::begin_block();
        for _ in 0..2000 {
            measure(Stage::OwnerService, || std::hint::black_box(()));
        }
        let snapshot = cost::snapshot_thread().unwrap();
        assert!(!snapshot.invalid);
        assert_eq!(snapshot.enabled_calls, if enabled { 2000 } else { 0 });
        assert_eq!(snapshot.disabled_calls, if enabled { 0 } else { 2000 });
        println!(
            "sophia_frame_timer schema=1 block={block} enabled={} iterations=2000 generation={generation}",
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
#[ignore = "release attribution; run alone with devices hidden and stdout saved"]
fn frame_attribution_measure() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    timer_controls();
    crate::output::install(crate::output::SessionOutput::new(
        |line| println!("{line}"),
        |line| eprintln!("{line}"),
    ))
    .unwrap();
    for height in [24, 512] {
        for (block, enabled) in [false, true, true, false].into_iter().enumerate() {
            let (snapshots, visits, fill_wire_turns) = exercise(height, 120, 2000, enabled);
            println!(
                "sophia_frame_block schema=1 height={height} block={block} enabled={} warmup=120 completed=2000 visits={visits} fill_wire_turns={fill_wire_turns} ring_slots=2 alternating_slots=1 generation={}",
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
