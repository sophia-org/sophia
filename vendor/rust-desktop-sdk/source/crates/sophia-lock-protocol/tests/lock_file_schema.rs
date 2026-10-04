//! The lock file KDL is the independent lock providers' layout authority.
//! These checks bind it to the native codec: every byte is declared, every
//! declared constraint is refused when a real encoded record violates it, and
//! fields without a declared rule still decode so their reader judges them.
#[path = "support/file_schema.rs"]
mod file_schema;

use std::collections::{BTreeMap, BTreeSet};

use file_schema::*;
use sophia_lock_protocol::lock_files::*;

const SCHEMA: &str = include_str!("../../../spec/sophia-lock-files-v1.kdl");
const EPOCH: u64 = 9;

type Decode = fn(&[u8]) -> bool;

/// A real encoded record and where each declared block sits inside it.
struct Sample {
    name: &'static str,
    bytes: Vec<u8>,
    decode: Decode,
    blocks: Vec<(String, usize)>,
    /// Fields without a declared rule are judged by the owner (candidates)
    /// or kept as an open vocabulary (reasons), never refused as syntax.
    open_fields: bool,
}

fn class(kind: u16) -> LockFileClass {
    match kind {
        1..=15 => LockFileClass::Object,
        16..=255 => LockFileClass::Event,
        _ => LockFileClass::Candidate,
    }
}

fn record(kind: LockFileKind, body: &[u8]) -> Vec<u8> {
    let (submission_id, sequence) = match kind.class() {
        LockFileClass::Object => (0, 0),
        LockFileClass::Candidate => (5, 0),
        LockFileClass::Event => (0, 3),
    };
    encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id,
            sequence,
        },
        body,
    )
    .unwrap()
}

fn body(bytes: &[u8]) -> Option<&[u8]> {
    let kind = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    decode_lock_file_record(bytes, class(kind))
        .ok()
        .map(|record| record.body)
}

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 4,
        upload_slots: 2,
        max_chords: 8,
        max_width_px: 3840,
        max_height_px: 2160,
        max_resource_bytes: 3840 * 4 * 2160,
        max_live_resources: 8,
        journal_records: 64,
        journal_bytes: 16_384,
        assembly_timeout_ms: 2_000,
        ack_progress_timeout_ms: 500,
    }
}

fn allocation(output_id: u64) -> LockAllocation {
    LockAllocation {
        output_id,
        output_generation: 2,
        allocation_id: 10 + output_id,
        allocation_generation: 1,
        pixel_width: 2560,
        pixel_height: 1440,
        scale_numerator: 2,
        scale_denominator: 1,
    }
}

fn lock() -> LockObject {
    LockObject {
        lock_epoch: 4,
        topology_generation: 7,
        phase: LockPhase::Locked,
        allocations: vec![allocation(1), allocation(2)],
    }
}

fn negotiate() -> LockNegotiate {
    LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS,
        chords: vec![
            LockChordRequest {
                keysym: 0x62,
                modifiers: 0b0100,
            },
            LockChordRequest {
                keysym: 0xff1b,
                modifiers: 0b0011,
            },
        ],
    }
}

const RESOURCE: LockResourceId = LockResourceId {
    id: 21,
    generation: 1,
};

fn candidate() -> LockCandidate {
    LockCandidate {
        transaction: 30,
        lock_epoch: 4,
        output_id: 1,
        output_generation: 2,
        allocation_id: 11,
        allocation_generation: 1,
        candidate_generation: 3,
        pacing_permit: 40,
        resource: RESOURCE,
    }
}

fn samples(schema: &Schema) -> Vec<Sample> {
    let header = |block: &str| vec![("header".to_owned(), 0), (block.to_owned(), 32)];
    // Place the tail rows of a variable body after its prefix.
    let tail = |block: &str, bytes: &[u8]| {
        let mut blocks = header(block);
        let mut at = 32 + schema.size(block);
        for (row, count) in schema.tail(block) {
            let count_field = schema.field(block, &count);
            let count = get(bytes, 32 + count_field.offset, count_field) as usize;
            for _ in 0..count {
                blocks.push((row.clone(), at));
                at += schema.size(&row);
            }
        }
        assert_eq!(at, bytes.len(), "{block} tail coverage");
        blocks
    };
    let fixed = |name: &'static str, kind, bytes: Vec<u8>, decode: Decode, open_fields| Sample {
        name,
        bytes: record(kind, &bytes),
        decode,
        blocks: header(name),
        open_fields,
    };
    let lock_record = record(LockFileKind::Lock, &lock().encode().unwrap());
    let negotiate_record = record(LockFileKind::Negotiate, &negotiate().encode().unwrap());
    let step = |total_bytes| LockResourceStep {
        transaction: 31,
        resource: RESOURCE,
        total_bytes,
    };
    vec![
        fixed(
            "Limits",
            LockFileKind::Limits,
            limits().encode().unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockFileLimits::decode(b).is_ok()),
            false,
        ),
        Sample {
            name: "Lock",
            blocks: tail("Lock", &lock_record),
            bytes: lock_record,
            decode: |bytes| body(bytes).is_some_and(|b| LockObject::decode(b).is_ok()),
            open_fields: true,
        },
        fixed(
            "Negotiated",
            LockFileKind::Negotiated,
            LockNegotiated {
                granted_chords: 2,
                granted_capabilities: 3,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockNegotiated::decode(b).is_ok()),
            false,
        ),
        fixed(
            "Refused",
            LockFileKind::Refused,
            LockRefusal::InvalidChord.encode(),
            |bytes| body(bytes).is_some_and(|b| LockRefusal::decode(b).is_ok()),
            false,
        ),
        fixed(
            "Submitted",
            LockFileKind::Submitted,
            LockSubmitted {
                submission_id: 5,
                candidate_kind: LockFileKind::Candidate,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockSubmitted::decode(b).is_ok()),
            false,
        ),
        fixed(
            "ObjectPublished",
            LockFileKind::ObjectPublished,
            LockObjectPublished {
                object_generation: 6,
                qid_path: 90,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockObjectPublished::decode(b).is_ok()),
            false,
        ),
        fixed(
            "ResourceStatus",
            LockFileKind::ResourceStatus,
            LockResourceStatus {
                transaction: 31,
                resource: RESOURCE,
                status: LockResourceState::Accepted,
                reason: 0,
                admitted_bytes: 4096,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceStatus::decode(b).is_ok()),
            true,
        ),
        fixed(
            "ResourceReleased",
            LockFileKind::ResourceReleased,
            LockResourceReleased {
                transaction: 31,
                resource: RESOURCE,
                reason: 8,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceReleased::decode(b).is_ok()),
            true,
        ),
        fixed(
            "CandidateOutcome",
            LockFileKind::CandidateOutcome,
            LockCandidateOutcome {
                transaction: 30,
                lock_epoch: 4,
                output_id: 1,
                allocation_id: 11,
                candidate_generation: 3,
                status: LockCandidateStatus::Presented,
                reason: 0,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockCandidateOutcome::decode(b).is_ok()),
            true,
        ),
        fixed(
            "FramePermit",
            LockFileKind::FramePermit,
            LockFramePermit {
                lock_epoch: 4,
                allocation_id: 11,
                allocation_generation: 1,
                demand_id: 12,
                pacing_permit: 40,
                expires_after_ms: 50,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockFramePermit::decode(b).is_ok()),
            false,
        ),
        fixed(
            "Entry",
            LockFileKind::Entry,
            LockEntry {
                lock_epoch: 4,
                entry: LockEntryKind::Delete,
                empty_after: true,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockEntry::decode(b).is_ok()),
            false,
        ),
        fixed(
            "Chord",
            LockFileKind::Chord,
            LockChord {
                lock_epoch: 4,
                chord: 1,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockChord::decode(b).is_ok()),
            false,
        ),
        Sample {
            name: "Negotiate",
            blocks: tail("Negotiate", &negotiate_record),
            bytes: negotiate_record,
            decode: |bytes| body(bytes).is_some_and(|b| LockNegotiate::decode(b).is_ok()),
            open_fields: true,
        },
        fixed(
            "ResourceBegin",
            LockFileKind::ResourceBegin,
            LockResourceBegin {
                transaction: 31,
                resource: RESOURCE,
                width_px: 2560,
                height_px: 1440,
                slot: 1,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceBegin::decode(b).is_ok()),
            true,
        ),
        fixed(
            "ResourceEnd",
            LockFileKind::ResourceEnd,
            step(Some(2560 * 4 * 1440)).encode().unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceStep::decode(b, true).is_ok()),
            true,
        ),
        fixed(
            "ResourceCancel",
            LockFileKind::ResourceCancel,
            step(None).encode().unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceStep::decode(b, false).is_ok()),
            true,
        ),
        fixed(
            "ResourceRetire",
            LockFileKind::ResourceRetire,
            step(None).encode().unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockResourceStep::decode(b, false).is_ok()),
            true,
        ),
        fixed(
            "Candidate",
            LockFileKind::Candidate,
            candidate().encode().unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockCandidate::decode(b).is_ok()),
            true,
        ),
        fixed(
            "FrameDemand",
            LockFileKind::FrameDemand,
            LockFrameDemand {
                transaction: 32,
                lock_epoch: 4,
                allocation_id: 11,
                allocation_generation: 1,
                demand_id: 12,
            }
            .encode()
            .unwrap(),
            |bytes| body(bytes).is_some_and(|b| LockFrameDemand::decode(b).is_ok()),
            true,
        ),
        Sample {
            name: "submit",
            bytes: encode_lock_file_submit(LockFileSubmit {
                connection_epoch: EPOCH,
                submission_id: 5,
                candidate_bytes: 48,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_lock_file_submit(bytes).is_ok(),
            blocks: vec![("submit".into(), 0)],
            open_fields: false,
        },
        Sample {
            name: "ack",
            bytes: encode_lock_file_ack(LockFileAck {
                connection_epoch: EPOCH,
                sequence: 3,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_lock_file_ack(bytes).is_ok(),
            blocks: vec![("ack".into(), 0)],
            open_fields: false,
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
fn every_block_covers_its_bytes_and_matches_the_native_sizes() {
    let schema = Schema::parse(SCHEMA);
    for (block, size) in [
        ("header", LOCK_FILE_HEADER_BYTES),
        ("submit", 24),
        ("ack", 16),
        ("Limits", LockFileLimits::BYTES),
        ("Lock", 32),
        ("Allocation", LockAllocation::BYTES),
        ("ChordRequest", 8),
        ("Negotiate", 16),
        ("Negotiated", 16),
        ("Refused", 8),
        ("Submitted", 16),
        ("ObjectPublished", 24),
        ("ResourceBegin", 40),
        ("ResourceEnd", 32),
        ("ResourceCancel", 24),
        ("ResourceRetire", 24),
        ("ResourceStatus", 40),
        ("ResourceReleased", 32),
        ("Candidate", 96),
        ("CandidateOutcome", 56),
        ("FrameDemand", 40),
        ("FramePermit", 48),
        ("Entry", 16),
        ("Chord", 16),
    ] {
        assert_eq!(schema.size(block), size, "{block}");
    }
    assert_eq!(schema.blocks.len(), 24, "every block is sized above");
    // Rows follow their prefix; no lock block embeds fixed member slots.
    assert!(
        schema
            .blocks
            .values()
            .flat_map(|(_, fields)| fields)
            .all(|field| field.row.is_none())
    );
    let root = &schema.root;
    assert_eq!(
        integer(root, "max-record-bytes"),
        Some(LOCK_FILE_MAX_BYTES as i128)
    );
    assert_eq!(
        integer(root, "max-candidate-bytes"),
        Some(LOCK_FILE_MAX_CANDIDATE_BYTES as i128)
    );
    let largest = |block: &str| {
        schema.size("header")
            + schema.size(block)
            + schema
                .tail(block)
                .iter()
                .map(|(row, count)| {
                    schema.size(row) * schema.field(block, count).max.unwrap() as usize
                })
                .sum::<usize>()
    };
    // Every candidate fits the candidate bound; the smallest sets its floor.
    let candidates: Vec<_> = schema
        .kinds
        .iter()
        .filter(|(_, (class, _))| class == "candidate")
        .map(|(name, _)| name.clone())
        .collect();
    let rows: BTreeSet<_> = schema
        .nodes("rows")
        .map(|node| name(node).to_owned())
        .collect();
    let sizes: Vec<_> = candidates
        .iter()
        .map(|name| {
            if rows.contains(name) {
                largest(name)
            } else {
                schema.size("header") + schema.size(name)
            }
        })
        .collect();
    assert_eq!(sizes.iter().max(), Some(&LOCK_FILE_MAX_CANDIDATE_BYTES));
    let smallest = candidates
        .iter()
        .map(|name| schema.size("header") + schema.size(name))
        .min();
    assert_eq!(
        schema
            .field("submit", "candidate_bytes")
            .min
            .map(|min| min as usize),
        smallest
    );
    assert_eq!(
        schema.field("submit", "candidate_bytes").max,
        Some(LOCK_FILE_MAX_CANDIDATE_BYTES as i128)
    );
    assert!(largest("Lock") <= LOCK_FILE_MAX_BYTES);
    assert_eq!(
        schema.field("Lock", "allocation_count").max,
        Some(LOCK_FILE_MAX_OUTPUTS as i128)
    );
    assert_eq!(
        schema.field("Negotiate", "chord_count").max,
        Some(LOCK_FILE_MAX_CHORDS as i128)
    );
}

#[test]
fn kinds_vocabulary_and_namespace_match_the_contract() {
    let schema = Schema::parse(SCHEMA);
    let native = [
        ("Limits", LockFileKind::Limits),
        ("Lock", LockFileKind::Lock),
        ("Negotiated", LockFileKind::Negotiated),
        ("Refused", LockFileKind::Refused),
        ("Submitted", LockFileKind::Submitted),
        ("ObjectPublished", LockFileKind::ObjectPublished),
        ("ResourceStatus", LockFileKind::ResourceStatus),
        ("ResourceReleased", LockFileKind::ResourceReleased),
        ("CandidateOutcome", LockFileKind::CandidateOutcome),
        ("FramePermit", LockFileKind::FramePermit),
        ("Entry", LockFileKind::Entry),
        ("Chord", LockFileKind::Chord),
        ("Negotiate", LockFileKind::Negotiate),
        ("ResourceBegin", LockFileKind::ResourceBegin),
        ("ResourceEnd", LockFileKind::ResourceEnd),
        ("ResourceCancel", LockFileKind::ResourceCancel),
        ("ResourceRetire", LockFileKind::ResourceRetire),
        ("Candidate", LockFileKind::Candidate),
        ("FrameDemand", LockFileKind::FrameDemand),
    ];
    assert_eq!(schema.kinds.len(), native.len());
    for (name, kind) in native {
        let (class, value) = &schema.kinds[name];
        assert_eq!(*value, kind as u16, "{name}");
        assert_eq!(LockFileKind::decode(*value), Ok(kind), "{name}");
        let expected = match kind.class() {
            LockFileClass::Object => "object",
            LockFileClass::Event => "event",
            LockFileClass::Candidate => "candidate",
        };
        assert_eq!(class, expected, "{name}");
        assert!(schema.blocks.contains_key(name), "{name} has no body");
    }
    let capabilities = schema.values("capability", "bit");
    assert_eq!(
        1u64 << capabilities["present"],
        LOCK_FILE_CAPABILITY_PRESENT
    );
    assert_eq!(1u64 << capabilities["chords"], LOCK_FILE_CAPABILITY_CHORDS);
    assert_eq!(capabilities.len(), 2);
    assert_eq!(
        schema.values("refusal", "value"),
        BTreeMap::from([
            (
                "unsupported_revision".into(),
                LockRefusal::UnsupportedRevision as i128
            ),
            (
                "presentation_required".into(),
                LockRefusal::PresentationRequired as i128
            ),
            ("invalid_chord".into(), LockRefusal::InvalidChord as i128),
        ])
    );
    assert_eq!(
        schema.values("phase", "value"),
        BTreeMap::from([
            ("unlocked".into(), LockPhase::Unlocked as i128),
            ("locking".into(), LockPhase::Locking as i128),
            ("locked".into(), LockPhase::Locked as i128),
            ("unlocking".into(), LockPhase::Unlocking as i128),
        ])
    );
    assert_eq!(
        schema.values("entry", "value"),
        BTreeMap::from([
            ("insert".into(), LockEntryKind::Insert as i128),
            ("delete".into(), LockEntryKind::Delete as i128),
            ("clear".into(), LockEntryKind::Clear as i128),
            ("submit".into(), LockEntryKind::Submit as i128),
            ("checking".into(), LockEntryKind::Checking as i128),
            ("failed".into(), LockEntryKind::Failed as i128),
            ("unavailable".into(), LockEntryKind::Unavailable as i128),
        ])
    );
    let modifiers = schema.values("modifier", "bit");
    assert_eq!(
        modifiers.values().fold(0u16, |mask, bit| mask | 1 << bit),
        LOCK_FILE_MODIFIER_MASK
    );
    assert_eq!(modifiers["shift"], 0);
    assert_eq!(
        schema.values("resource-status", "value"),
        BTreeMap::from([
            ("admitted".into(), LockResourceState::Admitted as i128),
            ("accepted".into(), LockResourceState::Accepted as i128),
            ("rejected".into(), LockResourceState::Rejected as i128),
            ("cancelled".into(), LockResourceState::Cancelled as i128),
        ])
    );
    assert_eq!(
        schema.values("candidate-status", "value"),
        BTreeMap::from([
            ("prepared".into(), LockCandidateStatus::Prepared as i128),
            ("presented".into(), LockCandidateStatus::Presented as i128),
            ("rejected".into(), LockCandidateStatus::Rejected as i128),
            ("superseded".into(), LockCandidateStatus::Superseded as i128),
            ("revoked".into(), LockCandidateStatus::Revoked as i128),
        ])
    );
    let api = schema.nodes("api").next().unwrap();
    assert_eq!(name(api), "sophia-lock-files version=1");
    let files: Vec<_> = schema
        .nodes("file")
        .map(|node| (name(node), text(node, "access").unwrap()))
        .collect();
    assert_eq!(
        files,
        [
            ("api", "read"),
            ("limits", "read"),
            ("lock", "read"),
            ("events", "read"),
            ("transaction", "read-write"),
            ("submit", "write"),
            ("ack", "write"),
            ("upload", "write"),
        ]
    );
}

#[test]
fn native_encoders_place_values_at_declared_offsets() {
    let schema = Schema::parse(SCHEMA);
    let samples = samples(&schema);
    let find = |name: &str| samples.iter().find(|sample| sample.name == name).unwrap();
    let at = |sample: &Sample, block: &str, index: usize, field: &str| {
        let base = sample
            .blocks
            .iter()
            .filter(|(name, _)| name == block)
            .nth(index)
            .unwrap()
            .1;
        let field = schema.field(block, field);
        get(&sample.bytes, base + field.offset, field)
    };
    let lock = find("Lock");
    assert_eq!(at(lock, "header", 0, "kind"), 2);
    assert_eq!(
        at(lock, "header", 0, "total_bytes"),
        lock.bytes.len() as i128
    );
    assert_eq!(at(lock, "Lock", 0, "phase"), 3);
    assert_eq!(at(lock, "Lock", 0, "allocation_count"), 2);
    assert_eq!(at(lock, "Allocation", 1, "output_id"), 2);
    assert_eq!(at(lock, "Allocation", 1, "allocation_id"), 12);
    assert_eq!(at(lock, "Allocation", 0, "scale_numerator"), 2);
    let negotiate = find("Negotiate");
    assert_eq!(at(negotiate, "Negotiate", 0, "requested_capabilities"), 3);
    assert_eq!(at(negotiate, "ChordRequest", 1, "keysym"), 0xff1b);
    assert_eq!(at(negotiate, "ChordRequest", 0, "modifiers"), 4);
    assert_eq!(at(find("Limits"), "Limits", 0, "max_height_px"), 2160);
    assert_eq!(at(find("Submitted"), "Submitted", 0, "candidate_kind"), 262);
    assert_eq!(
        at(find("ObjectPublished"), "ObjectPublished", 0, "qid_path"),
        90
    );
    assert_eq!(at(find("ResourceBegin"), "ResourceBegin", 0, "slot"), 1);
    assert_eq!(
        at(find("ResourceEnd"), "ResourceEnd", 0, "total_bytes"),
        2560 * 4 * 1440
    );
    assert_eq!(at(find("Candidate"), "Candidate", 0, "pacing_permit"), 40);
    assert_eq!(at(find("Candidate"), "Candidate", 0, "resource_id"), 21);
    assert_eq!(
        at(find("CandidateOutcome"), "CandidateOutcome", 0, "status"),
        2
    );
    assert_eq!(
        at(find("FramePermit"), "FramePermit", 0, "expires_after_ms"),
        50
    );
    assert_eq!(at(find("Entry"), "Entry", 0, "entry"), 2);
    assert_eq!(at(find("Entry"), "Entry", 0, "empty_after"), 1);
    assert_eq!(at(find("Chord"), "Chord", 0, "chord"), 1);
    assert_eq!(at(find("submit"), "submit", 0, "candidate_bytes"), 48);
}

#[test]
fn every_declared_constraint_is_refused_by_the_native_decoder() {
    let schema = Schema::parse(SCHEMA);
    let mut checked = 0;
    for sample in samples(&schema) {
        assert!((sample.decode)(&sample.bytes), "{} sample", sample.name);
        for (block, at, field) in placed(&schema, &sample) {
            // Header length and kind are exercised by the envelope tests;
            // changing them selects a different record rather than a value.
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
    assert!(checked > 150, "only {checked} violations exercised");
}

#[test]
fn fields_without_a_declared_rule_reach_their_reader() {
    let schema = Schema::parse(SCHEMA);
    let mut checked = 0;
    for sample in samples(&schema)
        .into_iter()
        .filter(|sample| sample.open_fields)
    {
        let counts: BTreeSet<String> = sample
            .blocks
            .iter()
            .filter(|(block, _)| schema.nodes("rows").any(|node| name(node) == block))
            .flat_map(|(block, _)| schema.tail(block).into_iter().map(|(_, count)| count))
            .collect();
        for (block, at, field) in placed(&schema, &sample) {
            if block == "header" || field.constrained() || counts.contains(&field.name) {
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
    // Lock epoch, negotiate revisions and capabilities, three reasons and
    // admitted bytes.
    assert_eq!(checked, 2 * 8, "open fields exercised");
}

#[test]
fn record_identity_classes_follow_the_header_rules() {
    let schema = Schema::parse(SCHEMA);
    let header = &schema.blocks["header"].1;
    let offset = |name: &str| header.iter().find(|f| f.name == name).unwrap().offset;
    for sample in samples(&schema)
        .into_iter()
        .filter(|sample| sample.blocks[0].0 == "header")
    {
        let kind = u16::from_le_bytes(sample.bytes[6..8].try_into().unwrap());
        let (submission, sequence) = match class(kind) {
            LockFileClass::Object => (1u64, 1u64),
            LockFileClass::Candidate => (0, 1),
            LockFileClass::Event => (1, 0),
        };
        for (field, value) in [("submission_id", submission), ("sequence", sequence)] {
            let mut bytes = sample.bytes.clone();
            let at = offset(field);
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            assert!(!(sample.decode)(&bytes), "{} {field}={value}", sample.name);
        }
        // A record read as another class is refused whole.
        for other in [
            LockFileClass::Object,
            LockFileClass::Candidate,
            LockFileClass::Event,
        ] {
            if other != class(kind) {
                assert!(decode_lock_file_record(&sample.bytes, other).is_err());
            }
        }
    }
}
