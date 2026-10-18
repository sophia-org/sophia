//! Generic protected-admission peer. The supervisor's test bootstrap moves the
//! original launch channel onto stdin without changing process identity.
use sophia_image_producer::{ChannelState, ProducerMetadata, create_isb, finish_isb};
use sophia_lock_client::{
    Connecting, ImageError, LockImageConfig, SessionState, protocol::lock_files::*,
};
use std::{
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn main() {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let socket = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap());
    let image = std::env::var("SOPHIA_TEST_LOCK_IMAGE").as_deref() == Ok("1");
    let stream = UnixStream::connect(&socket).unwrap();
    let auth_clock = Instant::now();
    let mut auth =
        sophia_launch_client::Client::new(stream, relay, b"lock", b"lock-provider", 0, 5000)
            .unwrap();
    for round in 0..2 {
        while !auth.ready() {
            assert!(Instant::now() < deadline, "authentication deadline");
            auth.dispatch(auth_clock.elapsed().as_millis() as u64, 65536)
                .unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        if round == 0 {
            let now = auth_clock.elapsed().as_millis() as u64;
            auth = auth
                .into_authenticated()
                .unwrap()
                .reauthenticate_at(now, now + 5000)
                .unwrap();
        }
    }
    let admitted = auth.into_authenticated().unwrap();
    let _relay = admitted.relay;
    let start = Instant::now();
    let mut connecting = Connecting::attached(
        admitted.role,
        admitted.root,
        LockNegotiate {
            minimum_revision: if image { 2 } else { 1 },
            maximum_revision: if image { 2 } else { 1 },
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT
                | if image {
                    LOCK_FILE_CAPABILITY_SEALED_MEMORY
                } else {
                    0
                },
            chords: Vec::new(),
        },
        0,
        5000,
    )
    .unwrap();
    if image {
        connecting
            .enable_images(LockImageConfig {
                endpoint_dir: socket.parent().unwrap().to_owned(),
                startup_deadline_ms: 5000,
                transfer_timeout_ms: 5000,
                formats_capacity: 65536,
            })
            .unwrap();
    }
    while !connecting.ready() {
        assert!(Instant::now() < deadline, "lock negotiation deadline");
        connecting
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut session = connecting.take_session().unwrap();
    assert_eq!(session.state(), SessionState::Ready);
    let expected_epoch: u64 = std::env::var("SOPHIA_TEST_LOCK_EPOCH")
        .unwrap_or_else(|_| "7".to_owned())
        .parse()
        .unwrap();
    assert_eq!(session.epoch(), expected_epoch);
    assert_eq!(
        session.welcome().unwrap().granted_capabilities,
        LOCK_FILE_CAPABILITY_PRESENT
            | if image {
                LOCK_FILE_CAPABILITY_SEALED_MEMORY
            } else {
                0
            }
    );
    let mut observed = false;
    while !observed || session.obligations().acked < 3 || session.obligations().deadline.is_some() {
        assert!(Instant::now() < deadline, "lock/ACK deadline");
        session
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        if session.event().is_some() {
            assert_eq!(session.lock().unwrap().1.phase, LockPhase::Locked);
            assert_eq!(session.lock().unwrap().1.lock_epoch, 3);
            session.consume().unwrap();
            observed = true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let report = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    let upload = image;
    if image {
        while session.image().unwrap().producer().channel() != ChannelState::Ready {
            assert!(Instant::now() < deadline, "image channel deadline");
            session
                .dispatch(start.elapsed().as_millis() as u64, 65536)
                .unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let source = create_isb(2, 2).unwrap();
        source.write_pixels(0, &[0x5a; 16]).unwrap();
        finish_isb(&source).unwrap();
        let metadata = ProducerMetadata {
            offer_id: 0,
            resource: 1,
            resource_generation: 1,
            admission_epoch: 3,
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
        };
        let mut source = source.into_fd();
        loop {
            assert!(Instant::now() < deadline);
            match session.image_begin(metadata, source, start.elapsed().as_millis() as u64) {
                Ok(_) => break,
                Err(e) if e.error == ImageError::Busy => {
                    source = e.source.unwrap();
                    session
                        .dispatch(start.elapsed().as_millis() as u64, 65536)
                        .unwrap();
                }
                Err(e) => panic!("image begin: {e:?}"),
            }
        }
        // The source belongs to the producer, but the role has not received
        // ImageBegin. Revoke before any submission custody can be established.
        session
            .dispatch(start.elapsed().as_millis() as u64, 0)
            .unwrap();
        assert_eq!(
            session.latest_submission(),
            Some((2, sophia_lock_client::submission::Custody::Staged))
        );
        std::fs::write(&report, b"image-waiting\n").unwrap();
        while std::fs::read_to_string(&report).unwrap() != "continue\n" {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    } else {
        std::fs::write(&report, b"waiting\n").unwrap();
    }
    while session.error().is_none() {
        assert!(Instant::now() < deadline, "revocation deadline");
        let _ = session.dispatch(start.elapsed().as_millis() as u64, 65536);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        session.state(),
        SessionState::Closed | SessionState::Stale
    ));
    assert_eq!(
        session.latest_submission(),
        Some((
            if upload { 2 } else { 1 },
            if upload {
                sophia_lock_client::submission::Custody::DroppedUnsent
            } else {
                sophia_lock_client::submission::Custody::Submitted
            }
        ))
    );
    assert!(session.fd().is_none() && session.event().is_none());
    std::fs::write(
        &report,
        if upload {
            b"image-revoked\n".as_slice()
        } else {
            b"revoked\n".as_slice()
        },
    )
    .unwrap();
    session.close();
}
