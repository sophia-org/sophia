#![allow(dead_code)]

//! The opt-in BAR image adapter against the scripted file peer:
//! discovery over the session's own 9P wire, endpoint connect, Hello,
//! ChannelReady, an ImageBegin ticket, one sealed Source crossing
//! with its descriptor, routed ResourceStatus, and settlement. Plus
//! the absent, malformed and no-adapter controls.

#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use std::io::IoSliceMut;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use file_wire_peer::{EPOCH, Object, Peer, Profile};
use rustix::net::{
    AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SocketAddrUnix,
    SocketFlags, SocketType, accept_with, listen, recvmsg, socket_with,
};
use sophia_image_producer::{create_isb, finish_isb};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const SEALED: u64 = IMAGE_CAPABILITY_SEALED;

fn profile() -> Profile {
    Profile {
        role: "bar",
        revision: 9,
        capabilities: BAR | SEALED,
        limits_published: false,
    }
}

fn options() -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: 9,
        maximum_revision: 9,
        required_capabilities: BAR | SEALED,
        handshake_timeout: Duration::from_secs(5),
    }
}

fn image_config(dir: PathBuf) -> ShellImageConfig {
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
        fallback_bytes: true,
        formats_capacity: 4096,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sophia-image-adapter-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn discovery_text(endpoint: &str) -> Vec<u8> {
    ShellImageDiscovery {
        connection_epoch: EPOCH,
        channel_generation: 3,
        endpoint: endpoint.to_owned(),
        source_kinds: 1,
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

fn limits_record() -> Vec<u8> {
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

fn formats_record() -> Vec<u8> {
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

fn connect_image(name: &str) -> (ShellConnection, Peer, PathBuf, OwnedFd) {
    connect_image_cap(name, 4096)
}

fn connect_image_cap(
    name: &str,
    formats_capacity: usize,
) -> (ShellConnection, Peer, PathBuf, OwnedFd) {
    let dir = temp_dir(name);
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let profile = profile();
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
    let connection =
        ShellConnection::connect_files_with_image(&socket_path, options(), config, origin, 1_000);
    let mut peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
    let mut connection = connection.unwrap();
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

fn drive(
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

fn accept_endpoint(listener: &OwnedFd) -> OwnedFd {
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

fn recv_packet(peer: &OwnedFd) -> (Vec<u8>, Vec<OwnedFd>) {
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

fn metadata() -> ProducerMetadata {
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

fn sealed_fd() -> OwnedFd {
    let source = create_isb(2, 2).unwrap();
    source.write_pixels(0, &[7u8; 16]).unwrap();
    finish_isb(&source).unwrap();
    source.into_fd()
}

fn drive_to_ready(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    listener: &OwnedFd,
) -> OwnedFd {
    drive(connection, peer, |connection, _| {
        connection.image_poll_fd().is_some()
    });
    let accepted = accept_endpoint(listener);
    let _hello = recv_packet(&accepted);
    let ready = ShellChannelReady {
        channel_generation: 3,
        granted_source_kinds: 1,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ChannelReady, &ready);
    peer.push(bytes);
    drive(connection, peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready)
    });
    accepted
}

fn answer_submit(connection: &mut ShellConnection, peer: &mut Peer, kind: ShellFileKind) {
    drive(connection, peer, |_, peer| !peer.submits.is_empty());
    let held = peer.submits.pop_front().unwrap();
    let record = peer.transactions.last().unwrap();
    let submission_id = u64::from_le_bytes(record[16..24].try_into().unwrap());
    peer.answer_write(held.tag, held.data.len() as u32);
    let submitted = peer.submitted_image(submission_id, kind);
    peer.push(submitted);
}

#[test]
fn image_adapter_runs_a_sealed_offer_end_to_end() {
    let (mut connection, mut peer, dir, listener) = connect_image("flow");
    // Discovery over 9P, endpoint connect, Hello on the side socket.
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_poll_fd().is_some()
    });
    let accepted = accept_endpoint(&listener);
    let (hello, hello_fds) = recv_packet(&accepted);
    assert_eq!(hello.len(), 40);
    assert!(hello_fds.is_empty());
    assert_eq!(
        connection.image_adapter().unwrap().channel(),
        Some(ImageChannelState::HelloSent)
    );

    // ChannelReady is consumed by the session, never delivered.
    let ready = ShellChannelReady {
        channel_generation: 3,
        granted_source_kinds: 1,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ChannelReady, &ready);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready)
    });
    assert_eq!(connection.take_content().unwrap(), None);

    // Begin: the ImageBegin record goes through the ordinary
    // transaction/submit custody cycle with its own ticket.
    let admission = connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .unwrap();
    assert_eq!(admission.offer, 1);
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(admission.ticket) == Some(Custody::Submitted)
    });
    let record = peer.transactions.last().unwrap().clone();
    assert_eq!(record.len(), 128);
    assert_eq!(
        u16::from_le_bytes(record[6..8].try_into().unwrap()),
        ShellFileKind::ImageBegin as u16
    );

    // Admission, then exactly one Source with one descriptor.
    let mut admitted = metadata();
    admitted.offer_id = 1;
    let body = ShellImageMetadata {
        offer_id: 1,
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
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ImageAdmitted, &body);
    peer.push(bytes);
    let _ = admitted;
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Sent
    });
    let (packet, fds) = recv_packet(&accepted);
    assert_eq!(packet.len(), 200);
    assert_eq!(fds.len(), 1);
    let decoded = ShellImagePacket::decode(&packet).unwrap();
    assert!(matches!(decoded, ShellImagePacket::Source { .. }));
    assert_eq!(
        connection.image_adapter().unwrap().offer(1).unwrap().state,
        OfferState::Sent
    );

    // ResourceStatus is routed to the producer and still delivered.
    let status_body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: ContentGrant {
                connection_epoch: EPOCH,
                content_grant_epoch: EPOCH,
            },
            resource: ContentResourceId {
                id: 21,
                generation: 1,
            },
            status: 2,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: 16,
        }),
    })
    .unwrap();
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().status
            == Some(sophia_image_producer::ResourceStatusKind::Accepted)
    });
    let (_, delivered) = connection.take_content().unwrap().unwrap();
    assert!(matches!(delivered, ShellContentRecord::ResourceStatus(_)));
    assert_eq!(
        connection.image_adapter().unwrap().offer(1).unwrap().state,
        OfferState::Sent,
        "a status records the outcome but does not settle custody"
    );

    // Only SourceReleased settles.
    let released = ShellSourceReleased {
        offer_id: 1,
        resource: 21,
        resource_generation: 1,
        disposition: ShellSourceDisposition::Retained,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::SourceReleased, &released);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Settled
    });
    connection.image_forget(1).unwrap();
    assert!(connection.image_adapter().unwrap().offer(1).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn absent_discovery_closes_clean_with_byte_fallback() {
    let (mut connection, mut peer, dir, _listener) = connect_image("absent");
    peer.error_nodes.insert("image-channel".to_owned(), 2);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    let adapter = connection.image_adapter().unwrap();
    assert_eq!(adapter.cause(), Some(ImageCloseCause::Absent));
    assert_eq!(adapter.path(), Some(ImagePath::Bytes));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn malformed_discovery_is_a_discovery_error_without_fallback() {
    let (mut connection, mut peer, dir, _listener) = connect_image("malformed");
    peer.objects.insert(
        "image-channel".to_owned(),
        Object {
            qid: 21,
            bytes: b"not a discovery\n".to_vec(),
            chunk: usize::MAX,
        },
    );
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    let adapter = connection.image_adapter().unwrap();
    assert_eq!(adapter.cause(), Some(ImageCloseCause::Discovery));
    assert_eq!(adapter.path(), Some(ImagePath::Error));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_adapter_connection_refuses_image_begin_and_returns_the_fd() {
    let dir = temp_dir("none");
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile()));
    let connection = ShellConnection::connect_files(&socket_path, options());
    let _peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
    let mut connection = connection.unwrap();
    assert!(connection.image_adapter().is_none());
    let error = connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .err()
        .unwrap();
    assert_eq!(error.error, ImageAdapterError::NotEnabled);
    assert!(error.source.is_some());
    let _ = std::fs::remove_dir_all(dir);
    let _ = connection.as_fd();
}

#[test]
fn cancelled_offer_is_never_sent_after_a_late_admission() {
    let (mut connection, mut peer, dir, listener) = connect_image("cancel");
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_poll_fd().is_some()
    });
    let accepted = accept_endpoint(&listener);
    let _hello = recv_packet(&accepted);
    let ready = ShellChannelReady {
        channel_generation: 3,
        granted_source_kinds: 1,
    }
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ChannelReady, &ready);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready)
    });

    let admission = connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .unwrap();
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    // Cancel before admission: room is reserved first, then the
    // producer marks the offer; the cancel record follows.
    let cancel_ticket = connection
        .image_cancel(admission.offer, Instant::now())
        .unwrap();
    assert!(
        connection
            .image_adapter()
            .unwrap()
            .offer(1)
            .unwrap()
            .cancel_requested
    );
    answer_submit(&mut connection, &mut peer, ShellFileKind::ResourceCancel);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(cancel_ticket) == Some(Custody::Submitted)
    });
    let cancel_record = peer.transactions.last().unwrap();
    assert_eq!(
        u16::from_le_bytes(cancel_record[6..8].try_into().unwrap()),
        ShellFileKind::ResourceCancel as u16
    );

    // A late admission does not revive the send.
    let body = ShellImageMetadata {
        offer_id: 1,
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
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ImageAdmitted, &body);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Admitted
    });
    // Nothing arrives on the side socket.
    let mut buf = [0u8; 1];
    let mut iov = [IoSliceMut::new(&mut buf)];
    let mut space = [std::mem::MaybeUninit::uninit(); 16];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    assert!(matches!(
        recvmsg(
            &accepted,
            &mut iov,
            &mut ancillary,
            RecvFlags::DONTWAIT | RecvFlags::PEEK
        ),
        Err(error) if error == rustix::io::Errno::AGAIN
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn estale_discovery_read_is_fatal_not_retried() {
    let (mut connection, mut peer, dir, _listener) = connect_image("estale");
    // ESTALE (116) from the node on the limits read.
    peer.object_read_error = Some(116);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    let adapter = connection.image_adapter().unwrap();
    assert_eq!(adapter.cause(), Some(ImageCloseCause::Discovery));
    assert_eq!(adapter.path(), Some(ImagePath::Error));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn eagain_discovery_read_is_fatal_not_absent_or_retried() {
    let (mut connection, mut peer, dir, _listener) = connect_image("eagain");
    // EAGAIN (11) from the node on the first file: not absence, and
    // no second pass is started.
    peer.object_read_error = Some(11);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    assert_eq!(
        connection.image_adapter().unwrap().cause(),
        Some(ImageCloseCause::Discovery)
    );
    let walks = peer.walk_count("image-channel");
    for _ in 0..50 {
        connection.poll_io().unwrap();
        peer.pump();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        peer.walk_count("image-channel"),
        walks,
        "a fatal discovery error starts no second pass"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn status_and_release_in_one_journal_read_keep_wire_order() {
    let (mut connection, mut peer, dir, listener) = connect_image("order");
    let accepted = drive_to_ready(&mut connection, &mut peer, &listener);
    let admission = connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .unwrap();
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    let body = ShellImageMetadata {
        offer_id: 1,
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
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ImageAdmitted, &body);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Sent
    });
    let _packet = recv_packet(&accepted);
    let _ = admission;

    // Accepted, then Released, in one journal read: the status must
    // be recorded before custody settles, and must survive it.
    let status_body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: ContentGrant {
                connection_epoch: EPOCH,
                content_grant_epoch: EPOCH,
            },
            resource: ContentResourceId {
                id: 21,
                generation: 1,
            },
            status: 2,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: 16,
        }),
    })
    .unwrap();
    let status = peer.event(ShellFileKind::ResourceStatus, &status_body);
    let released = ShellSourceReleased {
        offer_id: 1,
        resource: 21,
        resource_generation: 1,
        disposition: ShellSourceDisposition::Retained,
    }
    .encode()
    .unwrap();
    let released = peer.event_image(ShellFileKind::SourceReleased, &released);
    let mut journal = status;
    journal.extend(released);
    peer.push(journal);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Settled
    });
    let info = connection.image_adapter().unwrap().offer(1).unwrap();
    assert_eq!(
        info.status,
        Some(sophia_image_producer::ResourceStatusKind::Accepted),
        "the Accepted status precedes the release on the wire and is retained"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn oversized_formats_object_fails_discovery_at_the_cap() {
    let (mut connection, mut peer, dir, _listener) = connect_image_cap("oversize", 64);
    // One valid row makes the formats record larger than the
    // caller's 64-byte storage; the reader must fail the pass, not
    // truncate the object into the producer.
    peer.objects.insert(
        "image-formats".to_owned(),
        Object {
            qid: 23,
            bytes: formats_record_with_row(),
            chunk: usize::MAX,
        },
    );
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    assert_eq!(
        connection.image_adapter().unwrap().cause(),
        Some(ImageCloseCause::Discovery)
    );
    let _ = std::fs::remove_dir_all(dir);
}

fn formats_record_with_row() -> Vec<u8> {
    let body = ShellImageFormats {
        channel_generation: 3,
        formats_generation: 11,
        limits_generation: 9,
        rows: vec![ShellImageFormatRow {
            output: 1,
            device_major: 226,
            device_minor: 0,
            fourcc: 0x3432_4258,
            flags: 1,
            modifier: 0,
            max_width: 64,
            max_height: 64,
        }],
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

#[test]
fn invalid_producer_config_is_refused_at_construction() {
    let dir = temp_dir("badconfig");
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile()));
    // The startup deadline precedes the clock origin: the producer
    // cannot serve this epoch, and construction must say so.
    let mut config = image_config(dir.clone());
    config.startup_deadline_ms = 500;
    let origin = Instant::now();
    let result =
        ShellConnection::connect_files_with_image(&socket_path, options(), config, origin, 1_000);
    let _peer = thread.join().unwrap();
    assert!(matches!(
        result,
        Err(ShellClientError::InvalidArgument("image producer config"))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn backwards_dispatch_time_is_an_error_without_adapter_mutation() {
    let (mut connection, mut peer, dir, listener) = connect_image("clock");
    let _accepted = drive_to_ready(&mut connection, &mut peer, &listener);
    let before = connection.image_adapter().unwrap().channel();
    // One second before the construction origin (well under it, and
    // after any wire-level dispatch the handshake performed is on
    // the real timeline, so use a fresh connection drive instead):
    let origin_instant = Instant::now() - Duration::from_secs(3600);
    let result = connection.dispatch(origin_instant, 4096);
    assert!(matches!(
        result,
        Err(ShellClientError::InvalidArgument("image clock"))
    ));
    assert_eq!(connection.image_adapter().unwrap().channel(), before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rejected_backwards_drive_leaves_queued_work_unsent() {
    use file_wire_peer::CONTENT_GRANT_EPOCH;
    let (mut connection, mut peer, dir, listener) = connect_image("preflight");
    let _accepted = drive_to_ready(&mut connection, &mut peer, &listener);
    peer.reset_logs();
    // Queue ordinary content work without driving it.
    let admission = connection
        .enqueue_content_tracked(
            TransactionId::from_raw(9),
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: ContentGrant {
                    connection_epoch: EPOCH,
                    content_grant_epoch: CONTENT_GRANT_EPOCH,
                },
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                allocation: ContentAllocationId::default(),
                demand_id: 9,
                reason: 1,
            }),
        )
        .unwrap();
    assert_eq!(connection.custody(admission.first), Some(Custody::Queued));
    // A backwards drive is rejected before the wire transmits.
    let backwards = Instant::now() - Duration::from_secs(3600);
    assert!(matches!(
        connection.dispatch(backwards, 4096),
        Err(ShellClientError::InvalidArgument("image clock"))
    ));
    peer.pump();
    assert!(
        peer.transactions.is_empty(),
        "the rejected drive transmitted queued work"
    );
    assert_eq!(connection.custody(admission.first), Some(Custody::Queued));
    // A valid drive then sends it normally.
    drive(&mut connection, &mut peer, |_, peer| {
        !peer.transactions.is_empty()
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn live_forget_is_refused_and_cancellation_still_works() {
    let (mut connection, mut peer, dir, listener) = connect_image("forget");
    let _accepted = drive_to_ready(&mut connection, &mut peer, &listener);
    let admission = connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .unwrap();
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    // A live offer cannot be forgotten, and the refused forget must
    // not destroy the identity its cancellation needs.
    assert_eq!(
        connection.image_forget(admission.offer),
        Err(ImageAdapterError::Refused)
    );
    let ticket = connection
        .image_cancel(admission.offer, Instant::now())
        .unwrap();
    assert!(
        connection
            .image_adapter()
            .unwrap()
            .offer(admission.offer)
            .unwrap()
            .cancel_requested
    );
    answer_submit(&mut connection, &mut peer, ShellFileKind::ResourceCancel);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn wrong_transaction_status_is_delivered_but_never_mutates_custody() {
    let (mut connection, mut peer, dir, listener) = connect_image("wrongtx");
    let accepted = drive_to_ready(&mut connection, &mut peer, &listener);
    connection
        .image_begin(metadata(), sealed_fd(), Instant::now())
        .unwrap();
    answer_submit(&mut connection, &mut peer, ShellFileKind::ImageBegin);
    let body = ShellImageMetadata {
        offer_id: 1,
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
    .encode()
    .unwrap();
    let bytes = peer.event_image(ShellFileKind::ImageAdmitted, &body);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().state == OfferState::Sent
    });
    let _packet = recv_packet(&accepted);

    let status_body = |transaction: u64| {
        encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(transaction),
            record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant: ContentGrant {
                    connection_epoch: EPOCH,
                    content_grant_epoch: EPOCH,
                },
                resource: ContentResourceId {
                    id: 21,
                    generation: 1,
                },
                status: 2,
                reason: 0,
                next_ordinal: 0,
                admitted_bytes: 16,
            }),
        })
        .unwrap()
    };
    // Same resource, wrong transaction: delivered to the caller,
    // invisible to image custody.
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(99));
    peer.push(bytes);
    for _ in 0..50 {
        connection.poll_io().unwrap();
        peer.pump();
        if connection
            .image_adapter()
            .unwrap()
            .offer(1)
            .unwrap()
            .status
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let (_, delivered) = connection.take_content().unwrap().unwrap();
    assert!(matches!(delivered, ShellContentRecord::ResourceStatus(_)));
    assert_eq!(
        connection.image_adapter().unwrap().offer(1).unwrap().status,
        None,
        "a status naming another transaction must not mutate custody"
    );
    // The correctly named status still routes.
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(1));
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().offer(1).unwrap().status
            == Some(sophia_image_producer::ResourceStatusKind::Accepted)
    });
    let _ = std::fs::remove_dir_all(dir);
}
