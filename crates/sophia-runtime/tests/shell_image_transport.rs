//! Real 9P and SEQPACKET wiring with supplied afid authority. These tests do
//! not establish a launch-domain or Factotum authentication verdict.
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use std::time::{Duration, Instant};
#[path = "support/image_test_authority.rs"]
mod authority;
#[path = "support/shell_image_cancellation.rs"]
mod cancellation;
#[path = "support/shell_file_peer.rs"]
mod raw;

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
            source_kinds: 1,
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

fn ack(peer: &mut raw::Peer, event: &[u8]) {
    let record = decode_shell_file_record_at_revision(event, ShellFileClass::Event, 9).unwrap();
    let bytes = encode_shell_file_ack(ShellFileAck {
        connection_epoch: record.header.connection_epoch,
        sequence: record.header.sequence,
    })
    .unwrap();
    assert_eq!(peer.write(4, &bytes).0, 119);
}

fn image_begin(peer: &mut raw::Peer) -> (u8, Vec<u8>) {
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
    peer.write(3, &submit)
}

fn fallback(peer: &mut raw::Peer) {
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
    peer.submit_acknowledged(&begin, 3);
    let admitted = peer.next_event();
    let ShellContentRecord::ResourceStatus(status) =
        decode_shell_file_resource_status(&admitted).unwrap().record
    else {
        panic!("status");
    };
    assert_eq!(status.status, 1);
    ack(peer, &admitted);
    assert_eq!(peer.open_path(11, &[b"upload", b"0"], 1).0, 13);
    assert_eq!(peer.write(11, &[1, 2, 3, 255, 4, 5, 6, 255]).0, 119);
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
    peer.submit_acknowledged(&end, 4);
    let accepted = peer.next_event();
    let ShellContentRecord::ResourceStatus(status) =
        decode_shell_file_resource_status(&accepted).unwrap().record
    else {
        panic!("status");
    };
    assert_eq!(status.status, 2);
    ack(peer, &accepted);
}

#[test]
fn configured_authenticated_epoch_publishes_discovery_and_ready_after_hello() {
    run_image_epoch(false, false, false, 0);
    run_image_epoch(true, false, false, 0);
    // An owner may retain an earlier epoch's optional image configuration.
    // It must not prevent a later byte-only export from authenticating.
    run_image_epoch(false, true, false, 0);
    run_image_epoch(false, false, true, 0);
    run_image_epoch(false, false, true, 1);
    run_image_epoch(false, false, true, 2);
    run_image_epoch(false, false, true, 3);
}

#[test]
fn submitted_source_cancel_waits_for_real_completion_and_fences_late_source() {
    run_image_epoch(false, false, true, 4);
}

#[test]
fn submitted_source_expiry_keeps_its_cause_after_wire_cancellation() {
    run_image_epoch(false, false, true, 5);
}

fn run_image_epoch(bad_hello: bool, legacy_successor: bool, admit_source: bool, coordinated: u8) {
    let epoch = if legacy_successor { 2 } else { 1 };
    let directory = std::env::temp_dir().join(format!(
        "shell-image-wire-{}-{coordinated}",
        std::process::id()
    ));
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
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let (request_sealed, receive_sealed) = std::sync::mpsc::channel();
    let (request_admission, receive_admission) = std::sync::mpsc::channel();
    let (enabled_admission, wait_admission) = std::sync::mpsc::channel();
    let (source_submitted, await_submission) = std::sync::mpsc::channel();
    let (allow_completion, collect_completion) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        setup(&mut peer);
        let negotiate = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: epoch,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: if legacy_successor { 6 } else { 9 },
                maximum_revision: if legacy_successor { 6 } else { 9 },
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | if legacy_successor { 0 } else { 1 << 13 },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&negotiate, 1);
        let event = peer.next_event();
        assert_eq!(
            decode_shell_file_negotiated(&event)
                .unwrap()
                .welcome
                .selected_revision,
            if legacy_successor { 6 } else { 9 }
        );
        peer.ack(&event);
        if legacy_successor {
            let walk = peer
                .rpc(
                    110,
                    &[
                        1u32.to_le_bytes().as_slice(),
                        &6u32.to_le_bytes(),
                        &1u16.to_le_bytes(),
                        &13u16.to_le_bytes(),
                        b"image-channel",
                    ]
                    .concat(),
                )
                .unwrap();
            assert_eq!(walk.0, 7);
            assert_eq!(u32::from_le_bytes(walk.1.try_into().unwrap()), 2);
            return;
        }
        peer.open(6, b"image-channel", 0);
        let discovery =
            ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
        assert_eq!(discovery.channel_generation, 2);
        let early = image_begin(&mut peer);
        assert_eq!(early.0, 7);
        assert_eq!(u32::from_le_bytes(early.1.try_into().unwrap()), 16);
        peer.clear();
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::SEQPACKET,
            rustix::net::SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::connect(
            &socket,
            &rustix::net::SocketAddrUnix::new(&image_path).unwrap(),
        )
        .unwrap();
        let hello = ShellImagePacket::Hello {
            connection_epoch: 1,
            channel_generation: if bad_hello { 3 } else { 2 },
        }
        .encode()
        .unwrap();
        rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
        let ready = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&ready, ShellFileClass::Event, 9).unwrap();
        if bad_hello {
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
            1
        );
        ack(&mut peer, &ready);
        if admit_source {
            request_admission.send(()).unwrap();
            wait_admission.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert_eq!(image_begin(&mut peer).0, 119);
        let submitted = peer.next_event();
        assert_eq!(
            decode_shell_file_submitted_at_revision(&submitted, 9)
                .unwrap()
                .candidate_kind,
            ShellFileKind::ImageBegin
        );
        ack(&mut peer, &submitted);
        peer.clear();
        if admit_source {
            if matches!(coordinated, 4 | 5) {
                cancellation::submitted_source(
                    &mut peer,
                    &socket,
                    await_submission,
                    allow_completion,
                    coordinated == 5,
                );
            } else if coordinated == 3 {
                let admitted = peer.next_event();
                let record =
                    decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9)
                        .unwrap();
                assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
                ack(&mut peer, &admitted);
            } else if coordinated == 2 {
                cancellation::cancelled_source(&mut peer);
            } else {
                accepted_source(&mut peer, &socket);
            }
            return;
        }
        let refused = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status_at_revision(&refused, 9)
                .unwrap()
                .record
        else {
            panic!("status");
        };
        assert_eq!((status.status, status.reason), (3, 13));
        ack(&mut peer, &refused);
        fallback(&mut peer);
        // The owner supplies a validated mapping here. This isolates the
        // live-journal release handoff; it does not claim Source admission.
        request_sealed.send(()).unwrap();
        let accepted = peer.next_event();
        let ShellContentRecord::ResourceStatus(status) =
            decode_shell_file_resource_status(&accepted).unwrap().record
        else {
            panic!("status")
        };
        assert_eq!((status.resource.id, status.status), (99, 2));
        ack(&mut peer, &accepted);
        let retire = encode_shell_file_resource_retire(
            ShellFileHeader {
                kind: ShellFileKind::ResourceRetire,
                connection_epoch: 1,
                submission_id: 5,
                sequence: 0,
            },
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(5),
                record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant: ContentGrant {
                        connection_epoch: 1,
                        content_grant_epoch: 1,
                    },
                    resource: ContentResourceId {
                        id: 99,
                        generation: 1,
                    },
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&retire, 5);
        let released = peer.next_event();
        let record = decode_shell_file_resource_released(&released).unwrap();
        assert!(
            matches!(record.record, ShellContentRecord::ResourceReleased(value) if value.resource.id == 99)
        );
        ack(&mut peer, &released);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut negotiated = false;
    let mut jobs = SealedImageJobs::default();
    let mut validation = SealedValidationWorker::new(4, 4 << 20).unwrap();
    let mut stored = false;
    let mut pending_release = None;
    let mut source_submitted = Some(source_submitted);
    let mut collect = !matches!(coordinated, 4 | 5);
    let mut image_owner = ShellSealedImageOwner::new(4 << 20, 65536).unwrap();
    while !worker.is_finished() {
        assert!(Instant::now() < deadline, "image wire stalled");
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
            if let Some(settlement) = pending_release.as_ref() {
                match transport.release_sealed_image_source(settlement) {
                    Ok(_) => pending_release = None,
                    Err(ShellTransportError::ContentQueueSaturated) => {}
                    Err(error) => panic!("release: {error}"),
                }
            }
            if receive_sealed.try_recv().is_ok() {
                supply_sealed(&mut epochs);
            }
            if receive_admission.try_recv().is_ok() {
                if (1..=3).contains(&coordinated) {
                    supply_allocation(&mut epochs);
                    image_owner
                        .service(&mut transport, &mut epochs, 0, Instant::now())
                        .unwrap();
                } else {
                    transport.enable_sealed_image_admission().unwrap();
                }
                enabled_admission.send(()).unwrap();
            }
            if (1..=3).contains(&coordinated) {
                image_owner
                    .service(&mut transport, &mut epochs, 0, Instant::now())
                    .unwrap();
                if let Some(grant) = transport.content_grant()
                    && let Some(lease) = epochs.resources(grant).and_then(|store| {
                        store
                            .lease(
                                grant,
                                ContentResourceId {
                                    id: 1,
                                    generation: 1,
                                },
                            )
                            .ok()
                    })
                {
                    assert_eq!(lease.bytes(), &[0; 8]);
                    stored = true;
                }
                std::thread::yield_now();
                continue;
            }
            if transport.pending_sealed_image_offer().is_some() {
                // A coherent packet is not an allocation grant. Failure after
                // journal preparation must return both terminal holds and
                // leave the same offer retryable, without ImageAdmitted.
                assert!(
                    transport
                        .admit_pending_sealed_image(
                            &mut epochs,
                            &mut jobs,
                            &mut validation,
                            0,
                            Instant::now()
                        )
                        .is_err()
                );
                assert_eq!(jobs.pending(), 0);
                assert_eq!(validation.pending(), 0);
                supply_allocation(&mut epochs);
                transport
                    .admit_pending_sealed_image(
                        &mut epochs,
                        &mut jobs,
                        &mut validation,
                        0,
                        Instant::now(),
                    )
                    .unwrap();
            }
            jobs.observe_store_stops(&mut epochs, &mut validation, 0)
                .unwrap();
            if let Some((job, source)) = transport.take_sealed_image_source() {
                assert!(
                    transport.take_sealed_image_source().is_none(),
                    "Source is take-once"
                );
                jobs.submit(&mut validation, job, source).unwrap();
                if coordinated == 5 {
                    // Advance only the supplied owner clock to the admission
                    // deadline. Collection remains gated, even if scanning won.
                    let expiry = jobs.deadline().unwrap();
                    jobs.expire(&mut epochs, &mut validation, expiry, 0)
                        .unwrap();
                    assert!(jobs.deadline().is_none());
                }
                if matches!(coordinated, 4 | 5) {
                    source_submitted.take().unwrap().send(()).unwrap();
                }
            }
            if !collect {
                collect = collect_completion.try_recv().is_ok();
                if !collect {
                    assert!(jobs.reconcile(&mut epochs, 0).is_none());
                    std::thread::yield_now();
                    continue;
                }
            }
            while let Some(completion) = validation.take_completed().unwrap() {
                assert!(jobs.receive(completion).is_ok());
            }
            while let Some(settlement) = jobs.reconcile(&mut epochs, 0) {
                if matches!(coordinated, 4 | 5) {
                    assert!(settlement.outcome.is_err());
                    assert_eq!(
                        settlement.stopped,
                        Some(if coordinated == 5 {
                            ContentReason::Timeout
                        } else {
                            ContentReason::Cancelled
                        })
                    );
                    assert_eq!(validation.pending(), 0);
                    assert!(pending_release.replace(settlement).is_none());
                    continue;
                }
                assert_eq!(settlement.outcome, Ok(()));
                let lease = epochs
                    .resources(settlement.admission.grant)
                    .unwrap()
                    .lease(
                        settlement.admission.grant,
                        ContentResourceId {
                            id: 1,
                            generation: 1,
                        },
                    )
                    .unwrap();
                assert_eq!(lease.bytes(), &[0; 8]);
                stored = true;
                assert!(
                    matches!(
                        transport.release_sealed_image_source(&settlement),
                        Err(ShellTransportError::ContentQueueSaturated)
                    ),
                    "release must wait for the queued status to reach the journal"
                );
                assert!(pending_release.replace(settlement).is_none());
            }
        }
        std::thread::yield_now();
    }
    worker.join().unwrap();
    if coordinated == 3 {
        transport.disconnect(&mut epochs).unwrap();
        image_owner
            .service(&mut transport, &mut epochs, 0, Instant::now())
            .unwrap();
        assert_eq!(image_owner.pending(), 0);
        assert_eq!(epochs.accounting().retired_epochs, 0);
        return;
    }
    assert_eq!(stored, admit_source && !matches!(coordinated, 2 | 4 | 5));
    assert_eq!(jobs.pending(), 0);
    assert!(pending_release.is_none());
    assert_eq!(image_owner.pending(), 0);
    if !bad_hello && !legacy_successor {
        let grant = ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        let usage = epochs.resources(grant).unwrap().usage();
        assert_eq!(
            usage.backing,
            if admit_source { 0 } else { 8 },
            "retired sealed backing is released; only byte fallback remains"
        );
        assert_eq!(usage.retiring, 0, "journal custody released sealed backing");
    }
    drop(transport);
    assert!(
        !directory.exists(),
        "image listener must unlink before role directory cleanup"
    );
}

fn supply_sealed(epochs: &mut ContentEpochRegistry) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use std::io::Write;
    let mut file = std::fs::File::from(
        memfd_create(
            "wire-sealed-release",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(&[0; 8]).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let mapping = sophia_linux_peer::SealedMapping::map(file.into(), 8).unwrap();
    let mut validation = SealedPixelValidation::new(mapping).unwrap();
    assert!(validation.step().unwrap());
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let resource = ContentResourceId {
        id: 99,
        generation: 1,
    };
    let store = epochs.resources_mut(grant).unwrap();
    store
        .begin_sealed(
            TransactionId::from_raw(99),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 2,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 8,
            },
            0,
        )
        .unwrap();
    store
        .complete_sealed(
            grant,
            resource,
            &mut Some(Ok(validation.finish().unwrap())),
            0,
        )
        .unwrap();
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

fn accepted_source(peer: &mut raw::Peer, socket: &std::os::fd::OwnedFd) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    let admitted = peer.next_event();
    let record = decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(record.body).unwrap();
    assert_eq!(metadata.offer_id, 1);
    ack(peer, &admitted);
    let source = std::fs::File::from(
        memfd_create(
            c"live-sealed-source",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    source.set_len(8).unwrap();
    fcntl_add_seals(
        &source,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let mut planes = [ShellImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    }; 4];
    planes[0].stride = 8;
    let bytes = ShellImagePacket::Source {
        connection_epoch: 1,
        channel_generation: 2,
        metadata,
        planes,
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    drop(source);
    let (mut status, mut released) = (false, false);
    for _ in 0..2 {
        let event = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 9).unwrap();
        match record.header.kind {
            ShellFileKind::ResourceStatus => {
                assert!(!status);
                let value = decode_shell_file_resource_status(&event).unwrap();
                assert!(
                    matches!(value.record, ShellContentRecord::ResourceStatus(value) if value.status == 2 && value.resource.id == 1)
                );
                status = true;
            }
            ShellFileKind::SourceReleased => {
                assert!(!released);
                assert!(status, "terminal status must precede SourceReleased");
                assert_eq!(
                    ShellSourceReleased::decode(record.body)
                        .unwrap()
                        .disposition,
                    ShellSourceDisposition::Retained
                );
                released = true;
            }
            _ => panic!("unexpected image event"),
        }
        ack(peer, &event);
    }
    assert!(status && released);
    // An expired/completed offer is a silent drain, even with a new source FD.
    // The following ordinary Retire must see no spurious ChannelClosed first.
    let late = std::fs::File::open("/dev/null").unwrap();
    let rights = [late.as_fd()];
    let mut control = SendAncillaryBuffer::new(&mut space);
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    let retire = encode_shell_file_resource_retire(
        ShellFileHeader {
            kind: ShellFileKind::ResourceRetire,
            connection_epoch: 1,
            submission_id: 3,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(3),
            record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: ContentGrant {
                    connection_epoch: 1,
                    content_grant_epoch: 1,
                },
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
            }),
        },
    )
    .unwrap();
    peer.submit_acknowledged(&retire, 3);
    let event = peer.next_event();
    assert!(
        matches!(decode_shell_file_resource_released(&event).unwrap().record,
        ShellContentRecord::ResourceReleased(value) if value.resource.id == 1)
    );
    ack(peer, &event);
    // Even a stale offer must pass descriptor shape first. Extra rights cannot
    // hide behind the silent-drain rule, and both received references close.
    let rights = [late.as_fd(), late.as_fd()];
    let mut larger = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = SendAncillaryBuffer::new(&mut larger);
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    let closed = peer.next_event();
    let record = decode_shell_file_record_at_revision(&closed, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ChannelClosed);
    assert_eq!(
        ShellChannelClosed::decode(record.body).unwrap().reason,
        ShellChannelClosedReason::ProtocolFault
    );
    ack(peer, &closed);
}
