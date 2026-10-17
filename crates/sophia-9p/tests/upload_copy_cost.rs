//! Isolate SDK upload staging from socket progress and peer allocations.
mod support;

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use sophia_9p::Fid;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use support::{Body, frame};

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn queued_upload_owns_payload_before_socket_progress() {
    for payload_len in [256, 4096, 65513] {
        let (client, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let handshake = std::thread::spawn(move || {
            let mut version = [0; 21];
            peer.read_exact(&mut version).unwrap();
            assert_eq!(version[4], 100);
            peer.write_all(&frame(
                101,
                u16::MAX,
                Body::default().u32(65536).string(b"9P2000.L"),
            ))
            .unwrap();
            peer
        });
        let mut pipeline =
            Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(5)).unwrap();
        // No peer thread runs while allocator snapshots are taken.
        let mut peer = handshake.join().unwrap();
        let mut payload = vec![0x5a; payload_len];
        let mut received = vec![0; payload_len + 23];
        for iteration in 0..2 {
            payload.fill(0x5a);
            let before = ALLOCATOR.stats();
            let tag = pipeline.write(Fid(1), 0, &payload).unwrap();
            let after = ALLOCATOR.stats();
            // The caller may reuse its bytes before poll sends the request.
            payload.fill(0xa5);
            pipeline.poll().unwrap();
            peer.read_exact(&mut received).unwrap();
            assert_eq!(received[4], 118);
            assert_eq!(&received[5..7], &tag.0.to_le_bytes());
            assert_eq!(&received[19..23], &(payload_len as u32).to_le_bytes());
            assert!(received[23..].iter().all(|byte| *byte == 0x5a));
            peer.write_all(&frame(119, tag.0, Body::default().u32(payload_len as u32)))
                .unwrap();
            assert_eq!(
                pipeline
                    .wait(tag, std::time::Instant::now() + Duration::from_secs(5))
                    .unwrap(),
                Reply::Write(payload_len as u32)
            );
            eprintln!(
                "upload_copy_cost payload_bytes={payload_len} iteration={iteration} staging_allocations={} staging_reallocations={} staging_allocated_bytes={} wire_bytes={}",
                after.allocations - before.allocations,
                after.reallocations - before.reallocations,
                after.bytes_allocated - before.bytes_allocated,
                received.len()
            );
        }
    }
}
