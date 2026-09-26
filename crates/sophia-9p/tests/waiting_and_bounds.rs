//! Reads that wait, flush, the waiting bound, and the output reservation that
//! keeps every export operation's reply.

mod support;

use sophia_9p::{ConnectionId, Epoch, Limits};
use support::*;

fn events_open() -> Harness {
    let mut harness = Harness::attached();
    harness.open(1, &[b"events"], 0).lopen();
    harness
}

#[test]
fn a_waiting_read_is_answered_when_its_event_arrives() {
    let mut harness = events_open();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    assert_eq!(harness.connection.waiting_count(), 1);
    // Retrying with nothing new answers nothing.
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    assert!(harness.take().is_empty());
    harness.export.push_event(b"first");
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    let replies = harness.take();
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].tag, 9);
    assert_eq!(replies[0].data(), b"first");
    assert_eq!(harness.connection.waiting_count(), 0);
}

#[test]
fn other_requests_are_answered_while_a_read_waits() {
    let mut harness = events_open();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    assert_eq!(harness.one(&tgetattr(10, 0)).tag, 10);
}

#[test]
fn a_flushed_read_is_never_answered_and_consumes_nothing() {
    let mut harness = events_open();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    let flushed = harness.one(&tflush(10, 9));
    assert_eq!((flushed.kind, flushed.tag), (109, 10));
    harness.export.push_event(b"kept");
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    assert!(harness.take().is_empty(), "the flushed read has no reply");
    // The event is still there for the next read, and the old tag is free.
    assert_eq!(harness.one(&tread(9, 1, 0, 64)).data(), b"kept");
}

#[test]
fn flushing_an_answered_or_unknown_tag_is_answered_at_once() {
    let mut harness = Harness::attached();
    harness.one(&tgetattr(4, 0));
    assert_eq!(harness.one(&tflush(5, 4)).kind, 109);
    assert_eq!(harness.one(&tflush(5, 77)).kind, 109);
}

#[test]
fn past_the_waiting_bound_a_read_is_eagain_and_consumes_nothing() {
    let limits = Limits::new(8192, 4096, 2, 16, 32768, 1).unwrap();
    let mut harness = Harness::with(StaticExport::new(), ConnectionId(1), limits);
    harness.versioned(8192);
    harness.attach(0);
    harness.open(1, &[b"events"], 0).lopen();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    assert!(harness.send(&tread(10, 1, 0, 64)).unwrap().is_empty());
    assert_eq!(harness.errno(&tread(11, 1, 0, 64)), EAGAIN);
    // Input keeps flowing: a flush still gets in.
    assert_eq!(harness.one(&tflush(12, 9)).kind, 109);
    harness.export.push_event(b"one");
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    let replies = harness.take();
    assert_eq!(replies.len(), 1);
    assert_eq!((replies[0].tag, replies[0].data()), (10, b"one".to_vec()));
}

#[test]
fn clunk_answers_the_reads_waiting_on_its_fid_first() {
    let mut harness = events_open();
    harness.open(2, &[b"events"], 0).lopen();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    assert!(harness.send(&tread(10, 2, 0, 64)).unwrap().is_empty());
    assert!(harness.send(&tread(11, 1, 0, 64)).unwrap().is_empty());
    let replies = harness.send(&tclunk(12, 1)).unwrap();
    let tags: Vec<_> = replies
        .iter()
        .map(|reply| (reply.tag, reply.errno()))
        .collect();
    assert_eq!(tags, vec![(9, Some(EBADF)), (11, Some(EBADF)), (12, None)]);
    assert_eq!(replies[2].kind, 121);
    assert_eq!(
        harness.connection.waiting_count(),
        1,
        "fid 2's read still waits"
    );
}

#[test]
fn revocation_ends_a_waiting_read_when_it_is_retried() {
    let mut harness = events_open();
    assert!(harness.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    harness.export.push_event(b"withheld");
    harness.export.revoke(Epoch(1));
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    let replies = harness.take();
    assert_eq!((replies[0].tag, replies[0].errno()), (9, Some(ESTALE)));
    assert_eq!(
        harness.export.state().events.len(),
        1,
        "nothing was consumed"
    );
}

/// 4 KiB messages and exactly 4 KiB of output: one full read reply fits.
fn tight() -> Harness {
    let limits = Limits::new(4096, 512, 4, 16, 4096, 1).unwrap();
    let mut harness = Harness::with(StaticExport::new(), ConnectionId(1), limits);
    harness.versioned(4096);
    harness.attach(0);
    harness
}

#[test]
fn a_request_without_room_for_its_reply_waits_unprocessed() {
    let mut harness = tight();
    harness.open(1, &[b"info"], 0).lopen();
    let mut requests = tread(4, 1, 0, 4000);
    requests.extend(tread(5, 1, 4000, 4000));
    requests.extend(tread(6, 1, 8000, 4000));
    let reads = harness.export.state().reads;
    harness
        .connection
        .receive(&mut harness.export, &requests)
        .unwrap();
    // One reply of 4011 bytes is held; the next would pass 4096.
    assert_eq!(harness.connection.output().len(), 4011);
    assert_eq!(
        harness.export.state().reads,
        reads + 1,
        "only one read reached the owner"
    );
    assert_eq!(harness.connection.input_room(), 0, "a driver stops reading");
    for tag in [4, 5, 6] {
        let replies = harness.take();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].tag, tag);
        harness.connection.resume(&mut harness.export).unwrap();
    }
    assert_eq!(harness.export.state().reads, reads + 3);
    assert!(harness.connection.input_room() > 0);
}

#[test]
fn a_write_without_room_for_its_reply_is_not_performed() {
    let mut harness = tight();
    harness.open(1, &[b"info"], 0).lopen();
    harness.open(2, &[b"sink"], 1).lopen();
    let mut requests = tread(4, 1, 0, 4000);
    requests.extend(twrite(5, 2, 0, b"data"));
    harness
        .connection
        .receive(&mut harness.export, &requests)
        .unwrap();
    assert_eq!(
        harness.export.state().writes,
        0,
        "held until its reply can be kept"
    );
    harness.take();
    harness.connection.resume(&mut harness.export).unwrap();
    assert_eq!(harness.take()[0].written(), 4);
    assert_eq!(harness.export.state().sink, b"data");
}

#[test]
fn a_waiting_read_is_not_retried_without_room_for_its_reply() {
    let mut harness = tight();
    harness.open(1, &[b"events"], 0).lopen();
    harness.open(2, &[b"info"], 0).lopen();
    harness
        .connection
        .receive(&mut harness.export, &tread(9, 1, 0, 100))
        .unwrap();
    harness
        .connection
        .receive(&mut harness.export, &tread(10, 2, 0, 4000))
        .unwrap();
    harness.export.push_event(b"event");
    let reads = harness.export.state().reads;
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    assert_eq!(
        harness.export.state().reads,
        reads,
        "the owner was not asked"
    );
    assert_eq!(harness.export.state().events.len(), 1, "no event consumed");
    assert_eq!(harness.take().len(), 1);
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    assert_eq!(harness.take()[0].data(), b"event");
}

#[test]
fn a_flush_held_behind_full_output_still_wins_over_the_retry() {
    let mut harness = tight();
    harness.open(1, &[b"events"], 0).lopen();
    harness.open(2, &[b"info"], 0).lopen();
    harness
        .connection
        .receive(&mut harness.export, &tread(9, 1, 0, 100))
        .unwrap();
    harness
        .connection
        .receive(&mut harness.export, &tread(10, 2, 0, 4000))
        .unwrap();
    harness.export.push_event(b"event");
    harness
        .connection
        .receive(&mut harness.export, &tflush(11, 9))
        .unwrap();
    assert_eq!(
        harness.take().len(),
        1,
        "only the read that filled the output"
    );
    // A driver resumes input before retrying: the flush is answered and the
    // read it names is gone before any retry could consume the event.
    harness.connection.resume(&mut harness.export).unwrap();
    harness
        .connection
        .retry_waiting(&mut harness.export)
        .unwrap();
    let replies = harness.take();
    assert_eq!(replies.len(), 1);
    assert_eq!((replies[0].kind, replies[0].tag), (109, 11));
    assert_eq!(harness.export.state().events.len(), 1);
}

#[test]
fn a_zero_count_read_is_answered_at_once_and_consumes_nothing() {
    let mut harness = events_open();
    harness.export.push_event(b"kept");
    let reads = harness.export.state().reads;
    assert!(harness.one(&tread(9, 1, 0, 0)).data().is_empty());
    assert_eq!(
        harness.export.state().reads,
        reads,
        "the owner was not asked"
    );
    assert_eq!(harness.connection.waiting_count(), 0);
    assert_eq!(harness.one(&tread(10, 1, 0, 64)).data(), b"kept");
    // With nothing queued it still does not wait.
    assert!(harness.one(&tread(11, 1, 0, 0)).data().is_empty());
}

#[test]
fn receive_takes_only_what_it_has_room_for_and_says_so() {
    let mut harness = tight();
    harness.open(1, &[b"info"], 0).lopen();
    let mut requests = tread(4, 1, 0, 4000);
    requests.extend(tread(5, 1, 4000, 4000));
    let taken = harness
        .connection
        .receive(&mut harness.export, &requests)
        .unwrap();
    assert_eq!(taken, requests.len());
    // Held: a request waits for room, so nothing more is taken.
    let more = tgetattr(6, 0);
    assert_eq!(
        harness.connection.receive(&mut harness.export, &more),
        Ok(0)
    );
    harness.take();
    harness.connection.resume(&mut harness.export).unwrap();
    harness.take();
    // The refused bytes, offered again, are answered.
    assert_eq!(harness.send(&more).unwrap()[0].tag, 6);
}
