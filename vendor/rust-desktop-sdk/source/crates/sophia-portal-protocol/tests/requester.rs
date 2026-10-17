use sophia_portal_protocol::requester::*;
fn record(kind: u16, length: usize, submission: u64, sequence: u64) -> Vec<u8> {
    let mut b = vec![0; length];
    b[..4].copy_from_slice(&(length as u32).to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&kind.to_le_bytes());
    b[8..16].copy_from_slice(&7u64.to_le_bytes());
    b[16..24].copy_from_slice(&submission.to_le_bytes());
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b
}
fn put(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}
#[test]
fn candidates_match_literal_layout_and_keep_domain_refusals_representable() {
    let mut out = [0xa5; 345];
    assert_eq!(
        encode_candidate(
            7,
            9,
            Candidate::Negotiate {
                minimum: 2,
                maximum: 0
            },
            &mut out
        ),
        Ok(40)
    );
    let mut expected = record(256, 40, 9, 0);
    expected[32] = 2;
    assert_eq!(&out[..40], expected);
    assert_eq!(out[40], 0xa5);
    let kinds = [
        TransferKind::Clipboard,
        TransferKind::DragAndDrop,
        TransferKind::FileHandoff,
        TransferKind::ScreenCapture,
        TransferKind::ScreenRecording,
        TransferKind::UriOpen,
        TransferKind::Notification,
    ];
    let max = "é".repeat(127) + "x";
    for kind in kinds {
        for mime in [None, Some(""), Some("text/plain"), Some(max.as_str())] {
            let req = TransferRequest {
                transfer: 0,
                source: 2,
                target: 3,
                kind,
                mime,
                byte_size: u64::MAX,
                generation: 0,
                deadline_msec: 0,
            };
            let mut expected = record(257, 344, 9, 0);
            put(&mut expected, 40, 2);
            put(&mut expected, 48, 3);
            expected[56..58].copy_from_slice(&(kind as u16).to_le_bytes());
            put(&mut expected, 64, u64::MAX);
            if let Some(m) = mime {
                expected[58] = 1;
                expected[60..62].copy_from_slice(&(m.len() as u16).to_le_bytes());
                expected[88..88 + m.len()].copy_from_slice(m.as_bytes());
            }
            assert_eq!(
                encode_candidate(7, 9, Candidate::Request(req), &mut out),
                Ok(344)
            );
            assert_eq!(&out[..344], expected);
            assert_eq!(out[344], 0xa5);
        }
    }
    for (candidate, tag, len, at) in [
        (
            Candidate::Begin {
                transfer: 5,
                total_bytes: 65536,
            },
            258,
            56,
            48,
        ),
        (
            Candidate::End {
                transfer: 5,
                total_bytes: 65536,
            },
            259,
            48,
            40,
        ),
        (Candidate::Cancel { transfer: 5 }, 260, 40, 0),
    ] {
        let mut expected = record(tag, len, 9, 0);
        put(&mut expected, 32, 5);
        if at != 0 {
            put(&mut expected, at, 65536);
        }
        assert_eq!(encode_candidate(7, 9, candidate, &mut out), Ok(len));
        assert_eq!(&out[..len], expected);
    }
    let before = out;
    let invalid = [
        Candidate::Begin {
            transfer: 0,
            total_bytes: 0,
        },
        Candidate::End {
            transfer: 1,
            total_bytes: 65537,
        },
        Candidate::Cancel { transfer: 0 },
        Candidate::Request(TransferRequest {
            transfer: 1,
            source: 2,
            target: 2,
            kind: TransferKind::Clipboard,
            mime: None,
            byte_size: 0,
            generation: 0,
            deadline_msec: 0,
        }),
    ];
    for c in invalid {
        assert!(encode_candidate(7, 9, c, &mut out).is_err());
        assert_eq!(out, before);
    }
    for (epoch, id, cap) in [(0, 9, 344), (7, 0, 344), (7, 9, 39)] {
        assert!(
            encode_candidate(
                epoch,
                id,
                Candidate::Cancel { transfer: 1 },
                &mut out[..cap]
            )
            .is_err()
        );
        assert_eq!(out, before);
    }
    let long = "x".repeat(256);
    assert!(
        encode_candidate(
            7,
            9,
            Candidate::Request(TransferRequest {
                transfer: 1,
                source: 2,
                target: 3,
                kind: TransferKind::Clipboard,
                mime: Some(&long),
                byte_size: 0,
                generation: 0,
                deadline_msec: 0
            }),
            &mut out
        )
        .is_err()
    );
    assert_eq!(out, before);
    let mut submit = [0; 24];
    submit[0] = 7;
    submit[8] = 9;
    submit[16..20].copy_from_slice(&344u32.to_le_bytes());
    assert_eq!(encode_submit(7, 9, 344), Ok(submit));
    assert!(encode_submit(7, 9, 345).is_err());
    let mut ack = [0; 16];
    ack[0] = 7;
    ack[8] = 11;
    assert_eq!(encode_ack(7, 11), Ok(ack));
    assert!(encode_ack(0, 1).is_err());
    assert!(encode_ack(7, 0).is_err());
}
#[test]
fn discovery_limits_and_cross_field_bounds() {
    let prefix = "sophia-portal-files version=1 role=portal-requester epoch=";
    assert_eq!(
        decode_api(format!("{prefix}18446744073709551615 fd_transfer=none\n").as_bytes()),
        Ok(u64::MAX)
    );
    for n in ["0", "01", "18446744073709551616", "-1", "", " 1"] {
        assert!(decode_api(format!("{prefix}{n} fd_transfer=none\n").as_bytes()).is_err());
    }
    let mut b = record(1, 80, 0, 0);
    b[32] = 1;
    b[34] = 1;
    b[44] = 255;
    for (at, v) in [
        (36, 64u32),
        (40, 65536),
        (48, 256),
        (52, 32768),
        (56, 12000),
        (60, 2000),
        (64, 344),
        (68, 4096),
    ] {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    put(&mut b, 72, 17);
    let (epoch, limits) = decode_limits(&b).unwrap();
    assert_eq!(epoch, 7);
    assert_eq!(limits.broker_generation, 17);
    assert_eq!(limits.max_payload_bytes, 65536);
    for n in 0..80 {
        assert!(decode_limits(&b[..n]).is_err());
    }
    for (at, v) in [
        (36, 65u32),
        (40, 0),
        (48, 3),
        (52, 279),
        (56, 12001),
        (60, 2001),
        (64, 345),
        (68, 63),
    ] {
        let mut bad = b.clone();
        bad[at..at + 4].copy_from_slice(&v.to_le_bytes());
        assert!(decode_limits(&bad).is_err());
    }
    for at in [4, 8, 16, 24, 32, 34, 44, 46, 72] {
        let mut bad = b.clone();
        if at == 8 || at == 72 {
            put(&mut bad, at, 0);
        } else {
            bad[at] ^= 1;
        }
        assert!(decode_limits(&bad).is_err(), "offset {at}");
    }
}
#[test]
fn exact_events_padding_epochs_and_all_outcomes() {
    let mut cases = Vec::new();
    let mut b = record(16, 48, 0, 1);
    b[32] = 1;
    put(&mut b, 40, 7);
    cases.push((b, Event::Negotiated));
    let mut b = record(17, 40, 0, 1);
    b[32] = 1;
    cases.push((b, Event::Refused));
    let mut b = record(18, 48, 0, 1);
    put(&mut b, 32, 9);
    b[40..42].copy_from_slice(&257u16.to_le_bytes());
    cases.push((
        b,
        Event::Submitted {
            submission: 9,
            candidate_kind: 257,
        },
    ));
    let mut denied = record(32, 88, 0, 1);
    denied[40] = 1;
    cases.push((denied.clone(), Event::Denied { transfer: 0 }));
    for at in 41..88 {
        let mut bad = denied.clone();
        bad[at] = 1;
        assert!(decode_event(&bad).is_err());
    }
    let grant = Grant {
        transfer: 5,
        kind: TransferKind::Clipboard,
        source: 2,
        target: 3,
        source_generation: 9,
        broker_generation: 17,
        deadline_msec: 100,
    };
    let mut allowed = record(32, 88, 0, 1);
    allowed[40] = 2;
    allowed[41] = 1;
    allowed[42] = 1;
    for (at, v) in [(32, 5), (48, 2), (56, 3), (64, 9), (72, 17), (80, 100)] {
        put(&mut allowed, at, v);
    }
    cases.push((allowed.clone(), Event::Allowed(grant)));
    for at in [32, 48, 56, 64, 72, 80] {
        let mut bad = allowed.clone();
        put(&mut bad, at, 0);
        assert!(decode_event(&bad).is_err());
    }
    for at in 44..48 {
        let mut bad = allowed.clone();
        bad[at] = 1;
        assert!(decode_event(&bad).is_err());
    }
    for (state, outcome) in [
        (2, Outcome::Completed),
        (3, Outcome::Revoked),
        (4, Outcome::Expired),
    ] {
        let mut b = record(33, 48, 0, 1);
        put(&mut b, 32, 5);
        b[40] = state;
        cases.push((
            b,
            Event::TransferOutcome {
                transfer: 5,
                outcome,
            },
        ));
    }
    for (b, event) in cases {
        assert_eq!(
            decode_event(&b),
            Ok(EventRecord {
                epoch: 7,
                sequence: 1,
                event
            })
        );
        for n in 0..b.len() {
            assert!(decode_event(&b[..n]).is_err());
        }
        let mut bad = b.clone();
        bad.push(0);
        assert!(decode_event(&bad).is_err());
        for at in [4, 8, 16, 24] {
            let mut bad = b.clone();
            if at == 8 {
                put(&mut bad, 8, 0);
            } else {
                bad[at] ^= 1;
            }
            assert!(decode_event(&bad).is_err());
        }
    }
}
