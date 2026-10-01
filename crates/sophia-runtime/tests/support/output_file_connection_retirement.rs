//! Connection-scoped failures must not take down the optional listener.
use super::*;
use std::io::Write;
use std::os::unix::net::UnixStream;

fn service(label: &str) -> (OutputFileService, std::path::PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("output-file-{label}-{}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        7,
        OutputFileLimits::default(),
    )
    .unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    (
        OutputFileService::spawn(transport, snapshot(4)).unwrap(),
        path,
    )
}

#[test]
fn malformed_wire_disconnects_only_its_peer_and_preserves_the_listener() {
    let (service, path) = service("malformed");
    let mut stream = UnixStream::connect(&path).unwrap();
    let mut peer = raw_peer::Peer::from_stream(stream.try_clone().unwrap());
    peer.setup(); // Rversion and attach prove this connection was accepted.
    stream.write_all(&[6, 0, 0, 0, 116, 1, 0]).unwrap(); // shorter than a 9P header
    assert_eq!(
        service.event_timeout(Duration::from_secs(2)).unwrap(),
        OutputFileServiceEvent::Disconnected {
            connection_epoch: 7
        }
    );
    drop(peer);
    drop(stream);
    service
        .command(OutputFileServiceCommand::PublishSnapshot(snapshot(5)))
        .unwrap();
    let mut replacement = raw_peer::Peer::connect(&path);
    replacement.setup();
    replacement.open(6, b"limits", 0);
    let limits = replacement.read(6, 0);
    let limits = decode_output_file_record(&limits, OutputFileClass::Object).unwrap();
    assert_eq!(limits.header.connection_epoch, 8);
    // No client negotiation is needed for the owner to remain stoppable.
    assert!(
        service
            .pause_acceptance(Duration::from_secs(1))
            .unwrap()
            .is_empty()
    );
    drop(replacement);
    drop(service);
    assert!(!path.parent().unwrap().exists());
}

#[test]
fn a_partial_9p_request_does_not_block_the_owner_pause_barrier() {
    let (service, path) = service("partial");
    let stream = UnixStream::connect(&path).unwrap();
    let mut peer = raw_peer::Peer::from_stream(stream);
    peer.setup();
    let result = peer.rpc_with_prefix(
        24,
        &[1_u32.to_le_bytes().as_slice(), &0x7ff_u64.to_le_bytes()].concat(),
        8,
        || {
            assert!(
                service
                    .pause_acceptance(Duration::from_secs(1))
                    .unwrap()
                    .is_empty()
            )
        },
    );
    assert!(result.is_err(), "pause must revoke the incomplete request");
    drop(peer);
    drop(service);
    assert!(!path.parent().unwrap().exists());
}
