//! Sealed ISB images: a failed or foreign-transaction resource control must
//! not decide which ResourceStatus settles the offer's terminal hold.
//!
//! Bug verification only (Option B review 05 observation). Real 9P and
//! SEQPACKET wiring with supplied afid authority, the production
//! ShellIsbImageOwner and the default feature set. No launch-domain or
//! Factotum authentication verdict and no performance claim.
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};
#[path = "support/image_test_authority.rs"]
mod authority;
#[path = "support/shell_file_peer.rs"]
mod raw;

const OFFER: u64 = 1;

#[derive(Clone, Copy, Debug)]
enum Control {
    /// ResourceCancel under its own transaction, before any Source packet.
    CancelForeignTransaction,
    /// ResourceRetire under the offer's transaction while the transfer is
    /// still open, so the store refuses it as Stale; then the Source.
    FailedRetireWithOfferTransaction,
}

#[derive(Debug, PartialEq)]
enum Seen {
    Status {
        transaction: u64,
        status: u16,
        reason: u16,
    },
    SourceReleased(ShellSourceDisposition),
    Other(ShellFileKind),
}

#[derive(Debug)]
struct Trace {
    control_reply: u8,
    events: Vec<Seen>,
    peer_end: String,
    owner_fault: Option<String>,
    owner_pending: usize,
}

#[test]
fn cancel_under_foreign_transaction_settles_and_releases_sealed_source() {
    let trace = run(Control::CancelForeignTransaction);
    eprintln!("{trace:#?}");
    assert_eq!(trace.control_reply, 119, "cancel submission accepted");
    assert_eq!(trace.owner_fault, None);
    assert_eq!(
        trace.events,
        [
            Seen::Status {
                transaction: 999,
                status: 4,
                reason: ContentReason::Cancelled as u16
            },
            Seen::SourceReleased(ShellSourceDisposition::CancelledBeforeImport),
        ],
        "terminal status, then SourceReleased"
    );
    assert_eq!(trace.owner_pending, 0, "owner credit returned");
    assert_eq!(trace.peer_end, "SourceReleased");
}

#[test]
fn failed_retire_reply_under_offer_transaction_cannot_spend_terminal_hold() {
    let trace = run(Control::FailedRetireWithOfferTransaction);
    eprintln!("{trace:#?}");
    assert_eq!(trace.control_reply, 119, "retire submission accepted");
    assert_eq!(trace.owner_fault, None);
    let accepted = trace
        .events
        .iter()
        .position(|e| matches!(e, Seen::Status { status: 2, .. }));
    let released = trace
        .events
        .iter()
        .position(|e| matches!(e, Seen::SourceReleased(_)));
    assert!(
        accepted.is_some(),
        "the transfer's own terminal status is published"
    );
    assert!(released.is_some(), "the source is released");
    assert!(
        accepted < released,
        "the transfer's terminal status precedes SourceReleased"
    );
    assert_eq!(trace.owner_pending, 0, "owner credit returned");
    assert_eq!(trace.peer_end, "SourceReleased");
}

fn run(control: Control) -> Trace {
    let directory = std::env::temp_dir().join(format!(
        "sealed-control-provenance-{}-{control:?}",
        std::process::id()
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
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
    let (d, l, f, b) = config();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(&mut epochs, ContentLimits::prototype(grant()))
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
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let (request_admission, receive_admission) = mpsc::channel();
    let (enabled_admission, wait_admission) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        peer_side(
            &path,
            &image_path,
            control,
            request_admission,
            wait_admission,
        )
    });
    let mut image_owner = ShellIsbImageOwner::new(4 << 20, 65536).unwrap();
    let mut negotiated = false;
    let mut owner_fault = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !peer.is_finished() {
        assert!(Instant::now() < deadline, "peer thread stalled");
        if !negotiated {
            negotiated = transport
                .poll_negotiation(&mut epochs, 65536)
                .unwrap()
                .is_some();
        } else if owner_fault.is_none() {
            match transport.service_content_resources(&mut epochs, 0) {
                Ok(_) | Err(ShellTransportError::NotConnected) => {}
                Err(error) => owner_fault = Some(format!("resource service: {error}")),
            }
            if receive_admission.try_recv().is_ok() {
                supply_allocation(&mut epochs);
                enabled_admission.send(()).unwrap();
            }
            if owner_fault.is_none()
                && let Err(error) =
                    image_owner.service(&mut transport, &mut epochs, 0, Instant::now())
            {
                owner_fault = Some(format!("image owner: {error}"));
            }
        }
        std::thread::yield_now();
    }
    let (control_reply, events, peer_end) = peer.join().unwrap();
    let owner_pending = image_owner.pending();
    let _ = transport.disconnect(&mut epochs);
    Trace {
        control_reply,
        events,
        peer_end,
        owner_fault,
        owner_pending,
    }
}

fn peer_side(
    path: &std::path::Path,
    image_path: &std::path::Path,
    control: Control,
    request_admission: mpsc::Sender<()>,
    wait_admission: mpsc::Receiver<()>,
) -> (u8, Vec<Seen>, String) {
    let mut peer = raw::Peer::connect(path);
    authenticated_setup(&mut peer);
    let negotiate = encode_shell_file_negotiate(
        ShellFileHeader {
            kind: ShellFileKind::Negotiate,
            connection_epoch: 1,
            submission_id: 1,
            sequence: 0,
        },
        ShellV1ClientHello {
            minimum_revision: 9,
            maximum_revision: 9,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | 1 << 13,
        },
    )
    .unwrap();
    peer.submit_acknowledged(&negotiate, 1);
    let negotiated = peer.next_event();
    peer.ack(&negotiated);
    // Image records exist only at the negotiated revision 9; the raw peer's
    // own submit/ack decoding defaults to 8.
    peer.set_revision(9);
    peer.open(6, b"image-channel", 0);
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
        connection_epoch: 1,
        channel_generation: 2,
    }
    .encode()
    .unwrap();
    rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
    let ready = peer.next_event();
    assert_eq!(kind(&ready), ShellFileKind::ChannelReady);
    peer.ack(&ready);
    request_admission.send(()).unwrap();
    wait_admission.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(peer.submit(&image_begin()).0, 119);
    let submitted = peer.next_event();
    peer.ack(&submitted);
    peer.clear();
    let admitted = peer.next_event();
    assert_eq!(kind(&admitted), ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(
        decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9)
            .unwrap()
            .body,
    )
    .unwrap();
    assert_eq!(metadata.offer_id, OFFER);
    peer.ack(&admitted);
    let (record_kind, transaction, record) = match control {
        Control::CancelForeignTransaction => (
            ShellFileKind::ResourceCancel,
            999,
            ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: grant(),
                resource: resource(),
            }),
        ),
        Control::FailedRetireWithOfferTransaction => (
            ShellFileKind::ResourceRetire,
            OFFER,
            ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: grant(),
                resource: resource(),
            }),
        ),
    };
    let header = ShellFileHeader {
        kind: record_kind,
        connection_epoch: 1,
        submission_id: 3,
        sequence: 0,
    };
    let record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record,
    };
    let bytes = match control {
        Control::CancelForeignTransaction => encode_shell_file_resource_cancel(header, &record),
        Control::FailedRetireWithOfferTransaction => {
            encode_shell_file_resource_retire(header, &record)
        }
    }
    .unwrap();
    let control_reply = peer.submit(&bytes).0;
    let mut events = Vec::new();
    if control_reply == 119 {
        let submitted = peer.next_event();
        assert_eq!(kind(&submitted), ShellFileKind::Submitted);
        peer.ack(&submitted);
    }
    peer.clear();
    if matches!(control, Control::FailedRetireWithOfferTransaction) {
        // The store refuses the retire (no accepted resource yet); wait for
        // that reply before the Source, so the race order is fixed.
        match peer.try_next_event() {
            Ok(event) => {
                peer.ack(&event);
                events.push(seen(&event));
            }
            Err(error) => return (control_reply, events, format!("no retire reply: {error}")),
        }
        send_source(&socket, metadata);
    }
    let peer_end = loop {
        if events.len() >= 6 {
            break "event limit".to_owned();
        }
        match peer.try_next_event() {
            Ok(event) => {
                peer.ack(&event);
                let value = seen(&event);
                let done = matches!(value, Seen::SourceReleased(_));
                events.push(value);
                if done {
                    break "SourceReleased".to_owned();
                }
            }
            Err(error) => break format!("event read ended: {error}"),
        }
    };
    (control_reply, events, peer_end)
}

fn kind(event: &[u8]) -> ShellFileKind {
    decode_shell_file_record_at_revision(event, ShellFileClass::Event, 9)
        .unwrap()
        .header
        .kind
}

fn seen(event: &[u8]) -> Seen {
    let record = decode_shell_file_record_at_revision(event, ShellFileClass::Event, 9).unwrap();
    match record.header.kind {
        ShellFileKind::ResourceStatus => {
            let decoded = decode_shell_file_resource_status(event).unwrap();
            let ShellContentRecord::ResourceStatus(status) = decoded.record else {
                unreachable!()
            };
            Seen::Status {
                transaction: decoded.transaction.raw(),
                status: status.status,
                reason: status.reason,
            }
        }
        ShellFileKind::SourceReleased => Seen::SourceReleased(
            ShellSourceReleased::decode(record.body)
                .unwrap()
                .disposition,
        ),
        other => Seen::Other(other),
    }
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn resource() -> ContentResourceId {
    ContentResourceId {
        id: 1,
        generation: 1,
    }
}

fn image_begin() -> Vec<u8> {
    let metadata = ShellImageMetadata {
        offer_id: OFFER,
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
    encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageBegin,
            connection_epoch: 1,
            submission_id: 2,
            sequence: 0,
        },
        &metadata.encode().unwrap(),
        9,
    )
    .unwrap()
}

fn send_source(socket: &std::os::fd::OwnedFd, metadata: ShellImageMetadata) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    let source = std::fs::File::from(
        memfd_create(
            c"provenance-sealed-source",
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
    sendmsg(
        socket,
        &[std::io::IoSlice::new(&bytes)],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
}

/// Tversion, Tauth on afid 10 (verdict supplied by `authority::Factory`),
/// Tattach through that afid, then the three standing fids.
fn authenticated_setup(peer: &mut raw::Peer) {
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

fn supply_allocation(epochs: &mut ContentEpochRegistry) {
    let grant = grant();
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
    let pixel = ContentPixelRect {
        x: 0,
        y: 0,
        width: 200,
        height: 20,
    };
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
                pixel,
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 20,
            },
            &[],
        )
        .unwrap();
    while store.take_event().is_some() {}
}
