//! Independent SDK peer; the parent supplies launch policy, never a verdict.
use super::super::lock_authenticated_transport::rust_peer::{authenticate, open, read};
use sophia_9p_client::pipeline::Reply;
use sophia_launch_client::Client;
use sophia_protocol::inspection::decode_inspection_snapshot;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[test]
#[ignore = "parent invokes this peer through the protected launch channel"]
fn protected_inspection_child() {
    run(false);
}

#[test]
#[ignore = "parent invokes this peer through one protected multi-audience launch"]
fn protected_two_endpoints_child() {
    run(true);
}

fn run(two_endpoints: bool) {
    let socket = std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap();
    let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let names =
        sophia_namespace_client::Namespace::open(std::path::Path::new(&socket).parent().unwrap())
            .unwrap();
    assert_eq!(names.discovery().endpoints.len(), 2);
    let role = names.connect("inspection").unwrap();
    let admitted = authenticate(Client::over(role, relay, b"observer", b"inspection").unwrap());
    let mut admitted = authenticate(admitted.reauthenticate().unwrap());
    let mut second = if two_endpoints {
        Some(authenticate(
            Client::over_relay(
                names.connect("second").unwrap(),
                admitted.relay,
                b"observer",
                b"inspection",
            )
            .unwrap(),
        ))
    } else {
        None
    };
    let root = admitted.root;
    let pipe = &mut admitted.role;
    let api = open(pipe, root, b"api", 0);
    assert!(!read(pipe, api, 0, 1024).is_empty());
    let snapshot = open(pipe, root, b"snapshot", 0);
    let value = decode_inspection_snapshot(&read(pipe, snapshot, 0, 65_000)).unwrap();
    assert_eq!(value.snapshot.scene_generation, 4);
    let events = open(pipe, root, b"events", 0);
    let pending = pipe.read(events, value.event_offset, 4096).unwrap();
    pipe.poll().unwrap();
    std::fs::write(&report, "waiting\n").unwrap();
    match pipe.wait(pending, Instant::now() + Duration::from_secs(5)) {
        Ok(Reply::Error(errno)) => assert_eq!(errno.0, 116),
        Err(sophia_9p_client::pipeline::PipelineError::Io(
            std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe,
        )) => (),
        other => panic!("revoked observer answered its retained read: {other:?}"),
    }
    if let Some(second) = &mut second {
        let snapshot = open(&mut second.role, second.root, b"snapshot", 0);
        let value =
            decode_inspection_snapshot(&read(&mut second.role, snapshot, 0, 65_000)).unwrap();
        assert_eq!(
            value.snapshot.scene_generation, 4,
            "unrelated endpoint must still deliver inspection after grant revocation"
        );
        std::fs::write(report, "unrelated-progress\n").unwrap();
    } else {
        std::fs::write(report, "revoked\n").unwrap();
    }
}
