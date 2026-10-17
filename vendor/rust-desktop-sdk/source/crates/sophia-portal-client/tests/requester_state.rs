use sophia_portal_client::requester::{Custody, Error, Journal, Submission};
use sophia_portal_protocol::requester::{Candidate, Event, EventRecord};

fn receipt(sequence: u64) -> EventRecord {
    EventRecord {
        epoch: 7,
        sequence,
        event: Event::Submitted {
            submission: 9,
            candidate_kind: 256,
        },
    }
}
fn wire(sequence: u64) -> [u8; 48] {
    let mut b = [0; 48];
    b[..4].copy_from_slice(&48u32.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&18u16.to_le_bytes());
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b[32..40].copy_from_slice(&9u64.to_le_bytes());
    b[40..42].copy_from_slice(&256u16.to_le_bytes());
    b
}
fn submission() -> Submission {
    Submission::new(
        7,
        9,
        Candidate::Negotiate {
            minimum: 1,
            maximum: 1,
        },
        0,
        100,
    )
    .unwrap()
}

#[test]
fn receipt_and_write_reply_can_arrive_in_either_order() {
    for receipt_first in [false, true] {
        let mut s = submission();
        s.issued(1).unwrap();
        if receipt_first {
            s.receipt(receipt(3), 2).unwrap();
        }
        s.submit_reply(Ok(24), 3).unwrap();
        if !receipt_first {
            s.receipt(receipt(3), 4).unwrap();
        }
        assert_eq!(s.custody(), Custody::Submitted);
        assert!(!s.releasable(2));
        assert!(s.releasable(3));
        assert_eq!(s.advance(1000), Ok(()));
        s.close();
        assert_eq!(s.custody(), Custody::Submitted);
        assert!(!s.releasable(3));
    }
}

#[test]
fn retry_is_explicit_and_preserves_candidate_and_deadline() {
    let mut s = submission();
    let candidate = s.bytes().to_vec();
    let control = *s.control();
    s.issued(1).unwrap();
    s.submit_reply(Err(11), 2).unwrap();
    assert_eq!(s.custody(), Custody::Queued);
    assert_eq!(s.bytes(), candidate);
    assert_eq!(*s.control(), control);
    assert_eq!(s.deadline(), Some(100));
    assert_eq!(s.submit_reply(Ok(24), 3), Err(Error::Busy));
    s.issued(4).unwrap();
    s.close();
    assert_eq!(s.custody(), Custody::Unknown);
    let mut unsent = submission();
    assert_eq!(unsent.advance(100), Err(Error::Deadline));
    assert_eq!(unsent.custody(), Custody::DroppedUnsent);
}

#[test]
fn contradictory_receipt_or_short_write_never_claims_success() {
    let mut s = submission();
    s.issued(1).unwrap();
    let mut wrong = receipt(1);
    wrong.epoch = 8;
    assert_eq!(s.receipt(wrong, 2), Err(Error::Protocol));
    assert_eq!(s.custody(), Custody::Unknown);
    let mut s = submission();
    s.issued(1).unwrap();
    assert_eq!(s.submit_reply(Ok(23), 2), Err(Error::Protocol));
    assert_eq!(s.custody(), Custody::Unknown);
    let mut s = submission();
    s.issued(1).unwrap();
    s.receipt(receipt(1), 2).unwrap();
    assert_eq!(s.submit_reply(Err(11), 3), Err(Error::Protocol));
    assert_eq!(s.custody(), Custody::Submitted);
}

#[test]
fn fragmented_and_coalesced_events_require_explicit_consumption_and_ack() {
    let mut j = Journal::new(7, 0, 100).unwrap();
    let bytes = [wire(1), wire(2)].concat();
    for part in bytes[..47].chunks(3) {
        j.receive(part, 1).unwrap();
    }
    assert!(j.event().is_none());
    j.receive(&bytes[47..], 2).unwrap();
    assert_eq!(j.event(), Some(receipt(1)));
    assert_eq!(j.read_count(), 0);
    assert_eq!(j.receive(&wire(3), 3), Err(Error::Busy));
    j.consume(4).unwrap();
    let ack = j.begin_ack(5).unwrap();
    assert_eq!(u64::from_le_bytes(ack[8..].try_into().unwrap()), 1);
    assert_eq!(j.event(), Some(receipt(2)));
    j.consume(6).unwrap();
    assert_eq!(j.ack_bytes(), Some(&ack));
    assert_eq!(j.acknowledged(), 0);
    j.ack_reply(Ok(16), 7).unwrap();
    assert_eq!(j.acknowledged(), 1);
    assert_eq!(j.deadline(), Some(107));
    j.begin_ack(8).unwrap();
    j.ack_reply(Ok(16), 9).unwrap();
    assert_eq!(j.acknowledged(), 2);
    assert_eq!(j.deadline(), None);
    assert_eq!(j.offset(), 96);
}

#[test]
fn reads_and_consumption_do_not_extend_ack_deadline() {
    let mut j = Journal::new(7, 0, 100).unwrap();
    j.receive(&wire(1)[..4], 1).unwrap();
    j.receive(&wire(1)[4..], 80).unwrap();
    j.consume(90).unwrap();
    j.begin_ack(99).unwrap();
    assert_eq!(j.deadline(), Some(101));
    assert_eq!(j.ack_reply(Ok(16), 101), Err(Error::Deadline));
    assert_eq!(j.acknowledged(), 0);
}

#[test]
fn journal_rejects_gaps_empty_reads_short_acks_and_regressing_clocks() {
    let mut j = Journal::new(7, 5, 100).unwrap();
    assert_eq!(j.receive(&wire(1), 4), Err(Error::ClockRegression));
    assert_eq!(j.error(), None);
    j.receive(&wire(1), 5).unwrap();
    j.consume(6).unwrap();
    j.begin_ack(7).unwrap();
    assert_eq!(j.ack_reply(Ok(15), 8), Err(Error::Protocol));
    assert_eq!(j.acknowledged(), 0);
    let mut j = Journal::new(7, 0, 100).unwrap();
    assert_eq!(j.receive(&wire(2), 1), Err(Error::Protocol));
    assert_eq!(j.event(), None);
    let mut j = Journal::new(7, 0, 100).unwrap();
    assert_eq!(j.receive(&[], 1), Err(Error::Protocol));
}
