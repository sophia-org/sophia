use sophia_protocol::*;

fn snapshot() -> ShellIndicatorSnapshot {
    ShellIndicatorSnapshot {
        connection_epoch: 7,
        generation: 3,
        active_output: Some(OutputId::from_raw(2)),
        statuses: vec![
            ShellOutputStatus {
                output: OutputId::from_raw(1),
                focus_bits: 0,
                layout: "Tall".to_owned(),
            },
            ShellOutputStatus {
                output: OutputId::from_raw(2),
                focus_bits: 1,
                layout: "Scroller".to_owned(),
            },
        ],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(1),
            indicator: 11,
            action: 5,
            slot: 0,
            state_bits: 1,
            label: "web".to_owned(),
        }],
    }
}

#[test]
fn snapshot_round_trips() {
    let tx = TransactionId::from_raw(9);
    let frames = encode_shell_indicator_snapshot(tx, &snapshot()).expect("encode");
    let (decoded_tx, decoded) = decode_shell_indicator_snapshot(&frames).expect("decode");
    assert_eq!(decoded_tx, tx);
    assert_eq!(decoded, snapshot());
}

/// The case this vocabulary exists for: an output is focused while holding no
/// window, so it publishes a status and no indicators at all.
#[test]
fn focused_output_survives_with_no_indicators() {
    let mut s = snapshot();
    s.indicators.clear();
    let frames = encode_shell_indicator_snapshot(TransactionId::from_raw(1), &s).expect("encode");
    let (_, decoded) = decode_shell_indicator_snapshot(&frames).expect("decode");
    assert_eq!(decoded.active_output, Some(OutputId::from_raw(2)));
    assert!(decoded.indicators.is_empty());
    assert_eq!(decoded.statuses.len(), 2);
}

#[test]
fn absent_active_output_round_trips() {
    let mut s = snapshot();
    s.active_output = None;
    let frames = encode_shell_indicator_snapshot(TransactionId::from_raw(1), &s).expect("encode");
    let (_, decoded) = decode_shell_indicator_snapshot(&frames).expect("decode");
    assert_eq!(decoded.active_output, None);
}

/// A cleared present flag must carry a zeroed identity, or a stale output id
/// rides along behind it unnoticed.
#[test]
fn absent_active_output_rejects_a_stale_identity() {
    let mut s = snapshot();
    s.active_output = None;
    let mut frames =
        encode_shell_indicator_snapshot(TransactionId::from_raw(1), &s).expect("encode");
    let begin = &mut frames[0];
    let offset = begin.len() - 8 - 2 - 2 - 2 - 2;
    begin[offset..offset + 8].copy_from_slice(&9u64.to_le_bytes());
    assert!(decode_shell_indicator_snapshot(&frames).is_err());
}

#[test]
fn truncated_frame_sequence_is_rejected() {
    let frames =
        encode_shell_indicator_snapshot(TransactionId::from_raw(1), &snapshot()).expect("encode");
    assert!(decode_shell_indicator_snapshot(&frames[..frames.len() - 1]).is_err());
}

#[test]
fn oversized_label_is_rejected() {
    let mut s = snapshot();
    s.indicators[0].label = "x".repeat(SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES + 1);
    assert!(encode_shell_indicator_snapshot(TransactionId::from_raw(1), &s).is_err());
}

#[test]
fn too_many_indicators_is_rejected() {
    let mut s = snapshot();
    s.indicators = (0..=SOPHIA_SHELL_MAX_INDICATORS)
        .map(|i| ShellIndicator {
            output: OutputId::from_raw(1),
            indicator: i as u64,
            action: 1,
            slot: 0,
            state_bits: 0,
            label: "v".to_owned(),
        })
        .collect();
    assert!(encode_shell_indicator_snapshot(TransactionId::from_raw(1), &s).is_err());
}

/// Regression: `validate_shell_indicator_snapshot`'s count checks must keep
/// reporting `IpcCodecError::CountTooLarge { count, max }` exactly, not the
/// file-wire's own `InvalidRecord`-shaped error
/// (`crate::shell::indicators::validate`, which the neutral value codec
/// uses for the same bound). This is a distinct, byte/value-identical
/// public IPC error, not merely `.is_err()`.
#[test]
fn indicator_count_over_maximum_reports_the_exact_ipc_error() {
    let mut s = snapshot();
    let count = SOPHIA_SHELL_MAX_INDICATORS + 1;
    s.indicators = (0..count)
        .map(|i| ShellIndicator {
            output: OutputId::from_raw(1),
            indicator: i as u64,
            action: 1,
            slot: 0,
            state_bits: 0,
            label: "v".to_owned(),
        })
        .collect();
    assert_eq!(
        validate_shell_indicator_snapshot(&s),
        Err(IpcCodecError::CountTooLarge {
            count,
            max: SOPHIA_SHELL_MAX_INDICATORS,
        })
    );
}

#[test]
fn output_status_count_over_maximum_reports_the_exact_ipc_error() {
    let mut s = snapshot();
    let count = SOPHIA_SHELL_MAX_OUTPUT_STATUS + 1;
    s.statuses = (0..count)
        .map(|i| ShellOutputStatus {
            output: OutputId::from_raw(i as u64 + 1),
            focus_bits: 0,
            layout: "Tall".to_owned(),
        })
        .collect();
    assert_eq!(
        validate_shell_indicator_snapshot(&s),
        Err(IpcCodecError::CountTooLarge {
            count,
            max: SOPHIA_SHELL_MAX_OUTPUT_STATUS,
        })
    );
}

#[test]
fn activation_round_trips() {
    let tx = TransactionId::from_raw(4);
    let activation = ShellIndicatorActivation {
        connection_epoch: 7,
        snapshot_generation: 3,
        output: OutputId::from_raw(1),
        indicator: 11,
        action: 5,
        event_id: 88,
    };
    let bytes = encode_shell_indicator_activation(tx, &activation).expect("encode");
    let (decoded_tx, decoded) = decode_shell_indicator_activation(&bytes).expect("decode");
    assert_eq!(decoded_tx, tx);
    assert_eq!(decoded, activation);
}

#[test]
fn activation_outcome_round_trips_and_rejects_unknown_status() {
    let tx = TransactionId::from_raw(4);
    let outcome = ShellIndicatorActivationOutcome {
        connection_epoch: 7,
        snapshot_generation: 3,
        event_id: 88,
        status: ShellIndicatorActivationStatus::Stale,
        reason: 1,
    };
    let mut bytes = encode_shell_indicator_activation_outcome(tx, &outcome).expect("encode");
    let (_, decoded) = decode_shell_indicator_activation_outcome(&bytes).expect("decode");
    assert_eq!(decoded, outcome);

    let offset = bytes.len() - 4;
    bytes[offset..offset + 2].copy_from_slice(&9u16.to_le_bytes());
    assert!(decode_shell_indicator_activation_outcome(&bytes).is_err());
}

/// Regression: an invalid status byte must still report
/// `IpcCodecError::InvalidEnum { field: "shell_indicator_activation_status",
/// value }` exactly -- the original identifier, not the neutral value
/// codec's own spelling (`"shell indicator activation status"`, used only
/// by the file wire's decoder). The IPC boundary maps the neutral error back
/// to this field name.
#[test]
fn activation_outcome_invalid_status_reports_the_exact_ipc_error() {
    let tx = TransactionId::from_raw(4);
    let outcome = ShellIndicatorActivationOutcome {
        connection_epoch: 7,
        snapshot_generation: 3,
        event_id: 88,
        status: ShellIndicatorActivationStatus::Stale,
        reason: 1,
    };
    let mut bytes = encode_shell_indicator_activation_outcome(tx, &outcome).expect("encode");
    let offset = bytes.len() - 4;
    bytes[offset..offset + 2].copy_from_slice(&9u16.to_le_bytes());
    assert_eq!(
        decode_shell_indicator_activation_outcome(&bytes),
        Err(IpcCodecError::InvalidEnum {
            field: "shell_indicator_activation_status",
            value: 9,
        })
    );
}

#[test]
fn trailing_bytes_are_rejected() {
    let mut bytes = encode_shell_indicator_activation(
        TransactionId::from_raw(4),
        &ShellIndicatorActivation {
            connection_epoch: 7,
            snapshot_generation: 3,
            output: OutputId::from_raw(1),
            indicator: 11,
            action: 5,
            event_id: 88,
        },
    )
    .expect("encode");
    bytes.push(0);
    assert!(decode_shell_indicator_activation(&bytes).is_err());
}
