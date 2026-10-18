//! Actual sealed image admission for otherwise role-focused file-wire tests.
//! Authentication decisions are supplied; this is not launch acceptance.
use super::shell_file_peer::Peer;
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::*;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
#[path = "image_test_authority.rs"]
mod authority;

pub fn configure(
    transport: &mut ShellComponentTransport,
    limits: &ContentLimits,
) -> ShellIsbImageOwner {
    configure_for_peer(transport, limits, std::process::id())
}

/// Like [`configure`], for a peer in another process (an external oracle or
/// SDK client) whose PID the authenticated channel must accept.
#[allow(dead_code)] // Only external-peer fixtures supply an explicit PID.
pub fn configure_for_peer(
    transport: &mut ShellComponentTransport,
    limits: &ContentLimits,
    peer_pid: u32,
) -> ShellIsbImageOwner {
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid,
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    let offers = limits.max_open_transfers.min(4);
    let d = ShellImageDiscovery {
        connection_epoch: limits.grant.connection_epoch,
        channel_generation: 2,
        endpoint: "image.sock".into(),
        source_kinds: 1,
        max_offers: offers,
        max_packets_per_turn: 8,
        hello_timeout_ms: 2000,
        validation_timeout_ms: 2000,
        validation_chunk_bytes: 65536,
        capture_interest_timeout_ms: 500,
        formats_generation: 1,
        limits_generation: 1,
    };
    let l = ShellImageLimits {
        channel_generation: 2,
        limits_generation: 1,
        max_resource_bytes: limits.max_resource_bytes,
        max_sealed_backing_bytes: limits.max_resident_bytes + limits.max_retiring_bytes,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: offers,
        max_validation_jobs: offers,
        max_capture_jobs: 0,
        max_admitted_outputs: limits.max_outputs,
        max_format_rows_per_output: 1,
        max_width: limits.max_width_px,
        max_height: limits.max_height_px,
        validation_timeout_ms: 2000,
        capture_interest_timeout_ms: 500,
        validation_chunk_bytes: 65536,
        max_packets_per_turn: 8,
    };
    let f = ShellImageFormats {
        channel_generation: 2,
        formats_generation: 1,
        limits_generation: 1,
        rows: vec![],
    };
    let b = ShellImageRoleBounds {
        max_resource_bytes: limits.max_resource_bytes,
        max_width: limits.max_width_px,
        max_height: limits.max_height_px,
        max_admitted_outputs: limits.max_outputs,
        transfer_slots: offers,
    };
    transport.configure_image_channel(d, l, f, b).unwrap();
    ShellIsbImageOwner::new(limits.max_resource_bytes as usize, 65536).unwrap()
}

pub fn setup(peer: &mut Peer) {
    peer.set_revision(9);
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
                30u32.to_le_bytes().as_slice(),
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
                &30u32.to_le_bytes(),
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

pub fn ready(peer: &mut Peer, directory: &Path, epoch: u64) -> OwnedFd {
    let socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::connect(
        &socket,
        &rustix::net::SocketAddrUnix::new(directory.join("image.sock")).unwrap(),
    )
    .unwrap();
    let hello = ShellImagePacket::Hello {
        connection_epoch: epoch,
        channel_generation: 2,
    }
    .encode()
    .unwrap();
    rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
    let event = peer.next_event();
    let record = decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ChannelReady);
    peer.ack(&event);
    socket
}

pub fn upload(
    peer: &mut Peer,
    socket: &OwnedFd,
    epoch: u64,
    submission: u64,
    metadata: ShellImageMetadata,
    pixels: &[u8],
) {
    let bytes = encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageBegin,
            connection_epoch: epoch,
            submission_id: submission,
            sequence: 0,
        },
        &metadata.encode().unwrap(),
        9,
    )
    .unwrap();
    peer.submit_acknowledged(&bytes, submission);
    let event = peer.next_event();
    let record = decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    assert_eq!(ShellImageMetadata::decode(record.body).unwrap(), metadata);
    peer.ack(&event);
    let mut file = std::fs::File::from(
        rustix::fs::memfd_create(
            c"role-isb-fixture",
            rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(pixels).unwrap();
    use rustix::fs::SealFlags;
    rustix::fs::fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let mut planes = [ShellImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    }; 4];
    planes[0].stride = metadata.stride;
    let bytes = ShellImagePacket::Source {
        connection_epoch: epoch,
        channel_generation: 2,
        metadata,
        planes,
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
    let rights = [file.as_fd()];
    assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        rustix::net::sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            rustix::net::SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    let status = peer.next_event();
    let value = decode_shell_file_resource_status_at_revision(&status, 9).unwrap();
    assert!(
        matches!(value.record, ShellContentRecord::ResourceStatus(s) if s.resource.id == metadata.resource && s.status == 2)
    );
    peer.ack(&status);
    let release = peer.next_event();
    let record = decode_shell_file_record_at_revision(&release, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::SourceReleased);
    assert_eq!(
        ShellSourceReleased::decode(record.body)
            .unwrap()
            .disposition,
        ShellSourceDisposition::Retained
    );
    peer.ack(&release);
}

#[allow(dead_code)] // Role-focused fixtures may grant their own allocation.
pub fn supply_allocation(epochs: &mut ContentEpochRegistry, grant: ContentGrant) {
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
