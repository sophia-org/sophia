//! Generic protected-admission peer. The supervisor's test bootstrap moves the
//! original launch channel onto stdin without changing process identity.
use sophia_lock_client::{
    Connecting, SessionError, SessionState, journal::Event, protocol::lock_files::*,
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
    let stream = UnixStream::connect(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap()).unwrap();
    let mut auth =
        sophia_launch_client::Client::over(stream, relay, b"lock", b"lock-provider").unwrap();
    while !auth.ready() {
        assert!(Instant::now() < deadline, "authentication deadline");
        auth.poll().unwrap();
        auth.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = auth.into_authenticated().unwrap();
    let _relay = admitted.relay;
    let start = Instant::now();
    let mut connecting = Connecting::attached(
        admitted.role,
        admitted.root,
        LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
            chords: Vec::new(),
        },
        0,
        5000,
    )
    .unwrap();
    while !connecting.ready() {
        assert!(Instant::now() < deadline, "lock negotiation deadline");
        connecting
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut session = connecting.take_session().unwrap();
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.epoch(), 7);
    assert_eq!(
        session.welcome().unwrap().granted_capabilities,
        LOCK_FILE_CAPABILITY_PRESENT
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
    let upload = std::env::var("SOPHIA_TEST_LOCK_UPLOAD").as_deref() == Ok("1");
    let mut chunk_pointer = None;
    if upload {
        let begin = LockResourceBegin {
            transaction: 2,
            resource: LockResourceId {
                id: 1,
                generation: 1,
            },
            width_px: 2,
            height_px: 2,
            slot: 0,
        };
        loop {
            assert!(Instant::now() < deadline);
            match session.upload_begin(begin) {
                Ok(()) => break,
                Err(SessionError::Upload(sophia_lock_client::upload::Error::Busy)) => (),
                Err(e) => panic!("upload begin: {e:?}"),
            }
            session
                .dispatch(start.elapsed().as_millis() as u64, 65536)
                .unwrap();
        }
        let mut admitted = false;
        while !admitted || !session.upload_ready() {
            assert!(Instant::now() < deadline);
            session
                .dispatch(start.elapsed().as_millis() as u64, 65536)
                .unwrap();
            if let Some((_, event)) = session.event() {
                assert!(
                    matches!(event, Event::ResourceStatus(v) if v.status == LockResourceState::Admitted)
                );
                admitted = true;
                session.consume().unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // A real four-byte prefix has settled; the remaining resource is not
        // publishable. Queue its tail with no I/O, then wait for revocation.
        session.upload_chunk(vec![0x5a; 4]).unwrap();
        while !session.upload_ready() {
            assert!(Instant::now() < deadline);
            session
                .dispatch(start.elapsed().as_millis() as u64, 65536)
                .unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(session.take_upload_chunk().unwrap(), vec![0x5a; 4]);
        let tail = vec![0x5a; 12];
        chunk_pointer = Some(tail.as_ptr());
        session.upload_chunk(tail).unwrap();
        session
            .dispatch(start.elapsed().as_millis() as u64, 0)
            .unwrap();
        assert_eq!(session.obligations().upload_writes, 1);
        assert!(session.take_upload_chunk().is_none());
        std::fs::write(&report, b"upload-waiting\n").unwrap();
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
            sophia_lock_client::submission::Custody::Submitted
        ))
    );
    assert!(session.fd().is_none() && session.event().is_none());
    if upload {
        assert!(!session.upload_pending() && !session.upload_ready());
        assert_eq!(session.obligations().upload_writes, 0);
        assert!(session.upload_end().is_err() && session.upload_cancel().is_err());
        let returned = session.take_upload_chunk().unwrap();
        assert_eq!(Some(returned.as_ptr()), chunk_pointer);
    }
    std::fs::write(
        &report,
        if upload {
            b"upload-revoked\n".as_slice()
        } else {
            b"revoked\n".as_slice()
        },
    )
    .unwrap();
    session.close();
}
