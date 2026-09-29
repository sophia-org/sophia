//! Every native launcher invariant of `shell_native_launcher_wire.rs` on the
//! SDK's neutral value codec, and the transaction rule on the native launcher
//! file records, without the socket codecs. The socket frame is only a header
//! around the same value bytes, so the value offsets are the frame payload
//! offsets. The message kinds, frame prefixes, the kind-173 rewrite and the
//! CandidateBegin/CandidateChunk transfer retired with the socket test (t269);
//! files carry one whole `NativeCandidate`.
#[path = "support/native_launcher_fixtures.rs"]
mod fixtures;

use sophia_protocol::shell::encoding::ValueError;
use sophia_protocol::shell::encoding::content::{
    ShellContentValueKind, decode_shell_content_value, encode_shell_content_value,
};
use sophia_protocol::shell::encoding::native_launcher::{
    decode_shell_native_launcher_value, encode_shell_native_launcher_value,
    shell_native_launcher_value_kind,
};
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

fn value(record: &ShellNativeLauncherRecord) -> Vec<u8> {
    encode_shell_native_launcher_value(record).unwrap()
}

fn decode(
    record: &ShellNativeLauncherRecord,
    bytes: &[u8],
) -> Result<ShellNativeLauncherRecord, ValueError> {
    decode_shell_native_launcher_value(shell_native_launcher_value_kind(record), bytes)
}

fn changed(original: &[u8], offset: usize, replacement: &[u8]) -> Vec<u8> {
    let mut bytes = original.to_vec();
    bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
    bytes
}

fn valid(record: ShellNativeLauncherRecord) -> bool {
    encode_shell_native_launcher_value(&record).is_ok()
}

/// `every_kind_has_exact_length_and_rejects_truncation_and_trailing_bytes`.
#[test]
fn every_value_has_exact_length_and_rejects_truncation_and_trailing_bytes() {
    let lengths = [56, 84, 110, 184, 104, 108, 144, 124, 124, 128, 28];
    let records = fixtures::fixtures();
    assert_eq!(records.len(), lengths.len());
    for (index, record) in records.iter().enumerate() {
        let bytes = value(record);
        assert_eq!(bytes.len(), lengths[index], "value {index}");
        assert_eq!(decode(record, &bytes).unwrap(), *record);
        for end in 0..bytes.len() {
            assert!(decode(record, &bytes[..end]).is_err(), "{index} {end}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(record, &trailing).is_err(), "{index}");
    }
}

/// The socket's zero-transaction refusal, on the file records that carry a
/// single native launcher value.
#[test]
fn file_records_require_a_transaction_and_round_trip_exactly() {
    let mut carried = 0;
    for record in fixtures::fixtures() {
        let Some(kind) = shell_file_native_launcher_kind(&record) else {
            continue;
        };
        if kind == ShellFileKind::NativeInput {
            continue;
        }
        carried += 1;
        let header = ShellFileHeader {
            kind,
            connection_epoch: 2,
            submission_id: if shell_file_class(kind) == ShellFileClass::Candidate {
                31
            } else {
                0
            },
            sequence: if shell_file_class(kind) == ShellFileClass::Event {
                41
            } else {
                0
            },
        };
        let tx_record = ShellFileNativeLauncherRecord {
            transaction: TransactionId::from_raw(25),
            record: record.clone(),
        };
        let bytes = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
        assert_eq!(
            decode_shell_file_native_launcher_transaction(&bytes, kind).unwrap(),
            tx_record
        );
        let zero = ShellFileNativeLauncherRecord {
            transaction: TransactionId::from_raw(0),
            record,
        };
        assert_eq!(
            encode_shell_file_native_launcher_transaction(header, &zero),
            Err(ShellFilePayloadError::Identity)
        );
        let mut zeroed = bytes.clone();
        zeroed[SHELL_FILE_HEADER_BYTES..SHELL_FILE_HEADER_BYTES + 8].fill(0);
        assert!(decode_shell_file_native_launcher_transaction(&zeroed, kind).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode_shell_file_native_launcher_transaction(&trailing, kind).is_err());
        assert!(
            decode_shell_file_native_launcher_transaction(&bytes[..bytes.len() - 1], kind).is_err()
        );
    }
    // Control: the single-payload file kinds were actually exercised.
    assert!(carried >= 6, "only {carried} file kinds");
}

/// `every_presented_identity_component_is_required`.
#[test]
fn every_presented_identity_component_is_required_in_values() {
    let focus = ShellNativeLauncherRecord::Focus(fixtures::binding());
    let bytes = value(&focus);
    for offset in (0..104).step_by(8) {
        assert!(
            decode(&focus, &changed(&bytes, offset, &0u64.to_le_bytes())).is_err(),
            "{offset}"
        );
    }
    for record in fixtures::fixtures() {
        let bytes = value(&record);
        assert!(decode(&record, &changed(&bytes, 0, &0u64.to_le_bytes())).is_err());
        assert!(decode(&record, &changed(&bytes, 8, &0u64.to_le_bytes())).is_err());
    }
}

/// `catalog_rows_selection_and_declared_targets_agree`.
#[test]
fn catalog_rows_selection_and_declared_targets_agree_in_values() {
    let original = fixtures::fixtures()[2].clone();
    let ShellNativeLauncherRecord::CandidateBegin(mut v) = original.clone() else {
        unreachable!()
    };
    let begin = |v| valid(ShellNativeLauncherRecord::CandidateBegin(v));
    v.selected = 3;
    assert!(!begin(v.clone()));
    v.selected = 2;
    v.rows = vec![2, 2];
    v.content.target_count = 2;
    assert!(!begin(v.clone()));
    v.rows = vec![2, 4097];
    assert!(!begin(v.clone()));
    v.rows = vec![2];
    assert!(!begin(v.clone()));
    v.content.target_count = 1;
    v.content.surface_count = 2;
    assert!(!begin(v.clone()));
    v.content.surface_count = 1;
    v.rows.clear();
    v.content.target_count = 0;
    assert!(!begin(v.clone()));
    v.selected = 0;
    assert!(begin(v));
    let bytes = value(&original);
    assert!(decode(&original, &changed(&bytes, 106, &u16::MAX.to_le_bytes())).is_err());
}

/// `native_chunk_does_not_widen_legacy_roles_or_action_kinds`.
#[test]
fn native_chunk_values_do_not_widen_legacy_roles_or_action_kinds() {
    let native = fixtures::fixtures()[3].clone();
    let ShellNativeLauncherRecord::CandidateChunk(mut chunk) = native.clone() else {
        unreachable!()
    };
    assert!(
        encode_shell_content_value(&ShellContentRecord::CandidateChunk(chunk.clone())).is_err()
    );
    assert!(
        decode_shell_content_value(ShellContentValueKind::CandidateChunk, &value(&native)).is_err()
    );
    let accepts = |v| valid(ShellNativeLauncherRecord::CandidateChunk(v));
    chunk.surfaces[0].reservation_extent = 1;
    assert!(!accepts(chunk.clone()));
    chunk.surfaces[0].reservation_extent = 0;
    chunk.surfaces[0].parent_surface_index = 0;
    assert!(!accepts(chunk.clone()));
    chunk.surfaces[0].parent_surface_index = u16::MAX;
    chunk.surfaces[0].role = 1;
    assert!(!accepts(chunk.clone()));
    chunk.surfaces[0].role = 3;
    chunk.targets[0].action_kind = 1;
    assert!(!accepts(chunk.clone()));
    chunk.targets[0].action_kind = 2;
    chunk.targets[0].surface_index = 1;
    assert!(!accepts(chunk.clone()));
    chunk.targets[0].surface_index = 0;
    chunk.targets[0].action_id = 4097;
    assert!(!accepts(chunk));
}

/// `text_is_bounded_utf8_and_accept_requires_the_presented_revision`, on the
/// value and on the padded file input.
#[test]
fn input_text_is_bounded_utf8_and_accept_requires_the_presented_revision() {
    let original = fixtures::fixtures()[6].clone();
    let ShellNativeLauncherRecord::Input(mut v) = original.clone() else {
        unreachable!()
    };
    let accepts = |v: &NativeLauncherInput| {
        let value_ok = valid(ShellNativeLauncherRecord::Input(v.clone()));
        let file_ok = encode_shell_file_native_input_body(&ShellFileNativeLauncherRecord {
            transaction: TransactionId::from_raw(1),
            record: ShellNativeLauncherRecord::Input(v.clone()),
        })
        .is_ok();
        assert_eq!(value_ok, file_ok, "value and file input agree");
        value_ok
    };
    for text in [
        "".to_owned(),
        "x".repeat(257),
        "\0".to_owned(),
        "a\nb".to_owned(),
        "a\u{202e}b".to_owned(),
    ] {
        v.text = text;
        assert!(!accepts(&v));
    }
    v.text = "é".repeat(128);
    assert!(accepts(&v));
    v.kind = NativeLauncherInputKind::Accept;
    assert!(!accepts(&v));
    v.text.clear();
    assert!(!accepts(&v));
    v.event.state_revision = v.event.binding.state_revision;
    assert!(accepts(&v));
    v.kind = NativeLauncherInputKind::Next;
    assert!(!accepts(&v));
    let bytes = value(&original);
    assert!(decode(&original, &changed(&bytes, 132, &[0xff])).is_err());
    assert!(decode(&original, &changed(&bytes, 128, &18u16.to_le_bytes())).is_err());
    assert!(decode(&original, &changed(&bytes, 130, &257u16.to_le_bytes())).is_err());
}

/// `activation_cause_and_outcome_cannot_erase_the_exact_binding`.
#[test]
fn activation_cause_and_outcome_values_cannot_erase_the_exact_binding() {
    let ShellNativeLauncherRecord::ActivationOutcome(mut v) = fixtures::fixtures()[9].clone()
    else {
        unreachable!()
    };
    let accepts = |v| valid(ShellNativeLauncherRecord::ActivationOutcome(v));
    v.activation.cause = 0;
    assert!(!accepts(v));
    v.activation.cause = 2;
    assert!(accepts(v));
    v.activation.event.state_revision += 1;
    assert!(!accepts(v));
    v.activation.event.state_revision -= 1;
    v.status = 2;
    assert!(!accepts(v));
    v.reason = ContentReason::Stale as u16;
    assert!(accepts(v));
    v.status = 6;
    assert!(!accepts(v));
}

/// `reserved_tails_and_release_shape_are_strict`.
#[test]
fn reserved_tails_and_release_shape_are_strict_in_values() {
    for index in [5, 7, 10] {
        let record = fixtures::fixtures()[index].clone();
        let mut bytes = value(&record);
        *bytes.last_mut().unwrap() = 1;
        assert!(decode(&record, &bytes).is_err(), "{index}");
    }
    let ShellNativeLauncherRecord::AllocationRequest(mut v) = fixtures::fixtures()[1].clone()
    else {
        unreachable!()
    };
    let accepts = |v| valid(ShellNativeLauncherRecord::AllocationRequest(v));
    v.operation = 3;
    assert!(!accepts(v));
    v.prior = fixtures::binding().allocation;
    assert!(!accepts(v));
    v.desired_width = 0;
    v.desired_height = 0;
    assert!(accepts(v));
    v.margins.top = 1;
    assert!(!accepts(v));
}
