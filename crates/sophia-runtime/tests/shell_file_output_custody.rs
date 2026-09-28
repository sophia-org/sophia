//! A record the file journal refuses stays owned and charged in Session's
//! FIFO, and leaves exactly once, in order, when the reader makes room. The
//! peer is a raw 9P client driven from this test; no Session owner loop.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Unsolicited records the journal holds beside its terminal reserve.
const UNSOLICITED: u64 =
    (SHELL_FILE_MAX_JOURNAL_RECORDS - SHELL_FILE_TERMINAL_RESERVE_RECORDS) as u64;

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-file-custody-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    (transport, directory)
}

fn facts() -> Vec<ContentOutputFactsEntry> {
    vec![ContentOutputFactsEntry {
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        local_width: 64,
        local_height: 64,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 1,
    }]
}

#[test]
fn a_record_the_journal_refused_stays_charged_and_leaves_once_in_order() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    transport
        .reserve_content(&mut registry, ContentLimits::prototype(grant))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let (resume_tx, resume) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        // The reader stops until Session has filled the unsolicited share.
        resume.recv().unwrap();
        let mut generations = Vec::new();
        for _ in 0..=UNSOLICITED {
            let event = peer.next_event();
            let published = decode_shell_file_object_published(&event).unwrap();
            assert_eq!(published.object, ShellFileKind::Outputs);
            generations.push(published.generation);
            peer.ack(&event);
        }
        generations
    });
    transport
        .begin_file_negotiation(
            &registry,
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let start = Instant::now();
    while transport
        .poll_negotiation(&mut registry, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    // Wait for the peer's acknowledgement of Negotiated, so the unsolicited
    // share starts empty.
    let settle = Instant::now();
    while settle.elapsed() < Duration::from_millis(50) {
        transport.poll_io(&mut registry).unwrap();
        std::thread::yield_now();
    }
    for generation in 1..=UNSOLICITED + 1 {
        transport
            .publish_content_output_facts(
                &mut registry,
                TransactionId::from_raw(100 + generation),
                generation,
                facts(),
            )
            .unwrap();
        transport.poll_io(&mut registry).unwrap();
    }
    // The last publication did not fit: it stays owned and charged here.
    let retained = transport.content_accounting(&registry);
    assert_eq!(retained.response_records, 1);
    assert_eq!(retained.response_bytes, 40 + 40);
    for _ in 0..8 {
        transport.poll_io(&mut registry).unwrap();
    }
    assert_eq!(transport.content_accounting(&registry), retained);
    resume_tx.send(()).unwrap();
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the refused record never left"
        );
        std::thread::yield_now();
    }
    let generations = peer.join().unwrap();
    assert_eq!(generations, (1..=UNSOLICITED + 1).collect::<Vec<_>>());
    assert_eq!(transport.content_accounting(&registry).response_records, 0);
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
