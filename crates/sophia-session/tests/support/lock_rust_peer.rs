//! The protected child uses only the pinned Rust SDK for authentication and
//! role traffic. A C exec bootstrap moves the inherited kernel stream to stdin;
//! it knows no 9P record, challenge, verdict or role root.
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_client::records::{Fid, Tag};
use sophia_launch_client::Client;
use sophia_lock_protocol::lock_files::*;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

fn answer(pipe: &mut Pipeline, tag: Tag) -> Reply {
    pipe.wait(tag, Instant::now() + Duration::from_secs(3))
        .unwrap()
}
fn open(pipe: &mut Pipeline, root: Fid, name: &[u8], mode: u32) -> Fid {
    let (tag, fid) = pipe.walk(root, &[name]).unwrap();
    assert!(matches!(answer(pipe, tag), Reply::Walk(qids) if qids.len() == 1));
    let tag = pipe.lopen(fid, mode).unwrap();
    assert!(matches!(answer(pipe, tag), Reply::Lopen { .. }));
    fid
}
fn read(pipe: &mut Pipeline, fid: Fid, offset: u64, count: u32) -> Vec<u8> {
    let tag = pipe.read(fid, offset, count).unwrap();
    let Reply::Read(bytes) = answer(pipe, tag) else {
        panic!("expected file read")
    };
    bytes
}
fn write(pipe: &mut Pipeline, fid: Fid, bytes: &[u8]) {
    let tag = pipe.write(fid, 0, bytes).unwrap();
    assert_eq!(answer(pipe, tag), Reply::Write(bytes.len() as u32));
}
fn authenticate(mut client: Client) -> sophia_launch_client::Authenticated {
    while !client.ready() {
        client
            .poll()
            .expect("Rust SDK must authenticate through the protected launch channel");
        client.wait_for_io(Duration::from_secs(1)).unwrap();
    }
    client.into_authenticated().unwrap()
}

#[test]
#[ignore = "parent test invokes this child under the actual protected supervisor"]
fn protected_rust_child() {
    let socket = std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").expect("protected child socket");
    let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").expect("protected child report");
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let role = UnixStream::connect(socket).unwrap();
    let admitted = authenticate(Client::over(role, relay, b"lock", b"lock-provider").unwrap());
    assert_eq!(admitted.role.fid_count(), 1);
    assert_eq!(admitted.relay.fid_count(), 0);
    let mut admitted = authenticate(admitted.reauthenticate().unwrap());
    assert_eq!(admitted.role.fid_count(), 1);
    assert_eq!(admitted.relay.fid_count(), 0);
    let root = admitted.root;
    let pipe = &mut admitted.role;
    let api = open(pipe, root, b"api", 0);
    let api = String::from_utf8(read(pipe, api, 0, 256)).unwrap();
    let epoch = api
        .split_whitespace()
        .find_map(|word| word.strip_prefix("epoch="))
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(epoch, 7);
    let transaction = open(pipe, root, b"transaction", 2);
    let submit = open(pipe, root, b"submit", 1);
    let offer = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
        chords: Vec::new(),
    }
    .encode()
    .unwrap();
    let record = encode_lock_file_record(
        LockFileHeader {
            kind: LockFileKind::Negotiate,
            connection_epoch: epoch,
            submission_id: 1,
            sequence: 0,
        },
        &offer,
    )
    .unwrap();
    write(pipe, transaction, &record);
    write(
        pipe,
        submit,
        &encode_lock_file_submit(LockFileSubmit {
            connection_epoch: epoch,
            submission_id: 1,
            candidate_bytes: record.len() as u32,
        })
        .unwrap(),
    );
    let events = open(pipe, root, b"events", 0);
    let bytes = read(pipe, events, 0, 4096);
    let mut at = 0;
    let mut sequence = 0;
    while at < bytes.len() {
        let length = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let header = decode_lock_file_record(&bytes[at..at + length], LockFileClass::Event)
            .unwrap()
            .header;
        assert_eq!(header.connection_epoch, epoch);
        sequence = header.sequence;
        at += length;
    }
    assert!(sequence > 0);
    let ack = open(pipe, root, b"ack", 1);
    write(
        pipe,
        ack,
        &encode_lock_file_ack(LockFileAck {
            connection_epoch: epoch,
            sequence,
        })
        .unwrap(),
    );
    let pending = pipe.read(events, bytes.len() as u64, 4096).unwrap();
    pipe.poll().unwrap();
    std::fs::write(&report, "waiting\n").unwrap();
    match pipe.wait(pending, Instant::now() + Duration::from_secs(5)) {
        Ok(Reply::Error(errno)) => assert_eq!(errno.0, 116),
        Err(sophia_9p_client::pipeline::PipelineError::Io(
            std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe,
        )) => (),
        other => panic!("retired launch answered a retained read: {other:?}"),
    }
    std::fs::write(report, "revoked\n").unwrap();
}
