//! Shared service driver tests with synthetic protection evidence. Actual
//! supervised launch remains a separate gate.
use sophia_protocol::{
    NamespaceProfile, SurfaceId,
    broker_files::{BrokerFileLimits, BrokerFileRequest},
};
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole,
    broker_files::BrokerFileListener,
};
use sophia_session::{
    broker_file_driver::BrokerFileDriver,
    broker_file_owner::{BrokerCompletion, BrokerFileOwner},
};
use std::{
    io::Read,
    os::unix::net::UnixStream,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn fixture() -> (BrokerFileDriver<u64>, UnixStream) {
    let path = std::env::temp_dir().join(format!(
        "sophia-broker-driver-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = BrokerFileListener::bind(path, rustix::process::geteuid().as_raw()).unwrap();
    let stream = UnixStream::connect(listener.socket_path()).unwrap();
    let evidence = ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataBroker].into_iter().collect(),
    };
    let service = listener
        .accept(
            BrokerFileOwner::new(7).unwrap(),
            &evidence,
            7,
            1,
            0,
            BrokerFileLimits::default(),
            Duration::from_secs(1),
        )
        .unwrap();
    (BrokerFileDriver::new(service), stream)
}

#[test]
fn stalled_startup_times_out_and_closes_the_socket() {
    let (mut driver, mut peer) = fixture();
    let error = driver.negotiate(Duration::from_millis(20)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(driver.closed());
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    assert!(driver.poll().is_err());
}

#[test]
fn admitted_request_reaches_owner_and_receipt_cleanup_is_driven() {
    use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
    let (mut driver, stream) = fixture();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut wire =
            Pipeline::over(stream, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
        macro_rules! reply {
            ($tag:expr) => {{
                let tag = $tag.unwrap();
                wire.wait(tag, Instant::now() + Duration::from_secs(2))
                    .unwrap()
            }};
        }
        let (tag, root) = wire.attach(b"", b"").unwrap();
        assert!(matches!(reply!(Ok::<_, ()>(tag)), Reply::Attach(_)));
        macro_rules! open {
            ($name:expr,$flags:expr) => {{
                let (tag, fid) = wire.walk(root, &[$name]).unwrap();
                assert!(matches!(reply!(Ok::<_, ()>(tag)), Reply::Walk(_)));
                assert!(matches!(
                    reply!(wire.lopen(fid, $flags)),
                    Reply::Lopen { .. }
                ));
                fid
            }};
        }
        let tx = open!(b"transaction", 2);
        let submit = open!(b"submit", 1);
        let ack = open!(b"ack", 1);
        let events = open!(b"events", 0);
        let mut hello = [0; 40];
        hello[0] = 40;
        hello[4] = 1;
        hello[7] = 1;
        hello[8] = 7;
        hello[16] = 1;
        hello[32] = 2;
        hello[34] = 2;
        assert!(matches!(
            reply!(wire.write(tx, 0, &hello)),
            Reply::Write(40)
        ));
        let mut control = [0; 24];
        control[0] = 7;
        control[8] = 1;
        control[16] = 40;
        assert!(matches!(
            reply!(wire.write(submit, 0, &control)),
            Reply::Write(24)
        ));
        let Reply::Read(bytes) = reply!(wire.read(events, 0, 4096)) else {
            panic!()
        };
        assert_eq!(bytes.len(), 104);
        let mut ack_bytes = [0; 16];
        ack_bytes[0] = 7;
        ack_bytes[8] = 2;
        assert!(matches!(
            reply!(wire.write(ack, 0, &ack_bytes)),
            Reply::Write(16)
        ));
        assert!(matches!(reply!(wire.clunk(tx)), Reply::Clunk));
        let Reply::Read(bytes) = reply!(wire.read(events, 104, 4096)) else {
            panic!()
        };
        assert_eq!(bytes.len(), 200);
        assert_eq!((bytes[32], bytes[40]), (1, 1));
        let mut response = [0; 448];
        response[..4].copy_from_slice(&448u32.to_le_bytes());
        response[4] = 1;
        response[6] = 1;
        response[7] = 1;
        response[8] = 7;
        response[16] = 2;
        response[32] = 1;
        response[40] = 1;
        response[42] = 1;
        response[48] = 1;
        response[56] = 3;
        response[60] = 4;
        response[64] = 2;
        response[66] = 2;
        let tx = open!(b"transaction", 2);
        assert!(matches!(
            reply!(wire.write(tx, 0, &response)),
            Reply::Write(448)
        ));
        control[8] = 2;
        control[16..20].copy_from_slice(&448u32.to_le_bytes());
        assert!(matches!(
            reply!(wire.write(submit, 0, &control)),
            Reply::Write(24)
        ));
        let Reply::Read(bytes) = reply!(wire.read(events, 304, 4096)) else {
            panic!()
        };
        assert_eq!(bytes.len(), 48);
        ack_bytes[8] = 4;
        assert!(matches!(
            reply!(wire.write(ack, 0, &ack_bytes)),
            Reply::Write(16)
        ));
        assert!(matches!(reply!(wire.clunk(tx)), Reply::Clunk));
        done_tx.send(()).unwrap();
        stop_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    let surface = SurfaceId::new(3, 4);
    let completion = driver
        .request(
            BrokerFileRequest::SurfaceAdmitted {
                surface,
                profile: NamespaceProfile::ClassicShared,
            },
            42,
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(matches!(
        completion,
        BrokerCompletion::Applied {
            request: 1,
            rule: Some(_),
            retired: None
        }
    ));
    assert_eq!(driver.owner().route(surface), Some(42));
    let deadline = Instant::now() + Duration::from_secs(2);
    while done_rx.try_recv().is_err() {
        assert!(Instant::now() < deadline);
        driver.poll().unwrap();
        std::thread::yield_now();
    }
    driver.close().unwrap();
    stop_tx.send(()).unwrap();
    peer.join().unwrap();
}
