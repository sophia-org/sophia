use sophia_protocol::broker_files::*;
#[test]
fn limits_and_handshake_have_native_layouts() {
    let limits = BrokerFileLimits::default();
    let b = limits.encode(7).unwrap();
    assert_eq!(&b[..8], &[72, 0, 0, 0, 1, 0, 1, 0]);
    assert_eq!(
        &b[8..32],
        &[
            7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(
        &b[32..],
        &[
            2, 0, 0, 0, 0, 4, 0, 0, 128, 0, 2, 0, 16, 0, 0, 0, 0, 16, 0, 0, 0, 2, 0, 0, 136, 19, 0,
            0, 208, 7, 0, 0, 224, 46, 0, 0, 0, 0, 0, 0
        ]
    );
    let n = limits.negotiated(7, 2).unwrap();
    assert_eq!(
        (n[0], n[6], n[24], n[32], n[40], n[48], n[50]),
        (56, 16, 2, 2, 7, 128, 2)
    );
    let r = encode_broker_file_refused(7, 2).unwrap();
    assert_eq!((r[0], r[6], r[24], r[32]), (40, 17, 2, 1));
    let s = encode_broker_file_submitted(7, 1, 9, 257).unwrap();
    assert_eq!(&s[40..], &[1, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(s[32], 9);
}
#[test]
fn invalid_bounds_and_zero_identities_are_rejected() {
    assert!(
        BrokerFileLimits {
            journal_bytes: 351,
            ..Default::default()
        }
        .encode(7)
        .is_err()
    );
    assert!(BrokerFileLimits::default().encode(0).is_err());
    assert!(BrokerFileLimits::default().negotiated(7, 0).is_err());
    assert!(encode_broker_file_submitted(7, 1, 0, 256).is_err());
    assert!(encode_broker_file_submitted(7, 1, 1, 258).is_err());
}
#[test]
fn negotiation_preserves_domain_refusal_but_checks_envelope() {
    let mut b = [0; 40];
    b[0] = 40;
    b[4] = 1;
    b[7] = 1;
    b[8] = 7;
    b[16] = 9;
    b[32] = 3;
    b[34] = 1;
    let v = decode_broker_file_negotiate(&b).unwrap();
    assert_eq!((v.minimum, v.maximum), (3, 1));
    for at in [0, 4, 6, 24, 36] {
        let mut bad = b;
        bad[at] ^= 1;
        assert!(decode_broker_file_negotiate(&bad).is_err(), "{at}");
    }
    for at in [8, 16] {
        let mut bad = b;
        bad[at] = 0;
        assert!(decode_broker_file_negotiate(&bad).is_err());
    }
    assert!(decode_broker_file_negotiate(&b[..39]).is_err());
}

#[test]
fn requests_preserve_all_five_kinds_and_raw_domain_values() {
    use sophia_protocol::{AttentionState, MetadataDisclosure, NamespaceProfile, SurfaceId};
    let surface = SurfaceId::new(0, 0);
    let requests = [
        BrokerFileRequest::SurfaceAdmitted {
            surface,
            profile: NamespaceProfile::Confined,
        },
        BrokerFileRequest::CandidateReduced {
            surface,
            disclosure: MetadataDisclosure::ClassOnly,
            label: Some("\0\né"),
            redacted: true,
            generation: 0,
        },
        BrokerFileRequest::AttentionChanged {
            surface,
            attention: AttentionState::Critical,
        },
        BrokerFileRequest::SurfaceRemoved { surface },
        BrokerFileRequest::SetDisclosure {
            surface,
            disclosure: MetadataDisclosure::Full,
        },
    ];
    for (i, request) in requests.into_iter().enumerate() {
        let bytes = request.encode(7, 3, 9).unwrap();
        assert_eq!(&bytes[..8], &[200, 0, 0, 0, 1, 0, 32, 0]);
        assert_eq!(bytes[32], 9);
        assert_eq!(usize::from(bytes[40]), i + 1);
        assert_eq!(&bytes[48..56], &[0; 8]);
        assert!(request.encode(7, 3, 0).is_err());
        if i == 1 {
            assert_eq!(&bytes[58..64], &[1, 1, 4, 0, 0, 0]);
            assert_eq!(&bytes[72..76], b"\0\n\xc3\xa9");
        } else {
            assert!(bytes[58..].iter().all(|b| *b == 0));
        }
    }
}
#[test]
fn request_labels_keep_absent_empty_and_utf8_boundaries_distinct() {
    use sophia_protocol::{MetadataDisclosure, SurfaceId};
    let make = |label, redacted| BrokerFileRequest::CandidateReduced {
        surface: SurfaceId::new(1, 2),
        disclosure: MetadataDisclosure::Full,
        label,
        redacted,
        generation: 8,
    };
    assert_eq!(make(None, false).encode(7, 1, 1).unwrap()[59], 0);
    assert_eq!(make(Some(""), false).encode(7, 1, 1).unwrap()[59], 1);
    assert!(make(None, true).encode(7, 1, 1).is_err());
    let valid = "é".repeat(64);
    let invalid = "é".repeat(65);
    assert!(make(Some(&valid), false).encode(7, 1, 1).is_ok());
    assert!(make(Some(&invalid), false).encode(7, 1, 1).is_err());
}
#[test]
fn submit_and_ack_controls_are_exact_and_nonzero() {
    let mut b = [0; 24];
    b[0] = 7;
    b[8] = 9;
    b[16..20].copy_from_slice(&448u32.to_le_bytes());
    assert_eq!(
        decode_broker_file_submit(&b).unwrap(),
        BrokerFileSubmit {
            epoch: 7,
            submission: 9,
            length: 448
        }
    );
    assert_eq!(
        decode_broker_file_ack(&b[..16]).unwrap(),
        BrokerFileAck {
            epoch: 7,
            sequence: 9
        }
    );
    for at in [0, 8] {
        let mut bad = b;
        bad[at] = 0;
        assert!(decode_broker_file_submit(&bad).is_err());
        assert!(decode_broker_file_ack(&bad[..16]).is_err());
    }
    for length in [0u32, 39, 449, u32::MAX] {
        let mut bad = b;
        bad[16..20].copy_from_slice(&length.to_le_bytes());
        assert!(decode_broker_file_submit(&bad).is_err());
    }
    b[20] = 1;
    assert!(decode_broker_file_submit(&b).is_err());
    assert!(decode_broker_file_ack(&b).is_err());
    assert!(decode_broker_file_submit(&b[..23]).is_err());
}

fn response_pair() -> [u8; 448] {
    let mut b = [0; 448];
    b[..4].copy_from_slice(&448u32.to_le_bytes());
    b[4] = 1;
    b[6] = 1;
    b[7] = 1;
    b[8] = 7;
    b[16] = 2;
    b[32] = 9;
    b[40] = 5;
    b[42] = 2;
    b[48] = 1;
    b[56] = 3;
    b[60] = 4;
    b[64] = 2;
    b[66] = 1;
    b[70] = 1;
    b[80] = 8;
    b[248] = 2;
    b[256] = 3;
    b[260] = 4;
    b[266] = 1;
    b[268] = 2;
    b[280] = 8;
    b[288] = 12;
    b[296] = 1;
    b[304] = 8;
    b
}
#[test]
fn atomic_response_pair_validates_both_rows_before_returning() {
    use sophia_protocol::{MetadataDisclosure, SurfaceId};
    let request = BrokerFileRequest::SetDisclosure {
        surface: SurfaceId::new(3, 4),
        disclosure: MetadataDisclosure::ClassOnly,
    };
    let bytes = response_pair();
    let set = decode_broker_file_response(&bytes, 9, request).unwrap();
    assert_eq!(set.submission, 2);
    assert_eq!(set.rows().count(), 2);
    assert_eq!(set.rows().next().unwrap().icon, Some(0));
    for at in [
        44, 52, 65, 67, 71, 88, 112, 252, 264, 269, 270, 271, 312, 313, 316,
    ] {
        let mut bad = bytes;
        bad[at] = 255;
        assert!(
            decode_broker_file_response(&bad, 9, request).is_err(),
            "byte {at}"
        );
    }
    for at in [280, 288, 296, 304] {
        let mut bad = bytes;
        bad[at] = 0;
        assert!(decode_broker_file_response(&bad, 9, request).is_err());
    }
    assert!(decode_broker_file_response(&bytes, 8, request).is_err());
    assert!(decode_broker_file_response(&bytes[..447], 9, request).is_err());
    let mut bad = bytes;
    bad[42] = 1;
    assert!(decode_broker_file_response(&bad, 9, request).is_err());
}
#[test]
fn descriptor_borrows_control_label_and_keeps_request_generation() {
    use sophia_protocol::{MetadataDisclosure, SurfaceId};
    let request = BrokerFileRequest::CandidateReduced {
        surface: SurfaceId::new(3, 4),
        disclosure: MetadataDisclosure::Full,
        label: None,
        redacted: false,
        generation: 8,
    };
    let mut b = response_pair();
    b.copy_within(248..448, 48);
    b[248..].fill(0);
    b[40] = 2;
    b[42] = 1;
    b[112] = 1;
    b[114] = 4;
    b[120..124].copy_from_slice(b"\0\n\xc3\xa9");
    let decoded = decode_broker_file_response(&b, 9, request).unwrap();
    let label = decoded.rows().next().unwrap().label.unwrap();
    assert_eq!(label, "\0\né");
    assert_eq!(label.as_ptr(), b[120..].as_ptr());
    b[120] = 255;
    assert!(decode_broker_file_response(&b, 9, request).is_err());
    b[120] = 0;
    b[80] = 9;
    assert!(decode_broker_file_response(&b, 9, request).is_err());
}
#[test]
fn response_rejections_follow_each_request_and_keep_reason_five_distinct() {
    use sophia_protocol::{AttentionState, MetadataDisclosure, NamespaceProfile, SurfaceId};
    let surface = SurfaceId::new(3, 4);
    let requests = [
        BrokerFileRequest::SurfaceAdmitted {
            surface,
            profile: NamespaceProfile::Confined,
        },
        BrokerFileRequest::CandidateReduced {
            surface,
            disclosure: MetadataDisclosure::Full,
            label: None,
            redacted: false,
            generation: 8,
        },
        BrokerFileRequest::AttentionChanged {
            surface,
            attention: AttentionState::Notice,
        },
        BrokerFileRequest::SurfaceRemoved { surface },
        BrokerFileRequest::SetDisclosure {
            surface,
            disclosure: MetadataDisclosure::None,
        },
    ];
    for request in requests {
        for reason in 1..=6 {
            let mut b = response_pair();
            b[48..].fill(0);
            b[40] = request.kind() as u8;
            b[42] = 1;
            b[48] = 4;
            b[50] = reason;
            let allowed = reason == 5
                || match request.kind() {
                    1 => matches!(reason, 1 | 3),
                    2 => matches!(reason, 1 | 2 | 4),
                    _ => reason == 1,
                };
            assert_eq!(decode_broker_file_response(&b, 9, request).is_ok(), allowed);
        }
        let mut b = response_pair();
        b[48..].fill(0);
        b[40] = request.kind() as u8;
        b[42] = 1;
        b[48] = 5;
        assert_eq!(
            decode_broker_file_response(&b, 9, request).is_ok(),
            matches!(request.kind(), 2 | 3)
        );
    }
}
