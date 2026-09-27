//! Coverage for the t252 B5 native launcher (r7) file records in
//! `sophia_shell_protocol::shell_files`: the eight single-payload transaction
//! kinds, `NativeInput` (fixed, zero-padded text) and the whole
//! `NativeCandidate`.
// This binary only exercises the native-launcher fixtures; the rest of the
// shared `support` module (built for `shell_files_kdl.rs`, which uses all of
// it) is intentionally unused here.
#[allow(dead_code)]
#[path = "support/shell_files_kdl/mod.rs"]
mod support;

use sophia_shell_protocol::shell::encoding::ValueError;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use support::fixtures_native_launcher as fx;

fn header(kind: ShellFileKind, submission_id: u64, sequence: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id,
        sequence,
    }
}

fn assert_round_trips(
    kind: ShellFileKind,
    submission_id: u64,
    sequence: u64,
    record: ShellNativeLauncherRecord,
    transaction: u64,
) {
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record,
    };
    let h = header(kind, submission_id, sequence);
    let encoded = encode_shell_file_native_launcher_transaction(h, &tx_record).unwrap();
    assert_eq!(
        decode_shell_file_native_launcher_transaction(&encoded, kind).unwrap(),
        tx_record
    );
    let (body_kind, body) = encode_shell_file_native_launcher_transaction_body(&tx_record).unwrap();
    assert_eq!(body_kind, kind);
    assert_eq!(body, encoded[SHELL_FILE_HEADER_BYTES..]);
    assert_eq!(
        shell_file_native_launcher_kind(&tx_record.record),
        Some(kind)
    );
}

#[test]
fn native_opening_round_trips() {
    let (tx, value, _) = fx::native_opening();
    assert_round_trips(
        ShellFileKind::NativeOpening,
        0,
        1,
        ShellNativeLauncherRecord::Opening(value),
        tx,
    );
}

#[test]
fn native_focus_round_trips() {
    let (tx, value, _) = fx::native_focus();
    assert_round_trips(
        ShellFileKind::NativeFocus,
        0,
        1,
        ShellNativeLauncherRecord::Focus(value),
        tx,
    );
}

#[test]
fn native_focus_revoked_round_trips() {
    let (tx, value, _) = fx::native_focus_revoked();
    assert_round_trips(
        ShellFileKind::NativeFocusRevoked,
        0,
        1,
        ShellNativeLauncherRecord::FocusRevoked(value),
        tx,
    );
}

#[test]
fn native_activation_outcome_round_trips() {
    let (tx, value, _) = fx::native_activation_outcome();
    assert_round_trips(
        ShellFileKind::NativeActivationOutcome,
        0,
        1,
        ShellNativeLauncherRecord::ActivationOutcome(value),
        tx,
    );
}

#[test]
fn native_closed_round_trips() {
    let (tx, value, _) = fx::native_closed();
    assert_round_trips(
        ShellFileKind::NativeClosed,
        0,
        1,
        ShellNativeLauncherRecord::Closed(value),
        tx,
    );
}

#[test]
fn native_allocation_request_round_trips() {
    let (tx, value, _) = fx::native_allocation_request();
    assert_round_trips(
        ShellFileKind::NativeAllocationRequest,
        1,
        0,
        ShellNativeLauncherRecord::AllocationRequest(value),
        tx,
    );
}

#[test]
fn native_input_ack_round_trips() {
    let (tx, value, _) = fx::native_input_ack();
    assert_round_trips(
        ShellFileKind::NativeInputAck,
        1,
        0,
        ShellNativeLauncherRecord::InputAck(value),
        tx,
    );
}

#[test]
fn native_activate_round_trips() {
    let (tx, value, _) = fx::native_activate();
    assert_round_trips(
        ShellFileKind::NativeActivate,
        1,
        0,
        ShellNativeLauncherRecord::Activate(value),
        tx,
    );
}

#[test]
fn native_launcher_wrong_kind_is_refused() {
    let (tx, value, _) = fx::native_closed();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Closed(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(
        header(ShellFileKind::NativeClosed, 0, 1),
        &tx_record,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_native_launcher_transaction(&encoded, ShellFileKind::NativeOpening)
            .unwrap_err(),
        ShellFileCodecError::Kind.into()
    );
}

#[test]
fn native_closed_refuses_reserved_nonzero() {
    let (tx, value, _) = fx::native_closed();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Closed(value),
    };
    let mut bytes = encode_shell_file_native_launcher_transaction(
        header(ShellFileKind::NativeClosed, 0, 1),
        &tx_record,
    )
    .unwrap();
    // Body offset 34 (record offset 32 + 34 = 66): NativeClosed's trailing
    // reserved u16.
    let len = bytes.len();
    bytes[len - 2..].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_native_launcher_transaction(&bytes, ShellFileKind::NativeClosed)
            .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn native_activation_outcome_refuses_truncation_and_trailing_bytes() {
    let (tx, value, _) = fx::native_activation_outcome();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::ActivationOutcome(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(
        header(ShellFileKind::NativeActivationOutcome, 0, 1),
        &tx_record,
    )
    .unwrap();

    let mut truncated = encoded.clone();
    truncated.pop();
    let truncated_len = (truncated.len() as u32).to_le_bytes();
    truncated[0..4].copy_from_slice(&truncated_len);
    assert_eq!(
        decode_shell_file_native_launcher_transaction(
            &truncated,
            ShellFileKind::NativeActivationOutcome
        )
        .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::Truncated)
    );

    let mut trailing = encoded;
    trailing.push(0);
    let trailing_len = (trailing.len() as u32).to_le_bytes();
    trailing[0..4].copy_from_slice(&trailing_len);
    assert_eq!(
        decode_shell_file_native_launcher_transaction(
            &trailing,
            ShellFileKind::NativeActivationOutcome
        )
        .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn native_opening_refuses_invalid_semantic_value() {
    let (tx, mut value, _) = fx::native_opening();
    // `state_revision` must be exactly 1 for an Opening; 2 fails the
    // validator on both encode and decode.
    value.state_revision = 2;
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Opening(value),
    };
    assert_eq!(
        encode_shell_file_native_launcher_transaction(
            header(ShellFileKind::NativeOpening, 0, 1),
            &tx_record
        )
        .unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("native launcher opening"))
    );
}

// ---------------------------------------------------------------------
// NativeInput: fixed, zero-padded text, distinct from the transaction
// dispatch above.
// ---------------------------------------------------------------------

fn native_input_header() -> ShellFileHeader {
    header(ShellFileKind::NativeInput, 0, 1)
}

#[test]
fn native_input_round_trips() {
    let (tx, value, _) = fx::native_input();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Input(value),
    };
    let encoded = encode_shell_file_native_input(native_input_header(), &tx_record).unwrap();
    assert_eq!(decode_shell_file_native_input(&encoded).unwrap(), tx_record);
}

#[test]
fn native_input_refuses_text_over_the_maximum() {
    let (tx, mut value, _) = fx::native_input();
    value.text = "x".repeat(SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES + 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Input(value),
    };
    // The native-launcher validator's own text bound runs before the wire
    // encoder's padding check ever sees the length.
    assert_eq!(
        encode_shell_file_native_input(native_input_header(), &tx_record).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("native launcher text"))
    );
}

#[test]
fn native_input_refuses_a_nonzero_padding_tail() {
    let (tx, value, _) = fx::native_input();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Input(value),
    };
    let mut encoded = encode_shell_file_native_input(native_input_header(), &tx_record).unwrap();
    // The text field's length+reserved prefix starts at record offset 32
    // (header) + 138 (body offset) = 170; its 256-byte padded content
    // starts 4 bytes later, at 174. Its length is 13 ("open terminal"), so
    // one byte past the written text, still inside the padded field, must
    // be zero.
    let poison_at = SHELL_FILE_HEADER_BYTES + 138 + 4 + 13;
    encoded[poison_at] = 1;
    assert_eq!(
        decode_shell_file_native_input(&encoded).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn native_input_refuses_an_invalid_kind() {
    let (tx, value, _) = fx::native_input();
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(tx),
        record: ShellNativeLauncherRecord::Input(value),
    };
    let mut encoded = encode_shell_file_native_input(native_input_header(), &tx_record).unwrap();
    // `kind` sits at body offset 136, record offset 32 + 136 = 168.
    encoded[168..170].copy_from_slice(&99u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_native_input(&encoded).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidEnum {
            field: "native launcher input",
            value: 99,
        })
    );
}

// ---------------------------------------------------------------------
// NativeCandidate: whole value, no transfer shape.
// ---------------------------------------------------------------------

fn candidate_header() -> ShellFileHeader {
    header(ShellFileKind::NativeCandidate, 1, 0)
}

#[test]
fn native_candidate_round_trips() {
    let (tx, candidate, ..) = fx::native_candidate();
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(candidate_header(), &value).unwrap();
    assert_eq!(decode_shell_file_native_candidate(&encoded).unwrap(), value);
}

#[test]
fn native_candidate_refuses_zero_transaction() {
    let (tx, candidate, ..) = fx::native_candidate();
    let mut value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    value.transaction = TransactionId::INVALID;
    assert_eq!(
        encode_shell_file_native_candidate(candidate_header(), &value).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn native_candidate_refuses_reserved_nonzero() {
    let (tx, candidate, ..) = fx::native_candidate();
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(candidate_header(), &value).unwrap();
    let mut bad = encoded.clone();
    // Prefix reserved u16 at body offset 106, record offset 32 + 106 = 138.
    bad[138..140].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_native_candidate(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn native_candidate_refuses_count_above_maximum() {
    let (tx, candidate, ..) = fx::native_candidate();
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(candidate_header(), &value).unwrap();
    let mut bad = encoded.clone();
    // `surface_count` sits at body offset 98, record offset 32 + 98 = 130;
    // the native profile's maximum is 1.
    bad[130..132].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_native_candidate(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::CountTooLarge { count: 2, max: 1 })
    );
}

#[test]
fn native_candidate_refuses_trailing_bytes() {
    let (tx, candidate, ..) = fx::native_candidate();
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(candidate_header(), &value).unwrap();
    let mut trailing = encoded;
    trailing.push(0);
    let len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_native_candidate(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn native_candidate_refuses_invalid_row_content() {
    let (tx, mut candidate, ..) = fx::native_candidate();
    // A native launcher surface must be role=3; 1 fails the chunk validator
    // (every part is validated on encode, before any bytes are written).
    candidate.candidate.surfaces[0].role = 1;
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(tx),
        candidate,
    };
    assert_eq!(
        encode_shell_file_native_candidate(candidate_header(), &value).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("content surface"))
    );
}

#[test]
fn native_candidate_decode_refuses_bytes_over_the_max_cap() {
    let bytes = vec![0u8; SHELL_FILE_MAX_CANDIDATE_BYTES + 1];
    assert_eq!(
        decode_shell_file_native_candidate(&bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
    );
}

#[test]
fn maximal_native_candidate_fits_within_the_cap() {
    // The native profile's maximum shape: one surface, 32 placements, 32
    // targets and 32 displayed catalog rows (crates/sophia-protocol/src/
    // shell/content/validation.rs `validate_candidate_chunk_profile`, and
    // `SOPHIA_SHELL_MAX_LAUNCHER_ROWS`).
    let surface = ContentSurface {
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        scale_generation: 1,
        role: 3,
        edge: 1,
        margins: ContentMargins::default(),
        reservation_extent: 0,
        parent_surface_index: u16::MAX,
        anchor_parent_rect: ContentPixelRect::default(),
    };
    let placements: Vec<_> = (0..32)
        .map(|i| ContentPlacement {
            resource: ContentResourceId {
                id: i + 1,
                generation: 1,
            },
            surface_index: 0,
            destination_x_px: 0,
            destination_y_px: 0,
        })
        .collect();
    let targets: Vec<_> = (0..32)
        .map(|i| ContentTarget {
            surface_index: 0,
            action_kind: 2,
            target_id: i + 1,
            target_generation: 1,
            action_id: i + 1,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        })
        .collect();
    let rows: Vec<u16> = (1..=32).collect();
    let candidate = NativeContentCandidate {
        candidate: ContentCandidate {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            candidate_generation: 1,
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surfaces: vec![surface],
            placements,
            targets,
        },
        opening: 1,
        catalog_generation: 1,
        state_revision: 1,
        selected: 1,
        rows,
    };
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(1),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(candidate_header(), &value).unwrap();
    assert!(encoded.len() <= SHELL_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(decode_shell_file_native_candidate(&encoded).unwrap(), value);
    // 32-byte record header + 108-byte prefix + 1 surface row (64 B)
    // + 32 placement rows (32 B each) + 32 target rows (48 B each)
    // + 32 catalog-slot rows (2 B each) = 32 + 108 + 64 + 1024 + 1536 + 64
    // = 2828 bytes, well under the 8192 cap.
    assert_eq!(encoded.len(), 2828);
}
