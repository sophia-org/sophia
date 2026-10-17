//! Refusal controls for custody order and attach identity before admission.
#[allow(dead_code)]
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use file_wire_peer::{CONTENT_GRANT_EPOCH, EPOCH, Peer, Profile, WAIT};
use sophia_9p_client::pipeline::PipelineLimits;
use sophia_shell_client::{Connecting, ShellClientError, ShellClientOptions, ShellConnection};
use sophia_shell_protocol::{ContentGrant, ContentLimits};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

type Edit = fn(&mut Vec<u8>);

fn refuse(
    edit_events: Edit,
    edit_limits: Edit,
    content: bool,
    grant_epoch: u64,
) -> (ShellClientError, Peer) {
    let (blocking_error, _) = refuse_path(edit_events, edit_limits, content, grant_epoch, false);
    let result = refuse_path(edit_events, edit_limits, content, grant_epoch, true);
    assert_eq!(
        result.0, blocking_error,
        "startup APIs disagree on rejection"
    );
    result
}

fn refuse_path(
    edit_events: Edit,
    edit_limits: Edit,
    content: bool,
    grant_epoch: u64,
    driven: bool,
) -> (ShellClientError, Peer) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sdk-bootstrap-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let profile = Profile {
        role: "descriptor",
        revision: 8,
        capabilities: 3 | if content { 1 << 7 } else { 0 },
        limits_published: content,
    };
    let limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: grant_epoch,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    });
    let task = std::thread::spawn(move || {
        Peer::handshake_with_edits(listener, profile, limits, edit_events, edit_limits)
    });
    let options = ShellClientOptions {
        minimum_revision: 1,
        maximum_revision: 8,
        required_capabilities: profile.capabilities & !2,
        handshake_timeout: WAIT,
    };
    let result = if driven {
        let mut startup = Connecting::direct(
            UnixStream::connect(&path).unwrap(),
            PipelineLimits::default(),
            options,
            0,
            2000,
        )
        .unwrap();
        let start = Instant::now();
        loop {
            assert!(start.elapsed() < WAIT);
            match startup.dispatch(start.elapsed().as_millis() as u64, 1) {
                Err(error) => {
                    assert!(!startup.ready());
                    assert!(startup.fd().is_none());
                    assert_eq!(startup.error(), Some(&error));
                    break Err(error);
                }
                Ok(()) if startup.ready() => break Ok(startup.take_connection().unwrap()),
                Ok(()) => std::thread::yield_now(),
            }
        }
    } else {
        ShellConnection::connect_files(&path, options)
    };
    let peer = task.join().unwrap();
    std::fs::remove_file(path).unwrap();
    match result {
        Err(error) => (error, peer),
        Ok(_) => panic!("corrupt bootstrap admitted"),
    }
}

// Native file header: size at 0, attach epoch at 8, journal sequence at 24.
fn first_size(bytes: &[u8]) -> usize {
    u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize
}

#[test]
fn bootstrap_requires_ordered_custody_from_the_current_attach() {
    let cases: [(Edit, &str); 6] = [
        (
            |b| b[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes()),
            "handshake event identity",
        ),
        (
            |b| {
                let n = first_size(b);
                b[n + 24..n + 32].copy_from_slice(&1u64.to_le_bytes());
            },
            "handshake event identity",
        ),
        (
            |b| {
                let n = first_size(b);
                b.drain(..n);
            },
            "Negotiated before bootstrap custody",
        ),
        (
            |b| {
                let n = first_size(b);
                let mut duplicate = b[..n].to_vec();
                duplicate[24..32].copy_from_slice(&2u64.to_le_bytes());
                b[n + 24..n + 32].copy_from_slice(&3u64.to_le_bytes());
                b.splice(n..n, duplicate);
            },
            "unexpected Submitted before negotiation",
        ),
        (
            |b| b[..4].copy_from_slice(&31u32.to_le_bytes()),
            "handshake record length",
        ),
        (
            |b| b[..4].copy_from_slice(&65537u32.to_le_bytes()),
            "handshake record length",
        ),
    ];
    for (edit, expected) in cases {
        let (error, peer) = refuse(edit, |_| {}, false, EPOCH);
        assert_eq!(error, ShellClientError::Protocol(expected));
        assert_eq!(peer.walk_count("limits"), 0);
        assert!(peer.acks.iter().all(|ack| ack.sequence <= 1));
    }
}

#[test]
fn content_limits_bind_both_the_envelope_and_grant_to_the_attach() {
    let (error, peer) = refuse(
        |_| {},
        |b| b[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes()),
        true,
        EPOCH,
    );
    assert_eq!(
        error,
        ShellClientError::Protocol("Limits from another connection epoch")
    );
    assert_eq!(peer.walk_count("limits"), 1);
    let (error, peer) = refuse(|_| {}, |_| {}, true, EPOCH + 1);
    assert_eq!(
        error,
        ShellClientError::Protocol("Limits grant from another connection epoch")
    );
    assert_eq!(peer.walk_count("limits"), 1);
}

#[test]
fn admission_refusal_is_acknowledged_and_retained_by_both_startup_apis() {
    use sophia_shell_protocol::{ContentAdmissionRefused, shell_files::*};
    let (error, peer) = refuse(
        |bytes| {
            bytes.truncate(first_size(bytes));
            bytes.extend(
                encode_shell_file_refused(
                    ShellFileHeader {
                        kind: ShellFileKind::Refused,
                        connection_epoch: EPOCH,
                        submission_id: 0,
                        sequence: 2,
                    },
                    ContentAdmissionRefused {
                        reason: 2,
                        denied_capabilities: 1,
                    },
                )
                .unwrap(),
            );
        },
        |_| {},
        false,
        EPOCH,
    );
    assert_eq!(
        error,
        ShellClientError::AdmissionRefused(ContentAdmissionRefused {
            reason: 2,
            denied_capabilities: 1
        })
    );
    assert_eq!(
        peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(peer.walk_count("limits"), 0);
}
