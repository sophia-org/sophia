//! Independent startup peer for Sophia's explicitly selected export fixture.
use sophia_9p_client::pipeline::PipelineLimits;
use sophia_shell_client::{Connecting, ShellClientOptions};
use sophia_shell_protocol::ShellContentRecord;
use std::{
    io::Read,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let content = args[2] == "content";
    let mut go = [0];
    std::io::stdin().read_exact(&mut go).unwrap();
    assert_eq!(go, [1]);
    let stream = UnixStream::connect(&args[1]).unwrap();
    let mut startup = Connecting::direct(
        stream,
        PipelineLimits::default(),
        ShellClientOptions {
            minimum_revision: 8,
            maximum_revision: 8,
            required_capabilities: 1 | if content { 128 } else { 0 },
            handshake_timeout: Duration::from_secs(3),
        },
        0,
        3000,
    )
    .unwrap();
    let start = Instant::now();
    while !startup.ready() {
        assert!(start.elapsed() < Duration::from_secs(3));
        startup
            .dispatch(start.elapsed().as_millis() as u64, 1)
            .unwrap();
        std::thread::yield_now();
    }
    let mut client = startup.take_connection().unwrap();
    client.set_automatic_journal_ack(false).unwrap();
    client.set_automatic_snapshot_fetch(false).unwrap();
    assert_eq!(client.connection_epoch(), 41);
    assert_eq!(client.welcome().selected_revision, 8);
    assert_eq!(
        client.welcome().capabilities,
        3 | if content { 128 } else { 0 }
    );
    assert_eq!(
        matches!(
            client.take_content().unwrap(),
            Some((_, ShellContentRecord::Limits(_)))
        ),
        content
    );
    loop {
        assert!(start.elapsed() < Duration::from_secs(3));
        client.dispatch(Instant::now(), 1).unwrap();
        if let Some(request) = client.pending_snapshot() {
            assert_eq!(request.generation, 17);
            assert_eq!(client.journal_progress().ack_limit, 2);
            client.request_snapshot(request).unwrap();
        }
        if let Some(sophia_shell_client::DescriptorObservation::Record(record)) =
            client.take_descriptor_observation().unwrap()
        {
            let sophia_shell_protocol::shell_files::ShellDescriptorRecord::Descriptors(snapshot) =
                record.record
            else {
                panic!("expected descriptors")
            };
            assert_eq!(snapshot.snapshot_generation, 17);
            assert!(snapshot.descriptors.is_empty());
            break;
        }
        std::thread::yield_now();
    }
    let progress = client.journal_progress();
    assert!(progress.handled > 2 && progress.ack_limit == progress.handled);
    assert_eq!(progress.acknowledged, 2);
    assert!(!progress.object_pending);
    assert!(client.acknowledge_journal().unwrap());
    assert_eq!(client.journal_progress().acknowledged, 2);
    while client.journal_progress().acknowledged < progress.ack_limit {
        assert!(start.elapsed() < Duration::from_secs(3));
        client.dispatch(Instant::now(), 1).unwrap();
        std::thread::yield_now();
    }
    client.close();
}
