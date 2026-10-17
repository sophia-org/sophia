use sophia_broker_client::{
    Candidate, Custody as C, Error, Submission,
    protocol::{Event, EventRecord, Request, RequestKind, ResponseRow},
};
fn new() -> Submission {
    Submission::new(
        7,
        9,
        Candidate::Negotiate {
            minimum: 2,
            maximum: 2,
        },
        0,
        100,
    )
    .unwrap()
}
fn receipt() -> EventRecord<'static> {
    EventRecord {
        epoch: 7,
        sequence: 3,
        event: Event::Submitted {
            submission: 9,
            candidate_kind: 256,
        },
    }
}
#[test]
fn explicit_retry_is_immutable_and_receipt_orders_require_ack() {
    for first in [false, true] {
        let mut s = new();
        let bytes = s.bytes().to_vec();
        let control = *s.control();
        s.issued(1).unwrap();
        s.submit_reply(Err(11), 2).unwrap();
        assert_eq!(s.custody(), C::Queued);
        s.advance(3).unwrap();
        assert_eq!(s.bytes(), bytes);
        assert_eq!(*s.control(), control);
        s.issued(4).unwrap();
        if first {
            s.receipt(receipt(), 5).unwrap();
            assert!(!s.releasable(3));
        }
        s.submit_reply(Ok(24), 6).unwrap();
        if !first {
            assert_eq!(s.custody(), C::WriteAccepted);
            s.receipt(receipt(), 7).unwrap();
        }
        assert!(!s.releasable(2));
        assert!(s.releasable(3));
        s.advance(100).unwrap();
        assert_eq!(s.deadline(), None);
        s.close();
        assert_eq!(s.custody(), C::Submitted);
    }
}
#[test]
fn errors_keep_known_custody_and_never_replay() {
    let mut s = new();
    s.close();
    assert_eq!(s.custody(), C::DroppedUnsent);
    for count in [0, 23, 25] {
        let mut s = new();
        s.issued(1).unwrap();
        assert_eq!(s.submit_reply(Ok(count), 2), Err(Error::Protocol));
        assert_eq!(s.custody(), C::Unknown);
    }
    let mut s = new();
    s.issued(1).unwrap();
    assert_eq!(s.submit_reply(Err(116), 2), Err(Error::Remote(116)));
    assert_eq!(s.custody(), C::Refused);
    let mut s = new();
    s.issued(1).unwrap();
    s.receipt(receipt(), 2).unwrap();
    s.close();
    assert_eq!(s.custody(), C::Submitted);
    assert!(!s.releasable(3));
    for fault in 0..4 {
        let mut s = new();
        s.issued(1).unwrap();
        let mut r = receipt();
        match fault {
            0 => r.epoch = 8,
            1 => r.sequence = 0,
            2 => {
                r.event = Event::Submitted {
                    submission: 10,
                    candidate_kind: 256,
                }
            }
            _ => {
                r.event = Event::Submitted {
                    submission: 9,
                    candidate_kind: 257,
                }
            }
        }
        assert_eq!(s.receipt(r, 2), Err(Error::Protocol));
        assert_eq!(s.custody(), C::Unknown);
    }
}
#[test]
fn deadlines_and_caller_clock_do_not_extend_custody() {
    let mut s = new();
    s.advance(5).unwrap();
    assert_eq!(s.advance(4), Err(Error::ClockRegression));
    assert_eq!(s.error(), None);
    s.issued(6).unwrap();
    s.submit_reply(Err(11), 99).unwrap();
    assert_eq!(s.issued(100), Err(Error::Deadline));
    assert_eq!(s.custody(), C::DroppedUnsent);
    let mut s = new();
    s.issued(1).unwrap();
    assert_eq!(s.advance(100), Err(Error::Deadline));
    assert_eq!(s.custody(), C::Unknown);
}
#[test]
fn response_constructor_copies_borrowed_inputs_once() {
    let s = {
        let q = Request {
            id: 1,
            kind: RequestKind::CandidateReduced,
            surface_index: 0,
            surface_generation: 0,
            value: 1,
            redacted: false,
            label: None,
            candidate_generation: 0,
        };
        let rows = [ResponseRow::default()];
        Submission::new(
            7,
            9,
            Candidate::Response {
                request: &q,
                rows: &rows,
            },
            0,
            100,
        )
        .unwrap()
    };
    assert_eq!(s.bytes().len(), 448);
    assert_eq!(&s.bytes()[6..8], &[1, 1]);
    assert_eq!(&s.control()[16..20], &[192, 1, 0, 0]);
}
