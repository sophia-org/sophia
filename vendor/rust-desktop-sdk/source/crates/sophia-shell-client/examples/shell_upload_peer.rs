//! Independent multi-chunk upload peer for a supplied-admission export fixture.
use sophia_shell_client::*;
use sophia_shell_protocol::*;
use std::{
    io::Read,
    time::{Duration, Instant},
};

fn next_status(client: &mut ShellConnection, wanted: u16) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        client.poll_io().unwrap();
        while let Some((_, record)) = client.take_content().unwrap() {
            if let ShellContentRecord::ResourceStatus(status) = record {
                assert_eq!(status.status, wanted);
                return;
            }
        }
        assert!(Instant::now() < deadline, "resource status timeout");
        std::thread::yield_now();
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let mut go = [0];
    std::io::stdin().read_exact(&mut go).unwrap();
    assert_eq!(go, [b'G']);
    let mut client = ShellConnection::connect_files(
        std::path::Path::new(&args[1]),
        ShellClientOptions {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: args[2].parse().unwrap(),
            handshake_timeout: Duration::from_secs(3),
        },
    )
    .unwrap();
    let Some((_, ShellContentRecord::Limits(limits))) = client.take_content().unwrap() else {
        panic!("negotiated limits");
    };
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let transaction = TransactionId::from_raw(70);
    let mut begin = ContentResourceBegin {
        grant: limits.grant,
        resource,
        width_px: 2048,
        height_px: 32,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 5,
        total_bytes: 262144,
    };
    let layout = begin.layout(&limits).unwrap();
    assert_eq!(layout.chunk_count, 5);
    client
        .enqueue_content(
            transaction,
            &ShellContentRecord::ResourceBegin(begin.clone()),
        )
        .unwrap();
    next_status(&mut client, 1);
    let pixels = vec![0; begin.total_bytes as usize];
    let chunk_bytes = (layout.row_bytes * layout.rows_per_chunk) as usize;
    let mut tickets = Vec::new();
    for (ordinal, bytes) in pixels.chunks(chunk_bytes).enumerate() {
        let admission = client
            .enqueue_content_tracked(
                transaction,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant: limits.grant,
                    resource,
                    ordinal: ordinal as u32,
                    offset: (ordinal * chunk_bytes) as u64,
                    bytes: bytes.to_vec(),
                }),
            )
            .unwrap();
        tickets.extend(admission.tickets());
    }
    // Queue End behind all adjacent chunks. The SDK must drain their writes first.
    client
        .enqueue_content(
            transaction,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: limits.grant,
                resource,
                total_bytes: begin.total_bytes,
                chunk_count: layout.chunk_count,
            }),
        )
        .unwrap();
    next_status(&mut client, 2);
    for ticket in tickets {
        assert_eq!(client.custody(ticket), Some(Custody::Stored));
    }
    // Finish with a distinct resource cancellation to exercise binding reuse.
    begin.resource.id = 2;
    client
        .enqueue_content(
            TransactionId::from_raw(72),
            &ShellContentRecord::ResourceBegin(begin),
        )
        .unwrap();
    next_status(&mut client, 1);
    client
        .enqueue_content(
            TransactionId::from_raw(73),
            &ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: limits.grant,
                resource: ContentResourceId {
                    id: 2,
                    generation: 1,
                },
            }),
        )
        .unwrap();
    next_status(&mut client, 4);
    client.poll_io().unwrap();
}
