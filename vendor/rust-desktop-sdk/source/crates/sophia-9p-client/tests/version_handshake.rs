use sophia_9p_client::{
    pipeline::{PipelineError, PipelineLimits, VersionHandshake},
    records::Errno,
};
use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::UnixStream,
};

fn response(msize: u32, version: &[u8]) -> Vec<u8> {
    let mut bytes = ((13 + version.len()) as u32).to_le_bytes().to_vec();
    bytes.push(101);
    bytes.extend(u16::MAX.to_le_bytes());
    bytes.extend(msize.to_le_bytes());
    bytes.extend((version.len() as u16).to_le_bytes());
    bytes.extend(version);
    bytes
}
fn start() -> (VersionHandshake, UnixStream) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut handshake = VersionHandshake::new(client, PipelineLimits::default(), 10, 1000).unwrap();
    let mut b = [0; 21];
    assert_eq!(peer.read(&mut b).unwrap_err().kind(), ErrorKind::WouldBlock);
    handshake.dispatch(10, 65536).unwrap();
    peer.read_exact(&mut b).unwrap();
    let mut expected = response(65536, b"9P2000.L");
    expected[4] = 100;
    assert_eq!(b.as_slice(), expected);
    (handshake, peer)
}

#[test]
fn bytewise_reply_hands_off_once_without_second_version() {
    let (mut h, mut peer) = start();
    for b in response(4096, b"9P2000.L") {
        peer.write_all(&[b]).unwrap();
        h.dispatch(11, 1).unwrap();
    }
    assert!(h.ready());
    assert!(h.io_interest().immediate);
    let p = h.take_pipeline().unwrap();
    assert_eq!(p.msize(), 4096);
    assert!(p.is_quiescent());
    assert_eq!(p.fid_count(), 0);
    assert!(h.take_pipeline().is_none());
    let mut b = [0; 1];
    assert_eq!(peer.read(&mut b).unwrap_err().kind(), ErrorKind::WouldBlock);
}

#[test]
fn zero_budget_and_deadline_never_send_queued_version() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut h = VersionHandshake::new(client, PipelineLimits::default(), 10, 20).unwrap();
    assert_eq!(
        h.dispatch(9, 65536),
        Err(PipelineError::Limit("clock regressed"))
    );
    h.dispatch(11, 0).unwrap();
    let mut b = [0; 1];
    assert_eq!(peer.read(&mut b).unwrap_err().kind(), ErrorKind::WouldBlock);
    assert_eq!(h.dispatch(20, 65536), Err(PipelineError::Timeout));
    assert!(h.fd().is_none());
    assert_eq!(peer.read(&mut b).unwrap(), 0);
}

#[test]
fn malformed_version_replies_fail_closed() {
    let mut wrong_tag = response(4096, b"9P2000.L");
    wrong_tag[5] = 0;
    let mut wrong_type = response(4096, b"9P2000.L");
    wrong_type[4] = 105;
    for bytes in [
        wrong_tag,
        wrong_type,
        response(131072, b"9P2000.L"),
        response(1024, b"9P2000.L"),
        response(4096, b"9P2000"),
        65537u32.to_le_bytes().to_vec(),
        3u32.to_le_bytes().to_vec(),
    ] {
        let (mut h, mut peer) = start();
        peer.write_all(&bytes).unwrap();
        assert!(h.dispatch(11, 65536).is_err());
        assert!(h.fd().is_none());
        assert!(!h.ready());
    }
}

#[test]
fn remote_errno_and_truncated_eof_are_preserved() {
    let (mut h, mut peer) = start();
    let mut bytes = 11u32.to_le_bytes().to_vec();
    bytes.push(7);
    bytes.extend(u16::MAX.to_le_bytes());
    bytes.extend(13u32.to_le_bytes());
    peer.write_all(&bytes).unwrap();
    assert_eq!(h.dispatch(11, 65536), Err(PipelineError::Remote(Errno(13))));
    let (mut h, mut peer) = start();
    peer.write_all(&response(4096, b"9P2000.L")[..10]).unwrap();
    drop(peer);
    assert_eq!(
        h.dispatch(11, 65536),
        Err(PipelineError::Io(ErrorKind::UnexpectedEof))
    );
}

#[test]
fn following_bytes_stay_for_pipeline_validation() {
    let (mut h, mut peer) = start();
    let mut bytes = response(4096, b"9P2000.L");
    bytes.extend([7, 0, 0, 0, 121, 0, 0]);
    peer.write_all(&bytes).unwrap();
    h.dispatch(11, 65536).unwrap();
    let mut pipeline = h.take_pipeline().unwrap();
    assert!(matches!(pipeline.poll(), Err(PipelineError::Protocol(_))));
}

#[test]
fn partial_input_prevents_quiescent_handoff() {
    let (mut h, mut peer) = start();
    peer.write_all(&response(4096, b"9P2000.L")).unwrap();
    h.dispatch(11, 65536).unwrap();
    let mut pipeline = h.take_pipeline().unwrap();
    assert!(pipeline.is_quiescent());
    peer.write_all(&[7]).unwrap();
    pipeline.poll().unwrap();
    assert!(!pipeline.is_quiescent());
}
