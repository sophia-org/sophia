use super::*;
use std::os::unix::fs::FileExt;

#[test]
fn worker_fill_crosses_the_real_ring_and_exact_release_protocol() {
    let (mut connection, mut peer, dir, listener) = connect_image("owned-worker");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (source_fd, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    let source = std::fs::File::from(source_fd);
    let mut loan = connection
        .image_acquire_ring_fill(8, Instant::now())
        .unwrap();
    let worker = std::thread::spawn(move || {
        loan.pixels_mut()
            .unwrap()
            .copy_from_slice(&[2, 3, 5, 7, 11, 13, 17, 19]);
        loan
    });
    connection.poll_io().unwrap();
    peer.pump();
    assert_eq!(
        peer.control_writes.len(),
        1,
        "acquisition is not publication"
    );
    let mut loan = worker.join().unwrap();
    let invalid = RingUpload {
        width: 3,
        ..upload()
    };
    assert!(
        connection
            .image_commit_ring_fill(&mut loan, invalid, Instant::now())
            .is_err()
    );
    assert_eq!(loan.pixels_mut().unwrap(), &[2, 3, 5, 7, 11, 13, 17, 19]);
    let offer = connection
        .image_commit_ring_fill(&mut loan, upload(), Instant::now())
        .unwrap();
    assert_eq!(offer, 1);
    assert!(loan.pixels_mut().is_err());
    drive(&mut connection, &mut peer, |_, peer| {
        peer.control_writes.len() == 2
    });
    let offered = match RingControl::decode(&peer.control_writes[1]).unwrap() {
        RingControl::Offer(v) => v,
        _ => panic!("expected offer"),
    };
    assert_eq!(offered.slot_generation, 2);
    let mut pixels = [0; 8];
    source
        .read_exact_at(&mut pixels, OWNED_RING_HEADER_BYTES)
        .unwrap();
    assert_eq!(pixels, [2, 3, 5, 7, 11, 13, 17, 19]);
    let status = peer.event(ShellFileKind::ResourceStatus, &status_body(1, 21, 1, 2));
    peer.push(status);
    drive(&mut connection, &mut peer, |c, _| {
        c.image_ring_offer(1).unwrap().status.is_some()
    });
    assert_eq!(
        connection.image_ring_offer(1).unwrap().state,
        RingOfferState::Pending
    );
    assert!(
        connection
            .image_acquire_ring_fill(8, Instant::now())
            .is_err()
    );
    let released = ring_released(peer.take_sequence(), 1, 2, 21, 1, 0);
    peer.push(released);
    drive(&mut connection, &mut peer, |c, _| {
        c.image_ring_offer(1).unwrap().state == RingOfferState::Settled
    });
    connection.image_forget(1).unwrap();
    let mut next = connection
        .image_acquire_ring_fill(8, Instant::now())
        .unwrap();
    connection.image_abort_ring_fill(&mut next).unwrap();
    assert!(next.pixels_mut().is_err());
    assert_eq!(peer.control_writes.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn borrowed_and_slice_uploads_survive_an_owned_abort() {
    let (mut connection, mut peer, dir, listener) = connect_image("owned-abort");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (_source, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    let mut loan = connection
        .image_acquire_ring_fill(8, Instant::now())
        .unwrap();
    assert!(
        connection
            .image_upload_ring(upload(), &[0; 8], Instant::now())
            .is_err()
    );
    let mut called = false;
    assert!(
        connection
            .image_upload_ring_fill(upload(), 8, Instant::now(), |_| called = true)
            .is_err()
    );
    assert!(!called);
    connection.image_abort_ring_fill(&mut loan).unwrap();
    let offer = connection
        .image_upload_ring_fill(upload(), 8, Instant::now(), |p| p[..8].fill(29))
        .unwrap();
    assert_eq!(offer, 1);
    drive(&mut connection, &mut peer, |_, p| {
        p.control_writes.len() == 2
    });
    let status = peer.event(ShellFileKind::ResourceStatus, &status_body(1, 21, 1, 2));
    peer.push(status);
    drive(&mut connection, &mut peer, |c, _| {
        c.image_ring_offer(1).unwrap().status.is_some()
    });
    let release = ring_released(peer.take_sequence(), 1, 2, 21, 1, 0);
    peer.push(release);
    drive(&mut connection, &mut peer, |c, _| {
        c.image_ring_offer(1).unwrap().state == RingOfferState::Settled
    });
    assert_eq!(
        connection
            .image_upload_ring(upload(), &[31; 8], Instant::now())
            .unwrap(),
        2
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dropped_worker_loan_is_observable_without_any_wire_event() {
    let (mut connection, mut peer, dir, listener) = connect_image("owned-drop");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (_source, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    let loan = connection
        .image_acquire_ring_fill(8, Instant::now())
        .unwrap();
    std::thread::spawn(move || drop(loan)).join().unwrap();
    assert_eq!(
        connection.image_adapter().unwrap().channel(),
        Some(ImageChannelState::Closed)
    );
    assert_eq!(
        connection.image_adapter().unwrap().cause(),
        Some(ImageCloseCause::Fault)
    );
    connection.poll_io().unwrap();
    assert!(
        connection
            .image_acquire_ring_fill(8, Instant::now())
            .is_err()
    );
    assert_eq!(peer.control_writes.len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wire_disconnect_refuses_a_completed_fill_and_allows_abort() {
    let (mut connection, mut peer, dir, listener) = connect_image("owned-disconnect");
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (_source, _) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    let mut loan = connection
        .image_acquire_ring_fill(8, Instant::now())
        .unwrap();
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(2);
    while connection.image_adapter().unwrap().channel() != Some(ImageChannelState::Closed) {
        assert!(
            Instant::now() < deadline,
            "owner did not observe disconnection"
        );
        if connection.poll_io().is_err() {
            break;
        }
        std::thread::yield_now();
    }
    loan.pixels_mut().unwrap().fill(37);
    assert_eq!(
        connection.image_commit_ring_fill(&mut loan, upload(), Instant::now()),
        Err(ImageAdapterError::Closed)
    );
    connection.image_abort_ring_fill(&mut loan).unwrap();
    assert!(loan.pixels_mut().is_err());
    assert!(
        connection
            .image_acquire_ring_fill(8, Instant::now())
            .is_err()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
