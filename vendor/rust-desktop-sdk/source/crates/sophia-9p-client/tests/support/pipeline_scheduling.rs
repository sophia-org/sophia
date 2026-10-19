//! Deterministic socket controls: replies are queued before the tested round,
//! so absence from the completed queue proves actual read deferral, not a race.
use super::*;
use sophia_9p_client::pipeline::{IoBudget, PollCycle};
use std::net::Shutdown;

fn connected() -> (Pipeline, UnixStream) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let negotiation = std::thread::spawn(move || {
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        versioned(&mut server);
        server
    });
    let pipeline =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    (pipeline, negotiation.join().unwrap())
}

fn answer(server: &mut UnixStream, tag: Tag, bytes: &[u8]) {
    let mut body = (bytes.len() as u32).to_le_bytes().to_vec();
    body.extend_from_slice(bytes);
    server.write_all(&reply(117, tag.0, &body)).unwrap();
}

fn empty_round(pipeline: &mut Pipeline, server: &mut UnixStream, cycle: &mut PollCycle) -> Tag {
    let tag = pipeline.read(Fid(0), 0, 8).unwrap();
    pipeline.poll_in_cycle(cycle, None).unwrap();
    let (kind, sent, _) = request(server);
    assert_eq!((kind, sent), (116, tag.0));
    assert!(pipeline.take_reply().is_none());
    tag
}

#[test]
fn queued_reply_is_deferred_only_after_empty_read_and_successful_write() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    answer(&mut server, first, b"first");
    let second = pipeline.read(Fid(0), 8, 8).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    assert_eq!(request(&mut server).1, second.0, "write is not deferred");
    assert!(
        pipeline.take_reply().is_none(),
        "queued reply must actually be deferred"
    );
    assert!(pipeline.io_interest().unwrap().readable);
    pipeline.wait_for_io(Duration::ZERO).unwrap();
    pipeline
        .poll_in_cycle(&mut PollCycle::default(), None)
        .unwrap();
    assert_eq!(
        pipeline.take_reply(),
        Some((first, Reply::Read(b"first".to_vec())))
    );
}

#[test]
fn deferral_cannot_suppress_the_following_writing_round() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    answer(&mut server, first, b"first");
    pipeline.read(Fid(0), 8, 8).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    request(&mut server);
    assert!(pipeline.take_reply().is_none());
    pipeline.read(Fid(0), 16, 8).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    request(&mut server);
    assert_eq!(
        pipeline.take_reply(),
        Some((first, Reply::Read(b"first".to_vec())))
    );
}

#[test]
fn no_write_round_and_round_after_productive_read_still_receive() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    answer(&mut server, first, b"first");
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    assert_eq!(
        pipeline.take_reply(),
        Some((first, Reply::Read(b"first".to_vec())))
    );
    let second = pipeline.read(Fid(0), 8, 8).unwrap();
    answer(&mut server, second, b"second");
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    request(&mut server);
    assert_eq!(
        pipeline.take_reply(),
        Some((second, Reply::Read(b"second".to_vec())))
    );
}

#[test]
fn deferred_half_close_is_seen_on_entry_and_keeps_completed_reply() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    answer(&mut server, first, b"final");
    server.shutdown(Shutdown::Write).unwrap();
    pipeline.read(Fid(0), 8, 8).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    request(&mut server);
    assert!(!pipeline.is_poisoned());
    assert!(pipeline.take_reply().is_none());
    pipeline.wait_for_io(Duration::ZERO).unwrap();
    assert_eq!(
        pipeline.poll_in_cycle(&mut PollCycle::default(), None),
        Err(PipelineError::Io(ErrorKind::UnexpectedEof))
    );
    assert!(pipeline.is_poisoned());
    assert_eq!(
        pipeline.take_reply(),
        Some((first, Reply::Read(b"final".to_vec())))
    );
}

#[test]
fn exhausted_budget_does_not_create_empty_read_history() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    answer(&mut server, first, b"first");
    let second = pipeline.read(Fid(0), 8, 8).unwrap();
    let mut zero = IoBudget::new(0);
    pipeline.poll_in_cycle(&mut cycle, Some(&mut zero)).unwrap();
    assert!(zero.exhausted());
    assert!(pipeline.take_reply().is_none());
    let mut budget = IoBudget::new(4096);
    pipeline
        .poll_in_cycle(&mut cycle, Some(&mut budget))
        .unwrap();
    assert_eq!(request(&mut server).1, second.0);
    assert_eq!(
        pipeline.take_reply(),
        Some((first, Reply::Read(b"first".to_vec())))
    );
    assert!(!budget.exhausted());
}

#[test]
fn partial_reply_bytes_count_as_productive_even_without_a_completed_reply() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let first = empty_round(&mut pipeline, &mut server, &mut cycle);
    let frame = reply(117, first.0, &[1, 0, 0, 0, 42]);
    server.write_all(&frame[..1]).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    assert!(pipeline.take_reply().is_none());
    pipeline.read(Fid(0), 8, 8).unwrap();
    server.write_all(&frame[1..]).unwrap();
    pipeline.poll_in_cycle(&mut cycle, None).unwrap();
    request(&mut server);
    assert_eq!(pipeline.take_reply(), Some((first, Reply::Read(vec![42]))));
}

#[test]
fn failed_write_poisoning_is_not_deferred() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    empty_round(&mut pipeline, &mut server, &mut cycle);
    drop(server);
    pipeline.read(Fid(0), 8, 8).unwrap();
    assert!(matches!(
        pipeline.poll_in_cycle(&mut cycle, None),
        Err(PipelineError::Io(
            ErrorKind::BrokenPipe | ErrorKind::ConnectionReset
        ))
    ));
    assert!(pipeline.is_poisoned());
}

#[test]
fn deferred_read_spends_no_credit_and_shared_read_attempt_limit_still_applies() {
    let (mut pipeline, mut server) = connected();
    let mut cycle = PollCycle::default();
    let mut budget = IoBudget::new(4096);
    pipeline
        .poll_in_cycle(&mut cycle, Some(&mut budget))
        .unwrap();
    pipeline.read(Fid(0), 0, 8).unwrap();
    pipeline
        .poll_in_cycle(&mut cycle, Some(&mut budget))
        .unwrap();
    request(&mut server);
    for _ in 0..30 {
        pipeline
            .poll_in_cycle(&mut cycle, Some(&mut budget))
            .unwrap();
    }
    assert!(!budget.exhausted(), "only 31 read attempts have run");
    pipeline
        .poll_in_cycle(&mut cycle, Some(&mut budget))
        .unwrap();
    assert!(
        budget.exhausted(),
        "the 32nd read attempt spends the last credit"
    );
    let exhausted = budget;
    pipeline
        .poll_in_cycle(&mut cycle, Some(&mut budget))
        .unwrap();
    assert_eq!(budget, exhausted, "another round cannot replenish credit");
}
