#[allow(dead_code)]
#[path = "support/policy_record_fixture.rs"]
mod arrays;
#[path = "../examples/support/wm_file_inspection.rs"]
mod inspection;
#[path = "support/policy_scalar_fixture.rs"]
mod scalars;

use inspection::{Mode, Payload, inspect};
use sophia_protocol::wm_files::*;
use sophia_protocol::*;

const EPOCH: u64 = 3;
const CAPS: u64 = u64::MAX;

fn header(kind: WmFileKind, sequence: u64) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: EPOCH,
        submission_id: 0,
        sequence,
    }
}

fn snapshot() -> WmFileSnapshot {
    WmFileSnapshot {
        transaction: TransactionId::from_raw(9),
        snapshot: PolicyDecodedSnapshot {
            scene: arrays::scene(),
            actions: arrays::actions(),
            classifications: arrays::classifications(),
            launch_origins: arrays::origins()
                .into_iter()
                .map(|origin| PolicyLaunchContext {
                    epoch: EPOCH,
                    ..origin
                })
                .collect(),
        },
    }
}

fn submitted(sequence: u64) -> Vec<u8> {
    encode_wm_file_submitted(
        header(WmFileKind::Submitted, sequence),
        WmFileSubmitted {
            submission_id: 105,
            candidate_kind: WmFileKind::Projection,
        },
    )
    .unwrap()
}

fn event_corpus() -> Vec<(Vec<u8>, Payload)> {
    let mut values = vec![
        (
            encode_wm_file_negotiated(header(WmFileKind::Negotiated, 41), CAPS).unwrap(),
            Payload::Negotiated(CAPS),
        ),
        (
            submitted(42),
            Payload::Submitted(WmFileSubmitted {
                submission_id: 105,
                candidate_kind: WmFileKind::Projection,
            }),
        ),
    ];
    for (index, kind) in [
        WmFileKind::ProfilePrepare,
        WmFileKind::ProfileActivate,
        WmFileKind::ProfileRollback,
    ]
    .into_iter()
    .enumerate()
    {
        let command = PolicyProfileCommand {
            transaction: TransactionId::from_raw(12 + index as u64),
            identity: PolicyProfileIdentity::new(EPOCH, 7, [3; 32]).unwrap(),
        };
        values.push((
            encode_wm_file_profile_command(header(kind, 43 + index as u64), command, CAPS).unwrap(),
            Payload::Profile(command),
        ));
    }
    let configuration = WmFileConfigurationOutcome {
        transaction: TransactionId::from_raw(18),
        generation: 7,
        outcome: PolicyProjectionOutcome::Committed,
    };
    values.push((
        encode_wm_file_configuration_outcome(
            header(WmFileKind::ConfigurationOutcome, 46),
            &configuration,
            CAPS,
        )
        .unwrap(),
        Payload::ConfigurationOutcome(configuration),
    ));
    let cycle = WmFileCycle {
        snapshot_transaction: TransactionId::from_raw(19),
        request_transaction: TransactionId::from_raw(20),
        request: scalars::request(PolicyRequestCause::SceneChanged),
    };
    values.push((
        encode_wm_file_cycle(header(WmFileKind::Cycle, 47), &cycle, CAPS).unwrap(),
        Payload::Cycle(cycle),
    ));
    let projection = WmFileProjectionOutcome {
        transaction: TransactionId::from_raw(21),
        request_id: 17,
        scene_generation: 11,
        outcome: PolicyProjectionOutcome::Committed,
        expect_session_operation: true,
    };
    values.push((
        encode_wm_file_projection_outcome(
            header(WmFileKind::ProjectionOutcome, 48),
            &projection,
            CAPS,
        )
        .unwrap(),
        Payload::ProjectionOutcome(projection),
    ));
    let operation = WmFileSessionOperationOutcome {
        transaction: TransactionId::from_raw(22),
        outcome: PolicySessionOperationOutcome {
            connection_epoch: EPOCH,
            request_id: 17,
            outcome: PolicyProjectionOutcome::Committed,
        },
    };
    values.push((
        encode_wm_file_session_operation_outcome(
            header(WmFileKind::SessionOperationOutcome, 49),
            &operation,
            CAPS,
        )
        .unwrap(),
        Payload::SessionOperationOutcome(operation),
    ));
    let receipt = WmFilePresentationReceipt {
        transaction: TransactionId::from_raw(23),
        receipt: scalars::receipt(scalars::PRESENTATION_OUTCOMES[0]),
    };
    values.push((
        encode_wm_file_presentation_receipt(
            header(WmFileKind::PresentationReceipt, 50),
            &receipt,
            CAPS,
        )
        .unwrap(),
        Payload::PresentationReceipt(receipt),
    ));
    values
}

#[test]
fn snapshot_inspection_uses_complete_typed_row_validation() {
    let expected = snapshot();
    let bytes = encode_wm_file_snapshot(header(WmFileKind::Snapshot, 0), &expected, CAPS).unwrap();
    let capture = inspect(&bytes, Mode::Snapshot, EPOCH, CAPS).unwrap();
    assert_eq!(
        capture.records[0].payload,
        Payload::Snapshot(Box::new(expected))
    );
    assert!(capture.text().contains("transaction=9"));
    assert!(inspect(&bytes, Mode::Snapshot, EPOCH + 1, CAPS).is_err());
    assert!(inspect(&bytes, Mode::Events, EPOCH, CAPS).is_err());
    assert!(inspect(&bytes, Mode::Snapshot, EPOCH, 0).is_err());
    let mut bad = bytes.clone();
    bad[48..56].copy_from_slice(&99u64.to_le_bytes()); // active output absent from rows
    assert!(inspect(&bad, Mode::Snapshot, EPOCH, CAPS).is_err());
    let mut bad = bytes.clone();
    bad[58] = 1; // reserved prefix
    assert!(inspect(&bad, Mode::Snapshot, EPOCH, CAPS).is_err());
    for length in 0..bytes.len() {
        assert!(inspect(&bytes[..length], Mode::Snapshot, EPOCH, CAPS).is_err());
    }
    let mut bad = bytes;
    bad.push(0);
    assert!(inspect(&bad, Mode::Snapshot, EPOCH, CAPS).is_err());
}

#[test]
fn every_event_kind_retains_typed_domains_and_custody_is_not_an_outcome() {
    let corpus = event_corpus();
    let bytes: Vec<_> = corpus
        .iter()
        .flat_map(|(bytes, _)| bytes.iter().copied())
        .collect();
    let capture = inspect(&bytes, Mode::Events, EPOCH, CAPS).unwrap();
    assert_eq!(capture.records.len(), corpus.len());
    let mut offset = 0;
    for (record, (bytes, expected)) in capture.records.iter().zip(corpus) {
        assert_eq!(record.offset, offset);
        assert_eq!(record.payload, expected);
        offset += bytes.len();
    }
    let text = capture.text();
    assert!(text.contains("custody=accepted semantic_outcome=not_implied"));
    assert!(text.contains("reported_semantic_outcome="));
    assert!(text.contains("physical_completion_observed_by_tool=false"));
    assert!(text.contains("completeness=not_inferred phase=not_inferred"));
    // Unknown mask bits are not newly rejected by this tool or negotiated by it.
    assert_eq!(capture.capabilities, u64::MAX);
    for (bytes, _) in event_corpus() {
        let kind = decode_wm_file_record(&bytes, WmFileClass::Event)
            .unwrap()
            .header
            .kind;
        assert!(inspect(&bytes, Mode::Events, EPOCH + 1, CAPS).is_err());
        if !matches!(
            kind,
            WmFileKind::Negotiated | WmFileKind::Submitted | WmFileKind::Cycle
        ) {
            assert!(inspect(&bytes, Mode::Events, EPOCH, 0).is_err(), "{kind:?}");
        }
        let mut bad = bytes;
        bad.push(0);
        let length = u32::try_from(bad.len()).unwrap();
        bad[..4].copy_from_slice(&length.to_le_bytes());
        assert!(
            inspect(&bad, Mode::Events, EPOCH, CAPS).is_err(),
            "{kind:?}"
        );
    }
}

#[test]
fn supplied_context_does_not_replay_negotiation_or_invent_a_phase() {
    // Captured bytes report no selected features. The tool validates later
    // bodies against the explicit supplied context, not a second admission
    // state machine reconstructed from those bytes.
    let mut bytes = encode_wm_file_negotiated(header(WmFileKind::Negotiated, 72), 0).unwrap();
    let command = PolicyProfileCommand {
        transaction: TransactionId::from_raw(29),
        identity: PolicyProfileIdentity::new(EPOCH, 4, [9; 32]).unwrap(),
    };
    bytes.extend(
        encode_wm_file_profile_command(header(WmFileKind::ProfileRollback, 73), command, CAPS)
            .unwrap(),
    );
    let capture = inspect(&bytes, Mode::Events, EPOCH, CAPS).unwrap();
    assert_eq!(capture.records[0].payload, Payload::Negotiated(0));
    assert_eq!(capture.records[1].payload, Payload::Profile(command));
    assert!(inspect(&bytes, Mode::Events, EPOCH, 0).is_err());
    let custody_only = inspect(&submitted(51), Mode::Events, EPOCH, 0).unwrap();
    assert!(matches!(
        custody_only.records[0].payload,
        Payload::Submitted(_)
    ));
    assert!(!custody_only.text().contains("reported_semantic_outcome="));
}

#[test]
fn captured_window_counts_sequences_and_overflow_are_bounded() {
    assert!(
        inspect(&[], Mode::Events, EPOCH, 0)
            .unwrap()
            .records
            .is_empty()
    );
    assert!(inspect(&[], Mode::Snapshot, EPOCH, 0).is_err());
    let mut bytes: Vec<_> = (500..564).flat_map(submitted).collect();
    assert_eq!(
        inspect(&bytes, Mode::Events, EPOCH, 0)
            .unwrap()
            .records
            .len(),
        64
    );
    bytes.extend(submitted(564));
    assert!(inspect(&bytes, Mode::Events, EPOCH, 0).is_err());
    for next in [1, 499, 500, 502] {
        let mut bytes = submitted(500);
        bytes.extend(submitted(next));
        assert!(inspect(&bytes, Mode::Events, EPOCH, 0).is_err());
    }
    assert!(inspect(&submitted(u64::MAX), Mode::Events, EPOCH, 0).is_ok());
    let mut bytes = submitted(u64::MAX);
    bytes.extend(submitted(1));
    assert!(inspect(&bytes, Mode::Events, EPOCH, 0).is_err());
}

#[test]
fn malformed_suffix_length_class_version_and_body_never_return_partial_capture() {
    let good = submitted(8);
    for length in 1..good.len() {
        assert!(inspect(&good[..length], Mode::Events, EPOCH, CAPS).is_err());
    }
    for length in [0, 1, 31, 33, u32::MAX] {
        let mut bad = good.clone();
        bad[..4].copy_from_slice(&length.to_le_bytes());
        assert!(inspect(&bad, Mode::Events, EPOCH, CAPS).is_err());
    }
    for (at, data) in [
        (4, 2u16.to_le_bytes()),
        (6, 65535u16.to_le_bytes()),
        (6, (WmFileKind::Dirty as u16).to_le_bytes()),
    ] {
        let mut bad = good.clone();
        bad[at..at + 2].copy_from_slice(&data);
        assert!(inspect(&bad, Mode::Events, EPOCH, CAPS).is_err());
    }
    let mut bad = submitted(9);
    bad[42] = 1; // typed Submitted reserved bytes
    let mut prefix = good.clone();
    prefix.extend(bad);
    assert!(inspect(&prefix, Mode::Events, EPOCH, CAPS).is_err());
    let mut zero_sequence = good.clone();
    zero_sequence[24..32].fill(0);
    assert!(inspect(&zero_sequence, Mode::Events, EPOCH, CAPS).is_err());
    let mut trailing = good;
    trailing.extend([0, 0, 0]);
    assert!(inspect(&trailing, Mode::Events, EPOCH, CAPS).is_err());
}

#[test]
fn max_count_with_four_row_bytes_is_refused_by_the_shared_snapshot_codec() {
    let mut bytes =
        encode_wm_file_snapshot(header(WmFileKind::Snapshot, 0), &snapshot(), CAPS).unwrap();
    bytes.truncate(64 + WM_FILE_SECTION_HEADER_BYTES + 4);
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[68..72].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[72..76].copy_from_slice(&4u32.to_le_bytes());
    let length = u32::try_from(bytes.len()).unwrap();
    bytes[..4].copy_from_slice(&length.to_le_bytes());
    assert!(decode_wm_file_snapshot(&bytes, CAPS).is_err());
    assert!(inspect(&bytes, Mode::Snapshot, EPOCH, CAPS).is_err());
}

#[test]
fn bounded_reader_stops_at_one_extra_byte_without_trusting_metadata() {
    let bytes = vec![0; WM_FILE_MAX_BYTES];
    assert_eq!(
        inspection::read_bounded(bytes.as_slice()).unwrap().len(),
        WM_FILE_MAX_BYTES
    );
    let mut cursor = std::io::Cursor::new(vec![0; WM_FILE_MAX_BYTES + 100]);
    assert!(inspection::read_bounded(&mut cursor).is_err());
    assert_eq!(cursor.position(), WM_FILE_MAX_BYTES as u64 + 1);
    assert!(inspect(cursor.get_ref(), Mode::Events, EPOCH, CAPS).is_err());
}

#[test]
fn cli_requires_explicit_unique_context_and_preserves_unknown_mask_bits() {
    let args = [
        "events",
        "--path=capture.bin",
        "--epoch=3",
        "--capabilities=0xffffffffffffffff",
    ]
    .map(str::to_owned);
    let parsed = inspection::Options::parse(&args).unwrap();
    assert_eq!(parsed.mode, Mode::Events);
    assert_eq!(parsed.path, std::path::Path::new("capture.bin"));
    assert_eq!(parsed.epoch, EPOCH);
    assert_eq!(parsed.capabilities, CAPS);
    for removed in 0..args.len() {
        let mut bad = args.to_vec();
        bad.remove(removed);
        assert!(inspection::Options::parse(&bad).is_err());
    }
    for extra in ["--epoch=3", "--socket=live", "--path=", "--ack=1"] {
        let mut bad = args.to_vec();
        bad.push(extra.into());
        assert!(inspection::Options::parse(&bad).is_err());
    }
    for epoch in ["0", "bad", "18446744073709551616"] {
        let mut bad = args.to_vec();
        bad[2] = format!("--epoch={epoch}");
        assert!(inspection::Options::parse(&bad).is_err());
    }
}
