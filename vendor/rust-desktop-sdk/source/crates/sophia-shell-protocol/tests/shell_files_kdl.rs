//! Pins `protocol/sophia-shell-files-v1.kdl` to the shell file codec
//! (`sophia_shell_protocol::shell_files` and the neutral content value codec it
//! wraps). The KDL is meant to be a self-contained byte specification: a
//! client author must be able to implement every record from it alone.
//!
//! [`support`] parses the KDL and builds every fixture; this file just
//! encodes each fixture with the real public encoder and checks the result:
//! the encoded bytes at every KDL field's declared offset and type must
//! equal the fixture's own field value, the declared `size=` must match the
//! real encoded length, `value=0` fields must be zero, the fixed part must
//! have no undeclared gap or overlap, and every kind the codec knows must
//! have a KDL body. A wrong offset or type in the KDL fails a test because
//! the bytes read at that declared location will not equal the fixture's
//! real field value.
//!
//! The unpublished descriptor implementation additionally parses the exact
//! `descriptor-files-proposal.kdl` fragment. Its declaration and structural
//! checks run here; literal values and file identities have separate suites.

#[path = "support/shell_files_kdl/mod.rs"]
mod support;

use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::collections::BTreeMap;
use support::checks::{
    checked, header_expected, transaction_body, verify_block, verify_rows, verify_text_field,
};
use support::kdl_model::{all_shell_file_kinds, find_block, kind_class, kind_name, parse_kdl};
use support::{
    fixtures, fixtures_catalog, fixtures_indicators, fixtures_limits, fixtures_native_launcher,
    fixtures_tables,
};

fn text_field<'a>(
    block: &'a support::kdl_model::Block,
    name: &str,
) -> &'a support::kdl_model::FieldSpec {
    block
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field named `{name}` in `{}`", block.name))
}

// ---------------------------------------------------------------------
// Structural tests: every block is gap/overlap free, and every codec kind
// has a KDL body.
// ---------------------------------------------------------------------

#[test]
fn every_block_has_no_gap_or_overlap_in_its_fixed_part() {
    let kdl = parse_kdl();
    support::checks::assert_no_gaps_or_overlaps("header", kdl.header.size, &kdl.header.fields);
    support::checks::assert_no_gaps_or_overlaps("submit", kdl.submit.size, &kdl.submit.fields);
    support::checks::assert_no_gaps_or_overlaps("ack", kdl.ack.size, &kdl.ack.fields);
    for block in kdl
        .bodies
        .iter()
        .chain(kdl.prefixes.iter())
        .chain(kdl.rows.iter())
    {
        support::checks::assert_no_gaps_or_overlaps(
            &format!("{} \"{}\"", block.keyword, block.name),
            block.size,
            &block.fields,
        );
    }
}

#[test]
fn every_shell_file_kind_has_a_kdl_declaration_and_a_body() {
    let kdl = parse_kdl();
    for kind in all_shell_file_kinds() {
        let name = kind_name(kind);
        let class = kind_class(kind);
        let declared = kdl
            .kinds
            .iter()
            .find(|(_, decl_name, _)| decl_name == name)
            .unwrap_or_else(|| panic!("the KDL declares no object/event/candidate named `{name}`"));
        assert_eq!(
            declared.0, class,
            "`{name}` is declared as `{}` in the KDL but the codec classifies it as `{class}`",
            declared.0
        );
        assert_eq!(
            declared.2, kind as u16,
            "`{name}` is declared with kind={} in the KDL but the codec's kind is {}",
            declared.2, kind as u16
        );
        assert!(
            kdl.bodies.iter().any(|b| b.name == name)
                || kdl.prefixes.iter().any(|b| b.name == name),
            "`{name}` has no `body` or `body-prefix` in the KDL"
        );
    }
    for (_, name, _) in &kdl.kinds {
        assert!(
            all_shell_file_kinds().iter().any(|k| kind_name(*k) == name),
            "the KDL declares kind `{name}`, which is not a `ShellFileKind` variant"
        );
    }
}

// ---------------------------------------------------------------------
// header / submit / ack
// ---------------------------------------------------------------------

#[test]
fn header_fields_match_encoded_bytes() {
    let kdl = parse_kdl();
    let cases: [(ShellFileKind, u64, u64, u64, usize); 3] = [
        (ShellFileKind::Outputs, 0x1122_3344, 0, 0, 5),
        (ShellFileKind::Negotiate, 0x5566_7788, 0x99AA_BBCC, 0, 9),
        (ShellFileKind::Refused, 0x1020_3040, 0, 0xA1B2_C3D4, 13),
    ];
    for (kind, connection_epoch, submission_id, sequence, body_len) in cases {
        let header = ShellFileHeader {
            kind,
            connection_epoch,
            submission_id,
            sequence,
        };
        let encoded = encode_shell_file_record(header, &vec![0u8; body_len]).unwrap();
        let expected = header_expected(header, encoded.len() as u64);
        verify_block(&encoded, 0, "header", &kdl.header.fields, &expected);
    }
}

#[test]
fn submit_and_ack_fields_match_encoded_bytes() {
    let kdl = parse_kdl();
    let connection_epoch = 0x1111_2222u64;
    let submission_id = 0x3333_4444u64;
    let candidate_bytes = 0x5555u32;
    let submit = ShellFileSubmit {
        connection_epoch,
        submission_id,
        candidate_bytes,
    };
    let encoded_submit = encode_shell_file_submit(submit).unwrap();
    let expected_submit = BTreeMap::from([
        ("connection_epoch", connection_epoch as i128),
        ("submission_id", submission_id as i128),
        ("candidate_bytes", candidate_bytes as i128),
    ]);
    checked(encoded_submit, &kdl.submit, "submit", &expected_submit);

    let connection_epoch = 0x6666_7777u64;
    let sequence = 0x8888_9999u64;
    let ack = ShellFileAck {
        connection_epoch,
        sequence,
    };
    let encoded_ack = encode_shell_file_ack(ack).unwrap();
    let expected_ack = BTreeMap::from([
        ("connection_epoch", connection_epoch as i128),
        ("sequence", sequence as i128),
    ]);
    checked(encoded_ack, &kdl.ack, "ack", &expected_ack);
}

// ---------------------------------------------------------------------
// Negotiate / Negotiated / Refused / Submitted / ObjectPublished
// ---------------------------------------------------------------------

#[test]
fn negotiate_body_matches_kdl() {
    let kdl = parse_kdl();
    let header = ShellFileHeader {
        kind: ShellFileKind::Negotiate,
        connection_epoch: 42,
        submission_id: 43,
        sequence: 0,
    };
    let (hello, expected) = fixtures::negotiate();
    let encoded = encode_shell_file_negotiate(header, hello).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "Negotiate"),
        "Negotiate",
        &expected,
    );
}

#[test]
fn negotiated_body_matches_kdl() {
    let kdl = parse_kdl();
    let (value, expected) = fixtures::negotiated();
    let body = encode_shell_file_negotiated_body(value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "Negotiated"),
        "Negotiated",
        &expected,
    );
}

#[test]
fn refused_body_matches_kdl() {
    let kdl = parse_kdl();
    let (value, expected) = fixtures::refused();
    let body = encode_shell_file_refused_body(&value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "Refused"),
        "Refused",
        &expected,
    );
}

#[test]
fn submitted_body_matches_kdl() {
    let kdl = parse_kdl();
    let (value, expected) = fixtures::submitted();
    let body = encode_shell_file_submitted_body(value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "Submitted"),
        "Submitted",
        &expected,
    );
}

#[test]
fn object_published_body_matches_kdl() {
    let kdl = parse_kdl();
    let (value, expected) = fixtures::object_published();
    let body = encode_shell_file_object_published_body(value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ObjectPublished"),
        "ObjectPublished",
        &expected,
    );
}

// ---------------------------------------------------------------------
// Limits / Outputs
// ---------------------------------------------------------------------

#[test]
fn limits_body_matches_kdl() {
    let kdl = parse_kdl();
    let (limits, expected) = fixtures_limits::limits();
    limits
        .validate()
        .expect("fixture must be a valid ContentLimits");
    let header = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    let encoded = encode_shell_file_limits(header, limits).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(body, find_block(&kdl.bodies, "Limits"), "Limits", &expected);
}

#[test]
fn outputs_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "Outputs");
    let row_block = find_block(&kdl.rows, "ContentOutputFactsEntry");
    let (transaction, record, expected_prefix, rows_expected) = fixtures_tables::outputs();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::OutputFacts(record),
    };
    let body = encode_shell_file_outputs_body(&tx_record).unwrap();
    assert_eq!(
        body.len(),
        prefix.size + rows_expected.len() * row_block.size
    );
    verify_block(&body, 0, "Outputs", &prefix.fields, &expected_prefix);
    verify_rows(
        &body,
        prefix.size,
        row_block.size,
        "Outputs.ContentOutputFactsEntry",
        &row_block.fields,
        &rows_expected,
    );
}

// ---------------------------------------------------------------------
// AllocationRequest / AllocationResult
// ---------------------------------------------------------------------

#[test]
fn allocation_request_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::allocation_request();
    let header = ShellFileHeader {
        kind: ShellFileKind::AllocationRequest,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::AllocationRequest(record),
    };
    let encoded = encode_shell_file_allocation_request(header, tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "AllocationRequest"),
        "AllocationRequest",
        &expected,
    );
}

#[test]
fn allocation_result_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::allocation_result();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::AllocationResult(record),
    };
    let body = encode_shell_file_allocation_result_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "AllocationResult"),
        "AllocationResult",
        &expected,
    );
}

// ---------------------------------------------------------------------
// ResourceBegin / ResourceEnd / ResourceCancel / ResourceRetire /
// ResourceStatus / ResourceReleased
// ---------------------------------------------------------------------

#[test]
fn resource_begin_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, slot, record, expected) = fixtures::resource_begin();
    let value = ShellFileResourceBegin {
        transaction: TransactionId::from_raw(transaction),
        slot,
        record: ShellContentRecord::ResourceBegin(record),
    };
    let body = encode_shell_file_resource_begin_body(&value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceBegin"),
        "ResourceBegin",
        &expected,
    );
}

#[test]
fn resource_end_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::resource_end();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceEnd(record),
    };
    let body = encode_shell_file_resource_end_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceEnd"),
        "ResourceEnd",
        &expected,
    );
}

#[test]
fn resource_cancel_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::resource_cancel();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceCancel(record),
    };
    let body = encode_shell_file_resource_cancel_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceCancel"),
        "ResourceCancel",
        &expected,
    );
}

#[test]
fn resource_retire_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::resource_retire();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceRetire(record),
    };
    let body = encode_shell_file_resource_retire_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceRetire"),
        "ResourceRetire",
        &expected,
    );
}

#[test]
fn resource_status_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::resource_status();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceStatus(record),
    };
    let body = encode_shell_file_resource_status_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceStatus"),
        "ResourceStatus",
        &expected,
    );
}

#[test]
fn resource_released_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::resource_released();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ResourceReleased(record),
    };
    let body = encode_shell_file_resource_released_body(&tx_record).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "ResourceReleased"),
        "ResourceReleased",
        &expected,
    );
}

// ---------------------------------------------------------------------
// FrameDemand / FramePermit / FrameDemandCancel / Action / ActionAck /
// CandidateOutcome (all single-payload transaction records)
// ---------------------------------------------------------------------

#[test]
fn frame_demand_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::frame_demand();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::FrameDemand(record),
    };
    let body = transaction_body(ShellFileKind::FrameDemand, &tx_record);
    checked(
        body,
        find_block(&kdl.bodies, "FrameDemand"),
        "FrameDemand",
        &expected,
    );
}

#[test]
fn frame_permit_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::frame_permit();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::FramePermit(record),
    };
    let body = transaction_body(ShellFileKind::FramePermit, &tx_record);
    checked(
        body,
        find_block(&kdl.bodies, "FramePermit"),
        "FramePermit",
        &expected,
    );
}

#[test]
fn frame_demand_cancel_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::frame_demand_cancel();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::FrameDemandCancel(record),
    };
    let body = transaction_body(ShellFileKind::FrameDemandCancel, &tx_record);
    checked(
        body,
        find_block(&kdl.bodies, "FrameDemandCancel"),
        "FrameDemandCancel",
        &expected,
    );
}

#[test]
fn action_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::action();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::Action(record),
    };
    let body = transaction_body(ShellFileKind::Action, &tx_record);
    checked(body, find_block(&kdl.bodies, "Action"), "Action", &expected);
}

#[test]
fn action_ack_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::action_ack();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::ActionAck(record),
    };
    let body = transaction_body(ShellFileKind::ActionAck, &tx_record);
    checked(
        body,
        find_block(&kdl.bodies, "ActionAck"),
        "ActionAck",
        &expected,
    );
}

#[test]
fn candidate_outcome_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, record, expected) = fixtures::candidate_outcome();
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellContentRecord::CandidateOutcome(record),
    };
    let body = transaction_body(ShellFileKind::CandidateOutcome, &tx_record);
    checked(
        body,
        find_block(&kdl.bodies, "CandidateOutcome"),
        "CandidateOutcome",
        &expected,
    );
}

// ---------------------------------------------------------------------
// Candidate (prefix + surface/placement/target rows)
// ---------------------------------------------------------------------

#[test]
fn candidate_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "Candidate");
    let surface_row_block = find_block(&kdl.rows, "ContentSurface");
    let placement_row_block = find_block(&kdl.rows, "ContentPlacement");
    let target_row_block = find_block(&kdl.rows, "ContentTarget");

    let (
        transaction,
        candidate,
        expected_prefix,
        surfaces_expected,
        placements_expected,
        targets_expected,
    ) = fixtures_tables::candidate();

    let header = ShellFileHeader {
        kind: ShellFileKind::Candidate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let value = ShellFileCandidate {
        transaction: TransactionId::from_raw(transaction),
        candidate,
    };
    let encoded = encode_shell_file_candidate(header, &value).unwrap();
    let body = &encoded[SHELL_FILE_HEADER_BYTES..];

    let expected_size = prefix.size
        + surfaces_expected.len() * surface_row_block.size
        + placements_expected.len() * placement_row_block.size
        + targets_expected.len() * target_row_block.size;
    assert_eq!(body.len(), expected_size);
    verify_block(body, 0, "Candidate", &prefix.fields, &expected_prefix);

    verify_rows(
        body,
        prefix.size,
        surface_row_block.size,
        "Candidate.ContentSurface",
        &surface_row_block.fields,
        &surfaces_expected,
    );
    let placements_base = prefix.size + surfaces_expected.len() * surface_row_block.size;
    verify_rows(
        body,
        placements_base,
        placement_row_block.size,
        "Candidate.ContentPlacement",
        &placement_row_block.fields,
        &placements_expected,
    );
    let targets_base = placements_base + placements_expected.len() * placement_row_block.size;
    verify_rows(
        body,
        targets_base,
        target_row_block.size,
        "Candidate.ContentTarget",
        &target_row_block.fields,
        &targets_expected,
    );
}

// ---------------------------------------------------------------------
// t252 B5: native launcher (r7)
// ---------------------------------------------------------------------

fn native_launcher_header(
    kind: ShellFileKind,
    submission_id: u64,
    sequence: u64,
) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id,
        sequence,
    }
}

#[test]
fn native_opening_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_opening();
    let header = native_launcher_header(ShellFileKind::NativeOpening, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::Opening(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeOpening"),
        "NativeOpening",
        &expected,
    );
}

#[test]
fn native_focus_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_focus();
    let header = native_launcher_header(ShellFileKind::NativeFocus, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::Focus(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeFocus"),
        "NativeFocus",
        &expected,
    );
}

#[test]
fn native_focus_revoked_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_focus_revoked();
    let header = native_launcher_header(ShellFileKind::NativeFocusRevoked, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::FocusRevoked(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeFocusRevoked"),
        "NativeFocusRevoked",
        &expected,
    );
}

#[test]
fn native_input_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_input();
    let header = native_launcher_header(ShellFileKind::NativeInput, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::Input(value.clone()),
    };
    let encoded = encode_shell_file_native_input(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    let block = find_block(&kdl.bodies, "NativeInput");
    assert_eq!(body.len(), block.size);
    verify_block(&body, 0, "NativeInput", &block.fields, &expected);
    verify_text_field(
        &body,
        0,
        "NativeInput",
        text_field(block, "text"),
        &value.text,
    );
}

#[test]
fn native_activation_outcome_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_activation_outcome();
    let header = native_launcher_header(ShellFileKind::NativeActivationOutcome, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::ActivationOutcome(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeActivationOutcome"),
        "NativeActivationOutcome",
        &expected,
    );
}

#[test]
fn native_closed_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_closed();
    let header = native_launcher_header(ShellFileKind::NativeClosed, 0, 1);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::Closed(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeClosed"),
        "NativeClosed",
        &expected,
    );
}

#[test]
fn native_allocation_request_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_allocation_request();
    let header = native_launcher_header(ShellFileKind::NativeAllocationRequest, 1, 0);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::AllocationRequest(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeAllocationRequest"),
        "NativeAllocationRequest",
        &expected,
    );
}

#[test]
fn native_input_ack_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_input_ack();
    let header = native_launcher_header(ShellFileKind::NativeInputAck, 1, 0);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::InputAck(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeInputAck"),
        "NativeInputAck",
        &expected,
    );
}

#[test]
fn native_activate_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_native_launcher::native_activate();
    let header = native_launcher_header(ShellFileKind::NativeActivate, 1, 0);
    let tx_record = ShellFileNativeLauncherRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellNativeLauncherRecord::Activate(value),
    };
    let encoded = encode_shell_file_native_launcher_transaction(header, &tx_record).unwrap();
    let body = encoded[SHELL_FILE_HEADER_BYTES..].to_vec();
    checked(
        body,
        find_block(&kdl.bodies, "NativeActivate"),
        "NativeActivate",
        &expected,
    );
}

#[test]
fn native_candidate_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "NativeCandidate");
    let surface_row = find_block(&kdl.rows, "NativeSurface");
    let placement_row = find_block(&kdl.rows, "NativePlacement");
    let target_row = find_block(&kdl.rows, "NativeTarget");
    let slot_row = find_block(&kdl.rows, "NativeCandidateRow");

    let (
        transaction,
        candidate,
        prefix_expected,
        surface_expected,
        placement_expected,
        target_expected,
        row_expected,
    ) = fixtures_native_launcher::native_candidate();

    let header = native_launcher_header(ShellFileKind::NativeCandidate, 1, 0);
    let value = ShellFileNativeCandidate {
        transaction: TransactionId::from_raw(transaction),
        candidate,
    };
    let encoded = encode_shell_file_native_candidate(header, &value).unwrap();
    let body = &encoded[SHELL_FILE_HEADER_BYTES..];

    let expected_size =
        prefix.size + surface_row.size + placement_row.size + target_row.size + slot_row.size;
    assert_eq!(body.len(), expected_size);
    verify_block(body, 0, "NativeCandidate", &prefix.fields, &prefix_expected);

    verify_rows(
        body,
        prefix.size,
        surface_row.size,
        "NativeCandidate.NativeSurface",
        &surface_row.fields,
        &[surface_expected],
    );
    let placements_base = prefix.size + surface_row.size;
    verify_rows(
        body,
        placements_base,
        placement_row.size,
        "NativeCandidate.NativePlacement",
        &placement_row.fields,
        &[placement_expected],
    );
    let targets_base = placements_base + placement_row.size;
    verify_rows(
        body,
        targets_base,
        target_row.size,
        "NativeCandidate.NativeTarget",
        &target_row.fields,
        &[target_expected],
    );
    let rows_base = targets_base + target_row.size;
    verify_rows(
        body,
        rows_base,
        slot_row.size,
        "NativeCandidate.NativeCandidateRow",
        &slot_row.fields,
        &[row_expected],
    );
}

// ---------------------------------------------------------------------
// t252 B5: persistent catalog (r8)
// ---------------------------------------------------------------------

#[test]
fn catalog_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "Catalog");
    let row_block = find_block(&kdl.rows, "CatalogEntry");
    let (transaction, catalog, expected_prefix, rows_expected) = fixtures_catalog::catalog();
    let value = ShellFileCatalog {
        transaction: TransactionId::from_raw(transaction),
        catalog,
    };
    let body = encode_shell_file_catalog_body(&value).unwrap();
    assert_eq!(
        body.len(),
        prefix.size + rows_expected.len() * row_block.size
    );
    verify_block(&body, 0, "Catalog", &prefix.fields, &expected_prefix);
    verify_rows(
        &body,
        prefix.size,
        row_block.size,
        "Catalog.CatalogEntry",
        &row_block.fields,
        &rows_expected,
    );

    let label_field = text_field(row_block, "label");
    let keywords_field = text_field(row_block, "keywords");
    let identity_field = text_field(row_block, "identity");
    for (index, (label, keywords, identity)) in
        fixtures_catalog::catalog_row_text().into_iter().enumerate()
    {
        let base = prefix.size + index * row_block.size;
        verify_text_field(&body, base, "Catalog.CatalogEntry", label_field, label);
        verify_text_field(
            &body,
            base,
            "Catalog.CatalogEntry",
            keywords_field,
            keywords,
        );
        verify_text_field(
            &body,
            base,
            "Catalog.CatalogEntry",
            identity_field,
            identity,
        );
    }
}

#[test]
fn catalog_activation_outcome_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_catalog::catalog_activation_outcome();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellCatalogActionRecord::ActivationOutcome(value),
    };
    let (kind, body) = encode_shell_file_catalog_action_body(&tx_record).unwrap();
    assert_eq!(kind, ShellFileKind::CatalogActivationOutcome);
    checked(
        body,
        find_block(&kdl.bodies, "CatalogActivationOutcome"),
        "CatalogActivationOutcome",
        &expected,
    );
}

#[test]
fn catalog_activate_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, value, expected) = fixtures_catalog::catalog_activate();
    let tx_record = ShellFileCatalogActionRecord {
        transaction: TransactionId::from_raw(transaction),
        record: ShellCatalogActionRecord::Activate(value),
    };
    let (kind, body) = encode_shell_file_catalog_action_body(&tx_record).unwrap();
    assert_eq!(kind, ShellFileKind::CatalogActivate);
    checked(
        body,
        find_block(&kdl.bodies, "CatalogActivate"),
        "CatalogActivate",
        &expected,
    );
}

#[test]
fn catalog_candidate_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "CatalogCandidate");
    let surface_row = find_block(&kdl.rows, "ContentSurface");
    let placement_row = find_block(&kdl.rows, "ContentPlacement");
    let target_row = find_block(&kdl.rows, "CatalogTarget");

    let (
        transaction,
        candidate,
        prefix_expected,
        surfaces_expected,
        placements_expected,
        targets_expected,
    ) = fixtures_catalog::catalog_candidate();

    let header = ShellFileHeader {
        kind: ShellFileKind::CatalogCandidate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let value = ShellFileCatalogCandidate {
        transaction: TransactionId::from_raw(transaction),
        candidate,
    };
    let encoded = encode_shell_file_catalog_candidate(header, &value).unwrap();
    let body = &encoded[SHELL_FILE_HEADER_BYTES..];

    let expected_size = prefix.size
        + surfaces_expected.len() * surface_row.size
        + placements_expected.len() * placement_row.size
        + targets_expected.len() * target_row.size;
    assert_eq!(body.len(), expected_size);
    verify_block(
        body,
        0,
        "CatalogCandidate",
        &prefix.fields,
        &prefix_expected,
    );

    verify_rows(
        body,
        prefix.size,
        surface_row.size,
        "CatalogCandidate.ContentSurface",
        &surface_row.fields,
        &surfaces_expected,
    );
    let placements_base = prefix.size + surfaces_expected.len() * surface_row.size;
    verify_rows(
        body,
        placements_base,
        placement_row.size,
        "CatalogCandidate.ContentPlacement",
        &placement_row.fields,
        &placements_expected,
    );
    let targets_base = placements_base + placements_expected.len() * placement_row.size;
    verify_rows(
        body,
        targets_base,
        target_row.size,
        "CatalogCandidate.CatalogTarget",
        &target_row.fields,
        &targets_expected,
    );
}

// ---------------------------------------------------------------------
// t252 B5: view indicators (r6)
// ---------------------------------------------------------------------

#[test]
fn indicators_body_and_rows_match_kdl() {
    let kdl = parse_kdl();
    let prefix = find_block(&kdl.prefixes, "Indicators");
    let status_row = find_block(&kdl.rows, "IndicatorOutputStatus");
    let entry_row = find_block(&kdl.rows, "IndicatorEntry");

    let (transaction, snapshot, expected_prefix, statuses_expected, entries_expected) =
        fixtures_indicators::indicators();
    let value = ShellFileIndicators {
        transaction: TransactionId::from_raw(transaction),
        snapshot,
    };
    let body = encode_shell_file_indicators_body(&value).unwrap();
    let expected_size = prefix.size
        + statuses_expected.len() * status_row.size
        + entries_expected.len() * entry_row.size;
    assert_eq!(body.len(), expected_size);
    verify_block(&body, 0, "Indicators", &prefix.fields, &expected_prefix);
    verify_rows(
        &body,
        prefix.size,
        status_row.size,
        "Indicators.IndicatorOutputStatus",
        &status_row.fields,
        &statuses_expected,
    );
    let entries_base = prefix.size + statuses_expected.len() * status_row.size;
    verify_rows(
        &body,
        entries_base,
        entry_row.size,
        "Indicators.IndicatorEntry",
        &entry_row.fields,
        &entries_expected,
    );

    let layout_field = text_field(status_row, "layout");
    for (index, layout) in fixtures_indicators::indicators_status_text()
        .into_iter()
        .enumerate()
    {
        verify_text_field(
            &body,
            prefix.size + index * status_row.size,
            "Indicators.IndicatorOutputStatus",
            layout_field,
            layout,
        );
    }
    let label_field = text_field(entry_row, "label");
    for (index, label) in fixtures_indicators::indicators_entry_text()
        .into_iter()
        .enumerate()
    {
        verify_text_field(
            &body,
            entries_base + index * entry_row.size,
            "Indicators.IndicatorEntry",
            label_field,
            label,
        );
    }
}

#[test]
fn indicator_activation_outcome_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, outcome, expected) = fixtures_indicators::indicator_activation_outcome();
    let value = ShellFileIndicatorActivationOutcome {
        transaction: TransactionId::from_raw(transaction),
        outcome,
    };
    let body = encode_shell_file_indicator_activation_outcome_body(&value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "IndicatorActivationOutcome"),
        "IndicatorActivationOutcome",
        &expected,
    );
}

#[test]
fn indicator_activate_body_matches_kdl() {
    let kdl = parse_kdl();
    let (transaction, activation, expected) = fixtures_indicators::indicator_activate();
    let value = ShellFileIndicatorActivate {
        transaction: TransactionId::from_raw(transaction),
        activation,
    };
    let body = encode_shell_file_indicator_activate_body(&value).unwrap();
    checked(
        body,
        find_block(&kdl.bodies, "IndicatorActivate"),
        "IndicatorActivate",
        &expected,
    );
}
