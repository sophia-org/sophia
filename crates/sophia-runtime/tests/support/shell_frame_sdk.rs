//! The imported owning client against the real file/owner path, with supplied negotiation.
//! No authentication, image-copy, renderer or production-enablement claim.
use super::*;
use crate::shell_transport::files::{Inbound, ShellFileWire, ShellFiles, ShellImageChannelConfig};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_protocol::*;
use sophia_shell_client::{FrameStream, FrameStreamState, ShellClientOptions, ShellConnection};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn next(client: &mut ShellConnection, stream: &mut FrameStream) -> ShellFrameRecord {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if let Some(record) = client.take_frame().unwrap() {
            stream.observe(&record).unwrap();
            return record;
        }
        assert!(Instant::now() < until, "SDK frame response timed out");
        std::thread::yield_now();
    }
}

#[test]
fn imported_sdk_owns_credit_through_the_real_retained_scene_and_close() {
    retained_lifecycle(false);
}

#[test]
fn drained_stream_keeps_resources_and_accepts_a_legacy_candidate_on_the_same_connection() {
    retained_lifecycle(true);
}

fn retained_lifecycle(fallback: bool) {
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::take(&mut f);
    let key = fixture::key(1);
    let key = ShellFrameStreamKey {
        grant: key.grant,
        output: key.output,
        allocation: key.allocation,
        facts_generation: key.facts_generation,
        stream_id: key.stream_id,
    };
    let allocation = &f.allocations[0];
    let commit = ShellFrameRecord::Commit(ShellFrameCommit {
        transaction: fixture::tx(101),
        candidate: ContentCandidate {
            grant: key.grant,
            candidate_generation: 1,
            output: key.output,
            facts_generation: key.facts_generation,
            pacing_permit: key.stream_id,
            interaction_generation: 8,
            surfaces: vec![ContentSurface {
                allocation: allocation.allocation,
                scale_generation: allocation.scale_generation,
                role: allocation.role,
                edge: allocation.edge,
                margins: allocation.margins,
                reservation_extent: allocation.allowed_reservation_extent,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: allocation.anchor_parent_rect,
            }],
            placements: vec![ContentPlacement {
                resource: fixture::resource(2),
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        },
        image: None,
        retire: None,
    });
    let (local, remote) = UnixStream::pair().unwrap();
    let (role, bounds) = crate::shell_transport::files::role_bounds(None);
    let mut export = ShellFiles::awaiting_negotiation(
        key.grant.connection_epoch,
        role,
        false,
        bounds,
        100,
        Instant::now(),
    );
    let (mut discovery, limits, formats, role_bounds) =
        crate::shell_transport::owned_copy::wire_tests::config();
    discovery.connection_epoch = key.grant.connection_epoch;
    export
        .configure_image_channel(Some(ShellImageChannelConfig {
            discovery,
            limits,
            formats,
            role_bounds,
        }))
        .unwrap();
    t.transport.state.wire = Some(Box::new(
        ShellFileWire::adopt(local, export, None, false).unwrap(),
    ));
    let capabilities = t.transport.state.capabilities;
    let peer = std::thread::spawn(move || {
        let timeout = Duration::from_secs(5);
        let mut pipeline = Pipeline::over(remote, PipelineLimits::default(), timeout).unwrap();
        let (tag, root) = pipeline.attach(&[], &[]).unwrap();
        assert!(matches!(
            pipeline.wait(tag, Instant::now() + timeout).unwrap(),
            Reply::Attach(_)
        ));
        let mut client = ShellConnection::from_attached(
            pipeline,
            root,
            ShellClientOptions {
                minimum_revision: 13,
                maximum_revision: 13,
                required_capabilities: capabilities,
                handshake_timeout: timeout,
            },
        )
        .unwrap();
        assert_eq!(client.welcome().selected_revision, 13);
        let mut stream = FrameStream::new();
        stream.open(&mut client, fixture::tx(100), key).unwrap();
        assert!(!stream.has_credit());
        assert!(
            matches!(next(&mut client, &mut stream), ShellFrameRecord::Opened { key: actual, .. } if actual == key)
        );
        assert!(stream.has_credit());
        stream.submit(&mut client, &commit).unwrap();
        assert!(!stream.has_credit());
        assert!(stream.submit(&mut client, &commit).is_err());
        assert!(
            matches!(next(&mut client, &mut stream), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1)
        );
        assert!(!stream.has_credit(), "Prepared must not return credit");
        assert!(
            matches!(next(&mut client, &mut stream), ShellFrameRecord::Outcome { outcome, credit_expires_msec, .. } if outcome.kind == 2 && credit_expires_msec > 10)
        );
        assert!(stream.has_credit());
        stream.close(&mut client, fixture::tx(102)).unwrap();
        assert!(!stream.has_credit());
        assert!(
            matches!(next(&mut client, &mut stream), ShellFrameRecord::Closed { key: actual, .. } if actual == key)
        );
        assert_eq!(stream.state(), FrameStreamState::Closed);
        assert!(stream.frame_work_drained());
        if fallback {
            legacy_frame(&mut client, &commit);
        }
        client
    });
    let until = Instant::now() + Duration::from_secs(5);
    let mut negotiated = false;
    let mut submitted = 0;
    let mut displays = Vec::new();
    let mut owner = crate::shell_transport::owned_copy::OwnedCopyOwner::default();
    let mut worker = crate::IsbValidationWorker::new(1, 8).unwrap();
    while !peer.is_finished() {
        assert!(
            Instant::now() < until,
            "SDK/owner handshake or lifecycle hung"
        );
        t.transport.state.files_mut().unwrap().turn().unwrap();
        if !negotiated {
            let wire = t.transport.state.files_mut().unwrap();
            if let Some(request) = wire.export_mut().take_inbound() {
                let Inbound::Negotiate(hello) = request else {
                    panic!("expected negotiation")
                };
                assert_eq!((hello.minimum_revision, hello.maximum_revision), (13, 13));
                assert_eq!(hello.required_capabilities, capabilities);
                let limits = ContentLimits::prototype(key.grant);
                let bytes = crate::shell_transport::files::encode_limits_object(
                    key.grant.connection_epoch,
                    &limits,
                )
                .unwrap();
                wire.export_mut()
                    .complete_negotiation_with_image_revision(
                        Some((bytes, limits)),
                        capabilities,
                        13,
                    )
                    .unwrap();
                let body = crate::shell_transport::files::encode_negotiated(
                    ShellV1ServerWelcome {
                        connection_epoch: key.grant.connection_epoch,
                        selected_revision: 13,
                        capabilities,
                        max_descriptors: SOPHIA_SHELL_MAX_DESCRIPTORS as u16,
                        max_label_bytes: MAX_CHROME_LABEL_LEN as u16,
                        max_pending_activations: SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u16,
                    },
                    true,
                )
                .unwrap();
                assert!(wire.append(ShellFileKind::Negotiated, &body, true).unwrap());
                negotiated = true;
            }
        } else {
            owner
                .service(
                    &mut t.transport.state,
                    &mut t.transport.content_epochs,
                    &mut worker,
                    10,
                    Instant::now(),
                    0,
                )
                .unwrap();
            let contexts = [crate::ContentCandidateContext {
                output: key.output,
                facts_generation: key.facts_generation,
                interaction_generation: 8,
                allocations: &f.allocations,
            }];
            t.transport
                .state
                .service_content_demands(
                    &mut t.transport.content_epochs,
                    &[key.output],
                    &f.allocations,
                )
                .unwrap();
            if let Some((transaction, demand)) = t
                .transport
                .state
                .next_content_demand(&t.transport.content_epochs)
            {
                t.transport
                    .state
                    .grant_content_demand(
                        &mut t.transport.content_epochs,
                        transaction,
                        demand.output,
                        9,
                        10,
                    )
                    .unwrap();
            }
            t.transport
                .state
                .service_content_candidates(&mut t.transport.content_epochs, &contexts, 10)
                .unwrap();
            if t.candidates().pending_candidate_count() != 0 {
                let generation = submitted + 1;
                displays.push(
                    t.transport
                        .begin_content_submission(key.output, generation, 10)
                        .unwrap(),
                );
                t.transport
                    .content_prepared(key.grant, key.output, generation, 20, 30, 10)
                    .unwrap();
                t.transport
                    .content_presented_at(key.grant, key.output, generation, 40, 20, 30, 10)
                    .unwrap();
                submitted += 1;
            }
            t.transport.poll_io().unwrap();
        }
        std::thread::yield_now();
    }
    let _client = peer.join().unwrap();
    assert!(negotiated);
    assert_eq!(submitted, if fallback { 2 } else { 1 });
    assert_eq!((owner.pending(), worker.pending()), (0, 0));
    assert!(
        displays.len() == submitted as usize,
        "native owner lease is independent of frame credit"
    );
}

fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if let Some((_, record)) = client.take_content().unwrap()
            && matches!(
                record,
                ShellContentRecord::FramePermit(_) | ShellContentRecord::CandidateOutcome(_)
            )
        {
            return record;
        }
        assert!(
            Instant::now() < until,
            "legacy response after SFT drain timed out"
        );
        std::thread::yield_now();
    }
}

fn legacy_frame(client: &mut ShellConnection, prior: &ShellFrameRecord) {
    let ShellFrameRecord::Commit(prior) = prior else {
        panic!("commit")
    };
    let scene = &prior.candidate;
    client
        .enqueue_content(
            fixture::tx(110),
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: scene.grant,
                output: scene.output,
                allocation: scene.surfaces[0].allocation,
                demand_id: 1,
                reason: 1,
            }),
        )
        .unwrap();
    let ShellContentRecord::FramePermit(permit) = next_content(client) else {
        panic!("permit")
    };
    assert_eq!(permit.state, 1);
    let records = [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: scene.grant,
            candidate_generation: 2,
            output: scene.output,
            facts_generation: scene.facts_generation,
            pacing_permit: permit.permit_id,
            interaction_generation: scene.interaction_generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
        ShellContentRecord::CandidateChunk(ContentCandidateChunk {
            grant: scene.grant,
            candidate_generation: 2,
            chunk_ordinal: 0,
            surfaces: scene.surfaces.clone(),
            placements: scene.placements.clone(),
            targets: vec![],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: scene.grant,
            candidate_generation: 2,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        }),
    ];
    client
        .enqueue_content_group(fixture::tx(111), &records)
        .unwrap();
    for kind in [1, 2] {
        let ShellContentRecord::CandidateOutcome(outcome) = next_content(client) else {
            panic!("outcome")
        };
        assert_eq!((outcome.kind, outcome.candidate_generation), (kind, 2));
    }
    assert_eq!(
        client.welcome().selected_revision,
        13,
        "no reconnect or renegotiation"
    );
}
