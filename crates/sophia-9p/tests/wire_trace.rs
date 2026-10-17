#![cfg(feature = "comparator-wire-trace")]
mod support;
use support::*;

#[test]
fn trace_distinguishes_partial_delivery_overlap_and_sequential_writes() {
    let mut h = Harness::attached();
    h.open(1, &[b"sink"], 1);
    sophia_9p::wire_trace::begin();
    h.connection
        .receive(&mut h.export, &twrite(10, 1, 0, b"a"))
        .unwrap();
    h.connection.sent(3); // A partial header is not a delivered reply.
    h.connection
        .receive(&mut h.export, &twrite(11, 1, 1, b"b"))
        .unwrap();
    h.connection.sent(h.connection.output().len());
    h.one(&twrite(12, 1, 2, b"c"));
    h.one(&twrite(13, 1, 3, b"d"));
    let events = sophia_9p::wire_trace::take();
    let index = |phase, tag| {
        events
            .iter()
            .position(|e| e.phase == phase && e.tag == tag)
            .unwrap()
    };
    assert!(index("request", 11) < index("sent", 10));
    assert!(index("sent", 12) < index("request", 13));
    for tag in 10..14 {
        assert_eq!(
            events
                .iter()
                .filter(|e| e.phase == "request" && e.tag == tag)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.phase == "sent" && e.tag == tag)
                .count(),
            1
        );
    }
}
