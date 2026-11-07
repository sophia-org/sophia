//! Bundled copy through real component transport; supplied projection and completion.
use super::*;
use sophia_shell_client::{FrameStream, RingOfferState, RingUpload};

#[test]
fn bundled_copy_preserves_source_and_frame_credit_through_component_transport() {
    let mut h = Harness::new();
    h.owner.enable_frame_transactions(false, false).unwrap();
    let (key, mut peer) = wire::connect_revision(&mut h.owner, 0, 13);
    h.owner
        .with_service(key, |_, t| wire::allocate_extent(t, &mut peer, 1))
        .unwrap();
    let binding = sophia_protocol::shell_files::ShellFrameStreamKey {
        grant: key.grant,
        output: OUTPUT,
        allocation: ALLOCATION,
        facts_generation: 1,
        stream_id: 1,
    };
    let mut stream = FrameStream::new();
    stream
        .open(&mut peer, TransactionId::from_raw(30), binding)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        peer.poll_io().unwrap();
        for failure in h.owner.processes.turn_connected() {
            assert!(failure.is_none(), "{failure:?}");
        }
        h.owner
            .with_service(key, |_, t| {
                let allocations = t.content_allocation_snapshots();
                t.service_content_candidates(
                    &[ContentCandidateContext {
                        output: OUTPUT,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    crate::shell_component_connections::content_now_msec(),
                )
                .unwrap();
                t.poll_io().unwrap();
            })
            .unwrap();
        while let Some(event) = peer.take_frame().unwrap() {
            stream.observe(&event).unwrap();
        }
        if stream.has_credit() && peer.image_adapter().unwrap().ring_identity().is_some() {
            match peer.image_acquire_ring_fill(4, Instant::now()) {
                Ok(mut fill) => {
                    peer.image_abort_ring_fill(&mut fill).unwrap();
                    break;
                }
                Err(
                    sophia_shell_client::ImageAdapterError::Again
                    | sophia_shell_client::ImageAdapterError::Busy,
                ) => {}
                Err(error) => panic!("ring setup: {error:?}"),
            }
        }
        assert!(Instant::now() < deadline, "stream/ring setup stalled");
        std::thread::yield_now();
    }
    let mut commit = sophia_protocol::shell_files::ShellFrameRecord::Commit(
        sophia_protocol::shell_files::ShellFrameCommit {
            transaction: TransactionId::from_raw(31),
            image: None,
            retire: None,
            candidate: ContentCandidate {
                grant: key.grant,
                output: OUTPUT,
                candidate_generation: 1,
                facts_generation: 1,
                pacing_permit: 1,
                interaction_generation: 1,
                surfaces: vec![ContentSurface {
                    allocation: ALLOCATION,
                    scale_generation: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: 1,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: RESOURCE,
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![],
            },
        },
    );
    let (_, offer) = stream
        .submit_ring(
            &mut peer,
            &mut commit,
            RingUpload {
                resource: 1,
                resource_generation: 1,
                allocation: 1,
                allocation_generation: 1,
                output: 1,
                output_generation: 1,
                topology_generation: 1,
                width: 1,
                height: 1,
            },
            Instant::now(),
            |pixels| pixels.copy_from_slice(&[1, 2, 3, 255]),
        )
        .unwrap();
    assert!(!stream.has_credit());
    assert!(offer > 0);
    let mut visits = 0;
    let mut wire_services = 0;
    let mut prepared = 0;
    let mut presented = 0;
    let mut accepted = 0;
    let mut submitted = false;
    let mut admitted = false;
    loop {
        peer.poll_io().unwrap();
        let before = h.owner.processes.wire_turn_count(key);
        for failure in h.owner.processes.turn_connected() {
            assert!(failure.is_none(), "{failure:?}");
        }
        h.owner
            .with_service(key, |service, t| {
                let allocations = t.content_allocation_snapshots();
                let now = crate::shell_component_connections::content_now_msec();
                t.service_content_resources(now).unwrap();
                t.service_content_candidates(
                    &[ContentCandidateContext {
                        output: OUTPUT,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )
                .unwrap();
                if !admitted && t.next_content_submission() == Some((OUTPUT, 1)) {
                    let bundle = t.begin_content_submission(OUTPUT, 1, now).unwrap();
                    let ShellComponentService::Bar(bar) = service else {
                        panic!("bar required")
                    };
                    prepare(&mut bar.content, t, &mut h.backend, output(), bundle);
                    admitted = true;
                }
                t.poll_io().unwrap();
            })
            .unwrap();
        if submitted {
            h.owner
                .with_service(key, |service, t| {
                    let ShellComponentService::Bar(bar) = service else {
                        panic!("bar required")
                    };
                    if bar.observe_presentation(t, h.backend.runtime()).unwrap() {
                        t.poll_io().unwrap();
                    }
                })
                .unwrap();
        }
        visits += 1;
        wire_services += h.owner.processes.wire_turn_count(key) - before;
        while let Some(event) = peer.take_frame().unwrap() {
            stream.observe(&event).unwrap();
            if let sophia_protocol::shell_files::ShellFrameRecord::Outcome { outcome, .. } = event {
                match outcome.kind {
                    1 => prepared += 1,
                    2 => presented += 1,
                    _ => panic!("unexpected outcome: {outcome:?}"),
                }
            } else {
                panic!("unexpected frame event: {event:?}");
            }
        }
        while let Some((_, event)) = peer.take_content().unwrap() {
            assert!(
                matches!(&event, ShellContentRecord::ResourceStatus(status) if status.status == 2 && status.resource == RESOURCE),
                "{event:?}"
            );
            accepted += 1;
        }
        if prepared == 1 && !submitted {
            assert!(!stream.has_credit());
            h.backend.simulate_submit(output().id);
            h.backend.simulate_completion(output().id);
            submitted = true;
        }
        if presented == 1 && peer.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "bundled admission stalled: prepared={prepared} presented={presented} accepted={accepted}"
        );
        std::thread::yield_now();
    }
    assert_eq!((prepared, presented, accepted), (1, 1, 1));
    assert!(stream.has_credit());
    h.owner
        .with_service(key, |_, t| {
            let lease = t.lease_content_resource(key.grant, RESOURCE).unwrap();
            assert_eq!(lease.cpu_bytes().unwrap(), &[1, 2, 3, 255]);
        })
        .unwrap();
    peer.image_forget(offer).unwrap();
    eprintln!(
        "bundled_transport_proof supplied_service_iterations={visits} wire_services={wire_services} physical_owner_passes=unmeasured worker_completion=asynchronous native_completion=simulated"
    );
}
