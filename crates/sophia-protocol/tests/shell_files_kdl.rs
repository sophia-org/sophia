//! Pins `protocol/sophia-shell-files-v1.kdl` to the shell file codec
//! (`sophia_protocol::shell_files` and the neutral content value codec it
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

#[path = "support/shell_files_kdl/mod.rs"]
mod support;

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use std::collections::BTreeMap;
use support::checks::{checked, header_expected, transaction_body, verify_block, verify_rows};
use support::kdl_model::{all_shell_file_kinds, find_block, kind_class, kind_name, parse_kdl};
use support::{fixtures, fixtures_limits, fixtures_tables};

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
