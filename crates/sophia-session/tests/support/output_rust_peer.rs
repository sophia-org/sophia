//! Independent SDK authentication with Sophia's output-record codec. The
//! existing generic descriptor bootstrap supplies stdin, not a verdict.
use super::super::lock_authenticated_transport::rust_peer::{authenticate, open, read, write};
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_launch_client::Client;
use sophia_protocol::output_files::*;
use sophia_protocol::{OutputV1ClientHello, SOPHIA_OUTPUT_CAPABILITY_OBSERVE};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

#[test]
#[ignore = "parent invokes this child under a protected output supervisor"]
fn protected_output_child() {
    let socket = std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap();
    let report = std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap();
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let role = UnixStream::connect(socket).unwrap();
    // Plain attach on this same stream must fail before the SDK restarts wire state.
    let mut probe = Pipeline::over(
        role.try_clone().unwrap(),
        PipelineLimits::default(),
        Duration::from_secs(3),
    )
    .unwrap();
    let (tag, _) = probe.attach(b"output", b"output-authority").unwrap();
    assert_eq!(
        probe
            .wait(tag, Instant::now() + Duration::from_secs(3))
            .unwrap(),
        Reply::Error(sophia_9p_client::records::Errno(13)),
        "output files require a verified afid"
    );
    drop(probe);
    let admitted = authenticate(Client::over(role, relay, b"output", b"output-authority").unwrap());
    let mut admitted = authenticate(admitted.reauthenticate().unwrap());
    assert_eq!(admitted.role.fid_count(), 1);
    let root = admitted.root;
    let pipe = &mut admitted.role;
    let api = open(pipe, root, b"api", 0);
    assert_eq!(read(pipe, api, 0, 256), b"sophia-output-files version=1\n");
    let candidate = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Negotiate,
            connection_epoch: 7,
            submission_id: 1,
            sequence: 0,
        },
        &encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
        }),
    )
    .unwrap();
    let transaction = open(pipe, root, b"transaction", 2);
    write(pipe, transaction, &candidate);
    let submit = open(pipe, root, b"submit", 1);
    write(
        pipe,
        submit,
        &encode_output_file_submit(OutputFileSubmit {
            connection_epoch: 7,
            submission_id: 1,
            candidate_bytes: candidate.len() as u32,
        })
        .unwrap(),
    );
    let topology = open(pipe, root, b"topology", 0);
    assert!(!read(pipe, topology, 0, 65_000).is_empty());
    let events = open(pipe, root, b"events", 0);
    let bytes = read(pipe, events, 0, 4096);
    let mut at = 0;
    let mut sequence = 0;
    while at < bytes.len() {
        let length = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let record =
            decode_output_file_record(&bytes[at..at + length], OutputFileClass::Event).unwrap();
        assert_eq!(record.header.connection_epoch, 7);
        sequence = record.header.sequence;
        at += length;
    }
    assert!(sequence > 0);
    let ack = open(pipe, root, b"ack", 1);
    write(
        pipe,
        ack,
        &encode_output_file_ack(OutputFileAck {
            connection_epoch: 7,
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
        other => panic!("revoked output authority answered a retained read: {other:?}"),
    }
    std::fs::write(report, "revoked\n").unwrap();
}
