//! Authenticated r10 DMA-BUF intake beside the owned ring. Sources are
//! supplied memfds, so this pins admission, custody and journal order only:
//! no device import, GPU copy or source-read completion is exercised.
#![cfg(feature = "experimental-shell-capture")]
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};
#[path = "support/image_test_authority.rs"]
mod authority;
#[path = "support/shell_file_peer.rs"]
mod raw;

const DMA: u64 = 1 << 14;
const OWNED: u64 = 1 << 15;
const WIDTH: u32 = 200;
const HEIGHT: u32 = 20;
const STRIDE: u32 = WIDTH * 4 + 64;
const BOUND: u64 = (WIDTH * HEIGHT * 4) as u64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    /// Refuse a nonlinear offer, admit a LINEAR one, settle before use.
    Settled,
    /// A descriptor smaller than its LINEAR span is a protocol fault.
    ShortSource,
    /// A CPU-only client on a DMA-capable export is granted the ring alone.
    CpuOnly,
}

#[test]
fn linear_offer_is_admitted_received_and_released_in_journal_order() {
    run(Case::Settled);
}

#[test]
fn short_dma_source_closes_the_channel_and_retains_settlement() {
    run(Case::ShortSource);
}

#[test]
fn cpu_only_client_is_granted_only_the_owned_ring() {
    run(Case::CpuOnly);
}

#[test]
fn incomplete_dma_configuration_is_refused() {
    let directory =
        std::env::temp_dir().join(format!("shell-capture-config-{}", std::process::id()));
    let mut transport = authorized(&directory);
    let (d, mut l, f, b) = config();
    l.max_capture_jobs = 0;
    assert!(
        transport
            .configure_image_channel(d.clone(), l, f.clone(), b)
            .is_err()
    );
    let (_, l, mut f, _) = config();
    f.rows.clear();
    assert!(transport.configure_image_channel(d, l, f, b).is_err());
}

fn authorized(directory: &std::path::Path) -> ShellComponentTransport {
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
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
    transport
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
            source_kinds: 6,
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
            max_dma_source_bytes: 1 << 20,
            max_renderer_allocation_bytes: 1 << 20,
            max_renderer_entries: 2,
            max_offers: 4,
            max_validation_jobs: 4,
            max_capture_jobs: 2,
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
            rows: vec![ShellImageFormatRow {
                output: 1,
                device_major: 226,
                device_minor: 128,
                fourcc: DRM_FORMAT_XRGB8888,
                flags: SHELL_IMAGE_FORMAT_FLAG_OPAQUE,
                modifier: 0,
                max_width: 4096,
                max_height: 4096,
            }],
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

fn metadata(offer_id: u64, modifier: u64) -> ShellImageMetadata {
    ShellImageMetadata {
        offer_id,
        resource: offer_id,
        resource_generation: 1,
        admission_epoch: 1,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: SHELL_IMAGE_SOURCE_DMA_BUF,
        source_format: DRM_FORMAT_XRGB8888,
        width: WIDTH,
        height: HEIGHT,
        stride: 0,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: BOUND,
        modifier,
    }
}

fn connector(image_path: &std::path::Path) -> std::os::fd::OwnedFd {
    let socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::connect(
        &socket,
        &rustix::net::SocketAddrUnix::new(image_path).unwrap(),
    )
    .unwrap();
    socket
}

fn event_kind(peer: &mut raw::Peer) -> (ShellFileKind, Vec<u8>) {
    let bytes = peer.next_event();
    let record = decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
    let kind = record.header.kind;
    let body = record.body.to_vec();
    peer.ack(&bytes);
    (kind, body)
}

fn begin(peer: &mut raw::Peer, submission: u64, metadata: ShellImageMetadata) {
    let bytes = encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageBegin,
            connection_epoch: 1,
            submission_id: submission,
            sequence: 0,
        },
        &metadata.encode().unwrap(),
        10,
    )
    .unwrap();
    peer.submit_acknowledged(&bytes, submission);
}

fn send_source(socket: &impl AsFd, metadata: ShellImageMetadata, bytes: u64) {
    let source = rustix::fs::memfd_create("dma-stand-in", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&source, bytes).unwrap();
    let zero = ShellImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    };
    let packet = ShellImagePacket::Source {
        connection_epoch: 1,
        channel_generation: 2,
        metadata,
        planes: [
            ShellImagePlane {
                offset: 0,
                stride: STRIDE,
                fd_index: 0,
            },
            zero,
            zero,
            zero,
        ],
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)));
    rustix::net::sendmsg(
        socket,
        &[std::io::IoSlice::new(&packet)],
        &mut control,
        rustix::net::SendFlags::NOSIGNAL,
    )
    .unwrap();
}

/// Version, authenticated attach through afid 10, then the event fids.
fn setup(peer: &mut raw::Peer) {
    let version = [
        65536u32.to_le_bytes().as_slice(),
        &8u16.to_le_bytes(),
        b"9P2000.L",
    ]
    .concat();
    assert_eq!(peer.rpc(100, &version).unwrap().0, 101);
    let auth = [
        10u32.to_le_bytes().as_slice(),
        &[0; 4],
        &u32::MAX.to_le_bytes(),
    ]
    .concat();
    assert_eq!(peer.rpc(102, &auth).unwrap().0, 103);
    let attach = [
        1u32.to_le_bytes().as_slice(),
        &10u32.to_le_bytes(),
        &[0; 4],
        &u32::MAX.to_le_bytes(),
    ]
    .concat();
    assert_eq!(peer.rpc(104, &attach).unwrap().0, 105);
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

fn peer_script(peer: &mut raw::Peer, image_path: &std::path::Path, case: Case) {
    setup(peer);
    let required = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | OWNED
        | if case == Case::CpuOnly { 0 } else { DMA };
    let negotiate = encode_shell_file_negotiate(
        ShellFileHeader {
            kind: ShellFileKind::Negotiate,
            connection_epoch: 1,
            submission_id: 1,
            sequence: 0,
        },
        ShellV1ClientHello {
            minimum_revision: 10,
            maximum_revision: 10,
            required_capabilities: required,
        },
    )
    .unwrap();
    peer.submit_acknowledged(&negotiate, 1);
    let negotiated = peer.next_event();
    let welcome = decode_shell_file_negotiated(&negotiated).unwrap().welcome;
    assert_eq!(welcome.selected_revision, 10);
    assert_eq!(welcome.capabilities & DMA != 0, case != Case::CpuOnly);
    peer.ack(&negotiated);
    peer.set_revision(10);
    peer.open(6, b"image-channel", 0);
    let discovery =
        ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
    assert_eq!(discovery.source_kinds, 6);
    let socket = connector(image_path);
    let hello = ShellImagePacket::Hello {
        connection_epoch: 1,
        channel_generation: 2,
    }
    .encode()
    .unwrap();
    rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
    let (kind, body) = event_kind(peer);
    assert_eq!(kind, ShellFileKind::ChannelReady);
    let granted = ShellChannelReady::decode(&body)
        .unwrap()
        .granted_source_kinds;
    if case == Case::CpuOnly {
        assert_eq!(granted, 4, "CPU clients keep exactly the ring");
        return;
    }
    assert_eq!(granted, 6);
    // Nonlinear sizes are never guessed: refused before ImageAdmitted.
    begin(peer, 2, metadata(1, 1));
    let (kind, body) = event_kind(peer);
    assert_eq!(kind, ShellFileKind::ResourceStatus, "{body:?}");
    let admitted = metadata(2, 0);
    begin(peer, 3, admitted);
    let (kind, body) = event_kind(peer);
    assert_eq!(kind, ShellFileKind::ImageAdmitted);
    assert_eq!(ShellImageMetadata::decode(&body).unwrap(), admitted);
    let span = u64::from(STRIDE) * u64::from(HEIGHT - 1) + u64::from(WIDTH * 4);
    if case == Case::ShortSource {
        send_source(&socket, admitted, span - 1);
        let (kind, body) = event_kind(peer);
        assert_eq!(kind, ShellFileKind::ChannelClosed);
        assert_eq!(
            ShellChannelClosed::decode(&body).unwrap().reason,
            ShellChannelClosedReason::ProtocolFault
        );
    } else {
        send_source(&socket, admitted, span);
    }
    // Terminal status always precedes SourceReleased, even after closure.
    let (kind, _) = event_kind(peer);
    assert_eq!(kind, ShellFileKind::ResourceStatus);
    let (kind, body) = event_kind(peer);
    assert_eq!(kind, ShellFileKind::SourceReleased);
    let released = ShellSourceReleased::decode(&body).unwrap();
    assert_eq!(released.offer_id, 2);
    assert_eq!(
        released.disposition,
        ShellSourceDisposition::RefusedBeforeUse
    );
}

fn run(case: Case) {
    let directory = std::env::temp_dir().join(format!(
        "shell-capture-wire-{}-{case:?}",
        std::process::id()
    ));
    let mut transport = authorized(&directory);
    let (d, l, f, b) = config();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(&mut epochs, ContentLimits::prototype(grant))
        .unwrap();
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            Duration::from_secs(3),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    supply_allocation(&mut epochs);
    let baseline = epochs.accounting();
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let peer = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        peer_script(&mut peer, &image_path, case);
    });
    let mut owner = ShellIsbImageOwner::new(4 << 20, 65536).unwrap();
    let mut negotiated = false;
    let mut admission: Option<ShellCaptureAdmission> = None;
    let mut settled: Option<ShellImageMetadata> = None;
    let mut refused = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "capture wire stalled: {case:?}");
        if !negotiated {
            negotiated = transport
                .poll_negotiation(&mut epochs, 65536)
                .unwrap()
                .is_some();
            std::thread::yield_now();
            continue;
        }
        match transport.service_content_resources(&mut epochs, 0) {
            Ok(_) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("resource service: {error}"),
        }
        // The CPU owner shares every turn and must leave DMA-BUF offers alone.
        owner
            .service(&mut transport, &mut epochs, 0, Instant::now())
            .unwrap();
        if transport.capture_image_channel_ready() {
            transport.enable_capture_image_admission().unwrap();
        }
        if let Some((offer, expires)) = transport.pending_capture_image_offer() {
            assert!(transport.pending_isb_image_offer().is_none());
            assert!(expires > Instant::now());
            match transport.admit_capture_image_offer(&mut epochs, BOUND, 0) {
                Ok(admitted) => {
                    assert_eq!(admitted.metadata, offer);
                    assert_eq!(admitted.deadline, expires, "admission cannot renew it");
                    admission = Some(admitted);
                }
                Err(ShellTransportError::WrongContentRecord) => {
                    assert_eq!(offer.modifier, 1);
                    transport
                        .refuse_pending_image(ContentReason::Malformed)
                        .unwrap();
                    refused += 1;
                }
                Err(error) => panic!("capture admission: {error}"),
            }
        }
        let closed = admission.is_some() && !transport.capture_image_channel_ready();
        let source = transport.take_capture_image_source();
        if let Some(source) = &source {
            let admitted = admission.as_ref().expect("source follows admission");
            assert_eq!(source.metadata, admitted.metadata);
            assert_eq!(source.plane.stride, STRIDE);
            assert_eq!(
                source.source_bytes,
                u64::from(STRIDE) * u64::from(HEIGHT - 1) + u64::from(WIDTH * 4)
            );
        }
        if source.is_some() || closed {
            // Pre-use rollback: the supplied source was never imported.
            let ShellCaptureAdmission {
                metadata,
                reservation,
                ..
            } = admission.take().unwrap();
            let mut reservation = Some(reservation);
            epochs
                .fail_capture(&mut reservation, ContentCaptureFailure::Withdrawn, 0)
                .unwrap();
            assert!(reservation.is_none());
            settled = Some(metadata);
        }
        if let Some(metadata) = settled {
            match transport.release_capture_image_source(
                grant,
                metadata,
                ShellSourceDisposition::RefusedBeforeUse,
            ) {
                Ok(_) => settled = None,
                // The resource drain journals the terminal status first.
                Err(ShellTransportError::ContentQueueSaturated) => {}
                Err(error) => panic!("source release: {error}"),
            }
        }
        std::thread::yield_now();
    }
    peer.join().unwrap();
    assert!(admission.is_none() && settled.is_none());
    if case != Case::CpuOnly {
        assert_eq!(refused, 1);
    }
    // Every capture reservation was settled on the live grant.
    let after = epochs.accounting();
    assert_eq!(after.reserved_bytes, baseline.reserved_bytes);
    assert_eq!(
        after.reserved_backing_bytes,
        baseline.reserved_backing_bytes
    );
    transport.disconnect(&mut epochs).unwrap();
    drop(owner);
    drop(transport);
    assert!(!directory.exists());
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
                desired_width: WIDTH,
                desired_height: HEIGHT,
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
                    width: WIDTH,
                    height: HEIGHT,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: WIDTH,
                    height: HEIGHT,
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
