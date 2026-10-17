//! Independent startup peer for Sophia's explicitly selected export fixture.
use sophia_9p_client::pipeline::PipelineLimits;
use sophia_shell_client::{Connecting, ShellClientOptions};
use sophia_shell_protocol::ShellContentRecord;
use sophia_shell_protocol::shell_files::{ShellDescriptorRecord, ShellFileKind};
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
            required_capabilities: 1 | 4 | if content { 128 } else { 0 },
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
    client.set_automatic_event_delivery(false).unwrap();
    assert_eq!(client.connection_epoch(), 41);
    assert_eq!(client.welcome().selected_revision, 8);
    assert_eq!(
        client.welcome().capabilities,
        3 | 4 | if content { 128 } else { 0 }
    );
    assert_eq!(
        matches!(
            client.take_content().unwrap(),
            Some((_, ShellContentRecord::Limits(_)))
        ),
        content
    );
    let mut tabs_received = false;
    let mut rejected_probe = false;
    let mut probe_started = false;
    let mut head_checks = 0;
    loop {
        assert!(start.elapsed() < Duration::from_secs(3));
        client.dispatch(Instant::now(), 1).unwrap();
        if let Some(event) = client.journal_event() {
            assert!((3..=4).contains(&event.sequence));
            assert!(matches!(
                event.record,
                sophia_shell_client::JournalRecord::ObjectPublished(_)
            ));
            assert_eq!(client.journal_progress().ack_limit, 2);
            head_checks += 1;
            if head_checks == 4 {
                assert!(client.consume_journal_event().unwrap());
                head_checks = 0;
            }
        }
        let wanted = if tabs_received {
            ShellFileKind::Descriptors
        } else {
            ShellFileKind::Tabs
        };
        let request = client.pending_snapshots().find(|p| p.object == wanted);
        if let Some(request) = request {
            assert_eq!(request.generation, if tabs_received { 17 } else { 18 });
            assert_eq!(client.journal_progress().ack_limit, 2);
            if !rejected_probe {
                if !probe_started {
                    client
                        .prefetch_snapshot(
                            sophia_shell_protocol::shell_files::ShellFileObjectPublished {
                                generation: request.generation + 1,
                                ..request
                            },
                        )
                        .unwrap();
                    probe_started = true;
                }
                if let Some(failure) = client.take_snapshot_failure() {
                    assert_eq!(failure.request.generation, 19);
                    assert_eq!(client.journal_progress().ack_limit, 2);
                    rejected_probe = true;
                }
            } else {
                client.request_snapshot(request).unwrap();
            }
        }
        if let Some(sophia_shell_client::DescriptorObservation::Record(record)) =
            client.take_descriptor_observation().unwrap()
        {
            match record.record {
                ShellDescriptorRecord::Tabs(snapshot) => {
                    assert!(!tabs_received);
                    assert_eq!(snapshot.generation, 18);
                    assert!(snapshot.groups.is_empty());
                    assert_eq!(client.journal_progress().ack_limit, 2);
                    tabs_received = true;
                }
                ShellDescriptorRecord::Descriptors(snapshot) => {
                    assert!(tabs_received);
                    assert_eq!(snapshot.snapshot_generation, 17);
                    assert!(snapshot.descriptors.is_empty());
                    break;
                }
                _ => panic!("unexpected snapshot"),
            }
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
