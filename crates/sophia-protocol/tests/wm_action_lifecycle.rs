//! The chord lifecycle contract (t277): the ActionLifecycle cause and the
//! Configuration rows declaring which actions the WM follows. Wire offsets
//! come from protocol/sophia-wm-files-v1.kdl.
use sophia_protocol::wm_files::*;
use sophia_protocol::*;

#[path = "support/policy_scalar_fixture.rs"]
mod fixture;

const LIFECYCLE_CAPABILITIES: u64 = SOPHIA_WM_CAPABILITY_ACTIONS
    | SOPHIA_WM_CAPABILITY_CONFIGURATION
    | SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE;
const ENDS: [PolicyChordEnd; 5] = [
    PolicyChordEnd::Released,
    PolicyChordEnd::Cancelled,
    PolicyChordEnd::Completed,
    PolicyChordEnd::Aborted,
    PolicyChordEnd::TimedOut,
];

fn header(kind: WmFileKind) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: 3,
        submission_id: if wm_file_class(kind) == WmFileClass::Candidate {
            101
        } else {
            0
        },
        sequence: if wm_file_class(kind) == WmFileClass::Event {
            203
        } else {
            0
        },
    }
}

fn lifecycle(phase: PolicyChordPhase, count: u32) -> PolicyRequestCause {
    PolicyRequestCause::ActionLifecycle {
        activation_serial: 41,
        action: WmActionId::from_raw(186),
        phase,
        count,
    }
}

fn cycle(cause: PolicyRequestCause) -> WmFileCycle {
    WmFileCycle {
        snapshot_transaction: TransactionId::from_raw(7),
        request_transaction: TransactionId::from_raw(9),
        request: fixture::request(cause),
    }
}

fn phases() -> impl Iterator<Item = PolicyChordPhase> {
    std::iter::once(PolicyChordPhase::Held).chain(ENDS.into_iter().map(PolicyChordPhase::Ended))
}

#[test]
fn every_phase_round_trips_with_its_literal_codes() {
    for phase in phases() {
        let value = cycle(lifecycle(phase, 3));
        let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, u64::MAX).unwrap();
        assert_eq!(decode_wm_file_cycle(&bytes, u64::MAX).unwrap(), value);
        let body = &bytes[bytes.len() - 24..];
        let (code, reason) = phase.codes();
        assert_eq!(bytes[72..74], 7u16.to_le_bytes(), "cause kind");
        assert_eq!(body[..8], 41u64.to_le_bytes());
        assert_eq!(body[8..16], 186u64.to_le_bytes());
        assert_eq!(body[16..18], code.to_le_bytes());
        assert_eq!(body[18..20], reason.to_le_bytes());
        assert_eq!(body[20..24], 3u32.to_le_bytes());
    }
    // The count saturates rather than wrapping, so its maximum is valid.
    let value = cycle(lifecycle(
        PolicyChordPhase::Ended(PolicyChordEnd::Released),
        u32::MAX,
    ));
    let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, u64::MAX).unwrap();
    assert_eq!(decode_wm_file_cycle(&bytes, u64::MAX).unwrap(), value);
}

#[test]
fn only_held_with_reason_zero_or_ended_with_a_known_reason_decodes() {
    let value = cycle(lifecycle(PolicyChordPhase::Held, 1));
    let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, u64::MAX).unwrap();
    let body = bytes.len() - 24;
    for phase in 0u16..4 {
        for reason in 0u16..7 {
            let mut wire = bytes.clone();
            wire[body + 16..body + 18].copy_from_slice(&phase.to_le_bytes());
            wire[body + 18..body + 20].copy_from_slice(&reason.to_le_bytes());
            let valid = (phase == 1 && reason == 0) || (phase == 2 && (1..=5).contains(&reason));
            assert_eq!(
                decode_wm_file_cycle(&wire, u64::MAX).is_ok(),
                valid,
                "phase {phase} reason {reason}"
            );
            assert_eq!(PolicyChordPhase::from_codes(phase, reason).is_some(), valid);
        }
    }
    for (at, len) in [(0, 8), (8, 8), (20, 4)] {
        let mut wire = bytes.clone();
        wire[body + at..body + at + len].fill(0);
        assert!(
            decode_wm_file_cycle(&wire, u64::MAX).is_err(),
            "zero at {at}"
        );
    }
}

#[test]
fn the_cause_needs_actions_configuration_and_the_lifecycle() {
    let cause = lifecycle(PolicyChordPhase::Held, 1);
    assert_eq!(
        policy_request_cause_capabilities(&cause),
        LIFECYCLE_CAPABILITIES
    );
    let value = cycle(cause);
    let h = header(WmFileKind::Cycle);
    let bytes = encode_wm_file_cycle(h, &value, u64::MAX).unwrap();
    for bit in [
        SOPHIA_WM_CAPABILITY_ACTIONS,
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
        SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE,
    ] {
        assert_eq!(
            decode_wm_file_cycle(&bytes, !bit),
            Err(WmFilePayloadError::Capabilities { missing: bit })
        );
        assert!(encode_wm_file_cycle(h, &value, !bit).is_err());
    }
}

#[test]
fn scalar_validation_refuses_zero_serial_action_and_count() {
    let valid = fixture::request(lifecycle(PolicyChordPhase::Held, 1));
    assert_eq!(validate_policy_projection_request(&valid), Ok(()));
    for cause in [
        PolicyRequestCause::ActionLifecycle {
            activation_serial: 0,
            action: WmActionId::from_raw(186),
            phase: PolicyChordPhase::Held,
            count: 1,
        },
        PolicyRequestCause::ActionLifecycle {
            activation_serial: 41,
            action: WmActionId::from_raw(0),
            phase: PolicyChordPhase::Held,
            count: 1,
        },
        lifecycle(PolicyChordPhase::Held, 0),
    ] {
        assert!(
            validate_policy_projection_request(&fixture::request(cause)).is_err(),
            "{cause:?}"
        );
    }
}

fn registration(raw: u64, session_operation_slot: Option<u16>) -> PolicyActionRegistration {
    PolicyActionRegistration {
        action: WmActionId::from_raw(raw),
        name: format!("action-{raw}"),
        session_operation_slot,
    }
}

fn configuration(interests: &[(u64, u32)]) -> WmFileConfiguration {
    WmFileConfiguration {
        transaction: TransactionId::from_raw(23),
        configuration: PolicyConfiguration {
            connection_epoch: 3,
            generation: 4,
            actions: vec![
                registration(186, None),
                registration(187, None),
                registration(190, Some(1)),
            ],
            chrome: WmChromePolicy::default(),
            action_lifecycles: interests
                .iter()
                .map(|&(action, held_ms)| PolicyActionLifecycleInterest {
                    action: WmActionId::from_raw(action),
                    held_ms,
                })
                .collect(),
        },
    }
}

#[test]
fn configuration_rows_round_trip_with_their_literal_layout() {
    let value = configuration(&[(186, 150), (187, 0)]);
    let h = header(WmFileKind::Configuration);
    let bytes = encode_wm_file_configuration(h, &value, u64::MAX).unwrap();
    assert_eq!(
        decode_wm_file_configuration(&bytes, u64::MAX).unwrap(),
        value
    );
    // The lifecycle section follows the action section: kind, count, then rows.
    let rows = &bytes[bytes.len() - 32..];
    assert_eq!(rows[..8], 186u64.to_le_bytes());
    assert_eq!(rows[8..12], 150u32.to_le_bytes());
    assert_eq!(rows[12..16], [0; 4]);
    assert_eq!(rows[16..24], 187u64.to_le_bytes());
    let section = &bytes[bytes.len() - 48..bytes.len() - 32];
    assert_eq!(section[..2], 0xff0eu16.to_le_bytes());
    assert_eq!(section[4..8], 2u32.to_le_bytes());

    // Without lifecycle rows the configuration needs no lifecycle capability.
    let plain = configuration(&[]);
    let without = !SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE;
    let plain_bytes = encode_wm_file_configuration(h, &plain, without).unwrap();
    assert_eq!(
        decode_wm_file_configuration(&plain_bytes, without).unwrap(),
        plain
    );
}

#[test]
fn configuration_rows_need_the_lifecycle_actions_and_configuration() {
    let value = configuration(&[(186, 150)]);
    let h = header(WmFileKind::Configuration);
    let bytes = encode_wm_file_configuration(h, &value, u64::MAX).unwrap();
    for bit in [
        SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE,
        SOPHIA_WM_CAPABILITY_ACTIONS,
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
    ] {
        assert_eq!(
            decode_wm_file_configuration(&bytes, !bit),
            Err(WmFilePayloadError::Capabilities { missing: bit })
        );
        assert!(encode_wm_file_configuration(h, &value, !bit).is_err());
    }
    let mut reserved = bytes.clone();
    let at = reserved.len() - 4;
    reserved[at] = 1;
    assert!(decode_wm_file_configuration(&reserved, u64::MAX).is_err());
}

#[test]
fn configuration_rows_follow_the_catalog_once_within_the_held_range() {
    let h = header(WmFileKind::Configuration);
    for held_ms in [0, 50, 150, 5000] {
        assert!(
            encode_wm_file_configuration(h, &configuration(&[(186, held_ms)]), u64::MAX).is_ok(),
            "held_ms {held_ms}"
        );
    }
    let refused: [&[(u64, u32)]; 7] = [
        &[(186, 1)],
        &[(186, 49)],
        &[(186, 5001)],
        &[(186, 150), (186, 0)], // the same action twice
        &[(188, 150)],           // not in the catalog
        &[(190, 150)],           // a session operation
        &[(0, 150)],
    ];
    for interests in refused {
        let value = configuration(interests);
        assert!(
            encode_wm_file_configuration(h, &value, u64::MAX).is_err(),
            "{interests:?}"
        );
        assert!(
            encode_policy_configuration_records(&value.configuration).is_err(),
            "{interests:?}"
        );
    }
    // Decoding applies the same rules: a row the encoder would refuse, put on
    // the wire by hand, is refused too.
    let mut wire =
        encode_wm_file_configuration(h, &configuration(&[(186, 150)]), u64::MAX).unwrap();
    let at = wire.len() - 8;
    wire[at..at + 4].copy_from_slice(&49u32.to_le_bytes());
    assert!(decode_wm_file_configuration(&wire, u64::MAX).is_err());
}

#[test]
fn a_full_catalog_may_be_followed_and_the_section_bounds_more() {
    // Every row names a distinct catalog action and the catalog holds at most
    // 256, so 256 rows is the most a valid configuration can carry.
    let mut value = configuration(&[]);
    value.configuration.actions = (1..=POLICY_MAX_BINDINGS as u64)
        .map(|raw| registration(raw, None))
        .collect();
    value.configuration.action_lifecycles = (1..=POLICY_MAX_BINDINGS as u64)
        .map(|raw| PolicyActionLifecycleInterest {
            action: WmActionId::from_raw(raw),
            held_ms: 0,
        })
        .collect();
    let h = header(WmFileKind::Configuration);
    let bytes = encode_wm_file_configuration(h, &value, u64::MAX).unwrap();
    assert_eq!(
        decode_wm_file_configuration(&bytes, u64::MAX).unwrap(),
        value
    );
    // A 257th row on the wire is refused by the section bound before any
    // catalog rule is consulted.
    let rows = vec![PolicyRecordSectionRef {
        kind: CONFIGURATION_ACTION_LIFECYCLE_RECORD_KIND,
        count: 257,
        bytes: &[0; 257 * CONFIGURATION_ACTION_LIFECYCLE_RECORD_LEN],
    }];
    assert!(matches!(
        validate_policy_record_sections(PolicyRecordContext::Configuration, &rows),
        Err(BinaryCodecError::CountTooLarge {
            count: 257,
            max: 256
        })
    ));
}

const CHORD_ACTION_CAPABILITIES: u64 = LIFECYCLE_CAPABILITIES | SOPHIA_WM_CAPABILITY_CHORD_ACTIONS;

fn chord_action(activation_serial: u64, chord_serial: u64) -> PolicyRequestCause {
    PolicyRequestCause::ChordAction {
        activation_serial,
        chord_serial,
        action: WmActionId::from_raw(186),
    }
}

/// ChordAction is cause 8: activation_serial, chord_serial, action. The
/// opener carries equal serials; a join its own with the opener's chord.
#[test]
fn a_chord_action_round_trips_with_its_literal_layout() {
    for (activation, chord) in [(41, 41), (43, 41)] {
        let value = cycle(chord_action(activation, chord));
        let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, u64::MAX).unwrap();
        assert_eq!(decode_wm_file_cycle(&bytes, u64::MAX).unwrap(), value);
        let body = &bytes[bytes.len() - 24..];
        assert_eq!(bytes[72..74], 8u16.to_le_bytes(), "cause kind");
        assert_eq!(body[..8], activation.to_le_bytes());
        assert_eq!(body[8..16], chord.to_le_bytes());
        assert_eq!(body[16..24], 186u64.to_le_bytes());
    }
}

/// A peer that selected the lifecycle alone never accepts cause 8.
#[test]
fn a_chord_action_needs_all_four_capabilities() {
    let cause = chord_action(43, 41);
    assert_eq!(
        policy_request_cause_capabilities(&cause),
        CHORD_ACTION_CAPABILITIES
    );
    let value = cycle(cause);
    let h = header(WmFileKind::Cycle);
    let bytes = encode_wm_file_cycle(h, &value, u64::MAX).unwrap();
    for bit in [
        SOPHIA_WM_CAPABILITY_ACTIONS,
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
        SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE,
        SOPHIA_WM_CAPABILITY_CHORD_ACTIONS,
    ] {
        assert_eq!(
            decode_wm_file_cycle(&bytes, !bit),
            Err(WmFilePayloadError::Capabilities { missing: bit })
        );
        assert!(encode_wm_file_cycle(h, &value, !bit).is_err());
    }
}

#[test]
fn a_chord_action_refuses_any_zero_field() {
    let value = cycle(chord_action(43, 41));
    let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, u64::MAX).unwrap();
    let body = bytes.len() - 24;
    for at in [0, 8, 16] {
        let mut wire = bytes.clone();
        wire[body + at..body + at + 8].fill(0);
        assert!(
            decode_wm_file_cycle(&wire, u64::MAX).is_err(),
            "zero at {at}"
        );
    }
    for cause in [
        chord_action(0, 41),
        chord_action(43, 0),
        PolicyRequestCause::ChordAction {
            activation_serial: 43,
            chord_serial: 41,
            action: WmActionId::from_raw(0),
        },
    ] {
        assert!(
            validate_policy_projection_request(&fixture::request(cause)).is_err(),
            "{cause:?}"
        );
    }
}
