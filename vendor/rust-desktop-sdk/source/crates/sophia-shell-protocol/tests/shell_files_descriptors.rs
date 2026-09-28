//! File/domain identities and caps are checked separately from value layouts.
use sophia_shell_protocol::{shell_files::*, *};

struct Case {
    raw: u16,
    class: ShellFileClass,
    cap: usize,
    record: ShellDescriptorRecord,
}
fn cases() -> Vec<Case> {
    use ShellDescriptorRecord as R;
    use ShellFileClass::{Candidate as C, Event as E, Object as O};
    let activation = ShellLauncherActivation {
        connection_epoch: 11,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        presentation_epoch: 5,
        activation: 6,
        slot: 7,
    };
    vec![
        Case {
            raw: 5,
            class: O,
            cap: 4096,
            record: R::Descriptors(ShellV1DescriptorSnapshot {
                connection_epoch: 11,
                snapshot_generation: 2,
                output: OutputId::from_raw(3),
                output_generation: 4,
                broker_epoch: 5,
                broker_revocation_epoch: 6,
                descriptors: vec![],
            }),
        },
        Case {
            raw: 6,
            class: O,
            cap: 1_048_576,
            record: R::Tabs(ShellTabSnapshot {
                connection_epoch: 11,
                generation: 2,
                groups: vec![],
            }),
        },
        Case {
            raw: 7,
            class: O,
            cap: 131_072,
            record: R::Shortcuts(ShellShortcutCatalog {
                connection_epoch: 11,
                generation: 2,
                entries: vec![],
            }),
        },
        Case {
            raw: 46,
            class: E,
            cap: 68,
            record: R::DescriptorOutcome(ShellV1CandidateOutcome {
                connection_epoch: 11,
                candidate_generation: 2,
                presentation_epoch: 3,
                kind: ShellV1CandidateOutcomeKind::Presented,
            }),
        },
        Case {
            raw: 47,
            class: E,
            cap: 116,
            record: R::DescriptorActivation(ShellV1Activation {
                connection_epoch: 11,
                candidate_generation: 2,
                presentation_epoch: 3,
                activation: 4,
                action: ToplevelActionCapabilityRef {
                    token: 5,
                    issuer_epoch: 6,
                    issuer_revocation_epoch: 7,
                    recipient_epoch: 11,
                    target_slot: 8,
                    target_generation: 9,
                },
            }),
        },
        Case {
            raw: 48,
            class: E,
            cap: 92,
            record: R::ReferenceRequest(ShellReferenceRequest {
                connection_epoch: 11,
                catalog_generation: 2,
                request_generation: 3,
                output: OutputId::from_raw(4),
                output_generation: 5,
                presentation_epoch: 0,
                operation: ShellReferenceOperation::Startup,
            }),
        },
        Case {
            raw: 49,
            class: E,
            cap: 88,
            record: R::ReferenceOutcome(ShellReferenceOutcome {
                connection_epoch: 11,
                catalog_generation: 2,
                request_generation: 3,
                candidate_generation: 4,
                presentation_epoch: 5,
                page: 0,
                pages: 1,
                kind: ShellV1CandidateOutcomeKind::Presented,
            }),
        },
        Case {
            raw: 50,
            class: E,
            cap: 352,
            record: R::LauncherRequest(ShellLauncherRequest {
                connection_epoch: 11,
                catalog_generation: 2,
                request_generation: 3,
                output: OutputId::from_raw(4),
                output_generation: 5,
                presentation_epoch: 0,
                operation: ShellLauncherOperation::Open,
                query: String::new(),
            }),
        },
        Case {
            raw: 51,
            class: E,
            cap: 76,
            record: R::LauncherOutcome(ShellLauncherOutcome {
                connection_epoch: 11,
                request_generation: 2,
                candidate_generation: 3,
                presentation_epoch: 4,
                kind: ShellV1CandidateOutcomeKind::Presented,
            }),
        },
        Case {
            raw: 52,
            class: E,
            cap: 92,
            record: R::LauncherActivation(activation),
        },
        Case {
            raw: 53,
            class: E,
            cap: 92,
            record: R::LaunchOutcome(ShellLaunchOutcome {
                activation,
                status: ShellLaunchStatus::Started,
            }),
        },
        Case {
            raw: 273,
            class: C,
            cap: 276,
            record: R::DescriptorCandidate(ShellV1Candidate {
                connection_epoch: 11,
                snapshot_generation: 2,
                candidate_generation: 3,
                output: OutputId::from_raw(4),
                visible: false,
                selected_slot: None,
                reservation: None,
                entries: vec![],
            }),
        },
        Case {
            raw: 274,
            class: C,
            cap: 60,
            record: R::DescriptorActivationAck(ShellV1ActivationAck {
                connection_epoch: 11,
                activation: 2,
                disposition: ShellV1ActivationDisposition::Consumed,
            }),
        },
        Case {
            raw: 275,
            class: C,
            cap: 8260,
            record: R::TabsCandidate(ShellTabCandidate {
                connection_epoch: 11,
                snapshot_generation: 2,
                candidate_generation: 3,
                groups: vec![4],
            }),
        },
        Case {
            raw: 276,
            class: C,
            cap: 52488,
            record: R::ReferenceCandidate(ShellReferenceCandidate {
                connection_epoch: 11,
                catalog_generation: 2,
                request_generation: 3,
                candidate_generation: 4,
                output: OutputId::from_raw(5),
                visible: false,
                page: 0,
                style: ShellReferenceStyle {
                    body_size: 12,
                    title_size: 16,
                    padding: 8,
                    row_gap: 4,
                    key_gap: 5,
                    column_gap: 6,
                    border: 2,
                    margin: 7,
                    columns: 2,
                    colors: [0xff223344; 6],
                    title: "Title".into(),
                },
                entries: vec![],
            }),
        },
        Case {
            raw: 277,
            class: C,
            cap: 168,
            record: R::LauncherCandidate(ShellLauncherCandidate {
                connection_epoch: 11,
                catalog_generation: 2,
                request_generation: 3,
                candidate_generation: 4,
                output: OutputId::from_raw(5),
                visible: true,
                selected: 0,
                entries: vec![],
                font_size: 14,
                colors: [0xff223344; 4],
            }),
        },
        Case {
            raw: 278,
            class: C,
            cap: 92,
            record: R::LauncherActivationAck(ShellLauncherActivationAck {
                activation,
                consumed: false,
            }),
        },
    ]
}
fn header(case: &Case) -> ShellFileHeader {
    ShellFileHeader {
        kind: shell_file_descriptor_kind(&case.record),
        connection_epoch: 11,
        submission_id: if case.class == ShellFileClass::Candidate {
            19
        } else {
            0
        },
        sequence: if case.class == ShellFileClass::Event {
            23
        } else {
            0
        },
    }
}
fn set64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn all_seventeen_native_kinds_have_distinct_file_and_domain_identity() {
    for case in cases() {
        let h = header(&case);
        let value = ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(17),
            record: case.record,
        };
        assert_eq!(h.kind as u16, case.raw);
        assert_eq!(shell_file_class(h.kind), case.class);
        assert_eq!(shell_file_descriptor_max_bytes(h.kind), Some(case.cap));
        let (kind, body) = encode_shell_file_descriptor_body(&value).unwrap();
        assert_eq!(kind, h.kind);
        assert_eq!(&body[..8], &17u64.to_le_bytes());
        assert_eq!(&body[8..16], &11u64.to_le_bytes());
        let bytes = encode_shell_file_descriptor(h, &value).unwrap();
        let mut literal = vec![0; 32];
        literal[..4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        literal[4..6].copy_from_slice(&1u16.to_le_bytes());
        literal[6..8].copy_from_slice(&case.raw.to_le_bytes());
        set64(&mut literal, 8, 11);
        set64(&mut literal, 16, h.submission_id);
        set64(&mut literal, 24, h.sequence);
        literal.extend(body);
        assert_eq!(bytes, literal);
        assert_eq!(
            decode_shell_file_descriptor(&literal, h.kind).unwrap(),
            value
        );
        for len in 0..literal.len() {
            assert!(
                decode_shell_file_descriptor(&literal[..len], h.kind).is_err(),
                "kind {} length {len}",
                case.raw
            );
        }
        let mut extra = literal.clone();
        extra.push(0);
        let len = extra.len() as u32;
        extra[..4].copy_from_slice(&len.to_le_bytes());
        assert!(decode_shell_file_descriptor(&extra, h.kind).is_err());
        for (at, n) in [(8, 12), (32, 0), (40, 12)] {
            let mut wrong = literal.clone();
            set64(&mut wrong, at, n);
            assert!(
                decode_shell_file_descriptor(&wrong, h.kind).is_err(),
                "kind {} offset {at}",
                case.raw
            );
        }
        for (at, n) in [
            (16, if h.submission_id == 0 { 19 } else { 0 }),
            (24, if h.sequence == 0 { 23 } else { 0 }),
        ] {
            let mut wrong = literal.clone();
            set64(&mut wrong, at, n);
            assert!(decode_shell_file_descriptor(&wrong, h.kind).is_err());
        }
        let mut wrong = h;
        wrong.connection_epoch = 12;
        assert!(encode_shell_file_descriptor(wrong, &value).is_err());
        wrong = h;
        wrong.kind = ShellFileKind::Limits;
        assert!(encode_shell_file_descriptor(wrong, &value).is_err());
        assert!(decode_shell_file_descriptor(&literal, ShellFileKind::Limits).is_err());
        let zero = ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(0),
            record: value.record,
        };
        assert!(encode_shell_file_descriptor_body(&zero).is_err());
    }
}

#[test]
fn new_objects_and_candidates_have_publication_and_custody_receipts() {
    for case in cases() {
        let kind = shell_file_descriptor_kind(&case.record);
        if case.class == ShellFileClass::Object {
            let value = ShellFileObjectPublished {
                object: kind,
                generation: 31,
                qid: 37,
            };
            let h = ShellFileHeader {
                kind: ShellFileKind::ObjectPublished,
                connection_epoch: 11,
                submission_id: 0,
                sequence: 23,
            };
            let body = encode_shell_file_object_published_body(value).unwrap();
            let bytes = encode_shell_file_record(h, &body).unwrap();
            assert_eq!(decode_shell_file_object_published(&bytes).unwrap(), value);
        }
        if case.class == ShellFileClass::Candidate {
            let value = ShellFileSubmitted {
                submission_id: 19,
                candidate_kind: kind,
            };
            let h = ShellFileHeader {
                kind: ShellFileKind::Submitted,
                connection_epoch: 11,
                submission_id: 0,
                sequence: 23,
            };
            let bytes = encode_shell_file_submitted(h, value).unwrap();
            assert_eq!(decode_shell_file_submitted(&bytes).unwrap(), value);
        } else {
            assert!(
                encode_shell_file_submitted_body(ShellFileSubmitted {
                    submission_id: 19,
                    candidate_kind: kind
                })
                .is_err()
            );
        }
    }
}

#[test]
fn large_tab_candidate_uses_its_own_bound_not_the_content_candidate_cap() {
    let record = ShellDescriptorRecord::TabsCandidate(ShellTabCandidate {
        connection_epoch: 11,
        snapshot_generation: 2,
        candidate_generation: 3,
        groups: (1..=1024).collect(),
    });
    let value = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(17),
        record,
    };
    let h = ShellFileHeader {
        kind: ShellFileKind::TabsCandidate,
        connection_epoch: 11,
        submission_id: 19,
        sequence: 0,
    };
    let bytes = encode_shell_file_descriptor(h, &value).unwrap();
    assert_eq!(bytes.len(), 8260);
    assert!(bytes.len() > SHELL_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(decode_shell_file_descriptor(&bytes, h.kind).unwrap(), value);
    let mut extra = bytes;
    extra.push(0);
    let size = extra.len() as u32;
    extra[..4].copy_from_slice(&size.to_le_bytes());
    assert_eq!(
        decode_shell_file_descriptor(&extra, h.kind),
        Err(ShellFilePayloadError::Envelope(ShellFileCodecError::Length))
    );
    assert_eq!(SHELL_FILE_MAX_CANDIDATE_BYTES, 8192);
    assert_eq!(SHELL_FILE_MAX_TRANSACTION_BYTES, 65536);
}

#[test]
fn proposal_kind_caps_and_fixed_prefixes_match_the_native_encoders() {
    let proposal: kdl::KdlDocument = include_str!("../../../spec/descriptor-files-proposal.kdl")
        .parse()
        .unwrap();
    let declarations: Vec<_> = proposal
        .nodes()
        .iter()
        .filter(|n| matches!(n.name().value(), "object" | "event" | "candidate"))
        .collect();
    assert_eq!(declarations.len(), 17);
    for case in cases() {
        let decl = declarations
            .iter()
            .find(|n| {
                n.get("kind").and_then(kdl::KdlValue::as_integer) == Some(i128::from(case.raw))
            })
            .unwrap();
        assert_eq!(
            decl.get("max-bytes").and_then(kdl::KdlValue::as_integer),
            Some(case.cap as i128)
        );
        assert_eq!(
            decl.name().value(),
            match case.class {
                ShellFileClass::Object => "object",
                ShellFileClass::Event => "event",
                ShellFileClass::Candidate => "candidate",
            }
        );
        let name = decl.get(0).and_then(kdl::KdlValue::as_string).unwrap();
        let block = proposal
            .nodes()
            .iter()
            .find(|n| {
                matches!(n.name().value(), "body" | "body-prefix")
                    && n.get(0).and_then(kdl::KdlValue::as_string) == Some(name)
            })
            .unwrap();
        let size = block
            .get("size")
            .and_then(kdl::KdlValue::as_integer)
            .unwrap() as usize;
        let value = ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(17),
            record: case.record,
        };
        let (_, body) = encode_shell_file_descriptor_body(&value).unwrap();
        // This fixture has empty variable tables except one tab group ID.
        assert_eq!(
            body.len(),
            size + if case.raw == 275 { 8 } else { 0 },
            "{name}"
        );
    }
}
