//! The portal file KDL is the requester's layout authority. These checks bind
//! it to the native codec: every block covers its bytes at the native size,
//! every declared constraint is refused when a real encoded record violates
//! it, and fields without a declared rule still decode so the owner judges
//! them.
#[path = "support/file_schema.rs"]
#[allow(dead_code)] // This fixed-size schema has no array rows or tail sections.
mod file_schema;

use file_schema::*;
use sophia_protocol::portal_files::*;
use sophia_protocol::{
    NamespaceId, PortalBrokerResponseDecision, PortalBrokerResponsePacket, PortalDecision,
    PortalGrant, PortalGrantState, PortalRequest, PortalTransfer, PortalTransferId,
    PortalTransferKind,
};

const SCHEMA: &str = include_str!("../../../protocol/sophia-portal-files-v1.kdl");
const EPOCH: u64 = 9;

type Decode = fn(&[u8]) -> bool;

struct Sample {
    name: &'static str,
    bytes: Vec<u8>,
    decode: Decode,
    blocks: Vec<(String, usize)>,
}

fn request() -> PortalRequest {
    PortalRequest {
        transfer: PortalTransfer {
            transfer: PortalTransferId::from_raw(41),
            source_namespace: NamespaceId::from_raw(2),
            target_namespace: NamespaceId::from_raw(3),
            kind: PortalTransferKind::Clipboard,
            mime_type: Some("text/plain;charset=utf-8".into()),
            byte_size: 12,
            decision: PortalDecision::Pending,
            generation: 5,
        },
        deadline_msec: 9_000,
    }
}

fn grant() -> PortalBrokerResponsePacket {
    PortalBrokerResponsePacket {
        transfer: PortalTransferId::from_raw(41),
        decision: PortalBrokerResponseDecision::Allowed(PortalGrant {
            transfer: PortalTransferId::from_raw(41),
            source_namespace: NamespaceId::from_raw(2),
            target_namespace: NamespaceId::from_raw(3),
            kind: PortalTransferKind::UriOpen,
            source_generation: 5,
            broker_generation: 7,
            deadline_msec: 9_000,
            state: PortalGrantState::Active,
        }),
    }
}

fn candidate(value: PortalFileCandidate) -> Vec<u8> {
    encode_portal_file_candidate(EPOCH, 5, &value).unwrap()
}

fn event(value: PortalFileEvent) -> Vec<u8> {
    encode_portal_file_event(EPOCH, 3, &value).unwrap()
}

fn samples() -> Vec<Sample> {
    let record = |name: &'static str, bytes: Vec<u8>, decode: Decode| Sample {
        name,
        bytes,
        decode,
        blocks: vec![("header".into(), 0), (name.into(), 32)],
    };
    let candidates: Decode = |bytes| decode_portal_file_candidate(bytes, EPOCH).is_ok();
    let events: Decode = |bytes| decode_portal_file_event(bytes, EPOCH).is_ok();
    let transfer = PortalTransferId::from_raw(41);
    vec![
        record(
            "Limits",
            encode_portal_file_limits(EPOCH, &PortalFileLimits::defaults(7)).unwrap(),
            |bytes| decode_portal_file_limits(bytes, EPOCH).is_ok(),
        ),
        record("Negotiated", event(PortalFileEvent::Negotiated), events),
        record("Refused", event(PortalFileEvent::Refused), events),
        record(
            "Submitted",
            event(PortalFileEvent::Submitted(PortalSubmitted {
                submission_id: 5,
                candidate_kind: PortalFileKind::PayloadEnd,
            })),
            events,
        ),
        record(
            "Decision",
            event(PortalFileEvent::Decision(grant())),
            events,
        ),
        record(
            "TransferOutcome",
            event(PortalFileEvent::TransferOutcome(PortalTransferOutcome {
                transfer,
                state: PortalGrantState::Expired,
            })),
            events,
        ),
        record(
            "Negotiate",
            candidate(PortalFileCandidate::Negotiate(PortalNegotiate {
                minimum_revision: 1,
                maximum_revision: 1,
            })),
            candidates,
        ),
        record(
            "TransferRequest",
            candidate(PortalFileCandidate::TransferRequest(request())),
            candidates,
        ),
        record(
            "PayloadBegin",
            candidate(PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
                transfer,
                total_bytes: 12,
            })),
            candidates,
        ),
        record(
            "PayloadEnd",
            candidate(PortalFileCandidate::PayloadEnd(PortalPayloadEnd {
                transfer,
                total_bytes: 12,
            })),
            candidates,
        ),
        record(
            "TransferCancel",
            candidate(PortalFileCandidate::TransferCancel(transfer)),
            candidates,
        ),
        Sample {
            name: "submit",
            bytes: encode_portal_file_submit(PortalFileSubmit {
                connection_epoch: EPOCH,
                submission_id: 5,
                candidate_bytes: 344,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_portal_file_submit(bytes, EPOCH).is_ok(),
            blocks: vec![("submit".into(), 0)],
        },
        Sample {
            name: "ack",
            bytes: encode_portal_file_ack(PortalFileAck {
                connection_epoch: EPOCH,
                sequence: 3,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_portal_file_ack(bytes, EPOCH).is_ok(),
            blocks: vec![("ack".into(), 0)],
        },
    ]
}

fn placed(schema: &Schema, sample: &Sample) -> Vec<(String, usize, Field)> {
    sample
        .blocks
        .iter()
        .flat_map(|(block, base)| {
            schema.blocks[block]
                .1
                .iter()
                .map(move |field| (block.clone(), base + field.offset, field.clone()))
        })
        .collect()
}

#[test]
fn every_block_matches_the_native_sizes_and_kinds() {
    let schema = Schema::parse(SCHEMA);
    let native = [
        ("Limits", PortalFileKind::Limits),
        ("Negotiated", PortalFileKind::Negotiated),
        ("Refused", PortalFileKind::Refused),
        ("Submitted", PortalFileKind::Submitted),
        ("Decision", PortalFileKind::Decision),
        ("TransferOutcome", PortalFileKind::TransferOutcome),
        ("Negotiate", PortalFileKind::Negotiate),
        ("TransferRequest", PortalFileKind::TransferRequest),
        ("PayloadBegin", PortalFileKind::PayloadBegin),
        ("PayloadEnd", PortalFileKind::PayloadEnd),
        ("TransferCancel", PortalFileKind::TransferCancel),
    ];
    assert_eq!(schema.kinds.len(), native.len());
    for (name, kind) in native {
        let (class, value) = &schema.kinds[name];
        assert_eq!(*value, kind as u16, "{name}");
        assert_eq!(PortalFileKind::decode(*value), Ok(kind), "{name}");
        let expected = match kind.class() {
            PortalFileClass::Object => "object",
            PortalFileClass::Event => "event",
            PortalFileClass::Candidate => "candidate",
        };
        assert_eq!(class, expected, "{name}");
        assert_eq!(schema.size(name), kind.body_bytes(), "{name}");
    }
    assert_eq!(schema.size("header"), PORTAL_FILE_HEADER_BYTES);
    assert_eq!(schema.size("submit"), PORTAL_FILE_SUBMIT_BYTES);
    assert_eq!(schema.size("ack"), PORTAL_FILE_ACK_BYTES);
    assert_eq!(schema.size("Limits"), PortalFileLimits::BYTES);
    let max = |class: PortalFileClass| {
        native
            .iter()
            .filter(move |(_, kind)| kind.class() == class)
            .map(|(_, kind)| kind.record_bytes())
    };
    assert_eq!(
        max(PortalFileClass::Object).max(),
        Some(PORTAL_FILE_MAX_OBJECT_BYTES)
    );
    assert_eq!(
        integer(&schema.root, "max-object-bytes"),
        Some(PORTAL_FILE_MAX_OBJECT_BYTES as i128)
    );
    assert_eq!(
        max(PortalFileClass::Event).max(),
        Some(PORTAL_FILE_MAX_EVENT_BYTES)
    );
    assert_eq!(
        max(PortalFileClass::Candidate).max(),
        Some(PORTAL_FILE_MAX_CANDIDATE_BYTES)
    );
    assert_eq!(
        max(PortalFileClass::Candidate).min(),
        Some(PORTAL_FILE_MIN_CANDIDATE_BYTES)
    );
    let submit = schema.field("submit", "candidate_bytes");
    assert_eq!(submit.min, Some(PORTAL_FILE_MIN_CANDIDATE_BYTES as i128));
    assert_eq!(submit.max, Some(PORTAL_FILE_MAX_CANDIDATE_BYTES as i128));
    assert_eq!(
        integer(&schema.root, "api-version"),
        Some(i128::from(PORTAL_FILE_API_VERSION))
    );
}

#[test]
fn native_encoders_place_values_at_declared_offsets() {
    let schema = Schema::parse(SCHEMA);
    let samples = samples();
    let at = |name: &str, block: &str, field: &str| {
        let sample = samples.iter().find(|sample| sample.name == name).unwrap();
        let base = sample.blocks.iter().find(|(b, _)| b == block).unwrap().1;
        let field = schema.field(block, field);
        get(&sample.bytes, base + field.offset, field)
    };
    for sample in &samples {
        if sample.blocks[0].0 == "header" {
            assert_eq!(
                at(sample.name, "header", "total_bytes"),
                sample.bytes.len() as i128
            );
            assert_eq!(at(sample.name, "header", "connection_epoch"), 9);
        }
    }
    assert_eq!(at("Limits", "Limits", "max_retained_transfers"), 4096);
    assert_eq!(at("Limits", "Limits", "broker_generation"), 7);
    assert_eq!(at("Negotiated", "Negotiated", "connection_epoch"), 9);
    assert_eq!(at("Submitted", "Submitted", "candidate_kind"), 259);
    assert_eq!(at("Decision", "Decision", "kind"), 6);
    assert_eq!(at("Decision", "Decision", "broker_generation"), 7);
    assert_eq!(at("TransferOutcome", "TransferOutcome", "state"), 4);
    assert_eq!(at("TransferRequest", "TransferRequest", "mime_len"), 24);
    assert_eq!(
        at("TransferRequest", "TransferRequest", "deadline_msec"),
        9000
    );
    assert_eq!(
        at("TransferRequest", "TransferRequest", "target_namespace"),
        3
    );
    assert_eq!(at("PayloadBegin", "PayloadBegin", "total_bytes"), 12);
    assert_eq!(at("TransferCancel", "TransferCancel", "transfer"), 41);
    assert_eq!(at("submit", "submit", "candidate_bytes"), 344);
    assert_eq!(at("ack", "ack", "sequence"), 3);
}

#[test]
fn every_declared_constraint_is_refused_by_the_native_decoder() {
    let schema = Schema::parse(SCHEMA);
    let mut checked = 0;
    for sample in samples() {
        assert!((sample.decode)(&sample.bytes), "{} sample", sample.name);
        for (block, at, field) in placed(&schema, &sample) {
            // Length and kind select another record; the envelope tests
            // cover them.
            if block == "header" && matches!(field.name.as_str(), "total_bytes" | "kind") {
                continue;
            }
            if field.ty != "bytes" {
                assert!(
                    satisfies(&field, get(&sample.bytes, at, &field)),
                    "{} {block}.{} sample",
                    sample.name,
                    field.name
                );
            }
            for bad in violations(&field) {
                let mut bytes = sample.bytes.clone();
                put(&mut bytes, at, &field, bad);
                assert!(
                    !(sample.decode)(&bytes),
                    "{} {block}.{}={bad} decoded",
                    sample.name,
                    field.name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 80, "only {checked} violations exercised");
}

#[test]
fn fields_without_a_declared_rule_reach_their_reader() {
    let schema = Schema::parse(SCHEMA);
    let mut checked = 0;
    for sample in samples() {
        for (block, at, field) in placed(&schema, &sample) {
            // Header identity is class-checked; MIME text is judged by its
            // length and UTF-8 rules in the prose tests.
            if block == "header" || field.constrained() || field.ty == "bytes" {
                continue;
            }
            for value in [0x5a, 0x7fff] {
                let mut bytes = sample.bytes.clone();
                put(&mut bytes, at, &field, value);
                assert!(
                    (sample.decode)(&bytes),
                    "{} {block}.{}={value} refused as syntax",
                    sample.name,
                    field.name
                );
                checked += 1;
            }
        }
    }
    // Negotiate's two revisions; TransferRequest's transfer, byte_size,
    // generation and deadline; Decision's transfer, two namespaces, two
    // generations and deadline.
    assert_eq!(checked, 2 * 12, "open fields exercised");
}

#[test]
fn record_identity_classes_follow_the_header_rules() {
    let schema = Schema::parse(SCHEMA);
    let offset = |name: &str| schema.field("header", name).offset;
    for sample in samples()
        .into_iter()
        .filter(|sample| sample.blocks[0].0 == "header")
    {
        let kind =
            PortalFileKind::decode(u16::from_le_bytes(sample.bytes[6..8].try_into().unwrap()))
                .unwrap();
        let (submission, sequence) = match kind.class() {
            PortalFileClass::Object => (1u64, 1u64),
            PortalFileClass::Candidate => (0, 1),
            PortalFileClass::Event => (1, 0),
        };
        for (field, value) in [("submission_id", submission), ("sequence", sequence)] {
            let mut bytes = sample.bytes.clone();
            let at = offset(field);
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            assert!(!(sample.decode)(&bytes), "{} {field}={value}", sample.name);
        }
    }
}
