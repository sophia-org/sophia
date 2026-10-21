//! Resource and candidate lifecycle over the public 9P SDK. Assertions from
//! the IPC fixture at ec56ef5eb are retained; typed custody replaces writes.
//! Each synchronous fixture loop models a fresh owner pass: input turn first,
//! then resource/role service and publication flush, as in the Session owner.
//! Pixels arrive only as sealed images over the authenticated channel, admitted
//! by the owner's own image worker against a live allocation.
use super::*;
use crate::shell_component_connections::content_now_msec;
use sophia_shell_client::{
    Admission, Custody, NativeLauncherObservation, RingUpload, ShellClientOptions, ShellConnection,
};

/// The actual owner of one connection. Each visit runs that owner's image
/// service before lending the connection view, as the Session owner pass does.
pub(super) trait Owner {
    fn visit<R>(&mut self, service: impl FnOnce(&mut ShellTransportConnection<'_>, u64) -> R) -> R;
}

pub(super) struct Component<'a>(
    pub &'a mut ShellComponentSession,
    pub ComponentConnectionKey,
);

impl Owner for Component<'_> {
    fn visit<R>(&mut self, service: impl FnOnce(&mut ShellTransportConnection<'_>, u64) -> R) -> R {
        for failure in self.0.processes.turn_connected() {
            assert!(failure.is_none(), "owner turn failed: {failure:?}");
        }
        let now = content_now_msec();
        self.0.with_service(self.1, |_, t| service(t, now)).unwrap()
    }
}

impl Owner for super::super::super::LiveMetadataShell {
    fn visit<R>(&mut self, service: impl FnOnce(&mut ShellTransportConnection<'_>, u64) -> R) -> R {
        let now = content_now_msec();
        self.transport.service_images(now, Instant::now()).unwrap();
        service(&mut self.transport.connection(), now)
    }
}

pub(super) fn read(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut ShellConnection,
) -> (TransactionId, ShellContentRecord) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        transport.service_owner_turn().unwrap();
        transport
            .service_content_resources(content_now_msec())
            .unwrap();
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
        transport.service_owner_turn().unwrap();
        transport
            .service_content_resources(content_now_msec())
            .unwrap();
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

/// Shell r9 with sealed images, over an afid-authenticated attach.
pub(super) fn client(socket: std::path::PathBuf, slot: usize) -> (ShellConnection, ContentLimits) {
    assert!(slot <= 2, "role");
    // Bit 13: sealed images, required for every content attach at r9.
    let capabilities = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | (1 << 15)
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
    let mut peer = sdk_image::authenticated_client(
        &socket,
        ShellClientOptions {
            minimum_revision: 10,
            maximum_revision: 10,
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
            sdk_image::enable(&mut peer, socket.parent().unwrap(), &limits);
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
            .with_service(key, |_, t| t.service_owner_turn().unwrap())
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

pub(super) fn upload(owner: &mut impl Owner, peer: &mut ShellConnection) -> ContentResourceLease {
    upload_id(owner, peer, RESOURCE)
}

pub(super) fn upload_id(
    owner: &mut impl Owner,
    peer: &mut ShellConnection,
    resource: ContentResourceId,
) -> ContentResourceLease {
    upload_sealed(owner, peer, resource, 1, 1, &[1, 2, 3, 255])
}

/// One owned-ring offer for a live allocation at facts generation 1. The
/// SDK peer awaits Accepted and its retained release; the owner admits it.
fn upload_sealed(
    owner: &mut impl Owner,
    peer: &mut ShellConnection,
    resource: ContentResourceId,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> ContentResourceLease {
    let grant = owner.visit(|t, _| t.content_grant().unwrap());
    let metadata = RingUpload {
        resource: resource.id,
        resource_generation: resource.generation,
        output: OUTPUT.id,
        output_generation: OUTPUT.generation,
        allocation: ALLOCATION.id,
        allocation_generation: ALLOCATION.generation,
        topology_generation: 1,
        width,
        height,
    };
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| sdk_image::upload(peer, metadata, pixels));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !worker.is_finished() {
            owner.visit(|t, now| {
                t.service_owner_turn().unwrap();
                t.service_content_resources(now).unwrap();
                t.poll_io().unwrap();
            });
            assert!(Instant::now() < deadline, "owned image admission hung");
            std::thread::yield_now();
        }
        worker.join().unwrap();
    });
    owner.visit(|t, _| t.lease_content_resource(grant, resource).unwrap())
}

/// The launcher's native opening allocation: facts, then the opening, then the
/// peer's native request granted by the fixture's supplied placement policy.
pub(super) fn allocate_native(owner: &mut impl Owner, peer: &mut ShellConnection) {
    let grant = owner.visit(|t, _| t.content_grant().unwrap());
    let opening = NativeLauncherOpening {
        grant,
        output: OUTPUT,
        opening: 1,
        catalog_generation: 1,
        state_revision: 1,
    };
    owner.visit(|t, _| {
        t.publish_content_output_facts(
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
        t.publish_native_launcher_opening(TransactionId::from_raw(79), opening)
            .unwrap();
    });
    peer.enqueue_native_launcher_tracked(
        &sophia_protocol::shell_files::ShellFileNativeLauncherRecord {
            transaction: TransactionId::from_raw(80),
            record: ShellNativeLauncherRecord::AllocationRequest(NativeLauncherAllocationRequest {
                grant,
                output: OUTPUT,
                opening: 1,
                request_id: 1,
                prior: ContentAllocationId::default(),
                operation: 1,
                edge: 1,
                desired_width: 64,
                desired_height: 16,
                margins: ContentMargins::default(),
            }),
        },
    )
    .unwrap();
    let catalog = ShellApplicationCatalog {
        connection_epoch: grant.connection_epoch,
        generation: 1,
        entries: Vec::new(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        owner.visit(|t, now| {
            t.service_owner_turn().unwrap();
            t.service_native_launcher_content(
                ContentCandidateContext {
                    output: OUTPUT,
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: &[],
                },
                NativeLauncherCandidateContext {
                    opening,
                    state_revision: 1,
                    catalog: &catalog,
                },
                now,
            )
            .unwrap();
            if let Some((_, request)) = t.next_content_allocation_request() {
                t.grant_content_allocation(
                    request.allocation_request_id,
                    ContentAllocationSnapshot {
                        native_opening: Some(1),
                        output: OUTPUT,
                        allocation: ALLOCATION,
                        scale_generation: 1,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        role: 3,
                        edge: 1,
                        margins: ContentMargins::default(),
                        logical: ContentLogicalRect {
                            x: 0,
                            y: 0,
                            width: 64,
                            height: 16,
                        },
                        pixel: ContentPixelRect {
                            x: 0,
                            y: 0,
                            width: 64,
                            height: 16,
                        },
                        parent: ContentAllocationId::default(),
                        anchor_parent_rect: ContentPixelRect::default(),
                        allowed_reservation_extent: 0,
                    },
                    &[],
                )
                .unwrap();
            }
            t.poll_io().unwrap();
        });
        peer.poll_io().unwrap();
        if let Some(NativeLauncherObservation::Content(
            _,
            ShellContentRecord::AllocationResult(result),
        )) = peer.take_native_launcher_observation().unwrap()
        {
            assert_eq!(result.status, 1, "{result:?}");
            return;
        }
        assert!(Instant::now() < deadline, "native allocation not granted");
        std::thread::yield_now();
    }
}

pub(super) fn allocate(transport: &mut ShellTransportConnection<'_>, peer: &mut ShellConnection) {
    allocate_extent(transport, peer, 1);
}

pub(super) fn upload_maximum(
    owner: &mut impl Owner,
    peer: &mut ShellConnection,
    id: u64,
) -> ContentResourceLease {
    let pin = upload_sealed(
        owner,
        peer,
        ContentResourceId { id, generation: 1 },
        1024,
        1024,
        &vec![0; 4 * 1024 * 1024],
    );
    assert_eq!(pin.description().total_bytes, 4 * 1024 * 1024);
    pin
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
        .service_content_allocation_requests(&[], content_now_msec())
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
            content_now_msec(),
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
                content_now_msec()
            )
            .unwrap(),
        3
    );
    transport
        .begin_content_submission(OUTPUT, generation, content_now_msec())
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
