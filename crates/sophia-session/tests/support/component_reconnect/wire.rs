use super::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

pub(super) fn read(peer: &mut UnixStream) -> Vec<u8> {
    let mut frame = vec![0; SOPHIA_IPC_HEADER_LEN];
    peer.read_exact(&mut frame).unwrap();
    let length = u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize;
    assert!(length <= 65536);
    frame.resize(SOPHIA_IPC_HEADER_LEN + length, 0);
    peer.read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}

pub(super) fn send(peer: &mut UnixStream, record: ShellContentRecord) {
    peer.write_all(&encode_shell_content_frame(TransactionId::from_raw(10), &record).unwrap())
        .unwrap();
}

pub(super) fn connect(
    owner: &mut ShellComponentSession,
    slot: usize,
) -> (ComponentConnectionKey, UnixStream) {
    let (key, mut peer) = owner.processes.reconnect_fixture_peer(slot, owner.policy);
    let native = slot == 1;
    peer.write_all(&hello(native)).unwrap();
    let visit = owner.poll(65536).unwrap();
    assert_eq!(
        visit
            .negotiations
            .into_iter()
            .flatten()
            .next()
            .unwrap()
            .1
            .unwrap()
            .connection_epoch,
        key.grant.connection_epoch
    );
    read(&mut peer);
    read(&mut peer);
    assert!(
        owner
            .connected_roles()
            .into_iter()
            .flatten()
            .any(|(current, _)| current == key)
    );
    (key, peer)
}

pub(super) fn hello(native: bool) -> Vec<u8> {
    let capabilities = SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
        | if native {
            SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
        } else {
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
        };
    encode_shell_v1_client_hello_frame(ShellV1ClientHello {
        minimum_revision: if native { 7 } else { 6 },
        maximum_revision: if native { 7 } else { 6 },
        required_capabilities: capabilities,
    })
    .unwrap()
}

pub(super) fn upload(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut UnixStream,
) -> ContentResourceLease {
    upload_id(transport, peer, RESOURCE)
}

pub(super) fn upload_id(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut UnixStream,
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
        send(peer, record);
    }
    transport.service_content_resources(0).unwrap();
    transport.poll_io().unwrap();
    for status in [1, 2] {
        let (_, ShellContentRecord::ResourceStatus(value)) =
            decode_shell_content_frame(&read(peer)).unwrap()
        else {
            panic!("resource status");
        };
        assert_eq!(value.status, status);
        assert_eq!(value.grant, grant);
    }
    transport.lease_content_resource(grant, resource).unwrap()
}

pub(super) fn allocate(transport: &mut ShellTransportConnection<'_>, peer: &mut UnixStream) {
    allocate_extent(transport, peer, 1);
}

pub(super) fn allocate_extent(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut UnixStream,
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
    read(peer);
    read(peer);
}

pub(super) fn candidate(
    transport: &mut ShellTransportConnection<'_>,
    peer: &mut UnixStream,
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
    read(peer);
    for record in [
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
    ] {
        send(peer, record);
    }
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
    peer: &mut UnixStream,
    generation: u64,
    kind: u16,
) {
    transport.poll_io().unwrap();
    let (_, ShellContentRecord::CandidateOutcome(value)) =
        decode_shell_content_frame(&read(peer)).unwrap()
    else {
        panic!("candidate outcome");
    };
    assert_eq!(
        (value.grant, value.candidate_generation, value.kind),
        (transport.content_grant().unwrap(), generation, kind)
    );
}
