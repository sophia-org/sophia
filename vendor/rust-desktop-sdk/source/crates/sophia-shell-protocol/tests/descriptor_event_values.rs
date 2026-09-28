//! Literal native layouts, independent of the writer and socket framing.
use sophia_shell_protocol::{shell::encoding::ValueError, shell::encoding::descriptor::*, *};

fn word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn wide(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn prefix(len: usize, values: &[u64]) -> Vec<u8> {
    let mut bytes = vec![0; len];
    for (i, value) in values.iter().enumerate() {
        wide(&mut bytes, i * 8, *value);
    }
    bytes
}
fn check<T: std::fmt::Debug + PartialEq>(
    value: &T,
    literal: &[u8],
    encode: fn(&T) -> Result<Vec<u8>, ValueError>,
    decode: fn(&[u8]) -> Result<T, ValueError>,
) {
    assert_eq!(encode(value).unwrap(), literal);
    assert_eq!(&decode(literal).unwrap(), value);
    for len in 0..literal.len() {
        assert!(decode(&literal[..len]).is_err(), "truncation {len}");
    }
    let mut extra = literal.to_vec();
    extra.push(0);
    assert!(matches!(decode(&extra), Err(ValueError::TrailingBytes(1))));
}
fn bad_word<T>(literal: &[u8], at: usize, value: u16, decode: fn(&[u8]) -> Result<T, ValueError>) {
    let mut bytes = literal.to_vec();
    word(&mut bytes, at, value);
    assert!(decode(&bytes).is_err(), "field {at} = {value}");
}

#[test]
fn descriptor_outcomes_and_ack_have_explicit_tags_and_reserved_words() {
    for (kind, tag) in [
        (ShellV1CandidateOutcomeKind::Prepared, 1),
        (ShellV1CandidateOutcomeKind::Presented, 2),
        (ShellV1CandidateOutcomeKind::Rejected, 3),
        (ShellV1CandidateOutcomeKind::Superseded, 4),
    ] {
        let epoch = if tag == 2 { 3 } else { 0 };
        let value = ShellV1CandidateOutcome {
            connection_epoch: 1,
            candidate_generation: 2,
            presentation_epoch: epoch,
            kind,
        };
        let mut literal = prefix(28, &[1, 2, epoch]);
        word(&mut literal, 24, tag);
        check(
            &value,
            &literal,
            encode_shell_descriptor_outcome_value,
            decode_shell_descriptor_outcome_value,
        );
        bad_word(&literal, 26, 1, decode_shell_descriptor_outcome_value);
        bad_word(&literal, 24, 0, decode_shell_descriptor_outcome_value);
        bad_word(&literal, 24, 5, decode_shell_descriptor_outcome_value);
        // Prepared/Rejected/Superseded have no presentation epoch in this family.
        let mut invalid = value;
        invalid.presentation_epoch = if epoch == 0 { 3 } else { 0 };
        assert!(encode_shell_descriptor_outcome_value(&invalid).is_err());
        wide(&mut literal, 16, invalid.presentation_epoch);
        assert!(decode_shell_descriptor_outcome_value(&literal).is_err());
    }
    for (disposition, tag) in [
        (ShellV1ActivationDisposition::Consumed, 1),
        (ShellV1ActivationDisposition::RejectedStale, 2),
    ] {
        let value = ShellV1ActivationAck {
            connection_epoch: 1,
            activation: 2,
            disposition,
        };
        let mut literal = prefix(20, &[1, 2]);
        word(&mut literal, 16, tag);
        check(
            &value,
            &literal,
            encode_shell_descriptor_ack_value,
            decode_shell_descriptor_ack_value,
        );
        for (at, n) in [(0, 0), (8, 0), (16, 0), (16, 3), (18, 1)] {
            bad_word(&literal, at, n, decode_shell_descriptor_ack_value);
        }
    }
}

#[test]
fn descriptor_activation_binds_recipient_and_carries_the_whole_action() {
    let value = ShellV1Activation {
        connection_epoch: 1,
        candidate_generation: 2,
        presentation_epoch: 3,
        activation: 4,
        action: ToplevelActionCapabilityRef {
            token: 5,
            issuer_epoch: 6,
            issuer_revocation_epoch: 7,
            recipient_epoch: 1,
            target_slot: 8,
            target_generation: 9,
        },
    };
    let mut literal = prefix(76, &[1, 2, 3, 4, 5, 6, 7, 1]);
    word(&mut literal, 64, 8);
    wide(&mut literal, 68, 9);
    check(
        &value,
        &literal,
        encode_shell_descriptor_activation_value,
        decode_shell_descriptor_activation_value,
    );
    for (at, n) in [
        (0, 0),
        (8, 0),
        (16, 0),
        (24, 0),
        (32, 0),
        (40, 0),
        (48, 0),
        (56, 2),
        (64, 0),
        (66, 1),
        (68, 0),
    ] {
        bad_word(&literal, at, n, decode_shell_descriptor_activation_value);
    }
    let mut invalid = value;
    invalid.action.recipient_epoch = 2;
    assert!(encode_shell_descriptor_activation_value(&invalid).is_err());
}

#[test]
fn reference_and_launcher_requests_preserve_operation_and_query_layout() {
    for (operation, tag) in [
        (ShellReferenceOperation::Startup, 0),
        (ShellReferenceOperation::Toggle, 1),
        (ShellReferenceOperation::Next, 2),
        (ShellReferenceOperation::Previous, 3),
        (ShellReferenceOperation::Dismiss, 4),
    ] {
        let value = ShellReferenceRequest {
            connection_epoch: 1,
            catalog_generation: 2,
            request_generation: 3,
            output: OutputId::from_raw(4),
            output_generation: 5,
            presentation_epoch: 0,
            operation,
        };
        let mut literal = prefix(52, &[1, 2, 3, 4, 5, 0]);
        word(&mut literal, 48, tag);
        check(
            &value,
            &literal,
            encode_shell_reference_request_value,
            decode_shell_reference_request_value,
        );
        bad_word(&literal, 48, 5, decode_shell_reference_request_value);
        bad_word(&literal, 50, 1, decode_shell_reference_request_value);
        bad_word(&literal, 32, 0, decode_shell_reference_request_value);
    }
    for (operation, tag) in [
        (ShellLauncherOperation::Open, 0),
        (ShellLauncherOperation::Query, 1),
        (ShellLauncherOperation::Next, 2),
        (ShellLauncherOperation::Previous, 3),
        (ShellLauncherOperation::Dismiss, 4),
    ] {
        let value = ShellLauncherRequest {
            connection_epoch: 1,
            catalog_generation: 2,
            request_generation: 3,
            output: OutputId::from_raw(4),
            output_generation: 5,
            presentation_epoch: 0,
            operation,
            query: "é".into(),
        };
        let mut literal = prefix(312, &[1, 2, 3, 4, 5, 0]);
        word(&mut literal, 48, tag);
        word(&mut literal, 52, 2);
        literal[56..58].copy_from_slice("é".as_bytes());
        check(
            &value,
            &literal,
            encode_shell_launcher_request_value,
            decode_shell_launcher_request_value,
        );
        for (at, n) in [(48, 5), (50, 1), (52, 257), (54, 1), (58, 1)] {
            bad_word(&literal, at, n, decode_shell_launcher_request_value);
        }
        let mut corrupt = literal;
        corrupt[56] = 0xff;
        assert!(decode_shell_launcher_request_value(&corrupt).is_err());
        let mut max = value;
        max.query = "q".repeat(256);
        assert_eq!(
            decode_shell_launcher_request_value(
                &encode_shell_launcher_request_value(&max).unwrap()
            )
            .unwrap(),
            max
        );
        max.query.push('q');
        assert!(encode_shell_launcher_request_value(&max).is_err());
    }
}

#[test]
fn reference_and_launcher_outcomes_keep_their_distinct_epoch_rules() {
    for (kind, tag) in [
        (ShellV1CandidateOutcomeKind::Prepared, 1),
        (ShellV1CandidateOutcomeKind::Presented, 2),
        (ShellV1CandidateOutcomeKind::Rejected, 3),
        (ShellV1CandidateOutcomeKind::Superseded, 4),
    ] {
        let reference = ShellReferenceOutcome {
            connection_epoch: 1,
            catalog_generation: 2,
            request_generation: 3,
            candidate_generation: 4,
            presentation_epoch: 5,
            page: 6,
            pages: 7,
            kind,
        };
        let mut literal = prefix(48, &[1, 2, 3, 4, 5]);
        word(&mut literal, 40, 6);
        word(&mut literal, 42, 7);
        word(&mut literal, 44, tag);
        check(
            &reference,
            &literal,
            encode_shell_reference_outcome_value,
            decode_shell_reference_outcome_value,
        );
        for (at, n) in [(40, 7), (42, 0), (44, 0), (44, 5), (46, 1)] {
            bad_word(&literal, at, n, decode_shell_reference_outcome_value);
        }
        let launcher = ShellLauncherOutcome {
            connection_epoch: 1,
            request_generation: 2,
            candidate_generation: 3,
            presentation_epoch: 4,
            kind,
        };
        let mut literal = prefix(36, &[1, 2, 3, 4]);
        word(&mut literal, 32, tag);
        check(
            &launcher,
            &literal,
            encode_shell_launcher_outcome_value,
            decode_shell_launcher_outcome_value,
        );
        for (at, n) in [(32, 0), (32, 5), (34, 1)] {
            bad_word(&literal, at, n, decode_shell_launcher_outcome_value);
        }
        let mut zero = launcher;
        zero.presentation_epoch = 0;
        assert_eq!(encode_shell_launcher_outcome_value(&zero).is_ok(), tag != 2);
        wide(&mut literal, 24, 0);
        assert_eq!(
            decode_shell_launcher_outcome_value(&literal).is_ok(),
            tag != 2
        );
    }
}

#[test]
fn launcher_event_ack_and_launch_status_share_identity_but_not_the_last_word() {
    let activation = ShellLauncherActivation {
        connection_epoch: 1,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        presentation_epoch: 5,
        activation: 6,
        slot: 4096,
    };
    let mut literal = prefix(52, &[1, 2, 3, 4, 5, 6]);
    word(&mut literal, 48, 4096);
    check(
        &activation,
        &literal,
        encode_shell_launcher_activation_value,
        decode_shell_launcher_activation_value,
    );
    bad_word(&literal, 50, 1, decode_shell_launcher_activation_value);
    for consumed in [false, true] {
        let value = ShellLauncherActivationAck {
            activation,
            consumed,
        };
        word(&mut literal, 50, u16::from(consumed));
        check(
            &value,
            &literal,
            encode_shell_launcher_ack_value,
            decode_shell_launcher_ack_value,
        );
        bad_word(&literal, 50, 2, decode_shell_launcher_ack_value);
        for at in [0, 8, 16, 24, 32, 40, 48] {
            bad_word(&literal, at, 0, decode_shell_launcher_ack_value);
        }
        bad_word(&literal, 48, 4097, decode_shell_launcher_ack_value);
        let mut invalid = value;
        invalid.activation.activation = 0;
        assert!(encode_shell_launcher_ack_value(&invalid).is_err());
    }
    for (status, tag) in [
        (ShellLaunchStatus::Started, 1),
        (ShellLaunchStatus::Rejected, 2),
        (ShellLaunchStatus::Failed, 3),
    ] {
        let value = ShellLaunchOutcome { activation, status };
        word(&mut literal, 50, tag);
        check(
            &value,
            &literal,
            encode_shell_launch_outcome_value,
            decode_shell_launch_outcome_value,
        );
        for n in [0, 4] {
            bad_word(&literal, 50, n, decode_shell_launch_outcome_value);
        }
        bad_word(&literal, 40, 0, decode_shell_launch_outcome_value);
    }
}
