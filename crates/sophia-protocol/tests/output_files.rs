use sophia_protocol::output_files::*;
use sophia_protocol::*;

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .flat_map(|word| {
            assert_eq!(word.len() % 2, 0);
            (0..word.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&word[offset..offset + 2], 16).unwrap())
        })
        .collect()
}

fn proposal_bytes() -> Vec<u8> {
    // Literal little-endian rows, independent of the encoder. Negative x is
    // intentionally syntactically valid; the topology owner must reject it.
    hex("
        0b00000000000000 0700000000000000 0200 0000 0100 0100
        0300000000000000 0400000000000000 0500000000000000 0100 0200 00000000
        0000000000000000 feffffff 00000000 20030000 58020000 0100 0000
        0300000000000000 0300 0000
        000000000000000000000000 000000000000000000000000 000000000000000000000000
    ")
}

fn proposal() -> OutputV1Proposal {
    OutputV1Proposal {
        connection_epoch: 9,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 7,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(3),
                head_generation: 4,
                mode: DisplayModeId::from_raw(5),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Automatic,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::INVALID,
                logical: Rect {
                    x: -2,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

#[test]
fn literal_negotiation_and_envelope_preserve_all_requested_bits() {
    let bytes = hex(
        "30000000 0100 0001 0900000000000000 0500000000000000 0000000000000000
                     0100 0100 00000000 0300000000000080",
    );
    let record = decode_output_file_record(&bytes, OutputFileClass::Candidate).unwrap();
    assert_eq!(
        record.header,
        OutputFileHeader {
            kind: OutputFileKind::Negotiate,
            connection_epoch: 9,
            submission_id: 5,
            sequence: 0,
        }
    );
    let hello = decode_output_file_negotiate(record.body).unwrap();
    assert_eq!(hello.capabilities, 3 | (1 << 63));
    assert_eq!(hello.minimum_revision, 1);
    assert_eq!(hello.maximum_revision, 1);
    assert_eq!(
        encode_output_file_record(record.header, &encode_output_file_negotiate(hello)).unwrap(),
        bytes
    );
    // These inputs need an owner negotiation refusal, not a codec error.
    for (minimum_revision, maximum_revision, capabilities) in [(0, 0, 0), (2, 1, 2), (2, 3, 1)] {
        let hello = OutputV1ClientHello {
            minimum_revision,
            maximum_revision,
            capabilities,
        };
        assert_eq!(
            decode_output_file_negotiate(&encode_output_file_negotiate(hello)).unwrap(),
            hello
        );
    }
}

#[test]
fn envelope_fences_classes_identities_lengths_version_and_kind() {
    let fixtures = [
        (OutputFileKind::Limits, OutputFileClass::Object, 0, 0),
        (OutputFileKind::Proposal, OutputFileClass::Candidate, 4, 0),
        (OutputFileKind::Outcome, OutputFileClass::Event, 0, 6),
    ];
    for (kind, class, submission_id, sequence) in fixtures {
        let header = OutputFileHeader {
            kind,
            connection_epoch: 9,
            submission_id,
            sequence,
        };
        let valid = encode_output_file_record(header, &[0; 24]).unwrap();
        for other in [
            OutputFileClass::Object,
            OutputFileClass::Candidate,
            OutputFileClass::Event,
        ] {
            assert_eq!(
                decode_output_file_record(&valid, other).is_ok(),
                class == other
            );
        }
        for end in 0..valid.len() {
            assert!(decode_output_file_record(&valid[..end], class).is_err());
        }
        for (offset, replacement) in [(0, 0), (4, 2), (6, 255), (8, 0)] {
            let mut bad = valid.clone();
            bad[offset] = replacement;
            assert!(
                decode_output_file_record(&bad, class).is_err(),
                "offset {offset}"
            );
        }
        for (submission_id, sequence) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let test = OutputFileHeader {
                submission_id,
                sequence,
                ..header
            };
            let expected = match class {
                OutputFileClass::Object => submission_id == 0 && sequence == 0,
                OutputFileClass::Candidate => submission_id != 0 && sequence == 0,
                OutputFileClass::Event => submission_id == 0 && sequence != 0,
            };
            assert_eq!(encode_output_file_record(test, &[0; 24]).is_ok(), expected);
            let mut bytes = valid.clone();
            bytes[16..24].copy_from_slice(&submission_id.to_le_bytes());
            bytes[24..32].copy_from_slice(&sequence.to_le_bytes());
            assert_eq!(decode_output_file_record(&bytes, class).is_ok(), expected);
        }
        let max = if class == OutputFileClass::Candidate {
            OUTPUT_FILE_MAX_CANDIDATE_BYTES
        } else {
            OUTPUT_FILE_MAX_BYTES
        };
        let bytes = encode_output_file_record(header, &vec![0; max - 32]).unwrap();
        assert!(decode_output_file_record(&bytes, class).is_ok());
        assert!(encode_output_file_record(header, &vec![0; max - 31]).is_err());
        let mut bytes = bytes;
        bytes.push(0);
        bytes[..4].copy_from_slice(&((max + 1) as u32).to_le_bytes());
        assert!(decode_output_file_record(&bytes, class).is_err());
    }
}

#[test]
fn literal_proposal_preserves_transaction_and_semantic_values() {
    let bytes = proposal_bytes();
    assert_eq!(bytes.len(), 132);
    let tx = TransactionId::from_raw(11);
    assert_eq!(
        decode_output_file_proposal(&bytes, 9).unwrap(),
        (tx, proposal())
    );
    assert_eq!(encode_output_file_proposal(tx, &proposal()).unwrap(), bytes);
    // Zero head identities and invalid primary/geometry get precise semantic
    // owner errors. They are not unknown wire enums or malformed row arrays.
    let mut semantic = proposal();
    semantic.candidate.primary_group_index = u16::MAX;
    semantic.candidate.heads[0].head = DisplayHeadId::INVALID;
    semantic.candidate.heads[0].head_generation = 0;
    semantic.candidate.heads[0].mode = DisplayModeId::INVALID;
    semantic.candidate.groups[0].logical.width = -1;
    let bytes = encode_output_file_proposal(tx, &semantic).unwrap();
    assert_eq!(decode_output_file_proposal(&bytes, 9).unwrap().1, semantic);
}

#[test]
fn proposal_rejects_truncation_counts_unknown_enums_and_every_padding_byte() {
    let bytes = proposal_bytes();
    for end in 0..bytes.len() {
        assert!(
            decode_output_file_proposal(&bytes[..end], 9).is_err(),
            "end {end}"
        );
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode_output_file_proposal(&extra, 9).is_err());
    assert!(decode_output_file_proposal(&bytes, 0).is_err());
    for offset in [0, 8, 16, 20, 22, 48, 50, 80, 92] {
        let mut bad = bytes.clone();
        bad[offset] = 0;
        assert!(
            decode_output_file_proposal(&bad, 9).is_err(),
            "zero {offset}"
        );
        let mut bad = bytes.clone();
        if [16, 20, 22, 48, 50, 80, 92].contains(&offset) {
            bad[offset] = 255;
            assert!(
                decode_output_file_proposal(&bad, 9).is_err(),
                "excess {offset}"
            );
        }
    }
    for offset in (52..56).chain(82..84).chain(94..132) {
        let mut bad = bytes.clone();
        bad[offset] = 1;
        assert!(
            decode_output_file_proposal(&bad, 9).is_err(),
            "padding {offset}"
        );
    }
}

#[test]
fn proposal_bounds_cover_the_largest_candidate_and_one_past_each_count() {
    let mut value = proposal();
    value.candidate.heads.resize(16, value.candidate.heads[0]);
    let group = &mut value.candidate.groups[0];
    group.members.resize(4, group.members[0]);
    let group = group.clone();
    value.candidate.groups.resize(16, group);
    let tx = TransactionId::from_raw(11);
    let body = encode_output_file_proposal(tx, &value).unwrap();
    assert_eq!(
        body.len() + OUTPUT_FILE_HEADER_BYTES,
        OUTPUT_FILE_MAX_CANDIDATE_BYTES
    );
    assert_eq!(
        decode_output_file_proposal(&body, 9).unwrap(),
        (tx, value.clone())
    );
    for which in 0..3 {
        let mut invalid = value.clone();
        match which {
            0 => invalid.candidate.heads.push(value.candidate.heads[0]),
            1 => invalid
                .candidate
                .groups
                .push(value.candidate.groups[0].clone()),
            _ => invalid.candidate.groups[0]
                .members
                .push(value.candidate.groups[0].members[0]),
        }
        assert!(encode_output_file_proposal(tx, &invalid).is_err());
    }
    for transform in [
        OutputTransform::Normal,
        OutputTransform::Rotate90,
        OutputTransform::Rotate180,
        OutputTransform::Rotate270,
        OutputTransform::Flipped,
        OutputTransform::Flipped90,
        OutputTransform::Flipped180,
        OutputTransform::Flipped270,
    ] {
        for vrr in [
            OutputVrrPolicy::Disabled,
            OutputVrrPolicy::Automatic,
            OutputVrrPolicy::Always,
        ] {
            for mapping in [
                OutputHeadMapping::Fit,
                OutputHeadMapping::Cover,
                OutputHeadMapping::Exact,
            ] {
                let mut value = proposal();
                value.candidate.heads[0].transform = transform;
                value.candidate.heads[0].vrr = vrr;
                value.candidate.groups[0].members[0].mapping = mapping;
                let bytes = encode_output_file_proposal(tx, &value).unwrap();
                assert_eq!(decode_output_file_proposal(&bytes, 9).unwrap().1, value);
            }
        }
    }
}

#[test]
fn literal_outcome_keeps_open_reason_codes_and_all_six_outcomes() {
    let literal = hex("0b00000000000000 0700000000000000 0500 ffff 00000000");
    let (tx, outcome) = decode_output_file_outcome(&literal, 9).unwrap();
    assert_eq!(tx.raw(), 11);
    assert_eq!(
        outcome,
        OutputV1Outcome {
            connection_epoch: 9,
            topology_epoch: 7,
            kind: OutputV1OutcomeKind::RolledBack,
            reason: u16::MAX
        }
    );
    assert_eq!(encode_output_file_outcome(tx, outcome).unwrap(), literal);
    for (code, kind) in (1..=6).zip([
        OutputV1OutcomeKind::Validated,
        OutputV1OutcomeKind::Committed,
        OutputV1OutcomeKind::Stale,
        OutputV1OutcomeKind::Rejected,
        OutputV1OutcomeKind::RolledBack,
        OutputV1OutcomeKind::Failed,
    ]) {
        let mut bytes = literal.clone();
        bytes[16] = code;
        assert_eq!(decode_output_file_outcome(&bytes, 9).unwrap().1.kind, kind);
    }
    for offset in [0, 8, 16] {
        let mut bad = literal.clone();
        bad[offset] = 0;
        assert!(decode_output_file_outcome(&bad, 9).is_err());
    }
    for offset in 20..24 {
        let mut bad = literal.clone();
        bad[offset] = 1;
        assert!(decode_output_file_outcome(&bad, 9).is_err());
    }
    for end in 0..literal.len() {
        assert!(decode_output_file_outcome(&literal[..end], 9).is_err());
    }
}

#[test]
fn literal_submit_ack_and_submitted_are_exact_and_bounded() {
    let bytes = hex("0900000000000000 0500000000000000 f8060000 00000000");
    let submit = decode_output_file_submit(&bytes).unwrap();
    assert_eq!(
        submit,
        OutputFileSubmit {
            connection_epoch: 9,
            submission_id: 5,
            candidate_bytes: 1784
        }
    );
    assert_eq!(encode_output_file_submit(submit).unwrap().as_slice(), bytes);
    for candidate_bytes in [0, 47, 1785, u32::MAX] {
        assert!(
            encode_output_file_submit(OutputFileSubmit {
                candidate_bytes,
                ..submit
            })
            .is_err()
        );
        let mut bad = bytes.clone();
        bad[16..20].copy_from_slice(&candidate_bytes.to_le_bytes());
        assert!(decode_output_file_submit(&bad).is_err());
    }
    let bytes = hex("0900000000000000 0600000000000000");
    let ack = decode_output_file_ack(&bytes).unwrap();
    assert_eq!(
        ack,
        OutputFileAck {
            connection_epoch: 9,
            sequence: 6
        }
    );
    assert_eq!(encode_output_file_ack(ack).unwrap().as_slice(), bytes);
    let bytes = hex("0500000000000000 0101 000000000000");
    let submitted = decode_output_file_submitted(&bytes).unwrap();
    assert_eq!(
        submitted,
        OutputFileSubmitted {
            submission_id: 5,
            candidate_kind: OutputFileKind::Proposal
        }
    );
    assert_eq!(
        encode_output_file_submitted(submitted).unwrap().as_slice(),
        bytes
    );
    for candidate_kind in [
        OutputFileKind::Limits,
        OutputFileKind::Topology,
        OutputFileKind::Outcome,
    ] {
        assert!(
            encode_output_file_submitted(OutputFileSubmitted {
                candidate_kind,
                ..submitted
            })
            .is_err()
        );
        let mut bad = bytes.clone();
        bad[8..10].copy_from_slice(&(candidate_kind as u16).to_le_bytes());
        assert!(decode_output_file_submitted(&bad).is_err());
    }
}

#[test]
fn all_fixed_controls_reject_truncation_trailing_bytes_and_reserved_bytes() {
    type Decode = fn(&[u8]) -> bool;
    let controls: Vec<(Vec<u8>, Decode, Vec<usize>)> = vec![
        (
            hex("01000100 00000000 0300000000000000"),
            |bytes| decode_output_file_negotiate(bytes).is_ok(),
            (4..8).collect(),
        ),
        (
            hex("0900000000000000 0500000000000000 30000000 00000000"),
            |bytes| decode_output_file_submit(bytes).is_ok(),
            (20..24).collect(),
        ),
        (
            hex("0900000000000000 0600000000000000"),
            |bytes| decode_output_file_ack(bytes).is_ok(),
            vec![],
        ),
        (
            hex("0500000000000000 0001 000000000000"),
            |bytes| decode_output_file_submitted(bytes).is_ok(),
            (10..16).collect(),
        ),
    ];
    for (bytes, decode, reserved) in controls {
        assert!(decode(&bytes));
        for end in 0..bytes.len() {
            assert!(!decode(&bytes[..end]));
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(!decode(&extra));
        for offset in reserved {
            let mut bad = bytes.clone();
            bad[offset] = 1;
            assert!(!decode(&bad));
        }
    }
}
