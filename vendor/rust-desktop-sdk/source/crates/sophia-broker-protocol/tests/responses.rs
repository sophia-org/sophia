use sophia_broker_protocol::{
    Request, RequestKind as K, ResponseRow as R, RowKind as T, encode_response,
};
fn request(kind: K) -> Request<'static> {
    Request {
        id: 9,
        kind,
        surface_index: 3,
        surface_generation: 4,
        value: 2,
        redacted: false,
        label: None,
        candidate_generation: 8,
    }
}
fn descriptor() -> R<'static> {
    R {
        kind: T::EmitDescriptor,
        surface_index: 3,
        surface_generation: 4,
        trust: 1,
        attention: 2,
        generation: 8,
        grant_token: 12,
        grant_revocation_epoch: 1,
        grant_target_generation: 8,
        ..R::default()
    }
}
#[test]
fn atomic_two_rows_and_failed_second_preserve_output() {
    let rule = R {
        kind: T::PublishRule,
        surface_index: 3,
        surface_generation: 4,
        disclosure: 2,
        trust: 1,
        icon: Some(0),
        generation: 7,
        ..R::default()
    };
    for kind in [K::SurfaceAdmitted, K::SetDisclosure] {
        let mut out = [0xa5; 448];
        assert_eq!(
            encode_response(7, 2, &request(kind), &[rule, descriptor()], &mut out),
            Ok(448)
        );
        assert_eq!(&out[..8], &[192, 1, 0, 0, 1, 0, 1, 1]);
        assert_eq!(
            (out[32], out[40], out[42], out[48], out[248]),
            (9, kind as u8, 2, 1, 2)
        );
        assert_eq!(out[70], 1);
        assert_eq!(&out[72..80], &[0; 8]);
        for bad in [
            R {
                grant_token: 0,
                ..descriptor()
            },
            R {
                surface_generation: 5,
                ..descriptor()
            },
            R {
                label: Some(""),
                ..descriptor()
            },
            R {
                grant_target_generation: 9,
                ..descriptor()
            },
        ] {
            out.fill(0xa5);
            assert!(encode_response(7, 2, &request(kind), &[rule, bad], &mut out).is_err());
            assert_eq!(out, [0xa5; 448]);
        }
        assert!(encode_response(7, 2, &request(kind), &[descriptor(), rule], &mut out).is_err());
    }
}
#[test]
fn all_request_shapes_rejections_and_labels() {
    for kind in [
        K::SurfaceAdmitted,
        K::CandidateReduced,
        K::AttentionChanged,
        K::SurfaceRemoved,
        K::SetDisclosure,
    ] {
        for reason in 1..=5 {
            let expected = reason == 5
                || match kind {
                    K::SurfaceAdmitted => matches!(reason, 1 | 3),
                    K::CandidateReduced => matches!(reason, 1 | 2 | 4),
                    _ => reason == 1,
                };
            assert_eq!(
                encode_response(
                    7,
                    2,
                    &request(kind),
                    &[R {
                        kind: T::Rejected,
                        value: reason,
                        ..R::default()
                    }],
                    &mut [0; 448]
                )
                .is_ok(),
                expected
            );
        }
    }
    for label in [None, Some(""), Some("\0\n"), Some("é")] {
        let mut out = [0; 448];
        let row = R {
            label,
            redacted: label.is_some(),
            ..descriptor()
        };
        encode_response(7, 2, &request(K::CandidateReduced), &[row], &mut out).unwrap();
        assert_eq!(out[112], u8::from(label.is_some()));
        assert!(out[248..].iter().all(|b| *b == 0));
    }
    let mut out = [0xa5; 448];
    assert!(
        encode_response(
            7,
            2,
            &request(K::CandidateReduced),
            &[R {
                generation: 9,
                grant_target_generation: 9,
                ..descriptor()
            }],
            &mut out
        )
        .is_err()
    );
    assert!(
        encode_response(
            7,
            2,
            &request(K::AttentionChanged),
            &[R {
                attention: 1,
                ..descriptor()
            }],
            &mut out
        )
        .is_err()
    );
    let retire = R {
        kind: T::RetireSurface,
        surface_index: 3,
        surface_generation: 4,
        ..R::default()
    };
    assert!(encode_response(7, 2, &request(K::SurfaceRemoved), &[retire], &mut out).is_ok());
    for kind in [K::CandidateReduced, K::AttentionChanged] {
        assert!(encode_response(7, 2, &request(kind), &[R::default()], &mut out).is_ok());
    }
    out.fill(0xa5);
    assert!(
        encode_response(
            0,
            2,
            &request(K::CandidateReduced),
            &[descriptor()],
            &mut out
        )
        .is_err()
    );
    assert!(
        encode_response(
            7,
            0,
            &request(K::CandidateReduced),
            &[descriptor()],
            &mut out
        )
        .is_err()
    );
    assert!(
        encode_response(
            7,
            2,
            &request(K::CandidateReduced),
            &[descriptor()],
            &mut out[..447]
        )
        .is_err()
    );
    assert_eq!(out, [0xa5; 448]);
}
