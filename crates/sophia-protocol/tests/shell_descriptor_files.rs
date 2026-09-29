//! Semantic assertions from protocol/shell_v1.rs, carried by descriptor
//! file records. Socket Hello/Welcome and unknown message-kind checks retire;
//! file negotiation is exercised against the production export separately.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
#[path = "support/descriptor_file.rs"]
mod file;
#[path = "support/descriptor_fixture.rs"]
mod fixture;

fn candidate() -> ShellV1Candidate {
    ShellV1Candidate {
        connection_epoch: 5,
        snapshot_generation: 6,
        candidate_generation: 1,
        output: OutputId::from_raw(7),
        visible: true,
        selected_slot: Some(2),
        reservation: None,
        entries: vec![
            ShellV1CandidateEntry {
                slot: 2,
                generation: 10,
            },
            ShellV1CandidateEntry {
                slot: 1,
                generation: 9,
            },
        ],
    }
}

#[test]
fn complete_descriptor_lifecycle_round_trips() {
    file::round_trip(ShellDescriptorRecord::Descriptors(fixture::snapshot()));
    file::round_trip(ShellDescriptorRecord::DescriptorCandidate(candidate()));
    file::round_trip(ShellDescriptorRecord::DescriptorOutcome(
        ShellV1CandidateOutcome {
            connection_epoch: 5,
            candidate_generation: 1,
            presentation_epoch: 12,
            kind: ShellV1CandidateOutcomeKind::Presented,
        },
    ));
    file::round_trip(ShellDescriptorRecord::DescriptorActivation(
        ShellV1Activation {
            connection_epoch: 5,
            candidate_generation: 1,
            presentation_epoch: 12,
            activation: 13,
            action: fixture::action(2, 10),
        },
    ));
    file::round_trip(ShellDescriptorRecord::DescriptorActivationAck(
        ShellV1ActivationAck {
            connection_epoch: 5,
            activation: 13,
            disposition: ShellV1ActivationDisposition::Consumed,
        },
    ));
}

#[test]
fn identity_leaks_and_torn_candidates_are_refused() {
    let mut c = candidate();
    c.entries.remove(0);
    assert!(file::encode(ShellDescriptorRecord::DescriptorCandidate(c.clone())).is_err());
    c.visible = false;
    c.selected_slot = None;
    assert!(file::encode(ShellDescriptorRecord::DescriptorCandidate(c)).is_err());
    let mut stale = fixture::snapshot();
    stale.descriptors[0].action.recipient_epoch = 4;
    assert!(file::encode(ShellDescriptorRecord::Descriptors(stale)).is_err());
    let mut duplicate = fixture::snapshot();
    duplicate.descriptors[1].slot = 1;
    duplicate.descriptors[1].action.target_slot = 1;
    assert!(file::encode(ShellDescriptorRecord::Descriptors(duplicate)).is_err());
}

#[test]
fn reservation_and_reserved_fields_remain_strict() {
    let hidden = ShellV1Candidate {
        visible: false,
        selected_slot: None,
        entries: Vec::new(),
        ..candidate()
    };
    let mut bytes = file::encode(ShellDescriptorRecord::DescriptorCandidate(hidden)).unwrap();
    // The native value uses u16 visibility and edge, after four u64 fields.
    let value = SHELL_FILE_HEADER_BYTES + 8;
    bytes[value + 34] = 1;
    assert!(decode_shell_file_descriptor(&bytes, ShellFileKind::DescriptorCandidate).is_err());
    let mut bytes = file::encode(ShellDescriptorRecord::DescriptorCandidate(candidate())).unwrap();
    // 44-byte candidate prefix, then slot, reserved u16, generation.
    bytes[value + 44 + 2] = 1;
    assert!(decode_shell_file_descriptor(&bytes, ShellFileKind::DescriptorCandidate).is_err());
}

#[test]
fn file_identity_fields_cannot_replace_the_domain_transaction() {
    let kind = ShellFileKind::DescriptorCandidate;
    let record = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(11),
        record: ShellDescriptorRecord::DescriptorCandidate(candidate()),
    };
    let mut header = file::header(kind);
    header.connection_epoch = 4;
    assert!(encode_shell_file_descriptor(header, &record).is_err());
    let mut no_transaction = record.clone();
    no_transaction.transaction = TransactionId::INVALID;
    assert!(encode_shell_file_descriptor(file::header(kind), &no_transaction).is_err());
    let bytes = file::encode(record.record).unwrap();
    assert!(decode_shell_file_descriptor(&bytes, ShellFileKind::TabsCandidate).is_err());
    let mut no_transaction = bytes.clone();
    no_transaction[SHELL_FILE_HEADER_BYTES..SHELL_FILE_HEADER_BYTES + 8].fill(0);
    assert!(decode_shell_file_descriptor(&no_transaction, kind).is_err());
    let mut stale = bytes;
    stale[SHELL_FILE_HEADER_BYTES + 8] = 4;
    assert!(decode_shell_file_descriptor(&stale, kind).is_err());
}
