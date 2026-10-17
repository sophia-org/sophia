use sophia_broker_client::{
    Error,
    phase::{Ordering, Phase},
    protocol::{Event, EventRecord, Limits},
};
fn new() -> Ordering {
    Ordering::new(
        7,
        Limits {
            max_surfaces: 1024,
            max_label_bytes: 128,
            journal_records: 16,
            journal_bytes: 4096,
            staging_bytes: 512,
            response_timeout_ms: 5000,
            ack_progress_timeout_ms: 2000,
            assembly_timeout_ms: 12000,
        },
        1,
        0,
    )
    .unwrap()
}
fn event(sequence: u64, event: Event<'static>) -> EventRecord<'static> {
    EventRecord {
        epoch: 7,
        sequence,
        event,
    }
}
#[test]
fn handshake_order_and_refusal_deadline() {
    let mut s = new();
    assert_eq!(
        s.event(
            event(
                1,
                Event::Negotiated {
                    max_surfaces: 1024,
                    max_label_bytes: 128
                }
            ),
            1
        ),
        Err(Error::Protocol)
    );
    let mut s = new();
    s.event(
        event(
            1,
            Event::Submitted {
                submission: 1,
                candidate_kind: 256,
            },
        ),
        1,
    )
    .unwrap();
    s.event(event(2, Event::Refused), 2).unwrap();
    assert_eq!(s.phase(), Phase::Refused);
    assert!(!s.refusal_acked(1));
    assert!(s.refusal_acked(2));
    assert_eq!(s.refusal_deadline(), Some(2002));
    assert_eq!(s.advance(2002), Err(Error::Deadline));
}
#[test]
fn request_response_correlation_and_monotonic_request_ids() {
    use sophia_broker_client::protocol::{Request, RequestKind};
    let mut s = new();
    s.event(
        event(
            1,
            Event::Submitted {
                submission: 1,
                candidate_kind: 256,
            },
        ),
        1,
    )
    .unwrap();
    s.event(
        event(
            2,
            Event::Negotiated {
                max_surfaces: 1024,
                max_label_bytes: 128,
            },
        ),
        2,
    )
    .unwrap();
    let req = Request {
        id: 8,
        kind: RequestKind::SurfaceRemoved,
        surface_index: 0,
        surface_generation: 0,
        value: 0,
        redacted: false,
        label: None,
        candidate_generation: 0,
    };
    s.event(event(3, Event::Request(req)), 3).unwrap();
    assert_eq!(s.response(9, 2, 4), Err(Error::Busy));
    s.response(8, 2, 4).unwrap();
    s.event(
        event(
            4,
            Event::Submitted {
                submission: 2,
                candidate_kind: 257,
            },
        ),
        5,
    )
    .unwrap();
    assert_eq!(s.pending(), None);
    assert_eq!(
        s.event(event(5, Event::Request(req)), 6),
        Err(Error::Protocol)
    );
}
