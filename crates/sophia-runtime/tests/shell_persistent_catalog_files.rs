//! Dock (r8 persistent catalog) over the file wire: the root vocabulary,
//! the `Catalog` object with r8 identities (pinned, `EBUSY` for a second
//! pin, a fresh qid on republish), a whole `CatalogCandidate` through
//! prepared/presented, and a `CatalogActivate` to its outcome.
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

#[allow(dead_code)] // Shared store fixture; this file drives only part of it.
#[path = "support/native_launcher_content.rs"]
mod fixtures;
use fixtures::{ALLOCATION, GRANT, OUTPUT, RESOURCE, allocation, facts, limits, tx};

const MIB: u64 = 1024 * 1024;
const RLERROR: u8 = 7;
const ENOENT: u32 = 2;
const EBUSY: u32 = 16;

const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
    | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}

fn dock_transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-dock-files-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    (transport, directory)
}

fn header(kind: ShellFileKind, epoch: u64, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}

fn errno(reply: (u8, Vec<u8>)) -> u32 {
    assert_eq!(reply.0, RLERROR);
    u32::from_le_bytes(reply.1[..4].try_into().unwrap())
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
    peer_done: &std::thread::JoinHandle<()>,
) -> ShellV1ServerWelcome {
    transport
        .begin_file_negotiation(registry, epoch, Duration::from_secs(2), granted())
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(welcome) = transport.poll_negotiation(registry, 64 * 1024).unwrap() {
            return welcome;
        }
        assert!(!peer_done.is_finished(), "peer ended before negotiation");
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
}

fn dock_catalog(generation: u64) -> ShellApplicationCatalog {
    ShellApplicationCatalog {
        connection_epoch: GRANT.connection_epoch,
        generation,
        entries: [1, 2]
            .into_iter()
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: true,
                label: format!("tile{slot}"),
                keywords: String::new(),
            })
            .collect(),
    }
}

fn persistent_catalog(generation: u64) -> ShellPersistentCatalog {
    let catalog = dock_catalog(generation);
    let identities = catalog
        .entries
        .iter()
        .map(|entry| (entry.slot, format!("registered:{}", entry.slot)))
        .collect();
    ShellPersistentCatalog {
        catalog,
        identities,
    }
}

fn candidate_begin(generation: u64) -> CatalogCandidateBegin {
    CatalogCandidateBegin {
        catalog_generation: generation,
        content: ContentCandidateBegin {
            grant: GRANT,
            candidate_generation: 1,
            output: OUTPUT,
            facts_generation: 5,
            pacing_permit: 1,
            interaction_generation: 4,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        },
    }
}

fn candidate_chunk() -> ContentCandidateChunk {
    ContentCandidateChunk {
        grant: GRANT,
        candidate_generation: 1,
        chunk_ordinal: 0,
        surfaces: vec![ContentSurface {
            allocation: ALLOCATION,
            scale_generation: 4,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 0,
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
    }
}

fn dock_allocation() -> ContentAllocationSnapshot {
    let mut value = allocation();
    value.native_opening = None;
    value.role = 1;
    value
}

#[test]
fn catalog_object_candidate_and_activate_cross_the_file_wire() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = dock_transport();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
        .reserve_content_with_profile(
            &mut registry,
            limits(),
            ContentStoreProfile::PersistentCatalog,
        )
        .unwrap();
    let socket = transport.socket_path().to_owned();

    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();

        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, GRANT.connection_epoch, 1),
            ShellV1ClientHello {
                minimum_revision: 8,
                maximum_revision: 8,
                required_capabilities: CAPS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(value.welcome.selected_revision, 8);
        assert_eq!(value.welcome.capabilities, CAPS);
        peer.ack(&negotiated);
        peer.open(6, b"limits", 0);
        assert_eq!(
            decode_shell_file_limits(&peer.read(6, 0)).unwrap(),
            limits()
        );

        // Root vocabulary: `indicators` does not exist at all for the dock.
        let indicators_walk = [
            1u32.to_le_bytes().as_slice(),
            &21u32.to_le_bytes(),
            &1u16.to_le_bytes(),
            &10u16.to_le_bytes(),
            b"indicators".as_slice(),
        ]
        .concat();
        assert_eq!(errno(peer.rpc(110, &indicators_walk).unwrap()), ENOENT);

        // The catalog object, with r8 identities, pinned by an open fid.
        let published = peer.next_event();
        let first = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(
            (first.object, first.generation),
            (ShellFileKind::Catalog, 8)
        );
        peer.ack(&published);
        peer.open(7, b"catalog", 0);
        let pinned = peer.read(7, 0);
        let value = decode_shell_file_catalog(&pinned).unwrap();
        assert_eq!(value.catalog.identities.len(), 2);
        assert_eq!(value.catalog.identities[&1], format!("registered:{}", 1));

        // The dock's output facts, published right after the catalog; not
        // otherwise exercised by this test, but drained so it never sits
        // ahead of a later expected event.
        let outputs_published = peer.next_event();
        assert_eq!(
            decode_shell_file_object_published(&outputs_published)
                .unwrap()
                .object,
            ShellFileKind::Outputs
        );
        peer.ack(&outputs_published);

        // One pin per feed per attach.
        peer.walk(8, b"catalog");
        let second = peer
            .rpc(12, &[8u32.to_le_bytes(), 0u32.to_le_bytes()].concat())
            .unwrap();
        assert_eq!(errno(second), EBUSY);

        // Allocation for the candidate's one surface.
        let request = encode_shell_file_allocation_request(
            header(ShellFileKind::AllocationRequest, GRANT.connection_epoch, 2),
            ShellFileTransactionRecord {
                transaction: tx(20),
                record: ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant: GRANT,
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
                    desired_height: 32,
                    margins: ContentMargins::default(),
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&request, 2);
        let event = peer.next_event();
        let value = decode_shell_file_allocation_result(&event).unwrap();
        let ShellContentRecord::AllocationResult(result) = value.record else {
            panic!("allocation result");
        };
        assert_eq!(result.status, 1);
        peer.ack(&event);

        // Resource upload for the candidate's one placement.
        let begin_resource = encode_shell_file_resource_begin(
            header(ShellFileKind::ResourceBegin, GRANT.connection_epoch, 3),
            &ShellFileResourceBegin {
                transaction: tx(21),
                slot: 0,
                record: ShellContentRecord::ResourceBegin(ContentResourceBegin {
                    grant: GRANT,
                    resource: RESOURCE,
                    width_px: 2,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 8,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&begin_resource, 3);
        let admitted = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status(&admitted).unwrap().record
        else {
            panic!("resource status");
        };
        assert_eq!(status.status, 1);
        peer.ack(&admitted);
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);
        assert_eq!(
            peer.write_at(10, 0, &[0, 0, 255, 255, 0, 128, 0, 128]).0,
            119
        );
        let end_resource = encode_shell_file_resource_end(
            header(ShellFileKind::ResourceEnd, GRANT.connection_epoch, 4),
            &ShellFileTransactionRecord {
                transaction: tx(22),
                record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant: GRANT,
                    resource: RESOURCE,
                    total_bytes: 8,
                    chunk_count: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&end_resource, 4);
        let accepted = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status(&accepted).unwrap().record
        else {
            panic!("resource status");
        };
        assert_eq!(status.status, 2);
        peer.ack(&accepted);

        // Pacing.
        let demand = encode_shell_file_transaction(
            header(ShellFileKind::FrameDemand, GRANT.connection_epoch, 5),
            &ShellFileTransactionRecord {
                transaction: tx(23),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: GRANT,
                    output: OUTPUT,
                    allocation: ALLOCATION,
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 5);
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(permit) = value.record else {
            panic!("frame permit");
        };
        peer.ack(&event);

        // The catalog republishes at a fresh generation once the demand is
        // granted; the pinned fid still reads the original (generation 8)
        // bytes, and the announcement carries a fresh qid.
        let republished = peer.next_event();
        let next = decode_shell_file_object_published(&republished).unwrap();
        assert_eq!(next.generation, 9);
        assert_ne!(next.qid, first.qid);
        peer.ack(&republished);
        assert_eq!(peer.read(7, 0), pinned);

        // The whole catalog candidate, naming the granted permit and the
        // republished catalog generation (9).
        let mut begin = candidate_begin(9);
        begin.content.pacing_permit = permit.permit_id;
        let candidate_bytes = encode_shell_file_catalog_candidate(
            header(ShellFileKind::CatalogCandidate, GRANT.connection_epoch, 6),
            &ShellFileCatalogCandidate {
                transaction: tx(25),
                candidate: CatalogContentCandidate {
                    candidate: ContentCandidate {
                        grant: begin.content.grant,
                        candidate_generation: begin.content.candidate_generation,
                        output: begin.content.output,
                        facts_generation: begin.content.facts_generation,
                        pacing_permit: begin.content.pacing_permit,
                        interaction_generation: begin.content.interaction_generation,
                        surfaces: candidate_chunk().surfaces,
                        placements: candidate_chunk().placements,
                        targets: vec![],
                    },
                    catalog_generation: begin.catalog_generation,
                },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&candidate_bytes, 6);

        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            let event = peer.next_event();
            let value =
                decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
            let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
                panic!("candidate outcome");
            };
            outcomes.push(outcome.kind);
            peer.ack(&event);
        }
        assert_eq!(outcomes, vec![1, 2]);

        // A pointer activation of one catalog tile, then its outcome.
        let activate = encode_shell_file_catalog_action(
            header(ShellFileKind::CatalogActivate, GRANT.connection_epoch, 7),
            &ShellFileCatalogActionRecord {
                transaction: tx(26),
                record: ShellCatalogActionRecord::Activate(CatalogActivation {
                    action: ContentAction {
                        grant: GRANT,
                        output: OUTPUT,
                        candidate_generation: 1,
                        presentation_epoch: 9,
                        interaction_generation: 4,
                        allocation: ALLOCATION,
                        target_id: 1,
                        target_generation: 1,
                        action_id: 1,
                        event_id: 30,
                        kind: 1,
                        reason: ContentReason::None as u16,
                    },
                    catalog_generation: 9,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&activate, 7);
        let event = peer.next_event();
        let value =
            decode_shell_file_catalog_action(&event, ShellFileKind::CatalogActivationOutcome)
                .unwrap();
        let ShellCatalogActionRecord::ActivationOutcome(outcome) = value.record else {
            panic!("activation outcome");
        };
        assert_eq!(outcome.status, 1);
        peer.ack(&event);
    });

    let start = Instant::now();
    let welcome = negotiate(&mut transport, &mut registry, GRANT.connection_epoch, &peer);
    assert_eq!(welcome.capabilities, CAPS);
    assert!(
        transport
            .connection(&mut registry)
            .supports_persistent_catalog()
    );

    transport
        .publish_catalog(&registry, tx(1), &persistent_catalog(8))
        .unwrap();
    transport
        .publish_content_output_facts(&mut registry, tx(10), 5, vec![facts()])
        .unwrap();
    while transport
        .next_content_allocation_request(&registry)
        .is_none()
    {
        transport
            .service_content_allocation_requests(&mut registry, &[], 0)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    transport
        .grant_content_allocation(&mut registry, 1, dock_allocation(), &[])
        .unwrap();

    // The resource upload and the pacing demand share this wait: the peer
    // submits the resource fully before it ever submits the demand.
    while transport.next_content_demand(&registry).is_none() {
        transport
            .service_content_resources(&mut registry, 0)
            .unwrap();
        transport
            .service_content_demands(&mut registry, &[OUTPUT], &[dock_allocation()])
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    transport
        .grant_content_demand(&mut registry, tx(24), OUTPUT, 1, 0)
        .unwrap();

    // Republish the catalog at a fresh generation (the pin from the first
    // object stays valid throughout), then drive the whole candidate through
    // the shared dock candidate servicer.
    transport
        .publish_catalog(&registry, tx(2), &persistent_catalog(9))
        .unwrap();
    let allocations = [dock_allocation()];
    let context = ContentCandidateContext {
        output: OUTPUT,
        facts_generation: 5,
        interaction_generation: 4,
        allocations: &allocations,
    };
    let mut processed = 0;
    while processed < 3 {
        processed += transport
            .connection(&mut registry)
            .service_catalog_candidates(&[context], &dock_catalog(9), 0)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let render = transport
        .connection(&mut registry)
        .begin_catalog_submission(1, context, &dock_catalog(9), 0)
        .unwrap();
    assert_eq!(render.resource(RESOURCE).unwrap().bytes().len(), 8);
    transport
        .content_prepared(&mut registry, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    transport
        .content_presented(&mut registry, GRANT, OUTPUT, 1, 9, 1, 1)
        .unwrap();
    drop(render);

    let (activation_transaction, activation);
    loop {
        if let Some((transaction, value)) = transport
            .connection(&mut registry)
            .poll_catalog_activation()
            .unwrap()
        {
            activation_transaction = transaction;
            activation = value;
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    transport
        .connection(&mut registry)
        .finish_catalog_activation(activation_transaction, &activation, 1)
        .unwrap();

    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}
