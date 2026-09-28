//! The view indicator invariants of `shell_indicators.rs` on the file
//! records (`Indicators` object, `IndicatorActivate` candidate and
//! `IndicatorActivationOutcome` event), without the socket codecs. The
//! Begin/OutputStatus/Entry/End transfer and the exact legacy error spellings
//! stay in the socket test and retire with the socket wire (t269).
//!
//! Not carried: `oversized_label_is_rejected`. The pinned SDK's file encoder
//! panics on a label or layout longer than 32 bytes (slice bounds in the
//! padded text writer) instead of refusing it, so the twin waits for an SDK
//! fix; see the t265 shell retirement map.
use sophia_protocol::shell_files::*;
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

fn header(kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 7,
        submission_id: if shell_file_class(kind) == ShellFileClass::Candidate {
            1
        } else {
            0
        },
        sequence: if shell_file_class(kind) == ShellFileClass::Event {
            1
        } else {
            0
        },
    }
}

fn encode(snapshot: &ShellIndicatorSnapshot) -> Result<Vec<u8>, ShellFilePayloadError> {
    encode_shell_file_indicators(
        header(ShellFileKind::Indicators),
        &ShellFileIndicators {
            transaction: TransactionId::from_raw(9),
            snapshot: snapshot.clone(),
        },
    )
}

fn round_trip(snapshot: &ShellIndicatorSnapshot) -> ShellIndicatorSnapshot {
    let decoded = decode_shell_file_indicators(&encode(snapshot).unwrap()).unwrap();
    assert_eq!(decoded.transaction, TransactionId::from_raw(9));
    decoded.snapshot
}

fn activation() -> ShellIndicatorActivation {
    ShellIndicatorActivation {
        connection_epoch: 7,
        snapshot_generation: 3,
        output: OutputId::from_raw(1),
        indicator: 11,
        action: 5,
        event_id: 88,
    }
}

/// `snapshot_round_trips`.
#[test]
fn snapshot_object_round_trips() {
    assert_eq!(round_trip(&snapshot()), snapshot());
}

/// `focused_output_survives_with_no_indicators`.
#[test]
fn focused_output_survives_with_no_indicators_in_the_object() {
    let mut s = snapshot();
    s.indicators.clear();
    let decoded = round_trip(&s);
    assert_eq!(decoded.active_output, Some(OutputId::from_raw(2)));
    assert!(decoded.indicators.is_empty());
    assert_eq!(decoded.statuses.len(), 2);
}

/// `absent_active_output_round_trips`. Its stale-identity companion is the
/// SDK's `shell_files_indicators.rs` exact `ReservedNonZero` case.
#[test]
fn absent_active_output_round_trips_in_the_object() {
    let mut s = snapshot();
    s.active_output = None;
    assert_eq!(round_trip(&s).active_output, None);
}

/// `too_many_indicators_is_rejected` and the two count-over-maximum cases:
/// each bound refuses on the file encoder, one past its maximum, and admits
/// exactly the maximum.
#[test]
fn indicator_and_output_status_counts_are_bounded_on_encode() {
    let indicators = |count: usize| {
        (0..count)
            .map(|i| ShellIndicator {
                output: OutputId::from_raw(1),
                indicator: i as u64,
                action: 1,
                slot: 0,
                state_bits: 0,
                label: "v".to_owned(),
            })
            .collect::<Vec<_>>()
    };
    let mut s = snapshot();
    s.indicators = indicators(SOPHIA_SHELL_MAX_INDICATORS);
    assert_eq!(round_trip(&s), s);
    s.indicators = indicators(SOPHIA_SHELL_MAX_INDICATORS + 1);
    assert!(encode(&s).is_err());

    let statuses = |count: usize| {
        (0..count)
            .map(|i| ShellOutputStatus {
                output: OutputId::from_raw(i as u64 + 1),
                focus_bits: 0,
                layout: "Tall".to_owned(),
            })
            .collect::<Vec<_>>()
    };
    let mut s = snapshot();
    s.statuses = statuses(SOPHIA_SHELL_MAX_OUTPUT_STATUS);
    assert_eq!(round_trip(&s), s);
    s.statuses = statuses(SOPHIA_SHELL_MAX_OUTPUT_STATUS + 1);
    assert!(encode(&s).is_err());
}

/// `activation_round_trips` and `trailing_bytes_are_rejected`.
#[test]
fn activation_candidate_round_trips_and_refuses_trailing_bytes() {
    let value = ShellFileIndicatorActivate {
        transaction: TransactionId::from_raw(4),
        activation: activation(),
    };
    let mut bytes =
        encode_shell_file_indicator_activate(header(ShellFileKind::IndicatorActivate), &value)
            .unwrap();
    assert_eq!(decode_shell_file_indicator_activate(&bytes).unwrap(), value);
    bytes.push(0);
    assert!(decode_shell_file_indicator_activate(&bytes).is_err());
}

/// `activation_outcome_round_trips_and_rejects_unknown_status`, over every
/// status the contract defines.
#[test]
fn activation_outcome_event_round_trips_every_status_and_refuses_unknown_ones() {
    for (status, reason) in [
        (ShellIndicatorActivationStatus::Accepted, 0),
        (ShellIndicatorActivationStatus::Stale, 1),
        (ShellIndicatorActivationStatus::Unknown, 2),
        (ShellIndicatorActivationStatus::Unauthorized, 0),
    ] {
        let value = ShellFileIndicatorActivationOutcome {
            transaction: TransactionId::from_raw(4),
            outcome: ShellIndicatorActivationOutcome {
                connection_epoch: 7,
                snapshot_generation: 3,
                event_id: 88,
                status,
                reason,
            },
        };
        let mut bytes = encode_shell_file_indicator_activation_outcome(
            header(ShellFileKind::IndicatorActivationOutcome),
            &value,
        )
        .unwrap();
        assert_eq!(
            decode_shell_file_indicator_activation_outcome(&bytes).unwrap(),
            value
        );
        let offset = bytes.len() - 4;
        bytes[offset..offset + 2].copy_from_slice(&9u16.to_le_bytes());
        assert!(decode_shell_file_indicator_activation_outcome(&bytes).is_err());
    }
}
