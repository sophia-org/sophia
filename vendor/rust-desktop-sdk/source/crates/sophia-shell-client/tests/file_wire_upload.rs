//! A whole upload through `ShellConnection` over the file wire under reduced
//! negotiated Limits (32 KiB `max_chunk_bytes`), against the scripted
//! `sophia_shell_fs_v1` peer acting as the content owner. The owner checks
//! each `ResourceBegin` against the negotiated grant's layout, as Sophia's
//! export and content store do; the client's codec checks only what every
//! valid grant shares. A 4096 x 8 raster needs 4 chunks under this grant and
//! 3 under the prototype, so the two checks disagree exactly where it matters.

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

fn connect(limits: ContentLimits) -> (ShellConnection, Peer) {
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
        revision: 6,
        capabilities: BAR,
        limits_published: true,
    };
    let thread = std::thread::spawn(move || Peer::handshake_with_limits(listener, profile, limits));
    let connection = ShellConnection::connect_files(
        &path,
        ShellClientOptions {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: BAR,
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

/// The owner's answer to one resource transaction.
fn status(
    peer: &mut Peer,
    transaction: TransactionId,
    resource: ContentResourceId,
    status: u16,
    reason: ContentReason,
    next_ordinal: u32,
    admitted_bytes: u64,
) {
    let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction,
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: grant(),
            resource,
            status,
            reason: reason as u16,
            next_ordinal,
            admitted_bytes,
        }),
    })
    .unwrap();
    let event = peer.event(ShellFileKind::ResourceStatus, &body);
    peer.push(event);
}

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn begin(id: u64, chunks: u32) -> ShellContentRecord {
    ShellContentRecord::ResourceBegin(ContentResourceBegin {
        grant: grant(),
        resource: resource(id),
        width_px: WIDTH,
        height_px: HEIGHT,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: chunks,
        total_bytes: TOTAL,
    })
}

/// The owner's decision on a begin: the negotiated layout, or refusal.
fn admit(record: &[u8], limits: &ContentLimits) -> Option<(u16, ContentResourceLayout)> {
    let value = decode_shell_file_resource_begin(record).expect("a well-formed begin record");
    let ShellContentRecord::ResourceBegin(begin) = &value.record else {
        panic!("not a begin");
    };
    begin.layout(limits).ok().map(|layout| (value.slot, layout))
}

#[test]
fn a_reduced_grant_upload_completes_with_its_negotiated_chunk_count() {
    complete_upload([]);
}

#[test]
fn short_writes_and_eagain_resume_at_the_exact_acknowledged_byte() {
    complete_upload([Ok(1), Ok(7), Err(11), Ok(19), Ok(4093), Err(11), Ok(3)]);
}

fn complete_upload(replies: impl IntoIterator<Item = Result<u32, u32>>) {
    complete_upload_cap(replies, 0);
}

#[test]
fn upload_fragments_respect_the_opened_writers_iounit() {
    complete_upload_cap([], 8192);
}

fn complete_upload_cap(replies: impl IntoIterator<Item = Result<u32, u32>>, iounit: u32) {
    let limits = reduced();
    let (mut connection, mut peer) = connect(limits.clone());
    peer.upload_replies.extend(replies);
    peer.upload_iounit = iounit;
    // The client holds the negotiated grant, not the prototype.
    assert_eq!(
        content(&mut connection, &mut peer),
        ShellContentRecord::Limits(limits.clone())
    );
    let pixels: Vec<u8> = (0..TOTAL).map(|i| (i % 251) as u8).collect();

    let upload = TransactionId::from_raw(7);
    let ticket = connection
        .enqueue_content_tracked(upload, &begin(1, 4))
        .unwrap();
    let record = commit(&mut connection, &mut peer, only(ticket));
    let (slot, layout) = admit(&record, &limits).expect("the negotiated layout admits it");
    assert_eq!((layout.rows_per_chunk, layout.chunk_count), (2, 4));
    status(&mut peer, upload, resource(1), 1, ContentReason::None, 0, 0);
    let ShellContentRecord::ResourceStatus(admitted) = content(&mut connection, &mut peer) else {
        panic!("expected the admitted status");
    };
    assert_eq!(admitted.status, 1);

    // Four 32 KiB chunks of two whole rows each, into the bound slot.
    let chunk = (layout.rows_per_chunk * WIDTH * 4) as usize;
    assert_eq!(chunk, 32768);
    for (ordinal, bytes) in pixels.chunks(chunk).enumerate() {
        connection
            .enqueue_content(
                upload,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant: grant(),
                    resource: resource(1),
                    ordinal: ordinal as u32,
                    offset: (ordinal * chunk) as u64,
                    bytes: bytes.to_vec(),
                }),
            )
            .unwrap();
    }
    let node = format!("upload/{slot}");
    next(&mut connection, &mut peer, |_, peer| {
        (peer.uploads.get(&node).map_or(0, Vec::len) == TOTAL as usize).then_some(())
    });
    assert_eq!(peer.uploads[&node], pixels, "every byte reached the slot");
    if iounit != 0 {
        assert_eq!(
            peer.upload_offsets,
            (0..TOTAL).step_by(iounit as usize).collect::<Vec<_>>()
        );
    }

    let ticket = connection
        .enqueue_content_tracked(
            upload,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: grant(),
                resource: resource(1),
                total_bytes: TOTAL,
                chunk_count: 4,
            }),
        )
        .unwrap();
    let record = commit(&mut connection, &mut peer, only(ticket));
    let end = decode_shell_file_resource_end(&record).unwrap();
    let ShellContentRecord::ResourceEnd(end) = end.record else {
        panic!("not an end");
    };
    assert_eq!(
        (end.total_bytes, end.chunk_count),
        (TOTAL, layout.chunk_count)
    );
    status(
        &mut peer,
        upload,
        resource(1),
        2,
        ContentReason::None,
        4,
        TOTAL,
    );
    let ShellContentRecord::ResourceStatus(accepted) = content(&mut connection, &mut peer) else {
        panic!("expected the accepted status");
    };
    assert_eq!(
        (
            accepted.status,
            accepted.next_ordinal,
            accepted.admitted_bytes
        ),
        (2, 4, TOTAL)
    );
}

/// The prototype's count is a well-formed record, so the client submits it;
/// the negotiated owner refuses it, and the client learns why.
#[test]
fn a_reduced_grant_refuses_the_prototype_chunk_count() {
    let limits = reduced();
    let (mut connection, mut peer) = connect(limits.clone());
    let _ = content(&mut connection, &mut peer);
    let upload = TransactionId::from_raw(9);
    let ticket = connection
        .enqueue_content_tracked(upload, &begin(2, 3))
        .unwrap();
    let record = commit(&mut connection, &mut peer, only(ticket));
    assert!(
        admit(&record, &limits).is_none(),
        "the owner refuses 3 chunks"
    );
    // Control: the same record under the prototype grant would be admitted.
    assert!(admit(&record, &ContentLimits::prototype(grant())).is_some());
    status(
        &mut peer,
        upload,
        resource(2),
        3,
        ContentReason::Malformed,
        0,
        0,
    );
    let ShellContentRecord::ResourceStatus(rejected) = content(&mut connection, &mut peer) else {
        panic!("expected the rejected status");
    };
    assert_eq!(
        (rejected.status, rejected.reason),
        (3, ContentReason::Malformed as u16)
    );
    assert!(peer.uploads.is_empty(), "no slot bytes for a refused begin");
}

/// Counts no valid grant could lay out never leave the client.
#[test]
fn a_chunk_count_no_grant_could_lay_out_is_refused_before_submission() {
    let (mut connection, mut peer) = connect(reduced());
    let _ = content(&mut connection, &mut peer);
    for chunks in [0, 2, 9] {
        assert!(
            connection
                .enqueue_content(TransactionId::from_raw(11), &begin(3, chunks))
                .is_err(),
            "{chunks} chunks"
        );
    }
    peer.pump();
    assert!(peer.transactions.is_empty(), "nothing was staged");
}

#[test]
fn a_terminal_slot_status_discards_both_pending_setup_replies() {
    let limits = reduced();
    let (mut connection, mut peer) = connect(limits);
    content(&mut connection, &mut peer);
    let transaction = TransactionId::from_raw(7);
    let ticket = only(
        connection
            .enqueue_content_tracked(transaction, &begin(1, 4))
            .unwrap(),
    );
    commit(&mut connection, &mut peer, ticket);
    peer.hold_upload_walks = true;
    peer.hold_writer_opens = true;
    status(
        &mut peer,
        transaction,
        resource(1),
        1,
        ContentReason::None,
        0,
        0,
    );
    next(&mut connection, &mut peer, |_, peer| {
        (peer.held_replies.len() == 2).then_some(())
    });
    assert!(peer.uploads.is_empty());
    peer.hold_clunks = true;
    status(
        &mut peer,
        transaction,
        resource(1),
        3,
        ContentReason::None,
        0,
        0,
    );
    next(&mut connection, &mut peer, |_, peer| {
        (peer.held_clunks > 0).then_some(())
    });
    peer.held_replies.reverse();
    peer.release_replies();
    for _ in 0..8 {
        connection.poll_io().unwrap();
        peer.pump();
    }
    assert!(peer.uploads.is_empty());
}

fn held_window(replies: &[Result<u32, u32>]) -> (ShellConnection, Peer, Vec<Ticket>) {
    held_window_n(replies, 4)
}

fn held_window_n(
    replies: &[Result<u32, u32>],
    chunks: u32,
) -> (ShellConnection, Peer, Vec<Ticket>) {
    held_window_transport(replies, chunks, false)
}

fn held_window_transport(
    replies: &[Result<u32, u32>],
    chunks: u32,
    slow: bool,
) -> (ShellConnection, Peer, Vec<Ticket>) {
    let (mut c, mut p) = connect(reduced());
    content(&mut c, &mut p);
    let tx = TransactionId::from_raw(7);
    let mut first = begin(1, chunks);
    if let ShellContentRecord::ResourceBegin(b) = &mut first {
        b.height_px = chunks * 2;
        b.total_bytes = chunks as u64 * 32768;
    }
    let t = only(c.enqueue_content_tracked(tx, &first).unwrap());
    commit(&mut c, &mut p, t);
    status(&mut p, tx, resource(1), 1, ContentReason::None, 0, 0);
    content(&mut c, &mut p);
    p.hold_upload_replies = true;
    if slow {
        rustix::net::sockopt::set_socket_send_buffer_size(&c, 4096).unwrap();
        p.read_budget = 512;
    }
    p.upload_replies.extend(replies.iter().copied());
    let mut tickets = Vec::new();
    for i in 0..chunks {
        tickets.push(only(
            c.enqueue_content_tracked(
                tx,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant: grant(),
                    resource: resource(1),
                    ordinal: i,
                    offset: i as u64 * 32768,
                    bytes: vec![i as u8; 32768],
                }),
            )
            .unwrap(),
        ));
    }
    next(&mut c, &mut p, |_, p| {
        (p.held_replies.len() == 4).then_some(())
    });
    assert_eq!(p.upload_offsets, [0, 32768, 65536, 98304]);
    assert!(
        tickets[..4]
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::InFlight))
    );
    (c, p, tickets)
}

#[test]
fn upload_window_refills_without_waiting_for_the_whole_burst() {
    let (mut c, mut p, tickets) = held_window_n(&[], 8);
    let (_, tag, _) = p.held_replies.remove(0);
    p.answer_write(tag, 32768);
    next(&mut c, &mut p, |_, p| {
        (p.upload_offsets.len() == 5).then_some(())
    });
    assert_eq!(c.custody(tickets[0]), Some(Custody::Stored));
    assert_eq!(p.upload_offsets[4], 4 * 32768);
    assert_eq!(p.held_replies.len(), 4);
    p.hold_upload_replies = false;
    p.held_replies.reverse();
    p.release_replies();
    next(&mut c, &mut p, |c, _| {
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Stored))
            .then_some(())
    });
    assert_eq!(
        p.upload_offsets,
        (0..8).map(|n| n * 32768).collect::<Vec<_>>()
    );
}

#[test]
fn four_tickets_are_outstanding_and_end_waits_for_all_replies() {
    let (mut c, mut p, tickets) = held_window(&[]);
    let tx = TransactionId::from_raw(7);
    let end = only(
        c.enqueue_content_tracked(
            tx,
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: grant(),
                resource: resource(1),
                total_bytes: TOTAL,
                chunk_count: 4,
            }),
        )
        .unwrap(),
    );
    // An early tail reply cannot settle its ticket or let End pass the head.
    let (kind, tag, body) = p.held_replies.pop().unwrap();
    assert_eq!(kind, 119);
    p.answer_write(tag, u32::from_le_bytes(body.try_into().unwrap()));
    for _ in 0..4 {
        c.poll_io().unwrap();
        p.pump();
    }
    assert!(
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::InFlight))
    );
    assert!(p.submits.is_empty());
    p.hold_upload_replies = false;
    p.held_replies.reverse();
    p.release_replies();
    next(&mut c, &mut p, |c, _| {
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Stored))
            .then_some(())
    });
    commit(&mut c, &mut p, end);
    assert_eq!(p.upload_offsets.len(), 4);
}

#[test]
fn invalidated_tickets_resume_after_short_head_or_backpressure() {
    for response in [Ok(7), Err(11)] {
        let (mut c, mut p, tickets) = held_window(&[response]);
        assert_eq!(p.invalidated_upload_tails, 3);
        assert_eq!(p.short_upload_heads, usize::from(response.is_ok()));
        assert_eq!(p.eagain_upload_heads, usize::from(response.is_err()));
        p.hold_upload_replies = false;
        p.held_replies.reverse();
        p.release_replies();
        c.poll_io().unwrap();
        if response.is_err() {
            assert!(c.wake_deadline().is_some());
        }
        next(&mut c, &mut p, |c, _| {
            tickets
                .iter()
                .all(|t| c.custody(*t) == Some(Custody::Stored))
                .then_some(())
        });
        assert_eq!(p.upload_offsets.len(), 8);
        assert_eq!(p.upload_offsets[4], if response.is_ok() { 7 } else { 0 });
        for (i, chunk) in p.uploads["upload/0"].chunks(32768).enumerate() {
            assert!(chunk.iter().all(|b| *b == i as u8));
        }
    }
}

#[test]
fn terminal_status_drains_writes_before_clunk_and_preserves_stored_bytes() {
    let (mut c, mut p, tickets) = held_window(&[]);
    p.hold_clunks = true;
    status(
        &mut p,
        TransactionId::from_raw(7),
        resource(1),
        3,
        ContentReason::None,
        0,
        0,
    );
    content(&mut c, &mut p);
    assert_eq!(p.held_clunks, 0);
    p.hold_upload_replies = false;
    p.held_replies.reverse();
    p.release_replies();
    next(&mut c, &mut p, |_, p| (p.held_clunks > 0).then_some(()));
    assert!(
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Stored))
    );
}

#[test]
fn a_fenced_tail_never_replays_the_failed_head() {
    let (mut c, mut p, tickets) = held_window(&[Err(11)]);
    let held = std::mem::take(&mut p.held_replies);
    p.answer_error(held[0].1, 11);
    c.poll_io().unwrap();
    for reply in &held[1..] {
        p.answer_error(reply.1, 116);
    }
    for _ in 0..8 {
        c.poll_io().unwrap();
        p.pump();
    }
    assert_eq!(p.upload_offsets.len(), 4);
    assert_eq!(c.custody(tickets[0]), Some(Custody::DroppedUnsent));
    for ticket in &tickets[1..] {
        assert_eq!(c.custody(*ticket), Some(Custody::Refused(116)));
    }
}

#[test]
fn contradictory_tail_acceptance_is_preserved_and_stops_the_connection() {
    let (mut c, mut p, tickets) = held_window(&[Ok(7)]);
    let held = std::mem::take(&mut p.held_replies);
    p.answer_write(held[0].1, 7);
    c.poll_io().unwrap();
    p.answer_write(held[1].1, 32768);
    assert!(c.poll_io().is_err());
    assert_eq!(c.custody(tickets[0]), Some(Custody::Unknown));
    assert_eq!(c.custody(tickets[1]), Some(Custody::Stored));
    assert_eq!(p.upload_offsets.len(), 4);
}

#[test]
fn invalid_write_counts_fail_without_claiming_unsent_bytes() {
    for count in [0, 32769] {
        let (mut c, mut p, tickets) = held_window(&[]);
        let held = std::mem::take(&mut p.held_replies);
        p.answer_write(held[0].1, count);
        assert!(c.poll_io().is_err());
        for ticket in tickets {
            assert_eq!(c.custody(ticket), Some(Custody::Unknown));
        }
        assert_eq!(p.upload_offsets.len(), 4);
    }
}

#[test]
fn cancelled_status_drains_inflight_and_drops_unissued_chunks() {
    let (mut c, mut p, tickets) = held_window_n(&[], 8);
    let cancel = only(
        c.enqueue_content_tracked(
            TransactionId::from_raw(8),
            &ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: grant(),
                resource: resource(1),
            }),
        )
        .unwrap(),
    );
    p.hold_clunks = true;
    status(
        &mut p,
        TransactionId::from_raw(7),
        resource(1),
        4,
        ContentReason::Cancelled,
        0,
        0,
    );
    content(&mut c, &mut p);
    assert_eq!(p.held_clunks, 0);
    assert_eq!(p.upload_offsets.len(), 4);
    p.held_replies.reverse();
    p.release_replies();
    next(&mut c, &mut p, |c, _| {
        tickets[4..]
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::DroppedUnsent))
            .then_some(())
    });
    assert!(
        tickets[..4]
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Stored))
    );
    assert_eq!(p.upload_offsets.len(), 4);
    next(&mut c, &mut p, |_, p| (p.held_clunks > 0).then_some(()));
    let record = commit(&mut c, &mut p, cancel);
    assert_eq!(
        decode_shell_file_record(&record, ShellFileClass::Candidate)
            .unwrap()
            .header
            .kind,
        ShellFileKind::ResourceCancel
    );
}

#[test]
fn stale_head_drains_tails_without_replay() {
    let (mut c, mut p, tickets) = held_window(&[]);
    let held = std::mem::take(&mut p.held_replies);
    p.answer_error(held[0].1, 116);
    c.poll_io().unwrap();
    for reply in held[1..].iter().rev() {
        p.answer_error(reply.1, 116);
    }
    next(&mut c, &mut p, |c, _| {
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Refused(116)))
            .then_some(())
    });
    assert_eq!(p.upload_offsets.len(), 4);
}

#[test]
fn a_slow_reader_preserves_window_offsets_and_refill() {
    let (mut c, mut p, tickets) = held_window_transport(&[], 8, true);
    assert!(p.partial_frames > 0);
    p.hold_upload_replies = false;
    p.release_replies();
    next(&mut c, &mut p, |c, _| {
        tickets
            .iter()
            .all(|t| c.custody(*t) == Some(Custody::Stored))
            .then_some(())
    });
    assert_eq!(
        p.upload_offsets,
        (0..8).map(|n| n * 32768).collect::<Vec<_>>()
    );
    assert_eq!(p.uploads["upload/0"].len(), 8 * 32768);
    for (i, chunk) in p.uploads["upload/0"].chunks(32768).enumerate() {
        assert!(chunk.iter().all(|byte| *byte == i as u8));
    }
}
