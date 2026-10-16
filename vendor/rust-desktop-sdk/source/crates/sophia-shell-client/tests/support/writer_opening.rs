use super::*;
use sophia_9p_client::pipeline::PipelineLimits;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut bytes = vec![0; u32::from_le_bytes(size) as usize];
    bytes[..4].copy_from_slice(&size);
    stream.read_exact(&mut bytes[4..]).unwrap();
    bytes
}

#[test]
fn one_credit_falls_back_to_sequential_open_without_raising_limits() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let handshake = std::thread::spawn(move || {
        let mut version = frame(&mut peer);
        version[4] = 101;
        peer.write_all(&version).unwrap();
        peer
    });
    let mut pipeline = Pipeline::over(
        client,
        PipelineLimits {
            max_outstanding: 1,
            ..PipelineLimits::default()
        },
        Duration::from_secs(2),
    )
    .unwrap();
    let mut peer = handshake.join().unwrap();
    let mut opening = Opening::start(&mut pipeline, Fid(0), &[b"transaction"], 2).unwrap();
    assert!(opening.open.is_none());
    pipeline.poll().unwrap();
    let request = frame(&mut peer);
    assert_eq!(request[4], 110);
    let mut reply = vec![22, 0, 0, 0, 111, request[5], request[6], 1, 0];
    reply.extend([0; 13]);
    peer.write_all(&reply).unwrap();
    let tag = opening.walk.unwrap();
    let reply = pipeline
        .wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        opening.receive(&mut pipeline, tag, &reply).unwrap(),
        Some(false)
    );
    assert!(opening.open.is_some());
    pipeline.poll().unwrap();
    let request = frame(&mut peer);
    assert_eq!(request[4], 12);
    let mut reply = vec![24, 0, 0, 0, 13, request[5], request[6]];
    reply.extend([0; 17]);
    peer.write_all(&reply).unwrap();
    let tag = opening.open.unwrap();
    let reply = pipeline
        .wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        opening.receive(&mut pipeline, tag, &reply).unwrap(),
        Some(true)
    );
}
