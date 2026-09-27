//! Coverage for the t252 B5 view indicator (r6) file records in
//! `sophia_shell_protocol::shell_files`: the whole `Indicators` object, the
//! indicator activation-outcome event and the indicator activate candidate.
// This binary only exercises the indicator fixtures; the rest of the shared
// `support` module (built for `shell_files_kdl.rs`, which uses all of it) is
// intentionally unused here.
#[allow(dead_code)]
#[path = "support/shell_files_kdl/mod.rs"]
mod support;

use sophia_shell_protocol::shell::encoding::ValueError;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use support::fixtures_indicators as fx;

fn object_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    }
}

fn event_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    }
}

fn candidate_header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    }
}

// ---------------------------------------------------------------------
// Indicators: the whole object.
// ---------------------------------------------------------------------

#[test]
fn indicators_round_trips() {
    let (tx, snapshot, ..) = fx::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(tx),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    assert_eq!(decode_shell_file_indicators(&encoded).unwrap(), value);
}

#[test]
fn indicators_refuses_zero_transaction() {
    let (_, snapshot, ..) = fx::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::INVALID,
        snapshot,
    };
    assert_eq!(
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn indicators_refuses_reserved_nonzero() {
    let (tx, snapshot, ..) = fx::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(tx),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    let mut bad = encoded.clone();
    // Prefix reserved u16 at body offset 38, record offset 32 + 38 = 70.
    bad[70..72].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicators(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn indicators_refuses_an_absent_output_with_a_stale_identity() {
    let (tx, mut snapshot, ..) = fx::indicators();
    snapshot.active_output = None;
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(tx),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    let mut bad = encoded.clone();
    // `active_output_present` sits at body offset 32, record offset 32 + 32
    // = 64; `active_output_id` at body offset 24, record offset 56, is
    // already nonzero from the fixture and must be cleared for an absent
    // output, so leaving it set behind a cleared flag is refused.
    assert_eq!(bad[64..66], [0, 0]);
    bad[56..64].copy_from_slice(&9u64.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicators(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn indicators_refuses_indicator_count_above_maximum() {
    let (tx, snapshot, ..) = fx::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(tx),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    let mut bad = encoded.clone();
    // `indicator_count` sits at body offset 36, record offset 32 + 36 = 68.
    bad[68..70].copy_from_slice(&257u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicators(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::CountTooLarge {
            count: 257,
            max: SOPHIA_SHELL_MAX_INDICATORS,
        })
    );
}

#[test]
fn indicators_refuses_trailing_bytes() {
    let (tx, snapshot, ..) = fx::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(tx),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    let mut trailing = encoded;
    trailing.push(0);
    let len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicators(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn indicators_decode_refuses_bytes_over_its_own_cap() {
    // Below the shared 4 MiB object cap but over the tighter 32 KiB
    // `Indicators` cap.
    let bytes = vec![0u8; SHELL_FILE_INDICATORS_MAX_BYTES + 1];
    assert_eq!(
        decode_shell_file_indicators(&bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
    );
}

/// The indicator snapshot at its maximum within the 32 KiB object cap: 16
/// output statuses and 256 indicators, each at its own 32-byte label
/// maximum.
#[test]
fn maximal_indicators_fit_within_32_kib() {
    let layout = "l".repeat(SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES);
    let statuses: Vec<_> = (0..SOPHIA_SHELL_MAX_OUTPUT_STATUS as u64)
        .map(|i| ShellOutputStatus {
            output: OutputId::from_raw(i + 1),
            focus_bits: 1,
            layout: layout.clone(),
        })
        .collect();
    let label = "n".repeat(SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES);
    let indicators: Vec<_> = (0..SOPHIA_SHELL_MAX_INDICATORS as u64)
        .map(|i| ShellIndicator {
            output: OutputId::from_raw(1),
            indicator: i + 1,
            action: i + 1,
            slot: i as u32,
            state_bits: 0,
            label: label.clone(),
        })
        .collect();
    let snapshot = ShellIndicatorSnapshot {
        connection_epoch: 1,
        generation: 1,
        active_output: Some(OutputId::from_raw(1)),
        statuses,
        indicators,
    };
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(1),
        snapshot,
    };
    let encoded =
        encode_shell_file_indicators(object_header(ShellFileKind::Indicators), &value).unwrap();
    assert!(encoded.len() <= SHELL_FILE_INDICATORS_MAX_BYTES);
    assert_eq!(decode_shell_file_indicators(&encoded).unwrap(), value);
    // 32-byte record header + 40-byte prefix + 16 status rows (46 B each)
    // + 256 indicator rows (66 B each) = 32 + 40 + 736 + 16,896 = 17,704
    // bytes, comfortably under the 32,768-byte (32 KiB) cap.
    assert_eq!(encoded.len(), 17_704);
}

// ---------------------------------------------------------------------
// IndicatorActivationOutcome / IndicatorActivate
// ---------------------------------------------------------------------

#[test]
fn indicator_activation_outcome_round_trips() {
    let (tx, outcome, _) = fx::indicator_activation_outcome();
    let value = ShellFileIndicatorActivationOutcome {
        transaction: TransactionId::from_raw(tx),
        outcome,
    };
    let encoded = encode_shell_file_indicator_activation_outcome(
        event_header(ShellFileKind::IndicatorActivationOutcome),
        &value,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_indicator_activation_outcome(&encoded).unwrap(),
        value
    );
}

#[test]
fn indicator_activation_outcome_refuses_an_invalid_status() {
    let (tx, outcome, _) = fx::indicator_activation_outcome();
    let value = ShellFileIndicatorActivationOutcome {
        transaction: TransactionId::from_raw(tx),
        outcome,
    };
    let encoded = encode_shell_file_indicator_activation_outcome(
        event_header(ShellFileKind::IndicatorActivationOutcome),
        &value,
    )
    .unwrap();
    let mut bad = encoded.clone();
    // `status` sits at body offset 32, record offset 32 + 32 = 64.
    bad[64..66].copy_from_slice(&9u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicator_activation_outcome(&bad).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidEnum {
            field: "shell indicator activation status",
            value: 9,
        })
    );
}

#[test]
fn indicator_activation_outcome_refuses_zero_transaction() {
    let (_, outcome, _) = fx::indicator_activation_outcome();
    let value = ShellFileIndicatorActivationOutcome {
        transaction: TransactionId::INVALID,
        outcome,
    };
    assert_eq!(
        encode_shell_file_indicator_activation_outcome(
            event_header(ShellFileKind::IndicatorActivationOutcome),
            &value
        )
        .unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn indicator_activation_outcome_refuses_trailing_bytes() {
    let (tx, outcome, _) = fx::indicator_activation_outcome();
    let value = ShellFileIndicatorActivationOutcome {
        transaction: TransactionId::from_raw(tx),
        outcome,
    };
    let encoded = encode_shell_file_indicator_activation_outcome(
        event_header(ShellFileKind::IndicatorActivationOutcome),
        &value,
    )
    .unwrap();
    let mut trailing = encoded;
    trailing.push(0);
    let len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicator_activation_outcome(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn indicator_activate_round_trips() {
    let (tx, activation, _) = fx::indicator_activate();
    let value = ShellFileIndicatorActivate {
        transaction: TransactionId::from_raw(tx),
        activation,
    };
    let encoded = encode_shell_file_indicator_activate(
        candidate_header(ShellFileKind::IndicatorActivate),
        &value,
    )
    .unwrap();
    assert_eq!(
        decode_shell_file_indicator_activate(&encoded).unwrap(),
        value
    );
}

#[test]
fn indicator_activate_refuses_zero_transaction() {
    let (_, activation, _) = fx::indicator_activate();
    let value = ShellFileIndicatorActivate {
        transaction: TransactionId::INVALID,
        activation,
    };
    assert_eq!(
        encode_shell_file_indicator_activate(
            candidate_header(ShellFileKind::IndicatorActivate),
            &value
        )
        .unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn indicator_activate_refuses_truncation() {
    let (tx, activation, _) = fx::indicator_activate();
    let value = ShellFileIndicatorActivate {
        transaction: TransactionId::from_raw(tx),
        activation,
    };
    let encoded = encode_shell_file_indicator_activate(
        candidate_header(ShellFileKind::IndicatorActivate),
        &value,
    )
    .unwrap();
    let mut truncated = encoded;
    truncated.pop();
    let len = truncated.len() as u32;
    truncated[0..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode_shell_file_indicator_activate(&truncated).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::Truncated)
    );
}
