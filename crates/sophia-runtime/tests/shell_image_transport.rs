//! Authenticated r10 owned-ring transport. Authority is supplied by the test;
//! this is not a launch-domain or native-display acceptance verdict.
//! Deterministic in-flight cancel/failed-retire provenance is exercised by
//! owned_copy::wire_tests with worker collection withheld until cancellation.
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::owned_ring_protocol::{
    CONTROL_FILE, Event, EventKind, Identity, Offer, Registration,
};
use sophia_runtime::*;
use std::time::{Duration, Instant};
#[path = "support/image_test_authority.rs"]
mod authority;
#[path = "support/shell_image_cancellation.rs"]
mod cancellation;
#[path = "support/shell_file_peer.rs"]
mod raw;
#[path = "support/shell_image_readiness.rs"]
mod readiness;
#[path = "support/shell_image_sources.rs"]
mod sources;
const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 1,
    content_grant_epoch: 1,
};
const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};

enum Signal {
    Expire,
    VerifyCopy(std::sync::mpsc::Sender<()>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Success,
    BadHello,
    Legacy,
    MissingFd,
    Unsealed,
    WrongExtent,
    DuplicateSource,
    StaleGeneration,
    Timeout,
    Disconnect,
}

#[test]
fn configured_authenticated_epoch_publishes_discovery_and_ready_after_hello() {
    for case in [Case::Success, Case::BadHello, Case::Legacy] {
        run(case);
    }
}
#[test]
fn malformed_or_duplicate_ring_sources_are_refused() {
    for case in [
        Case::MissingFd,
        Case::Unsealed,
        Case::WrongExtent,
        Case::DuplicateSource,
    ] {
        run(case);
    }
}
#[test]
fn stale_slot_generation_is_rejected_and_source_is_released() {
    run(Case::StaleGeneration);
}
#[test]
fn registration_timeout_refunds_ring_source_budget() {
    run(Case::Timeout);
}
#[test]
fn disconnected_transport_reclaims_ring_source_custody() {
    cancellation::isolated_shutdown();
}

fn config() -> (
    ShellImageDiscovery,
    ShellImageLimits,
    ShellImageFormats,
    ShellImageRoleBounds,
) {
    (
        ShellImageDiscovery {
            connection_epoch: 1,
            channel_generation: 2,
            endpoint: "image.sock".into(),
            source_kinds: 4,
            max_offers: 4,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: 2000,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 500,
            formats_generation: 1,
            limits_generation: 1,
        },
        ShellImageLimits {
            channel_generation: 2,
            limits_generation: 1,
            max_resource_bytes: 4 << 20,
            max_sealed_backing_bytes: 16 << 20,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 4,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 256,
            max_width: 4096,
            max_height: 4096,
            validation_timeout_ms: 2000,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 1,
        },
        ShellImageFormats {
            channel_generation: 2,
            formats_generation: 1,
            limits_generation: 1,
            rows: vec![],
        },
        ShellImageRoleBounds {
            max_resource_bytes: 4 << 20,
            max_width: 4096,
            max_height: 4096,
            max_admitted_outputs: 1,
            transfer_slots: 4,
        },
    )
}

fn setup(peer: &mut raw::Peer) {
    assert_eq!(
        peer.rpc(
            100,
            &[
                65536u32.to_le_bytes().as_slice(),
                &8u16.to_le_bytes(),
                b"9P2000.L"
            ]
            .concat()
        )
        .unwrap()
        .0,
        101
    );
    assert_eq!(
        peer.rpc(
            102,
            &[
                10u32.to_le_bytes().as_slice(),
                &[0; 4],
                &u32::MAX.to_le_bytes()
            ]
            .concat()
        )
        .unwrap()
        .0,
        103
    );
    assert_eq!(
        peer.rpc(
            104,
            &[
                1u32.to_le_bytes().as_slice(),
                &10u32.to_le_bytes(),
                &[0; 4],
                &u32::MAX.to_le_bytes()
            ]
            .concat()
        )
        .unwrap()
        .0,
        105
    );
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

fn ack(peer: &mut raw::Peer, bytes: &[u8]) {
    let sequence = if let Some(event) = Event::decode(bytes) {
        event.sequence
    } else {
        decode_shell_file_record_at_revision(bytes, ShellFileClass::Event, 10)
            .unwrap()
            .header
            .sequence
    };
    assert_eq!(
        peer.write(
            4,
            &encode_shell_file_ack(ShellFileAck {
                connection_epoch: 1,
                sequence,
            })
            .unwrap()
        )
        .0,
        119
    );
}

fn sealed_upload_is_refused(peer: &mut raw::Peer) {
    let metadata = ShellImageMetadata {
        offer_id: 1,
        resource: 1,
        resource_generation: 1,
        admission_epoch: 1,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 2,
        height: 1,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 8,
        modifier: 0,
    };
    let bytes = encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageBegin,
            connection_epoch: 1,
            submission_id: 2,
            sequence: 0,
        },
        &metadata.encode().unwrap(),
        9,
    )
    .unwrap();
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &bytes).0, 119);
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: 1,
        submission_id: 2,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    let reply = peer.write(3, &submit);
    assert_eq!(reply.0, 7);
    let errno = u32::from_le_bytes(reply.1.try_into().unwrap());
    // A retired r9 packet may be rejected by r10 decoding (EINVAL), or
    // decoded for an explicit admission refusal (EACCES). Neither grants custody.
    assert!(matches!(errno, 13 | 22));
    peer.clear();
}

fn byte_upload_is_refused(peer: &mut raw::Peer) {
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let begin = encode_shell_file_resource_begin(
        ShellFileHeader {
            kind: ShellFileKind::ResourceBegin,
            connection_epoch: 1,
            submission_id: 3,
            sequence: 0,
        },
        &ShellFileResourceBegin {
            transaction: TransactionId::from_raw(77),
            slot: 0,
            record: ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
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
    let walk = [
        1u32.to_le_bytes().as_slice(),
        &11u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &6u16.to_le_bytes(),
        b"upload",
    ]
    .concat();
    assert_eq!(peer.rpc(110, &walk).unwrap().0, 7);
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &begin).0, 119);
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: 1,
        submission_id: 3,
        candidate_bytes: begin.len() as u32,
    })
    .unwrap();
    let reply = peer.write(3, &submit);
    assert_eq!(reply.0, 7);
    assert_eq!(u32::from_le_bytes(reply.1.try_into().unwrap()), 22);
    peer.clear();
    let end = encode_shell_file_resource_end(
        ShellFileHeader {
            kind: ShellFileKind::ResourceEnd,
            connection_epoch: 1,
            submission_id: 4,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(78),
            record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant,
                resource,
                chunk_count: 1,
                total_bytes: 8,
            }),
        },
    )
    .unwrap();
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &end).0, 119);
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: 1,
        submission_id: 4,
        candidate_bytes: end.len() as u32,
    })
    .unwrap();
    let reply = peer.write(3, &submit);
    assert_eq!(reply.0, 7);
    assert_eq!(u32::from_le_bytes(reply.1.try_into().unwrap()), 22);
    peer.clear();
}

fn run(case: Case) {
    let epoch = if case == Case::Legacy { 2 } else { 1 };
    let directory =
        std::env::temp_dir().join(format!("shell-ring-wire-{}-{case:?}", std::process::id()));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let (d, l, f, b) = config();
    assert!(
        transport
            .configure_image_channel(d.clone(), l, f.clone(), b)
            .is_err(),
        "legacy authority cannot enable images"
    );
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(
            &mut epochs,
            ContentLimits::prototype(ContentGrant {
                connection_epoch: epoch,
                content_grant_epoch: 1,
            }),
        )
        .unwrap();
    transport
        .begin_file_negotiation(
            &epochs,
            epoch,
            Duration::from_secs(3),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    if case != Case::Legacy {
        supply_allocation(&mut epochs);
    }
    let baseline = epochs.accounting();
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let (expire, expiration) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        setup(&mut peer);
        let revision = if case == Case::Legacy { 6 } else { 10 };
        let negotiate = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: epoch,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: revision,
                maximum_revision: revision,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | if case == Case::Legacy { 0 } else { 1 << 15 },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&negotiate, 1);
        let negotiated = peer.next_event();
        assert_eq!(
            decode_shell_file_negotiated(&negotiated)
                .unwrap()
                .welcome
                .selected_revision,
            revision
        );
        peer.ack(&negotiated);
        if case == Case::Legacy {
            let walk = [
                1u32.to_le_bytes().as_slice(),
                &6u32.to_le_bytes(),
                &1u16.to_le_bytes(),
                &13u16.to_le_bytes(),
                b"image-channel",
            ]
            .concat();
            let reply = peer.rpc(110, &walk).unwrap();
            assert_eq!(reply.0, 7);
            assert_eq!(u32::from_le_bytes(reply.1.try_into().unwrap()), 2);
            return;
        }
        peer.set_revision(10);
        peer.open(6, b"image-channel", 0);
        let discovery =
            ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
        assert_eq!(discovery.source_kinds, 4);
        assert_eq!(discovery.channel_generation, 2);
        let socket = readiness::connector(&image_path);
        let hello = ShellImagePacket::Hello {
            connection_epoch: 1,
            channel_generation: if case == Case::BadHello { 3 } else { 2 },
        }
        .encode()
        .unwrap();
        rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
        let ready = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&ready, ShellFileClass::Event, 10).unwrap();
        if case == Case::BadHello {
            assert_eq!(record.header.kind, ShellFileKind::ChannelClosed);
            assert_eq!(
                ShellChannelClosed::decode(record.body).unwrap().reason,
                ShellChannelClosedReason::ProtocolFault
            );
            ack(&mut peer, &ready);
            return;
        }
        assert_eq!(record.header.kind, ShellFileKind::ChannelReady);
        assert_eq!(
            ShellChannelReady::decode(record.body)
                .unwrap()
                .granted_source_kinds,
            4
        );
        ack(&mut peer, &ready);
        sealed_upload_is_refused(&mut peer);
        sources::run(&mut peer, &socket, case, expire);
    });
    let mut owner = ShellIsbImageOwner::new(4 << 20, 65536).unwrap();
    let mut negotiated = false;
    let mut instant = Instant::now();
    let deadline = instant + Duration::from_secs(10);
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "ring wire stalled: {case:?}");
        if !negotiated {
            negotiated = transport
                .poll_negotiation(&mut epochs, 65536)
                .unwrap()
                .is_some();
        } else {
            match transport.service_content_resources(&mut epochs, 0) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("resource service: {error}"),
            }
            if let Ok(signal) = expiration.try_recv() {
                match signal {
                    Signal::Expire => instant = owner.deadline().expect("registration deadline"),
                    Signal::VerifyCopy(done) => {
                        let lease = epochs
                            .resources(GRANT)
                            .unwrap()
                            .lease(GRANT, RESOURCE)
                            .unwrap();
                        assert_eq!(
                            lease.cpu_bytes().unwrap(),
                            &[7; 8],
                            "slot rewrite cannot change accepted pixels"
                        );
                        done.send(()).unwrap();
                    }
                }
            }
            owner
                .service(&mut transport, &mut epochs, 0, instant)
                .unwrap();
        }
        std::thread::yield_now();
    }
    peer.join().unwrap();
    assert_eq!(owner.pending(), 0);
    if case == Case::Success {
        let usage = epochs.resources(GRANT).unwrap().usage();
        assert_eq!(usage.staging, 0);
        assert_eq!(
            usage.backing, 0,
            "resource retirement returns destination storage"
        );
        assert_eq!(
            usage.retiring, 0,
            "ResourceReleased ACK settles journal custody"
        );
    }
    if case == Case::Timeout {
        let after = epochs.accounting();
        assert_eq!(
            after.reserved_backing_bytes,
            baseline.reserved_backing_bytes
        );
        assert_eq!(after.reserved_bytes, baseline.reserved_bytes);
    }
    transport.disconnect(&mut epochs).unwrap();
    owner
        .service(&mut transport, &mut epochs, 0, Instant::now())
        .unwrap();
    assert_eq!(owner.pending(), 0);
    assert_eq!(
        epochs.accounting().source_bytes,
        0,
        "disconnect returns ring source charge"
    );
    drop(owner);
    drop(transport);
    assert!(
        !directory.exists(),
        "image socket must unlink before directory cleanup"
    );
}

fn supply_allocation(epochs: &mut ContentEpochRegistry) {
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let store = epochs.allocations_mut(grant).unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    store
        .publish_outputs(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 200,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    store
        .request(
            TransactionId::from_raw(2),
            ContentAllocationRequest {
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
                desired_width: 200,
                desired_height: 20,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    store
        .grant(
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output,
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 20,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 20,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 20,
            },
            &[],
        )
        .unwrap();
    while store.take_event().is_some() {}
}
