//! Shared scripted 9P and real ISB endpoint fixture.
use std::io::IoSliceMut;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::file_wire_peer::{CONTENT_GRANT_EPOCH, EPOCH, Object, Peer, Profile};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SocketAddrUnix,
    SocketFlags, SocketType, accept_with, listen, recvmsg, socket_with,
};
use sophia_image_producer::{create_isb, finish_isb};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

pub(super) const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
pub(super) const SEALED: u64 = IMAGE_CAPABILITY_SEALED;
pub(super) const OWNED: u64 = IMAGE_CAPABILITY_OWNED_RING;

pub(super) fn profile() -> Profile {
    Profile {
        role: "bar",
        revision: 10,
        capabilities: BAR | OWNED,
        limits_published: false,
    }
}

pub(super) fn options() -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: 10,
        maximum_revision: 10,
        required_capabilities: BAR | OWNED,
        handshake_timeout: Duration::from_secs(5),
    }
}

pub(super) fn image_config(dir: PathBuf) -> ShellImageConfig {
    ShellImageConfig {
        endpoint_dir: dir,
        bounds: ProducerRoleBounds {
            max_resource_bytes: 1 << 20,
            max_width: 64,
            max_height: 64,
            max_admitted_outputs: 1,
            transfer_slots: 4,
        },
        transfer_timeout_ms: 1_000,
        startup_deadline_ms: 60_000,
        formats_capacity: 4096,
        ring_slots: 0,
        ring_capacity: 0,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    }
}

pub(super) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sophia-image-adapter-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(super) fn discovery_text(endpoint: &str) -> Vec<u8> {
    ShellImageDiscovery {
        connection_epoch: EPOCH,
        channel_generation: 3,
        endpoint: endpoint.to_owned(),
        source_kinds: 4,
        max_offers: 4,
        max_packets_per_turn: 1,
        hello_timeout_ms: 250,
        validation_timeout_ms: 2_000,
        validation_chunk_bytes: 65_536,
        capture_interest_timeout_ms: 500,
        formats_generation: 11,
        limits_generation: 9,
    }
    .encode()
    .unwrap()
    .into_bytes()
}

pub(super) fn limits_record() -> Vec<u8> {
    let body = ShellImageLimits {
        channel_generation: 3,
        limits_generation: 9,
        max_resource_bytes: 1 << 20,
        max_sealed_backing_bytes: 1 << 20,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: 4,
        max_validation_jobs: 1,
        max_capture_jobs: 0,
        max_admitted_outputs: 1,
        max_format_rows_per_output: 1,
        max_width: 64,
        max_height: 64,
        validation_timeout_ms: 2_000,
        capture_interest_timeout_ms: 500,
        validation_chunk_bytes: 65_536,
        max_packets_per_turn: 1,
    }
    .encode()
    .unwrap();
    encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageLimits,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &body,
        9,
    )
    .unwrap()
}

pub(super) fn formats_record() -> Vec<u8> {
    let body = ShellImageFormats {
        channel_generation: 3,
        formats_generation: 11,
        limits_generation: 9,
        rows: Vec::new(),
    }
    .encode()
    .unwrap();
    encode_shell_file_record_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ImageFormats,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &body,
        9,
    )
    .unwrap()
}

pub(super) fn connect_image(name: &str) -> (ShellConnection, Peer, PathBuf, OwnedFd) {
    connect_image_cap(name, 4096)
}

pub(super) fn connect_image_cap(
    name: &str,
    formats_capacity: usize,
) -> (ShellConnection, Peer, PathBuf, OwnedFd) {
    connect_role_image(name, formats_capacity, profile(), false)
}

pub(super) fn connect_role_image(
    name: &str,
    formats_capacity: usize,
    profile: Profile,
    deferred: bool,
) -> (ShellConnection, Peer, PathBuf, OwnedFd) {
    let dir = temp_dir(name);
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let mut offered = options();
    offered.required_capabilities = profile.capabilities;
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile));
    // The endpoint listener the producer will connect to.
    let endpoint_path = dir.join("image.sock");
    let endpoint_listener = socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .unwrap();
    rustix::net::bind(
        &endpoint_listener,
        &SocketAddrUnix::new(&endpoint_path).unwrap(),
    )
    .unwrap();
    listen(&endpoint_listener, 4).unwrap();

    let origin = Instant::now();
    let mut config = image_config(dir.clone());
    config.formats_capacity = formats_capacity;
    let dma = profile.capabilities & IMAGE_CAPABILITY_DMA_BUF != 0;
    let connection = if deferred || dma {
        ShellConnection::connect_files(&socket_path, offered)
    } else {
        ShellConnection::connect_files_with_image(
            &socket_path,
            offered,
            config.clone(),
            origin,
            1_000,
        )
    };
    let mut peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
    let mut connection = connection.unwrap();
    if deferred {
        let limits = match take_role_content(&mut connection) {
            ShellContentRecord::Limits(limits) => limits,
            _ => panic!("Limits before ISB setup"),
        };
        config.bounds = ProducerRoleBounds {
            max_resource_bytes: limits.max_resource_bytes,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            max_admitted_outputs: limits.max_outputs,
            transfer_slots: limits.max_open_transfers,
        };
        config.transfer_timeout_ms = limits.transfer_timeout_ms;
        assert!(connection.image_adapter().is_none());
        connection
            .enable_image(config.clone(), origin, 1_000)
            .unwrap();
        assert!(
            connection
                .enable_image(image_config(dir.clone()), origin, 1_000)
                .is_err()
        );
    }
    if dma {
        connection.enable_dma_image(config, origin, 1_000).unwrap();
    }
    peer.objects.insert(
        "image-channel".to_owned(),
        Object {
            qid: 21,
            bytes: discovery_text("image.sock"),
            chunk: usize::MAX,
        },
    );
    peer.objects.insert(
        "image-limits".to_owned(),
        Object {
            qid: 22,
            bytes: limits_record(),
            chunk: usize::MAX,
        },
    );
    peer.objects.insert(
        "image-formats".to_owned(),
        Object {
            qid: 23,
            bytes: formats_record(),
            chunk: usize::MAX,
        },
    );
    connection.poll_io().unwrap();
    (connection, peer, dir, endpoint_listener)
}

pub(super) fn take_role_content(connection: &mut ShellConnection) -> ShellContentRecord {
    if connection.welcome().capabilities & SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG != 0 {
        let mut inbox = CatalogInbox::new(connection.connection_epoch()).unwrap();
        let Some(CatalogObservation::Content(_, record)) =
            connection.take_catalog_observation(&mut inbox).unwrap()
        else {
            panic!("expected dock content")
        };
        record
    } else if connection.welcome().capabilities & SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0 {
        let Some(NativeLauncherObservation::Content(_, record)) =
            connection.take_native_launcher_observation().unwrap()
        else {
            panic!("expected launcher content")
        };
        record
    } else {
        connection.take_content().unwrap().unwrap().1
    }
}

pub(super) fn drive(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    mut done: impl FnMut(&ShellConnection, &Peer) -> bool,
) {
    let start = Instant::now();
    loop {
        connection.poll_io().unwrap();
        peer.pump();
        if done(connection, peer) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "drive timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn accept_endpoint(listener: &OwnedFd) -> OwnedFd {
    loop {
        match accept_with(listener, SocketFlags::CLOEXEC | SocketFlags::NONBLOCK) {
            Ok(fd) => return fd,
            Err(error) if error == rustix::io::Errno::AGAIN => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

pub(super) fn recv_packet(peer: &OwnedFd) -> (Vec<u8>, Vec<OwnedFd>) {
    let start = Instant::now();
    loop {
        let mut buf = vec![0u8; 512];
        let mut space = [std::mem::MaybeUninit::uninit(); 128];
        let mut ancillary = RecvAncillaryBuffer::new(&mut space);
        let mut iov = [IoSliceMut::new(&mut buf)];
        match recvmsg(
            peer,
            &mut iov,
            &mut ancillary,
            RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
        ) {
            Ok(result) => {
                buf.truncate(result.bytes);
                let mut fds = Vec::new();
                for message in ancillary.drain() {
                    if let RecvAncillaryMessage::ScmRights(rights) = message {
                        fds.extend(rights);
                    }
                }
                return (buf, fds);
            }
            Err(error) if error == rustix::io::Errno::AGAIN => {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("recv failed: {error}"),
        }
    }
}

pub(super) fn metadata() -> ProducerMetadata {
    ProducerMetadata {
        offer_id: 0,
        resource: 21,
        resource_generation: 1,
        admission_epoch: EPOCH,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 2,
        height: 2,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 16,
        modifier: 0,
    }
}

pub(super) fn sealed_fd() -> OwnedFd {
    let source = create_isb(2, 2).unwrap();
    source.write_pixels(0, &[7u8; 16]).unwrap();
    finish_isb(&source).unwrap();
    source.into_fd()
}

pub(super) const RING_IDENTITY: sophia_shell_protocol::shell_files::RingIdentity =
    sophia_shell_protocol::shell_files::RingIdentity {
        connection_epoch: EPOCH,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
        channel_generation: 3,
        ring: 1,
        generation: 1,
    };

pub(super) fn ring_event_kind(
    kind: sophia_shell_protocol::shell_files::RingEventKind,
    sequence: u64,
) -> Vec<u8> {
    use sophia_shell_protocol::shell_files::RingEvent;
    RingEvent {
        kind,
        identity: RING_IDENTITY,
        sequence,
        offer_id: 0,
        slot: 0,
        slot_generation: 0,
        resource: 0,
        resource_generation: 0,
        reason: u16::from(kind == RingEventKind::RingRefused),
        changed_generation: false,
    }
    .encode()
}

pub(super) fn ring_released(
    sequence: u64,
    offer_id: u64,
    slot_generation: u64,
    resource: u64,
    resource_generation: u64,
    reason: u16,
) -> Vec<u8> {
    use sophia_shell_protocol::shell_files::{RingEvent, RingEventKind};
    RingEvent {
        kind: RingEventKind::SlotReleased,
        identity: RING_IDENTITY,
        sequence,
        offer_id,
        slot: 0,
        slot_generation,
        resource,
        resource_generation,
        reason,
        changed_generation: false,
    }
    .encode()
}

pub(super) fn status_body(
    transaction: u64,
    resource: u64,
    generation: u64,
    status: u16,
) -> Vec<u8> {
    encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: ContentGrant {
                connection_epoch: EPOCH,
                content_grant_epoch: CONTENT_GRANT_EPOCH,
            },
            resource: ContentResourceId {
                id: resource,
                generation,
            },
            status,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: 8,
        }),
    })
    .unwrap()
}

/// Drive discovery and Hello, answer ChannelReady with kind 4, and
/// return the accepted image socket. The Register record is left in
/// `peer.control_writes` for the caller's assertions.
pub(super) fn drive_to_image_hello(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    listener: &OwnedFd,
) -> OwnedFd {
    drive(connection, peer, |connection, _| {
        connection.image_poll_fd().is_some()
    });
    let accepted = accept_endpoint(listener);
    let (hello, hello_fds) = recv_packet(&accepted);
    assert_eq!(hello.len(), 40);
    assert!(hello_fds.is_empty());
    accepted
}

pub(super) fn push_channel_ready(peer: &mut Peer) {
    let ready = ShellChannelReady {
        channel_generation: 3,
        granted_source_kinds: 4,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ChannelReady, &ready);
    peer.push(bytes);
}

pub(super) fn drive_to_channel_ready(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    listener: &OwnedFd,
) -> OwnedFd {
    let accepted = drive_to_image_hello(connection, peer, listener);
    push_channel_ready(peer);
    drive(connection, peer, |_, peer| !peer.control_writes.is_empty());
    accepted
}

/// From ChannelReady: admit the ring, receive the D4F1 source packet,
/// mark it ready. Returns the source descriptor and its size.
pub(super) fn drive_to_ring_ready(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    accepted: &OwnedFd,
) -> (OwnedFd, u64) {
    use sophia_shell_protocol::shell_files::RingEventKind;
    let admitted = ring_event_kind(RingEventKind::RingAdmitted, peer.take_sequence());
    peer.push(admitted);
    let (packet, fds) = drive_packet(connection, peer, accepted);
    assert_eq!(fds.len(), 1);
    let registration =
        sophia_shell_protocol::shell_files::RingRegistration::decode_source(&packet).unwrap();
    let ready = ring_event_kind(RingEventKind::RingReady, peer.take_sequence());
    peer.push(ready);
    drive(connection, peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready)
    });
    let fd = fds.into_iter().next().unwrap();
    let bytes = rustix::fs::fstat(&fd).unwrap().st_size as u64;
    assert_eq!(bytes, registration.source_bytes().unwrap());
    (fd, bytes)
}

fn drive_packet(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    accepted: &OwnedFd,
) -> (Vec<u8>, Vec<OwnedFd>) {
    // Pump the session while waiting for the side-socket packet.
    let start = Instant::now();
    loop {
        connection.poll_io().unwrap();
        peer.pump();
        let mut buf = vec![0u8; 512];
        let mut space = [std::mem::MaybeUninit::uninit(); 128];
        let mut ancillary = RecvAncillaryBuffer::new(&mut space);
        let mut iov = [IoSliceMut::new(&mut buf)];
        match recvmsg(
            accepted,
            &mut iov,
            &mut ancillary,
            RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
        ) {
            Ok(result) => {
                buf.truncate(result.bytes);
                let mut fds = Vec::new();
                for message in ancillary.drain() {
                    if let rustix::net::RecvAncillaryMessage::ScmRights(rights) = message {
                        fds.extend(rights);
                    }
                }
                return (buf, fds);
            }
            Err(error) if error == rustix::io::Errno::AGAIN => {
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("recv failed: {error}"),
        }
    }
}

pub(super) fn answer_submit(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    kind: ShellFileKind,
) {
    drive(connection, peer, |_, peer| !peer.submits.is_empty());
    let held = peer.submits.pop_front().unwrap();
    let record = peer.transactions.last().unwrap();
    let submission_id = u64::from_le_bytes(record[16..24].try_into().unwrap());
    peer.answer_write(held.tag, held.data.len() as u32);
    let submitted = peer.submitted_image(submission_id, kind);
    peer.push(submitted);
}
