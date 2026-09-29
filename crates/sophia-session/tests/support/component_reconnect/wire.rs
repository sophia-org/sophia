//! Resource and candidate lifecycle over the public 9P SDK. Assertions from
//! the IPC fixture at ec56ef5eb are retained; typed custody replaces writes.
use super::*;
use sophia_shell_client::{Admission, Custody, ShellClientOptions, ShellConnection};

pub(super) fn read(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
) -> (TransactionId, ShellContentRecord) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        transport.service_content_resources(0).unwrap();
        transport.poll_io().unwrap();
        if let Some(record) = peer.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "content observation missing");
        std::thread::yield_now();
    }
}

fn flush(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    admission: Admission,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        peer.poll_io().unwrap();
        transport.service_content_resources(0).unwrap();
        transport.poll_io().unwrap();
        let mut complete = true;
        for ticket in admission.tickets() {
            match peer.custody(ticket).unwrap() {
                Custody::Submitted | Custody::Stored => {}
                Custody::Queued | Custody::InFlight => complete = false,
                other => panic!("file custody failed: {other:?}"),
            }
        }
        if complete {
            return;
        }
        assert!(Instant::now() < deadline, "file custody missing");
        std::thread::yield_now();
    }
}

pub(super) fn send(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    record: ShellContentRecord,
) {
    let admission = peer
        .enqueue_content_tracked(TransactionId::from_raw(10), &record)
        .unwrap();
    flush(transport, peer, admission);
}

pub(super) fn client(socket: std::path::PathBuf, slot: usize) -> (ShellConnection, ContentLimits) {
    let revision = match slot {
        0 => 6,
        1 => 7,
        2 => 8,
        _ => panic!("role"),
    };
    let capabilities = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
        | match slot {
            0 => {
                SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                    | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
            }
            1 => {
                SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
            }
            2 => {
                SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
                    | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                    | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            }
            _ => unreachable!(),
        };
    let mut peer = ShellConnection::connect_files(
        socket,
        ShellClientOptions {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: capabilities,
            handshake_timeout: Duration::from_secs(3),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((_, record)) = peer.poll_content().unwrap() {
            let ShellContentRecord::Limits(limits) = record else {
                panic!("Limits");
            };
            return (peer, limits);
        }
        assert!(Instant::now() < deadline, "Limits fetch missing");
        std::thread::yield_now();
    }
}

pub(super) fn connect(
    owner: &mut ShellComponentSession,
    slot: usize,
) -> (ComponentConnectionKey, ShellConnection) {
    let (key, socket) = owner
        .processes
        .reconnect_fixture_endpoint(slot, owner.policy);
    let worker = std::thread::spawn(move || client(socket, slot));
    let deadline = Instant::now() + Duration::from_secs(5);
    let welcome = loop {
        let visit = owner.poll(65536).unwrap();
        if let Some((received, result)) = visit.negotiations.into_iter().flatten().next() {
            assert_eq!(received, key);
            break result.unwrap();
        }
        assert!(Instant::now() < deadline, "negotiation missing");
        std::thread::yield_now();
    };
    while !worker.is_finished() {
        owner
            .with_service(key, |_, t| t.poll_io().unwrap())
            .unwrap();
        assert!(Instant::now() < deadline, "client handshake missing");
        std::thread::yield_now();
    }
    let (peer, limits) = worker.join().unwrap();
    assert_eq!(welcome, peer.welcome());
    assert_eq!(welcome.connection_epoch, key.grant.connection_epoch);
    assert_eq!(limits.grant, key.grant);
    assert!(
        owner
            .connected_roles()
            .into_iter()
            .flatten()
            .any(|(current, _)| current == key)
    );
    (key, peer)
}

pub(super) fn upload(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
) -> ContentResourceLease {
    upload_id(transport, peer, RESOURCE)
}

pub(super) fn upload_id(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    resource: ContentResourceId,
) -> ContentResourceLease {
    let grant = transport.content_grant().unwrap();
    for record in [
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant,
            resource,
            width_px: 1,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 4,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal: 0,
            offset: 0,
            bytes: vec![1, 2, 3, 255],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 4,
            chunk_count: 1,
        }),
    ] {
        send(transport, peer, record);
    }
    transport.service_content_resources(0).unwrap();
    transport.poll_io().unwrap();
    for status in [1, 2] {
        let (_, ShellContentRecord::ResourceStatus(value)) = read(transport, peer) else {
            panic!("resource status");
        };
        assert_eq!(value.status, status);
        assert_eq!(value.grant, grant);
    }
    transport.lease_content_resource(grant, resource).unwrap()
}

pub(super) fn allocate(transport: &mut ShellTransportConnection<'_>, peer: &mut ShellConnection) {
    allocate_extent(transport, peer, 1);
}

pub(super) fn upload_maximum(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    id: u64,
) -> ContentResourceLease {
    let grant = transport.content_grant().unwrap();
    let description = ContentResourceBegin {
        grant,
        resource: ContentResourceId { id, generation: 1 },
        width_px: 1024,
        height_px: 1024,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 69,
        total_bytes: 4 * 1024 * 1024,
    };
    let layout = description
        .layout(transport.content_limits().unwrap())
        .unwrap();
    send(
        transport,
        peer,
        ShellContentRecord::ResourceBegin(description.clone()),
    );
    transport.service_content_resources(0).unwrap();
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::ResourceStatus(status)) = read(transport, peer) else {
        panic!("begin status")
    };
    assert_eq!(status.status, 1);
    let mut offset = 0;
    for ordinal in 0..layout.chunk_count {
        let bytes = (layout.total_bytes - offset)
            .min(u64::from(layout.row_bytes) * u64::from(layout.rows_per_chunk));
        send(
            transport,
            peer,
            ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant,
                resource: description.resource,
                ordinal,
                offset,
                bytes: vec![0; bytes as usize],
            }),
        );
        transport.service_content_resources(0).unwrap();
        transport.poll_io().unwrap();
        offset += bytes;
    }
    send(
        transport,
        peer,
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource: description.resource,
            total_bytes: description.total_bytes,
            chunk_count: layout.chunk_count,
        }),
    );
    transport.service_content_resources(0).unwrap();
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::ResourceStatus(status)) = read(transport, peer) else {
        panic!("end status")
    };
    assert_eq!(status.status, 2);
    transport
        .lease_content_resource(grant, description.resource)
        .unwrap()
}

pub(super) fn allocate_extent(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    extent: u32,
) {
    transport
        .publish_content_output_facts(
            TransactionId::from_raw(2),
            1,
            vec![ContentOutputFactsEntry {
                output: OUTPUT,
                local_width: 64,
                local_height: 32,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    let grant = transport.content_grant().unwrap();
    send(
        transport,
        peer,
        ShellContentRecord::AllocationRequest(ContentAllocationRequest {
            grant,
            output: OUTPUT,
            allocation_request_id: 1,
            operation: 1,
            role: 1,
            edge: 1,
            prior: ContentAllocationId::default(),
            parent: ContentAllocationId::default(),
            parent_presentation_epoch: 0,
            anchor_parent_rect: ContentPixelRect::default(),
            desired_width: 64,
            desired_height: extent,
            margins: ContentMargins::default(),
        }),
    );
    transport
        .service_content_allocation_requests(&[], 0)
        .unwrap();
    let (_, request) = transport.next_content_allocation_request().unwrap();
    transport
        .grant_content_allocation(
            request.allocation_request_id,
            ContentAllocationSnapshot {
                native_opening: None,
                output: OUTPUT,
                allocation: ALLOCATION,
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: extent,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: extent,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: extent,
            },
            &[],
        )
        .unwrap();
    transport.poll_io().unwrap();
    // Facts and the allocation outcome are the only server frames so far.
    read(transport, peer);
    read(transport, peer);
}

pub(super) fn candidate(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    generation: u64,
) -> ContentRenderBundle {
    let grant = transport.content_grant().unwrap();
    let extent = transport.content_allocation_snapshots()[0].allowed_reservation_extent;
    transport
        .grant_content_permit(
            TransactionId::from_raw(3),
            OUTPUT,
            generation,
            generation,
            0,
        )
        .unwrap();
    transport.poll_io().unwrap();
    read(transport, peer);
    let records = [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant,
            output: OUTPUT,
            candidate_generation: generation,
            facts_generation: 1,
            pacing_permit: generation,
            interaction_generation: 1,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
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
                reservation_extent: extent,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource: RESOURCE,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![ContentTarget {
                surface_index: 0,
                action_kind: 1,
                target_id: 1,
                target_generation: 1,
                action_id: 1,
                bounds_px: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            }],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant,
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        }),
    ];
    let admission = peer
        .enqueue_content_group_tracked(TransactionId::from_raw(10), &records)
        .unwrap();
    flush(transport, peer, admission);
    let allocations = transport.content_allocation_snapshots();
    assert_eq!(
        transport
            .service_content_candidates(
                &[ContentCandidateContext {
                    output: OUTPUT,
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: &allocations,
                }],
                0
            )
            .unwrap(),
        3
    );
    transport
        .begin_content_submission(OUTPUT, generation, 0)
        .unwrap()
}

pub(super) fn outcome(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
    generation: u64,
    kind: u16,
) {
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::CandidateOutcome(value)) = read(transport, peer) else {
        panic!("candidate outcome");
    };
    assert_eq!(
        (value.grant, value.candidate_generation, value.kind),
        (transport.content_grant().unwrap(), generation, kind)
    );
}
