//! Session intake over a real authenticated shell transport and image socket:
//! negotiated DMA-BUF ImageBegin, pre-admission destination, Source, real
//! mailbox submission and its exact receipt. The descriptor is a supplied
//! memfd and the receipt is the mailbox's pre-use rollback after executor
//! close: this proves custody and journal order, never a GPU copy.
#![cfg(feature = "native-session")]
use super::*;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::{ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole};
use std::os::fd::AsFd;
use std::sync::mpsc;
use std::time::Instant;
#[allow(clippy::duplicate_mod)]
#[path = "../../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod raw;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 20;
const LOGICAL: u64 = (WIDTH * HEIGHT * 4) as u64;
const STALE_WIDTH: u32 = 100;
const STALE_BOUND: u64 = (STALE_WIDTH * HEIGHT * 4) as u64;

fn event(peer: &mut raw::Peer) -> (ShellFileKind, Vec<u8>) {
    let bytes = peer.next_event();
    let record = decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
    let kind = record.header.kind;
    let body = record.body.to_vec();
    peer.ack(&bytes);
    (kind, body)
}

/// Skips custody receipts and object announcements.
fn next(peer: &mut raw::Peer, kind: ShellFileKind) -> Vec<u8> {
    for _ in 0..16 {
        let (got, body) = event(peer);
        if got == kind {
            return body;
        }
        assert!(
            matches!(
                got,
                ShellFileKind::Submitted | ShellFileKind::ObjectPublished
            ),
            "{got:?} before {kind:?}"
        );
    }
    panic!("missing {kind:?}");
}

fn submit(peer: &mut raw::Peer, bytes: &[u8]) {
    assert_eq!(peer.submit(bytes).0, 119);
    peer.clear();
}

fn attach(peer: &mut raw::Peer) {
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
    peer.set_revision(10);
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
}

fn output() -> ContentOutputId {
    ContentOutputId {
        id: 1,
        generation: 1,
    }
}

fn metadata(grant: ContentGrant) -> ShellImageMetadata {
    ShellImageMetadata {
        offer_id: 3,
        resource: 1,
        resource_generation: 1,
        admission_epoch: grant.content_grant_epoch,
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
        declared_bytes: LOGICAL,
        modifier: 0,
    }
}

enum Step {
    /// The owner saw the submitted job; the peer must observe no release yet.
    Submitted,
    /// The executor closed after epoch loss; settlement may now publish.
    Closed,
}

fn peer_script(
    peer: &mut raw::Peer,
    grant: ContentGrant,
    image_path: &std::path::Path,
    steps: mpsc::Receiver<Step>,
    checked: mpsc::Sender<()>,
    stale: bool,
) {
    attach(peer);
    let negotiate = encode_shell_file_negotiate(
        ShellFileHeader {
            kind: ShellFileKind::Negotiate,
            connection_epoch: grant.connection_epoch,
            submission_id: 1,
            sequence: 0,
        },
        ShellV1ClientHello {
            minimum_revision: 10,
            maximum_revision: 10,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | 1 << 15
                | 1 << 14,
        },
    )
    .unwrap();
    peer.submit_acknowledged(&negotiate, 1);
    let negotiated = peer.next_event();
    let welcome = decode_shell_file_negotiated(&negotiated).unwrap().welcome;
    assert_ne!(welcome.capabilities & 1 << 14, 0, "DMA-BUF granted");
    peer.ack(&negotiated);
    peer.open(6, b"image-channel", 0);
    let discovery =
        ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
    assert_eq!(discovery.source_kinds, 6);
    peer.open(9, b"image-formats", 0);
    let formats = peer.read(9, 0);
    let record =
        decode_shell_file_record_at_revision(&formats, ShellFileClass::Object, 10).unwrap();
    let formats = ShellImageFormats::decode(record.body).unwrap();
    assert_eq!(formats.rows.len(), 1);
    assert_eq!((formats.rows[0].output, formats.rows[0].modifier), (1, 0));
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
    let hello = ShellImagePacket::Hello {
        connection_epoch: discovery.connection_epoch,
        channel_generation: discovery.channel_generation,
    }
    .encode()
    .unwrap();
    rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
    let ready = next(peer, ShellFileKind::ChannelReady);
    assert_eq!(
        ShellChannelReady::decode(&ready)
            .unwrap()
            .granted_source_kinds,
        6
    );
    let allocation = encode_shell_file_allocation_request(
        ShellFileHeader {
            kind: ShellFileKind::AllocationRequest,
            connection_epoch: grant.connection_epoch,
            submission_id: 2,
            sequence: 0,
        },
        ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(2),
            record: ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                grant,
                output: output(),
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
            }),
        },
    )
    .unwrap();
    submit(peer, &allocation);
    next(peer, ShellFileKind::AllocationResult);
    if stale {
        // A narrower offer the owner refuses at its deadline mid-prepare.
        let refused = ShellImageMetadata {
            offer_id: 2,
            width: STALE_WIDTH,
            declared_bytes: STALE_BOUND,
            ..metadata(grant)
        };
        let begin = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind: ShellFileKind::ImageBegin,
                connection_epoch: grant.connection_epoch,
                submission_id: 3,
                sequence: 0,
            },
            &refused.encode().unwrap(),
            10,
        )
        .unwrap();
        submit(peer, &begin);
        next(peer, ShellFileKind::ResourceStatus);
    }
    let admitted = metadata(grant);
    let begin = encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageBegin,
            connection_epoch: grant.connection_epoch,
            submission_id: 3 + u64::from(stale),
            sequence: 0,
        },
        &admitted.encode().unwrap(),
        10,
    )
    .unwrap();
    submit(peer, &begin);
    let body = next(peer, ShellFileKind::ImageAdmitted);
    assert_eq!(ShellImageMetadata::decode(&body).unwrap(), admitted);
    // LINEAR, tight rows: the SDK's conservative layout.
    let source = rustix::fs::memfd_create("dma-stand-in", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&source, LOGICAL).unwrap();
    let zero = ShellImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    };
    let packet = ShellImagePacket::Source {
        connection_epoch: discovery.connection_epoch,
        channel_generation: discovery.channel_generation,
        metadata: admitted,
        planes: [
            ShellImagePlane {
                offset: 0,
                stride: WIDTH * 4,
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
        &socket,
        &[std::io::IoSlice::new(&packet)],
        &mut control,
        rustix::net::SendFlags::NOSIGNAL,
    )
    .unwrap();
    drop(source);
    // Submitted and still in flight: nothing has been released.
    assert!(matches!(
        steps.recv_timeout(Duration::from_secs(5)).unwrap(),
        Step::Submitted
    ));
    peer.no_event();
    checked.send(()).unwrap();
    // Epoch loss stops publication but not custody: still nothing.
    assert!(matches!(
        steps.recv_timeout(Duration::from_secs(5)).unwrap(),
        Step::Closed
    ));
    // Status strictly before SourceReleased: `next` rejects any other order.
    next(peer, ShellFileKind::ResourceStatus);
    let released = ShellSourceReleased::decode(&next(peer, ShellFileKind::SourceReleased)).unwrap();
    assert_eq!(released.offer_id, admitted.offer_id);
    assert_eq!(
        released.disposition,
        ShellSourceDisposition::CancelledBeforeImport,
        "withdrawn before import; never CaptureComplete without a GPU receipt"
    );
}

fn grant_allocation(transport: &mut sophia_runtime::ShellTransportConnection<'_>, now: u64) {
    transport.service_content_resources(now).unwrap();
    transport
        .service_content_allocation_requests(&[], now)
        .unwrap();
    if transport.next_content_allocation_request().is_some() {
        let rect = ContentPixelRect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT,
        };
        transport
            .grant_content_allocation(
                1,
                sophia_runtime::ContentAllocationSnapshot {
                    native_opening: None,
                    output: output(),
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
                    pixel: rect,
                    parent: ContentAllocationId::default(),
                    anchor_parent_rect: ContentPixelRect::default(),
                    allowed_reservation_extent: 20,
                },
                &[],
            )
            .unwrap();
    }
}

#[test]
fn negotiated_dma_offer_flows_through_intake_to_the_mailbox_and_settles_in_journal_order() {
    drive(false);
}

/// The backend allows one outstanding prepare. An offer refused at its
/// deadline mid-prepare must not leave that reply to block, or be spent
/// as the bound of, the next offer of another size.
#[test]
fn deadline_refusal_mid_prepare_leaves_no_stale_bound_for_the_next_offer() {
    drive(true);
}

fn drive(stale: bool) {
    let directory = std::env::temp_dir().join(format!(
        "session-capture-wire-{}-{stale}",
        std::process::id()
    ));
    let mut owner = ShellComponentConnections::new().unwrap();
    let slot = owner
        .add(
            "bar",
            ShellComponentRole::Bar,
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
    owner.grant_capture_device(slot, Some(1)).unwrap();
    // Endpoint first: negotiation publishes DMA-BUF only from a live endpoint.
    let mut native = Native {
        drives: Some((1, 1)),
        ..Native::default()
    };
    let destination = native.offer_with(1, 4 * LOGICAL);
    let now = Instant::now();
    owner.service_capture_intake(&mut native, 0, now);
    owner
        .capture_link
        .service(&mut native, &mut owner.capture, true)
        .unwrap();
    assert!(owner.capture_launch_ready(slot));
    let key = owner.reserve_attempt(slot).unwrap();
    owner
        .begin_authenticated_negotiation(
            key,
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
            Box::new(crate::test_image_authority::Factory),
        )
        .unwrap();
    let path = owner.socket_path(slot).unwrap().to_owned();
    let image_path = directory.join("image.sock");
    let (step, steps) = mpsc::channel();
    let (checked, check) = mpsc::channel();
    let grant = key.grant;
    let peer = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        peer_script(&mut peer, grant, &image_path, steps, checked, stale);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut published = false;
    let mut phase = 0;
    let mut refused = false;
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "session capture wire stalled");
        for (_, result) in owner.poll_negotiations(65536).into_iter().flatten() {
            result.unwrap();
        }
        assert!(owner.turn_connected().iter().all(Option::is_none));
        if owner.phase(key) == Ok(ComponentConnectionPhase::Connected) {
            let now = owner.content_clock_msec();
            owner
                .with_connection(key, |transport| {
                    if !published {
                        transport
                            .publish_content_output_facts(
                                TransactionId::from_raw(91),
                                1,
                                vec![ContentOutputFactsEntry {
                                    output: output(),
                                    local_width: 200,
                                    local_height: 100,
                                    scale_numerator: 1,
                                    scale_denominator: 1,
                                    scale_generation: 1,
                                }],
                            )
                            .unwrap();
                        published = true;
                    }
                    grant_allocation(transport, now);
                })
                .unwrap();
        }
        let first = (OutputId::from_raw(1), STALE_WIDTH, HEIGHT);
        if stale && !refused && native.prepares == [first] {
            // Past the offer's deadline with its prepare still in flight.
            let late = Instant::now() + Duration::from_secs(3600);
            let errors =
                owner.service_capture_intake(&mut native, owner.content_clock_msec(), late);
            assert!(errors.is_empty(), "{:?}", errors.len());
            assert_eq!(
                destination.budget.usage(),
                (0, 0),
                "no ticket for the refused offer"
            );
            // Its reply lands after the refusal, for the old size.
            native.prepared = Some(Ok(STALE_BOUND));
            refused = true;
        }
        // The renderer proves a destination bound only after a prepare.
        let expected = usize::from(stale) + 1;
        if native.prepares.len() == expected && native.prepared.is_none() && phase == 0 {
            assert_eq!(
                native.prepares.last(),
                Some(&(OutputId::from_raw(1), WIDTH, HEIGHT))
            );
            native.prepared = Some(Ok(LOGICAL));
        }
        let errors =
            owner.service_capture_intake(&mut native, owner.content_clock_msec(), Instant::now());
        assert!(errors.is_empty(), "{:?}", errors.len());
        if phase == 0 && owner.capture.as_ref().is_some_and(|o| o.pending() == 1) {
            assert_eq!(owner.capture_sources.pending(), 0);
            assert_eq!(
                owner.capture_pending(),
                1,
                "one global job, no extra capacity"
            );
            assert_eq!(destination.budget.usage(), (1, LOGICAL));
            // No receipt yet: settlement service publishes nothing.
            assert!(owner.service_capture_completions().is_empty());
            step.send(Step::Submitted).unwrap();
            phase = 1;
        }
        // The owner keeps turning while the peer fences for an absent event.
        if phase == 1 && check.try_recv().is_ok() {
            assert!(owner.service_capture_completions().is_empty());
            // Epoch loss withdraws the endpoint; a full close queue keeps the
            // executor, so the submitted backing and ticket stay retained.
            destination.epoch.invalidate();
            native.busy_close = true;
            assert!(
                owner
                    .capture_link
                    .service(&mut native, &mut owner.capture, true)
                    .is_err()
            );
            assert!(owner.service_capture_completions().is_empty());
            assert_eq!(owner.capture.as_ref().unwrap().pending(), 1);
            assert_eq!(destination.budget.usage(), (1, LOGICAL));
            native.busy_close = false;
            owner
                .capture_link
                .service(&mut native, &mut owner.capture, true)
                .unwrap();
            step.send(Step::Closed).unwrap();
            phase = 2;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    peer.join().unwrap();
    assert_eq!(owner.capture_pending(), 0);
    assert_eq!(
        destination.budget.usage(),
        (0, 0),
        "rollback returned the ticket"
    );
    owner.close(key).unwrap();
    assert!(owner.turn_connected().iter().all(Option::is_none));
    owner.finish_after_backend_drop(()).unwrap();
    drop(owner);
    let _ = std::fs::remove_dir_all(&directory);
}
