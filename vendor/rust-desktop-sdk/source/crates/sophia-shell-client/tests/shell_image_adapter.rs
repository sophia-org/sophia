#![allow(dead_code)]

//! Content-role owned-ring adapters against the scripted file peer:
//! discovery over the session's own 9P wire, endpoint connect, Hello,
//! ChannelReady(4), one control registration, the D4F1 source crossing,
//! routed ResourceStatus and SlotReleased settlement. Plus the absent,
//! malformed and no-adapter controls.

#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use file_wire_peer::{EPOCH, Object, Peer, Profile};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

#[path = "support/image_adapter_peer.rs"]
mod image_peer;
use image_peer::*;
use sophia_shell_client::{NativeLauncherLifecycle, NativeLifecycleState, NativeResourceState};

#[path = "support/native_image_adoption.rs"]
mod native_image_adoption;

fn upload() -> RingUpload {
    RingUpload {
        resource: 21,
        resource_generation: 1,
        allocation: 31,
        allocation_generation: 1,
        output: 41,
        output_generation: 1,
        topology_generation: 1,
        width: 2,
        height: 1,
    }
}

#[test]
fn ring_offer_runs_end_to_end() {
    let (mut connection, mut peer, dir, listener) = connect_image("flow");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    // The one registration: default one slot at grant capacity.
    assert_eq!(peer.control_writes.len(), 1);
    let registration = RingRegistration::decode(&peer.control_writes[0]).unwrap();
    assert_eq!(registration.identity, RING_IDENTITY);
    assert_eq!(registration.slots, 1);
    assert_eq!(registration.capacity, 1 << 20);
    let (source_fd, _size) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);

    let pixels = [7u8; 8];
    let offer = connection
        .image_upload_ring(upload(), &pixels, Instant::now())
        .unwrap();
    assert_eq!(offer, 1);
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    let offered = match RingControl::decode(&peer.control_writes[1]).unwrap() {
        RingControl::Offer(offer) => offer,
        RingControl::Register(_) => panic!("second control write is the offer"),
    };
    assert_eq!(offered.offer_id, 1);
    assert_eq!(offered.slot_generation, 2);
    assert_eq!(offered.resource, 21);

    // Status first, then the exact release: settled and adoptable.
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(1, 21, 1, 2));
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(1).unwrap().status
            == Some(sophia_image_producer::ResourceStatusKind::Accepted)
    });
    assert_eq!(
        connection.image_ring_offer(1).unwrap().state,
        RingOfferState::Pending,
        "a status alone does not settle adoption"
    );
    let released = ring_released(peer.take_sequence(), 1, 2, 21, 1, 0);
    peer.push(released);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(1).unwrap().state == RingOfferState::Settled
    });
    connection.image_forget(1).unwrap();
    assert!(connection.image_ring_offer(1).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_retire_replies_do_not_spend_ring_settlement() {
    let (mut connection, mut peer, dir, listener) = connect_image("retire-replies");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let offer = connection
        .image_upload_ring(upload(), &[3u8; 8], Instant::now())
        .unwrap();
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    // Failed ResourceRetire replies share the ResourceStatus kind,
    // with the offer transaction and with an unrelated transaction.
    // Neither settles, frees the slot, or faults the ring.
    for transaction in [offer, 777] {
        let bytes = peer.event(
            ShellFileKind::ResourceStatus,
            &status_body(transaction, 21, 1, 3),
        );
        peer.push(bytes);
    }
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().status
            == Some(sophia_image_producer::ResourceStatusKind::Rejected)
    });
    assert_eq!(
        connection.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    assert_eq!(
        connection.image_upload_ring(upload(), &[3u8; 8], Instant::now()),
        Err(ImageAdapterError::Busy),
        "the slot stays in custody until the exact release"
    );
    // The genuine final status supersedes the control replies, then
    // the exact SlotReleased settles the offer for adoption.
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(offer, 21, 1, 2));
    peer.push(bytes);
    let released = ring_released(peer.take_sequence(), offer, 2, 21, 1, 0);
    peer.push(released);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
    });
    connection.image_forget(offer).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_reply_after_final_status_cannot_poison_settlement() {
    let (mut connection, mut peer, dir, listener) = connect_image("reply-after-final");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let offer = connection
        .image_upload_ring(upload(), &[3u8; 8], Instant::now())
        .unwrap();
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    // The genuine final status arrives first, then a failed control
    // reply under an unrelated transaction. The exact successful
    // SlotReleased selects the Accepted evidence and settles. The
    // journal is FIFO, so the release queued now is processed only
    // after both statuses.
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(offer, 21, 1, 2));
    peer.push(bytes);
    let bytes = peer.event(ShellFileKind::ResourceStatus, &status_body(555, 21, 1, 3));
    peer.push(bytes);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().status
            == Some(sophia_image_producer::ResourceStatusKind::Accepted)
    });
    assert_eq!(
        connection.image_ring_offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    let released = ring_released(peer.take_sequence(), offer, 2, 21, 1, 0);
    peer.push(released);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().state == RingOfferState::Settled
    });
    // Terminal info reports the selected Accepted evidence, not the
    // last observed failed-reply status.
    assert_eq!(
        connection.image_ring_offer(offer).unwrap().status,
        Some(sophia_image_producer::ResourceStatusKind::Accepted)
    );
    connection.image_forget(offer).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn release_before_status_fails_the_ring() {
    let (mut connection, mut peer, dir, listener) = connect_image("order2");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let offer = connection
        .image_upload_ring(upload(), &[3u8; 8], Instant::now())
        .unwrap();
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    // The contract orders final status before SlotReleased: an exact
    // release with no status yet fails the ring, custody Unknown.
    let released = ring_released(peer.take_sequence(), offer, 2, 21, 1, 0);
    peer.push(released);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().state == RingOfferState::Unknown
    });
    assert_eq!(
        connection.image_adapter().unwrap().channel(),
        Some(ImageChannelState::Closed)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dock_and_launcher_enable_ring_from_negotiated_limits() {
    for (role, caps) in [("dock", 0x11a2), ("launcher", 0x9a0)] {
        let (mut connection, mut peer, dir, listener) = connect_role_image(
            &format!("role-{role}"),
            4096,
            Profile {
                role,
                revision: 10,
                capabilities: caps | OWNED,
                limits_published: true,
            },
            true,
        );
        let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
        let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
        drop(source_fd);
        assert!(connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Ready));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn absent_discovery_ends_image_admission_without_fallback() {
    let (mut connection, mut peer, dir, _listener) = connect_image("absent");
    peer.error_nodes.insert("image-channel".to_owned(), 2);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    let adapter = connection.image_adapter().unwrap();
    assert_eq!(adapter.cause(), Some(ImageCloseCause::Absent));
    assert_eq!(adapter.path(), Some(ImagePath::Error));
    assert!(matches!(
        connection.enable_image(image_config(dir.clone()), Instant::now(), 1_000),
        Err(ShellClientError::InvalidArgument(
            "image adapter already enabled"
        ))
    ));
    assert_eq!(
        connection.image_upload_ring(upload(), &[0u8; 8], Instant::now()),
        Err(ImageAdapterError::Closed)
    );
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
fn no_adapter_connection_refuses_uploads() {
    let dir = temp_dir("none");
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile()));
    let connection = ShellConnection::connect_files(&socket_path, options());
    let _peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
    let mut connection = connection.unwrap();
    assert!(connection.image_adapter().is_none());
    assert_eq!(
        connection.image_upload_ring(upload(), &[0u8; 8], Instant::now()),
        Err(ImageAdapterError::NotEnabled)
    );
    // The withdrawn sealed entry refuses explicitly and returns the fd.
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
fn refused_control_open_clunks_its_fid() {
    let (mut connection, mut peer, dir, listener) = connect_image("open-refused");
    peer.control_open_error = Some(13);
    let _accepted = drive_to_image_hello(&mut connection, &mut peer, &listener);
    push_channel_ready(&mut peer);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    assert_eq!(
        connection.image_adapter().unwrap().cause(),
        Some(ImageCloseCause::Fault)
    );
    assert!(peer.clunks >= 1, "the refused fid was clunked");
    assert_eq!(
        connection.image_upload_ring(upload(), &[0u8; 8], Instant::now()),
        Err(ImageAdapterError::Closed)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ring_refused_is_final_before_any_upload() {
    let (mut connection, mut peer, dir, listener) = connect_image("refused");
    let _accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let refused = ring_event_kind(RingEventKind::RingRefused, peer.take_sequence());
    peer.push(refused);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
    });
    assert_eq!(
        connection.image_upload_ring(upload(), &[0u8; 8], Instant::now()),
        Err(ImageAdapterError::Closed)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancellation_settles_by_resource_never_adopts() {
    let (mut connection, mut peer, dir, listener) = connect_image("cancel");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let offer = connection
        .image_upload_ring(upload(), &[5u8; 8], Instant::now())
        .unwrap();
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    // Live forget is refused and destroys nothing.
    assert_eq!(
        connection.image_forget(offer),
        Err(ImageAdapterError::Refused)
    );
    let ticket = connection.image_cancel(offer, Instant::now()).unwrap();
    answer_submit(&mut connection, &mut peer, ShellFileKind::ResourceCancel);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.custody(ticket) == Some(Custody::Submitted)
    });
    // The cancel status carries the cancel transaction (999), not the
    // offer id; matching is by grant and resource.
    let status = peer.event(ShellFileKind::ResourceStatus, &status_body(999, 21, 1, 4));
    peer.push(status);
    let released = ring_released(peer.take_sequence(), offer, 2, 21, 1, 11);
    peer.push(released);
    drive(&mut connection, &mut peer, |connection, _| {
        connection.image_ring_offer(offer).unwrap().state == RingOfferState::Refused
    });
    connection.image_forget(offer).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn over_capacity_upload_is_refused_before_any_write() {
    let (mut connection, mut peer, dir, listener) = connect_image("capacity");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let mut big = upload();
    big.width = 1024;
    big.height = 257; // 1024 * 257 * 4 > 1 MiB capacity
    let pixels = vec![0u8; 1024 * 257 * 4];
    assert_eq!(
        connection.image_upload_ring(big, &pixels, Instant::now()),
        Err(ImageAdapterError::Refused)
    );
    assert_eq!(
        peer.control_writes.len(),
        1,
        "only the registration was written"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn estale_discovery_read_is_fatal_not_retried() {
    let (mut connection, mut peer, dir, _listener) = connect_image("estale");
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
fn oversized_formats_object_fails_discovery_at_the_cap() {
    let (mut connection, mut peer, dir, _listener) = connect_image_cap("oversize", 64);
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
fn invalid_ring_config_is_refused_at_construction() {
    let dir = temp_dir("badconfig");
    let socket_path = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let thread = std::thread::spawn(move || Peer::handshake(listener, profile()));
    let mut config = image_config(dir.clone());
    config.content_grant_epoch = 0;
    let origin = Instant::now();
    let result =
        ShellConnection::connect_files_with_image(&socket_path, options(), config, origin, 1_000);
    let _peer = thread.join().unwrap();
    assert!(matches!(
        result,
        Err(ShellClientError::InvalidArgument("image ring config"))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn backwards_dispatch_time_is_an_error_without_adapter_mutation() {
    let (mut connection, mut peer, dir, listener) = connect_image("clock");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    drop(source_fd);
    let before = connection.image_adapter().unwrap().channel();
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
fn deferred_setup_cannot_add_an_unnegotiated_image_grant() {
    for (index, revision, caps) in [(0, 8, BAR), (1, 9, BAR | OWNED), (2, 10, BAR)] {
        let dir = temp_dir(&format!("grant-{index}"));
        let path = dir.join("shell.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let profile = Profile {
            role: "bar",
            revision,
            capabilities: caps,
            limits_published: false,
        };
        let thread = std::thread::spawn(move || Peer::handshake(listener, profile));
        let mut connection = ShellConnection::connect_files(
            &path,
            ShellClientOptions {
                minimum_revision: revision,
                maximum_revision: revision,
                required_capabilities: caps,
                handshake_timeout: Duration::from_secs(5),
            },
        )
        .unwrap();
        let _peer = thread.join().unwrap();
        assert!(matches!(
            connection.enable_image(image_config(dir.clone()), Instant::now(), 1_000),
            Err(ShellClientError::MissingCapability)
        ));
        assert!(connection.image_adapter().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
