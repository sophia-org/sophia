use sophia_broker_client::{Error, Journal, protocol::Event};
fn event(sequence: u64) -> [u8; 200] {
    let mut b = [0; 200];
    b[0] = 200;
    b[4] = 1;
    b[6] = 32;
    b[8] = 7;
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b[32] = 1;
    b[40] = 2;
    b[56] = 3;
    b[59] = 1;
    b[60] = 2;
    b[72] = 0;
    b[73] = 10;
    b
}
#[test]
fn fragments_retained_borrow_and_frozen_ack() {
    let mut j = Journal::new(7, 0, 100).unwrap();
    let a = event(1);
    let b = event(2);
    j.receive(&a[..3], 1).unwrap();
    assert!(j.event().is_none());
    assert_eq!(j.deadline(), Some(101));
    j.receive(&a[3..], 2).unwrap();
    let Event::Request(r) = j.event().unwrap().event else {
        panic!()
    };
    assert_eq!(r.label, Some("\0\n"));
    let pointer = r.label.unwrap().as_ptr();
    assert_eq!(j.read_count(), 0);
    assert_eq!(j.receive(&b, 3), Err(Error::Busy));
    let Event::Request(r) = j.event().unwrap().event else {
        panic!()
    };
    assert_eq!(r.label.unwrap().as_ptr(), pointer);
    j.consume(3).unwrap();
    let ack = j.begin_ack(4).unwrap();
    j.receive(&b, 5).unwrap();
    j.consume(6).unwrap();
    assert_eq!(j.ack_bytes(), Some(&ack));
    assert_eq!(j.acknowledged(), 0);
    j.ack_reply(Ok(16), 7).unwrap();
    assert_eq!(j.acknowledged(), 1);
    assert_eq!(j.deadline(), Some(107));
    j.begin_ack(8).unwrap();
    j.ack_reply(Ok(16), 9).unwrap();
    assert_eq!(j.deadline(), None);
}
#[test]
fn coalescing_compaction_and_sequence_failures() {
    let mut j = Journal::new(7, 0, 100).unwrap();
    let mut batch = event(1).to_vec();
    batch.extend(event(2));
    batch.extend(&event(3)[..100]);
    j.receive(&batch, 1).unwrap();
    j.consume(2).unwrap();
    j.consume(3).unwrap();
    assert!(j.event().is_none());
    j.receive(&event(3)[100..], 4).unwrap();
    assert_eq!(j.event().unwrap().sequence, 3);
    assert_eq!(j.offset(), 600);
    for fault in 0..4 {
        let mut j = Journal::new(7, 0, 100).unwrap();
        let mut b = event(1);
        match fault {
            0 => b[24] = 2,
            1 => b[8] = 8,
            2 => b[0] = 201,
            _ => b[72] = 255,
        }
        assert_eq!(j.receive(&b, 1), Err(Error::Protocol));
        assert!(j.event().is_none());
    }
}
#[test]
fn observed_deadline_ack_errors_and_clock_bounds() {
    let mut j = Journal::new(7, 0, 10).unwrap();
    j.receive(&event(1)[..1], 1).unwrap();
    assert_eq!(j.advance(0), Err(Error::ClockRegression));
    assert_eq!(j.error(), None);
    j.receive(&event(1)[1..], 10).unwrap();
    assert_eq!(j.advance(11), Err(Error::Deadline));
    for reply in [Ok(15), Err(5)] {
        let mut j = Journal::new(7, 0, 100).unwrap();
        j.receive(&event(1), 1).unwrap();
        j.consume(2).unwrap();
        j.begin_ack(3).unwrap();
        assert!(j.ack_reply(reply, 4).is_err());
        assert_eq!(j.acknowledged(), 0);
    }
    let mut j = Journal::new(7, u64::MAX - 1, 10).unwrap();
    assert_eq!(j.receive(&event(1), u64::MAX - 1), Err(Error::Overflow));
    assert_eq!(j.offset(), 0);
}
