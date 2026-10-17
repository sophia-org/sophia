//! Slice 4b over real sockets: Sources sent with their sealed memfd reach
//! the transport's validation worker, completions settle offers, a retired
//! sealed image stays charged until its last reader, an ended epoch's
//! images hold the next epoch's budget, and dropping the service never
//! waits for a reader.
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use sophia_runtime::SharedPixels;

const PIXELS: [u8; 64] = [9; 64];

fn memfd(bytes: &[u8], sealed: bool) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "lock-source",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    if sealed {
        fcntl_add_seals(
            &file,
            SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
        )
        .unwrap();
    }
    file.into()
}

/// Sends offer `offer`'s Source on the image channel with `source`.
fn send_source(socket: &OwnedFd, epoch: u64, offer: u64, source: &OwnedFd) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    let metadata = offer_metadata(offer);
    let unused = LockImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    };
    let mut planes = [unused; LOCK_IMAGE_MAX_PLANES];
    planes[0].stride = metadata.stride;
    let bytes = LockImagePacket::Source {
        connection_epoch: epoch,
        channel_generation: epoch,
        metadata,
        planes,
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    let sent = sendmsg(
        socket,
        &[std::io::IoSlice::new(&bytes)],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
    assert_eq!(sent, bytes.len());
}

/// The pixels of the first `ResourceReady` the transport delivered.
fn ready_pixels(transport: &mut LockFileTransport) -> SharedPixels {
    while let Some(inbound) = transport.take_inbound() {
        if let LockInbound::ResourceReady { pixels, .. } = inbound {
            return pixels;
        }
    }
    panic!("no resource ready");
}

fn accepted() -> (LockResourceState, u16, LockSourceDisposition) {
    (
        LockResourceState::Accepted,
        0,
        LockSourceDisposition::Retained,
    )
}

#[test]
fn a_sealed_source_becomes_a_resource_released_after_its_last_reader() {
    let (mut transport, directory) = transport("source");
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let (files, socket, provider) = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.admitted(1);
            send_source(&socket, EPOCH, 1, &memfd(&PIXELS, true));
            assert_eq!(files.settled(1), accepted());
            (files, socket, provider)
        }),
    );
    let mut held = Some(ready_pixels(&mut transport));
    assert_eq!(&held.as_ref().unwrap()[..], &PIXELS[..]);
    assert_eq!(transport.image_jobs_pending(), 0);
    // Retire it while Session still holds a clone: the release waits.
    let retired = Arc::new(AtomicBool::new(false));
    let seen = retired.clone();
    let files = pump_with(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = files;
            let step = LockResourceStep {
                transaction: 30,
                resource: LockResourceId {
                    id: 21,
                    generation: 1,
                },
                total_bytes: None,
            };
            assert_eq!(
                files.submit(LockFileKind::ResourceRetire, &step.encode().unwrap(), 2),
                None
            );
            assert_eq!(files.next().0, LockFileKind::Submitted);
            retired.store(true, Ordering::Release);
            let (kind, body) = files.next();
            assert_eq!(kind, LockFileKind::ResourceReleased);
            assert_eq!(
                LockResourceReleased::decode(&body).unwrap().resource,
                step.resource
            );
            files
        }),
        |transport| {
            if seen.load(Ordering::Acquire) && held.is_some() {
                // Several turns pass with the clone held; nothing releases.
                for _ in 0..5 {
                    let _ = transport.turn();
                }
                assert_eq!(transport.sealed_leases_pending(), 1);
                held = None;
            }
        },
    );
    assert!(held.is_none());
    assert_eq!(transport.sealed_leases_pending(), 0);
    drop((files, socket, provider));
}

fn run_refused_source(label: &str, bytes: &[u8], sealed: bool) {
    let (mut transport, directory) = transport(label);
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let source = memfd(bytes, sealed);
    let provider = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.admitted(1);
            send_source(&socket, EPOCH, 1, &source);
            assert_eq!(
                files.settled(1),
                (
                    LockResourceState::Rejected,
                    LOCK_RESOURCE_REASON_INVALID_SOURCE,
                    LockSourceDisposition::RefusedBeforeUse,
                )
            );
            // The worker credit came back: the next offer is admitted.
            files.admitted(2);
            provider
        }),
    );
    assert_eq!(transport.image_jobs_pending(), 1);
    drop(provider);
}

#[test]
fn an_unsealed_source_is_refused_as_invalid() {
    run_refused_source("unsealed", &PIXELS, false);
}

#[test]
fn a_short_source_is_refused_as_invalid() {
    run_refused_source("short", &PIXELS[..32], true);
}

#[test]
fn a_source_for_an_unadmitted_offer_faults_the_channel() {
    let (mut transport, directory) = transport("unowed");
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let provider = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            send_source(&socket, EPOCH, 5, &memfd(&PIXELS, true));
            files.closed(LockChannelClosedReason::ProtocolFault);
            provider
        }),
    );
    drop(provider);
}

/// An upload of 4x4 pixels into slot 0; returns its status and reason.
fn begin_upload(files: &mut Files, transaction: u64) -> (LockResourceState, u16) {
    let begin = LockResourceBegin {
        transaction,
        resource: LockResourceId {
            id: 60 + transaction,
            generation: 1,
        },
        width_px: 4,
        height_px: 4,
        slot: 0,
    };
    assert_eq!(
        files.submit(LockFileKind::ResourceBegin, &begin.encode().unwrap(), 2),
        None
    );
    assert_eq!(files.next().0, LockFileKind::Submitted);
    let (kind, body) = files.next();
    assert_eq!(kind, LockFileKind::ResourceStatus);
    let status = LockResourceStatus::decode(&body).unwrap();
    (status.status, status.reason)
}

#[test]
fn an_ended_epochs_held_image_charges_the_next_epoch_until_released() {
    // Two live places; the ended epoch's image keeps one of them. Its
    // bytes are charged the same way (lock_image_leases.rs).
    let (mut transport, directory) = transport("orphan");
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let (files, socket, mut provider) = pump(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream);
            let socket = ready(&mut files, &mut provider);
            files.admitted(1);
            send_source(&socket, EPOCH, 1, &memfd(&PIXELS, true));
            assert_eq!(files.settled(1), accepted());
            (files, socket, provider)
        }),
    );
    let mut held = Some(ready_pixels(&mut transport));
    drop((files, socket));
    provider.exit();
    let deadline = Instant::now() + Duration::from_secs(5);
    while transport.turn().unwrap_or(false) {
        assert!(Instant::now() < deadline, "connection did not end");
        std::thread::sleep(Duration::from_millis(1));
    }
    transport.disconnect().unwrap();
    // The image Session still holds is an orphan between epochs.
    assert_eq!(transport.sealed_leases_pending(), 1);
    transport.poll_detached_images();
    assert_eq!(transport.sealed_leases_pending(), 1);
    drop(provider);

    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let refused = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let (seen, release) = (refused.clone(), released.clone());
    let provider = pump_with(
        &mut transport,
        std::thread::spawn(move || {
            let mut files = Files::attach(stream).at_epoch(EPOCH + 1);
            let socket = ready(&mut files, &mut provider);
            // The orphan and this offer fill both places.
            files.admitted(2);
            assert_eq!(
                begin_upload(&mut files, 40),
                (LockResourceState::Rejected, 6)
            );
            refused.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(5);
            while !released.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "orphan never released");
                std::thread::sleep(Duration::from_millis(1));
            }
            // Session let go: the place is free again.
            assert_eq!(
                begin_upload(&mut files, 41),
                (LockResourceState::Admitted, 0)
            );
            drop(socket);
            provider
        }),
        |transport| {
            if seen.load(Ordering::Acquire) && held.is_some() {
                assert_eq!(transport.sealed_leases_pending(), 1);
                held = None;
                release.store(true, Ordering::Release);
            }
        },
    );
    assert_eq!(transport.sealed_leases_pending(), 0);
    drop(provider);
}

#[test]
fn dropping_the_service_does_not_wait_for_a_held_image() {
    let (mut transport, directory) = transport("service-drop");
    transport.enable_sealed_images(policy(2000)).unwrap();
    let (mut provider, stream) = Provider::start(&mut transport, &directory);
    let service = LockFileService::spawn(transport, locked(), Vec::new()).unwrap();
    let peer = std::thread::spawn(move || {
        let mut files = Files::attach(stream);
        let socket = ready(&mut files, &mut provider);
        files.admitted(1);
        send_source(&socket, EPOCH, 1, &memfd(&PIXELS, true));
        assert_eq!(files.settled(1), accepted());
        (files, socket, provider)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let held = loop {
        assert!(Instant::now() < deadline, "no resource ready");
        if let Ok(LockFileServiceEvent::Inbound {
            inbound: LockInbound::ResourceReady { pixels, .. },
            ..
        }) = service.event_timeout(Duration::from_millis(50))
        {
            break pixels;
        }
    };
    let peer = peer.join().unwrap();
    // A queued event may hold another clone too; neither blocks the drop.
    let (dropped, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(service);
        dropped.send(()).unwrap();
    });
    assert!(
        done.recv_timeout(Duration::from_secs(5)).is_ok(),
        "service drop waited for a reader"
    );
    assert_eq!(&held[..], &PIXELS[..]);
    drop((held, peer));
}
