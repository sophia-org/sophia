use sophia_broker_protocol::{Event, decode_event};
fn record(kind: u8, size: usize) -> Vec<u8> {
    let mut b = vec![0; size];
    b[0] = size as u8;
    b[4] = 1;
    b[6] = kind;
    b[8] = 7;
    b[24] = 1;
    b
}
#[test]
fn all_requests_preserve_domain_values_and_borrow_labels() {
    for kind in 1..=5 {
        let mut b = record(32, 200);
        b[32] = 1;
        b[40] = kind;
        b[56] = if kind == 4 { 0 } else { 1 };
        let Event::Request(r) = decode_event(&b, 7).unwrap().event else {
            panic!()
        };
        assert_eq!(
            (
                r.surface_index,
                r.surface_generation,
                r.candidate_generation
            ),
            (0, 0, 0)
        );
        assert_eq!(r.label, None);
        for n in 0..200 {
            assert!(decode_event(&b[..n], 7).is_err());
        }
        for offset in [16, 42, 44, 62] {
            let mut bad = b.clone();
            bad[offset] = 1;
            assert!(decode_event(&bad, 7).is_err());
        }
        if kind != 2 {
            b[59] = 1;
            assert!(decode_event(&b, 7).is_err());
        }
    }
    let mut b = record(32, 200);
    b[32] = 1;
    b[40] = 2;
    b[56] = 3;
    b[59] = 1;
    for label in ["", "\0\n", "é", "😀"] {
        b[72..].fill(0);
        b[60] = label.len() as u8;
        b[72..72 + label.len()].copy_from_slice(label.as_bytes());
        let Event::Request(r) = decode_event(&b, 7).unwrap().event else {
            panic!()
        };
        assert_eq!(r.label, Some(label));
        assert_eq!(r.label.unwrap().as_ptr(), b[72..].as_ptr());
    }
    b[60] = 128;
    b[72..].fill(b'x');
    assert!(decode_event(&b, 7).is_ok());
    for (offset, value) in [(58, 2), (59, 2), (60, 129), (72, 0xff)] {
        let mut bad = b.clone();
        bad[offset] = value;
        assert!(decode_event(&bad, 7).is_err());
    }
    b[59] = 0;
    assert!(decode_event(&b, 7).is_err());
    b[60] = 0;
    b[72..].fill(0);
    b[58] = 1;
    assert!(decode_event(&b, 7).is_err());
}
#[test]
fn handshake_events_and_envelope_validation() {
    let mut negotiated = record(16, 56);
    negotiated[32] = 2;
    negotiated[36] = 1;
    negotiated[40] = 7;
    negotiated[48] = 128;
    negotiated[50] = 2;
    let mut refused = record(17, 40);
    refused[32] = 1;
    let mut submitted = record(18, 48);
    submitted[32] = 1;
    submitted[41] = 1;
    for good in [negotiated, refused, submitted] {
        assert!(decode_event(&good, 7).is_ok());
        assert!(decode_event(&good, 8).is_err());
        for n in 0..good.len() {
            assert!(decode_event(&good[..n], 7).is_err());
        }
        for offset in [0, 4, 8, 16, 24, good.len() - 1] {
            let mut bad = good.clone();
            bad[offset] ^= 1;
            assert!(decode_event(&bad, 7).is_err());
        }
    }
}
