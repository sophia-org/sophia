//! Retired byte uploads cannot acquire queue or submission custody.

#[allow(dead_code)] // Shared scripted peer; this file drives only part of it.
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use file_wire_peer::{CONTENT_GRANT_EPOCH, EPOCH, Peer, Profile, WAIT};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

const BAR: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const WIDTH: u32 = 4096;
const HEIGHT: u32 = 8;
const TOTAL: u64 = WIDTH as u64 * HEIGHT as u64 * 4;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    }
}

/// A valid reduced grant: 32 KiB chunks, frame cap chunk + 48.
fn reduced() -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_chunk_bytes = 32768;
    limits.max_frame_payload = 32816;
    assert_eq!(limits.validate(), Ok(()));
    limits
}

fn connect(limits: ContentLimits, revision: u16) -> (ShellConnection, Peer) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sophia-shell-upload-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    let profile = Profile {
        role: "bar",
        revision,
        capabilities: BAR | if revision == 9 { 1 << 13 } else { 0 },
        limits_published: true,
    };
    let thread = std::thread::spawn(move || Peer::handshake_with_limits(listener, profile, limits));
    let connection = ShellConnection::connect_files(
        &path,
        ShellClientOptions {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: BAR | if revision == 9 { 1 << 13 } else { 0 },
            handshake_timeout: WAIT,
        },
    );
    let peer = thread.join().unwrap();
    let _ = std::fs::remove_file(&path);
    (connection.unwrap(), peer)
}

/// Alternates the peer and the client until `take` yields, within [`WAIT`].
fn next<T>(
    connection: &mut ShellConnection,
    peer: &mut Peer,
    mut take: impl FnMut(&mut ShellConnection, &mut Peer) -> Option<T>,
) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        peer.pump();
        connection.poll_io().unwrap();
        if let Some(value) = take(connection, peer) {
            return value;
        }
        assert!(Instant::now() < deadline, "the upload stalled");
    }
}

fn content(connection: &mut ShellConnection, peer: &mut Peer) -> ShellContentRecord {
    next(connection, peer, |connection, _| {
        connection.take_content().unwrap().map(|(_, record)| record)
    })
}

/// The one ticket a single-record admission names.
fn only(admission: Admission) -> Ticket {
    let mut tickets = admission.tickets();
    let ticket = tickets.next().expect("one ticket");
    assert!(tickets.next().is_none(), "one record, one ticket");
    ticket
}

/// Carries the front submission through `transaction` and `submit` to its
/// `Submitted` event, returning the record the owner received.
fn commit(connection: &mut ShellConnection, peer: &mut Peer, ticket: Ticket) -> Vec<u8> {
    let held = next(connection, peer, |_, peer| peer.submits.pop_front());
    let submit = decode_shell_file_submit(&held.data).unwrap();
    let record = peer.transactions.last().unwrap().clone();
    let kind = decode_shell_file_record(&record, ShellFileClass::Candidate)
        .unwrap()
        .header
        .kind;
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = peer.submitted(submit.submission_id, kind);
    peer.push(event);
    next(connection, peer, |connection, _| {
        (connection.custody(ticket) == Some(Custody::Submitted)).then_some(())
    });
    record
}

fn resource() -> ContentResourceId {
    ContentResourceId {
        id: 1,
        generation: 1,
    }
}

#[test]
fn byte_records_are_refused_without_spending_tickets_or_blocking_controls() {
    for revision in [6, 9] {
        let (mut connection, mut peer) = connect(reduced(), revision);
        let _ = content(&mut connection, &mut peer);
        let cancel = ShellContentRecord::ResourceCancel(ContentResourceCancel {
            grant: grant(),
            resource: resource(),
        });
        let first = only(
            connection
                .enqueue_content_tracked(TransactionId::from_raw(1), &cancel)
                .unwrap(),
        );
        commit(&mut connection, &mut peer, first);
        let records = [
            ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant: grant(),
                resource: resource(),
                width_px: WIDTH,
                height_px: HEIGHT,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 4,
                total_bytes: TOTAL,
            }),
            ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant: grant(),
                resource: resource(),
                ordinal: 0,
                offset: 0,
                bytes: vec![0; 32768],
            }),
            ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: grant(),
                resource: resource(),
                total_bytes: TOTAL,
                chunk_count: 4,
            }),
        ];
        for record in &records {
            assert!(matches!(
                connection.enqueue_content_tracked(TransactionId::from_raw(2), record),
                Err(ShellClientError::WrongDirection)
            ));
        }
        assert!(
            connection
                .enqueue_content_group_tracked(TransactionId::from_raw(3), &records)
                .is_err()
        );
        let tail = only(
            connection
                .enqueue_content_tracked(TransactionId::from_raw(4), &cancel)
                .unwrap(),
        );
        assert_eq!(tail.0, first.0 + 1, "refused bytes spend no tickets");
        commit(&mut connection, &mut peer, tail);
        assert_eq!(peer.transactions.len(), 2);
        assert!(peer.uploads.is_empty());
        assert!(peer.upload_offsets.is_empty());
    }
}
