#![cfg(feature = "experimental-draw")]
//! `sophia_shell_fs_v1` over a real private socket, with the actual content
//! owners and supplied protection evidence. No protected child, compositor
//! or Session selection here; those are separate evidence.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

#[allow(dead_code)]
#[path = "support/shell_required_image.rs"]
mod image;

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    (transport, directory)
}

fn limits(epoch: u64) -> ContentLimits {
    ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    })
}

fn candidate(kind: ShellFileKind, epoch: u64, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
    policy: ShellContentAdmissionPolicy,
    peer_done: &std::thread::JoinHandle<()>,
) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
    transport.begin_file_negotiation(registry, epoch, Duration::from_secs(2), policy)?;
    let start = Instant::now();
    loop {
        if let Some(welcome) = transport.poll_negotiation(registry, 64 * 1024)? {
            return Ok(Some(welcome));
        }
        if peer_done.is_finished() {
            return Ok(None);
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
}

pub fn paced_draw_panel(preview: Option<&std::path::Path>) {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    let mut image_owner = image::configure(&mut transport, &expected);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };

    let (uploaded_tx, uploaded_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        image::setup(&mut peer);
        let capabilities = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
            | (1 << 13);
        let offer = encode_shell_file_negotiate(
            candidate(ShellFileKind::Negotiate, 1, 1),
            ShellV1ClientHello {
                minimum_revision: 9,
                maximum_revision: 9,
                required_capabilities: capabilities,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        let published = peer.next_event();
        let first = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(
            (first.object, first.generation),
            (ShellFileKind::Outputs, 3)
        );
        peer.ack(&published);
        peer.open(6, b"outputs", 0);
        let object = peer.read(6, 0);
        let ShellContentRecord::OutputFacts(facts) =
            decode_shell_file_outputs(&object).unwrap().record
        else {
            panic!("output facts");
        };
        let output = facts.outputs[0].output;

        let request = ContentAllocationRequest {
            grant,
            output,
            allocation_request_id: 1,
            operation: 1,
            role: 1,
            edge: 1,
            prior: ContentAllocationId::default(),
            parent: ContentAllocationId::default(),
            parent_presentation_epoch: 0,
            anchor_parent_rect: ContentPixelRect::default(),
            desired_width: 1920,
            desired_height: 32,
            margins: ContentMargins::default(),
        };
        let bytes = encode_shell_file_allocation_request(
            candidate(ShellFileKind::AllocationRequest, 1, 2),
            ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(6),
                record: ShellContentRecord::AllocationRequest(request),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&bytes, 2);
        let event = peer.next_event();
        let value = decode_shell_file_allocation_result(&event).unwrap();
        let ShellContentRecord::AllocationResult(allocation) = value.record else {
            panic!("allocation result");
        };
        assert_eq!(allocation.status, 1);
        peer.ack(&event);

        peer.open(20, b"experimental-draw-v1", 1);
        let mut command_bytes = 0;
        command_bytes += draw(&mut peer, panel_batch(grant));
        for frame in 1..=64u64 {
            let value_generation = (frame - 1) / 8 + 1;
            if (frame - 1) % 8 == 0 {
                command_bytes += draw(&mut peer, value_batch(grant, value_generation));
            }
            let demand = encode_shell_file_transaction(
                candidate(ShellFileKind::FrameDemand, 1, 5 + frame * 3),
                &ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(9),
                    record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                        grant,
                        output,
                        allocation: ContentAllocationId::default(),
                        demand_id: frame,
                        reason: 1,
                    }),
                },
            )
            .unwrap();
            peer.submit_acknowledged(&demand, 5 + frame * 3);
            uploaded_tx.send(()).unwrap();
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
            let ShellContentRecord::FramePermit(permit) = value.record else {
                panic!("frame permit");
            };
            assert_eq!(permit.state, 1);
            peer.ack(&event);

            let generation = frame;
            let candidate_bytes = encode_shell_file_candidate(
                candidate(ShellFileKind::Candidate, 1, 6 + frame * 3),
                &ShellFileCandidate {
                    transaction: TransactionId::from_raw(20),
                    candidate: ContentCandidate {
                        grant,
                        candidate_generation: generation,
                        output: permit.output,
                        facts_generation: facts.facts_generation,
                        pacing_permit: permit.permit_id,
                        interaction_generation: 4,
                        surfaces: vec![ContentSurface {
                            allocation: allocation.allocation,
                            scale_generation: allocation.scale_generation,
                            role: 1,
                            edge: 1,
                            margins: ContentMargins::default(),
                            reservation_extent: 24,
                            parent_surface_index: u16::MAX,
                            anchor_parent_rect: ContentPixelRect::default(),
                        }],
                        placements: vec![
                            ContentPlacement {
                                resource,
                                surface_index: 0,
                                destination_x_px: 0,
                                destination_y_px: 0,
                            },
                            ContentPlacement {
                                resource: ContentResourceId {
                                    id: 2,
                                    generation: value_generation,
                                },
                                surface_index: 0,
                                destination_x_px: 1824,
                                destination_y_px: 0,
                            },
                        ],
                        targets: vec![ContentTarget {
                            surface_index: 0,
                            action_kind: 1,
                            target_id: 1,
                            target_generation: 1,
                            action_id: 1,
                            bounds_px: ContentPixelRect {
                                x: 3,
                                y: 4,
                                width: 2,
                                height: 1,
                            },
                        }],
                    },
                },
            )
            .unwrap();
            peer.submit_acknowledged(&candidate_bytes, 6 + frame * 3);

            let mut outcomes = Vec::new();
            while outcomes.len() < 2 {
                let event = peer.next_event();
                let value = decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome)
                    .unwrap_or_else(|error| {
                        panic!("frame {frame} outcome error {error:?}: {event:?}")
                    });
                let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
                    panic!("candidate outcome");
                };
                outcomes.push((outcome.kind, outcome.presentation_epoch));
                peer.ack(&event);
            }
            assert_eq!(outcomes, vec![(1, 0), (2, frame + 8)]);
            if frame > 1 && (frame - 1) % 8 == 0 {
                let retire = encode_shell_file_resource_retire(
                    candidate(ShellFileKind::ResourceRetire, 1, 7 + frame * 3),
                    &ShellFileTransactionRecord {
                        transaction: TransactionId::from_raw(500 + frame),
                        record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                            grant,
                            resource: ContentResourceId {
                                id: 2,
                                generation: value_generation - 1,
                            },
                        }),
                    },
                )
                .unwrap();
                peer.submit_acknowledged(&retire, 7 + frame * 3);
                let event = peer.next_event();
                let released = decode_shell_file_resource_released(&event).unwrap();
                assert!(matches!(
                    released.record,
                    ShellContentRecord::ResourceReleased(_)
                ));
                peer.ack(&event);
            }
        }
        assert!(command_bytes < 2048, "small commands, no pixel payload");
        eprintln!(
            "draw_panel frames=64 raster_jobs=9 static_rasters=1 value_rasters=8 command_bytes={command_bytes} client_pixel_bytes=0"
        );
    });

    negotiate(
        &mut transport,
        &mut registry,
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        &peer,
    )
    .unwrap()
    .expect("negotiated");
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let start = Instant::now();
    transport
        .publish_content_output_facts(
            &mut registry,
            TransactionId::from_raw(5),
            3,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 1920,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 5,
            }],
        )
        .unwrap();
    while transport
        .next_content_allocation_request(&registry)
        .is_none()
    {
        transport
            .service_content_allocation_requests(
                &mut registry,
                &[],
                start.elapsed().as_millis() as u64,
            )
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let allocation_id = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    transport
        .grant_content_allocation(
            &mut registry,
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output,
                allocation: allocation_id,
                scale_generation: 5,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 32,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 32,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 32,
            },
            &[],
        )
        .unwrap();
    let mut raster = sophia_draw::Rasterizer::default();
    let expected_panel = raster.render(&panel_batch(grant), || false).unwrap();
    let mut first_pointer = None;
    for frame in 1..=64u64 {
        while uploaded_rx.try_recv().is_err() {
            image_owner
                .service(
                    &mut transport,
                    &mut registry,
                    start.elapsed().as_millis() as u64,
                    Instant::now(),
                )
                .unwrap();
            transport
                .service_content_resources(&mut registry, start.elapsed().as_millis() as u64)
                .unwrap();
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
        let allocations = transport.content_allocation_snapshots(&registry);
        let context = ContentCandidateContext {
            output,
            facts_generation: 3,
            interaction_generation: 4,
            allocations: &allocations,
        };
        while transport.next_content_demand(&registry).is_none() {
            transport
                .service_content_demands(&mut registry, &[output], &allocations)
                .unwrap();
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
        transport
            .grant_content_demand(
                &mut registry,
                TransactionId::from_raw(1000 + frame),
                output,
                frame,
                start.elapsed().as_millis() as u64,
            )
            .unwrap();
        let mut processed = 0;
        while processed < 3 {
            processed += transport
                .service_content_candidates(
                    &mut registry,
                    &[context],
                    start.elapsed().as_millis() as u64,
                )
                .unwrap();
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
        let render = transport
            .begin_content_submission(
                &mut registry,
                output,
                frame,
                start.elapsed().as_millis() as u64,
            )
            .unwrap();
        let pixels = render.resource(resource).unwrap().cpu_bytes().unwrap();
        assert_eq!(pixels, expected_panel);
        let pointer = pixels.as_ptr();
        assert_eq!(
            *first_pointer.get_or_insert(pointer),
            pointer,
            "static resource mapping retained"
        );
        let value = ContentResourceId {
            id: 2,
            generation: (frame - 1) / 8 + 1,
        };
        let value_pixels = render.resource(value).unwrap().cpu_bytes().unwrap();
        assert_eq!(
            value_pixels,
            raster
                .render(&value_batch(grant, value.generation), || false)
                .unwrap()
        );
        if frame == 64
            && let Some(path) = preview
        {
            let mut combined = pixels.to_vec();
            for y in 0..24 {
                combined[(y * 1920 + 1824) * 4..(y + 1) * 1920 * 4]
                    .copy_from_slice(&value_pixels[y * 96 * 4..(y + 1) * 96 * 4]);
            }
            let mut ppm = b"P6\n1920 24\n255\n".to_vec();
            for p in combined.chunks_exact(4) {
                ppm.extend([p[2], p[1], p[0]]);
            }
            std::fs::write(path, ppm).unwrap();
        }
        transport
            .content_prepared(
                &mut registry,
                grant,
                output,
                frame,
                7,
                frame,
                start.elapsed().as_millis() as u64,
            )
            .unwrap();
        transport
            .content_presented(&mut registry, grant, output, frame, frame + 8, 7, frame)
            .unwrap();
    }
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    assert_eq!(registry.accounting().memory.resident, 0);
    assert_eq!(image_owner.pending(), 0);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn draw_content_crosses_real_9p_worker_resource_and_presentation_owners() {
    paced_draw_panel(None);
}

fn panel_batch(grant: ContentGrant) -> sophia_draw::Batch {
    sophia_draw::Batch {
        transaction: 100,
        grant,
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width: 1920,
        height: 24,
        commands: vec![
            sophia_draw::Command::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 24,
                rgb: [20, 30, 40],
            },
            sophia_draw::Command::Text {
                x: 8,
                y: 2,
                size: 16,
                rgb: [240, 240, 240],
                text: "Sophia | retained Vello panel".into(),
            },
        ],
    }
}
fn value_batch(grant: ContentGrant, generation: u64) -> sophia_draw::Batch {
    sophia_draw::Batch {
        transaction: 100 + generation,
        grant,
        resource: ContentResourceId { id: 2, generation },
        width: 96,
        height: 24,
        commands: vec![
            sophia_draw::Command::Rect {
                x: 0,
                y: 0,
                width: 96,
                height: 24,
                rgb: [20, 30, 40],
            },
            sophia_draw::Command::Text {
                x: 8,
                y: 2,
                size: 16,
                rgb: [100, 220, 180],
                text: format!("{:02}", generation),
            },
        ],
    }
}
fn draw(peer: &mut Peer, batch: sophia_draw::Batch) -> usize {
    let bytes = batch.encode().unwrap();
    assert_eq!(peer.write(20, &bytes).0, 119);
    assert_eq!(peer.write(20, &bytes).0, 119);
    let event = peer.next_event();
    let status = decode_shell_file_resource_status(&event).unwrap();
    let ShellContentRecord::ResourceStatus(status) = status.record else {
        panic!("draw status")
    };
    assert_eq!((status.status, status.resource), (2, batch.resource));
    peer.ack(&event);
    bytes.len()
}
