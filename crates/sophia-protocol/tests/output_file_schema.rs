//! The output file KDL is the independent clients' layout authority. These
//! checks bind it to the native codec: every byte is declared, and every
//! declared constraint is refused when a real encoded record violates it.
//! Unconstrained candidate fields must still decode so the owner sees them.
use std::collections::{BTreeMap, BTreeSet};

use kdl::{KdlDocument, KdlNode};
use sophia_protocol::output_files::*;
use sophia_protocol::*;

const SCHEMA: &str = include_str!("../../../protocol/sophia-output-files-v1.kdl");
const EPOCH: u64 = 9;

fn text<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.get(key).and_then(|value| value.as_string())
}

fn integer(node: &KdlNode, key: &str) -> Option<i128> {
    node.get(key).and_then(|value| value.as_integer())
}

fn name(node: &KdlNode) -> &str {
    node.get(0)
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| panic!("{} has no name", node.name().value()))
}

fn flag(node: &KdlNode, key: &str) -> bool {
    node.get(key).and_then(|value| value.as_bool()) == Some(true)
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    ty: String,
    offset: usize,
    width: usize,
    value: Option<i128>,
    min: Option<i128>,
    max: Option<i128>,
    mask: Option<i128>,
    required: Option<i128>,
    nonzero: bool,
    row: Option<(String, usize)>,
}

impl Field {
    fn constrained(&self) -> bool {
        self.value.is_some()
            || self.min.is_some()
            || self.max.is_some()
            || self.mask.is_some()
            || self.nonzero
    }
}

struct Schema {
    root: KdlNode,
    blocks: BTreeMap<String, (usize, Vec<Field>)>,
    kinds: BTreeMap<String, (String, u16)>,
}

impl Schema {
    fn parse() -> Self {
        let document = KdlDocument::parse_v2(SCHEMA).unwrap();
        let [root] = document.nodes() else {
            panic!("exactly one protocol")
        };
        assert_eq!(root.name().value(), "protocol");
        let mut blocks = BTreeMap::new();
        let mut kinds = BTreeMap::new();
        let children = root.children().unwrap().nodes();
        // Rows first: later blocks may embed them by name.
        for pass in 0..2 {
            for node in children {
                let class = node.name().value();
                let key = match class {
                    "row" if pass == 0 => name(node).to_owned(),
                    "body" | "body-prefix" if pass == 1 => name(node).to_owned(),
                    "header" | "submit" | "ack" if pass == 1 => class.to_owned(),
                    _ => continue,
                };
                let size = integer(node, "size").unwrap() as usize;
                let fields = fields(node, size, &blocks);
                assert!(
                    blocks.insert(key.clone(), (size, fields)).is_none(),
                    "duplicate {key}"
                );
            }
        }
        for node in children {
            let class = node.name().value();
            if matches!(class, "object" | "event" | "candidate") {
                let kind = integer(node, "kind").unwrap() as u16;
                let range = match class {
                    "object" => 1..=15,
                    "event" => 16..=255,
                    _ => 256..=u16::MAX,
                };
                assert!(range.contains(&kind), "{} misclassified", name(node));
                assert!(
                    kinds
                        .insert(name(node).to_owned(), (class.to_owned(), kind))
                        .is_none()
                );
            }
        }
        Self {
            root: root.clone(),
            blocks,
            kinds,
        }
    }

    fn nodes(&self, class: &str) -> impl Iterator<Item = &KdlNode> {
        self.root
            .children()
            .unwrap()
            .nodes()
            .iter()
            .filter(move |node| node.name().value() == class)
    }

    fn values(&self, class: &str, key: &str) -> BTreeMap<String, i128> {
        self.nodes(class)
            .map(|node| (name(node).to_owned(), integer(node, key).unwrap()))
            .collect()
    }

    fn size(&self, block: &str) -> usize {
        self.blocks[block].0
    }

    fn field(&self, block: &str, field: &str) -> &Field {
        self.blocks[block]
            .1
            .iter()
            .find(|candidate| candidate.name == field)
            .unwrap_or_else(|| panic!("{block}.{field}"))
    }

    /// The row sequence following a variable body prefix.
    fn tail(&self, block: &str) -> Vec<(String, String)> {
        let node = self.nodes("rows").find(|node| name(node) == block).unwrap();
        node.children()
            .unwrap()
            .nodes()
            .iter()
            .map(|row| (name(row).to_owned(), text(row, "count").unwrap().to_owned()))
            .collect()
    }
}

fn fields(node: &KdlNode, size: usize, rows: &BTreeMap<String, (usize, Vec<Field>)>) -> Vec<Field> {
    let mut covered = vec![false; size];
    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    for field in node.children().unwrap().nodes() {
        assert_eq!(field.name().value(), "field");
        let label = name(field);
        assert!(names.insert(label.to_owned()), "repeated {label}");
        let ty = text(field, "type").unwrap().to_owned();
        let mut row = None;
        let width = match ty.as_str() {
            "u16" => 2,
            "u32" | "i32" => 4,
            "u64" => 8,
            "bytes" => integer(field, "size").unwrap() as usize,
            other => {
                let target = other.strip_prefix("row:").expect("known type");
                let count = integer(field, "count").unwrap() as usize;
                row = Some((target.to_owned(), count));
                rows[target].0 * count
            }
        };
        let offset = integer(field, "offset").unwrap() as usize;
        assert!(width > 0 && offset + width <= size, "{label} overflows");
        assert!(
            covered[offset..offset + width].iter().all(|used| !used),
            "{label} overlaps"
        );
        covered[offset..offset + width].fill(true);
        let parsed = Field {
            name: label.to_owned(),
            ty,
            offset,
            width,
            value: integer(field, "value"),
            min: integer(field, "min"),
            max: integer(field, "max"),
            mask: integer(field, "mask"),
            required: integer(field, "required"),
            nonzero: flag(field, "nonzero"),
            row,
        };
        if let (Some(min), Some(max)) = (parsed.min, parsed.max) {
            assert!(min <= max, "{label} reversed");
        }
        fields.push(parsed);
    }
    assert!(covered.iter().all(|used| *used), "gap in {}", name(node));
    fields
}

fn put(bytes: &mut [u8], at: usize, field: &Field, value: i128) {
    let width = if field.ty == "bytes" { 1 } else { field.width };
    let raw = value.to_le_bytes();
    bytes[at..at + width].copy_from_slice(&raw[..width]);
}

fn get(bytes: &[u8], at: usize, field: &Field) -> i128 {
    let mut raw = [0; 16];
    raw[..field.width].copy_from_slice(&bytes[at..at + field.width]);
    let value = i128::from_le_bytes(raw);
    if field.ty == "i32" {
        i128::from(value as u32 as i32)
    } else {
        value
    }
}

fn fits(field: &Field, value: i128) -> bool {
    match field.ty.as_str() {
        "i32" => i32::try_from(value).is_ok(),
        "bytes" => (0..=255).contains(&value),
        _ => value >= 0 && value < 1i128 << (8 * field.width),
    }
}

fn satisfies(field: &Field, value: i128) -> bool {
    field.value.is_none_or(|expected| value == expected)
        && field.min.is_none_or(|min| value >= min)
        && field.max.is_none_or(|max| value <= max)
        && field.mask.is_none_or(|mask| value & !mask == 0)
        && field
            .required
            .is_none_or(|required| value & required == required)
        && (!field.nonzero || value != 0)
}

/// Values violating exactly one declared rule of a field.
fn violations(field: &Field) -> Vec<i128> {
    let mut values = Vec::new();
    if let Some(value) = field.value {
        values.push(value + 1);
    }
    if field.nonzero {
        values.push(0);
    }
    if let Some(min) = field.min {
        values.push(min - 1);
    }
    if let Some(max) = field.max {
        values.push(max + 1);
    }
    if let Some(mask) = field.mask {
        values.push(mask + 1);
        if let Some(required) = field.required {
            values.push(mask & !required);
        }
    }
    values.retain(|value| fits(field, *value));
    values
}

type Decode = fn(&[u8]) -> bool;

/// A real encoded record and where each declared block sits inside it.
struct Sample {
    name: &'static str,
    bytes: Vec<u8>,
    decode: Decode,
    blocks: Vec<(String, usize)>,
    /// Candidate fields whose semantic validation belongs to the owner.
    owner_checked: bool,
}

fn record(kind: OutputFileKind, submission_id: u64, sequence: u64, body: &[u8]) -> Vec<u8> {
    encode_output_file_record(
        OutputFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id,
            sequence,
        },
        body,
    )
    .unwrap()
}

fn class(kind: u16) -> OutputFileClass {
    match kind {
        1..=15 => OutputFileClass::Object,
        16..=255 => OutputFileClass::Event,
        _ => OutputFileClass::Candidate,
    }
}

fn body(bytes: &[u8]) -> Option<&[u8]> {
    let kind = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    decode_output_file_record(bytes, class(kind))
        .ok()
        .map(|record| record.body)
}

fn topology() -> OutputV1Snapshot {
    let mode = |id, width, height, preferred| OutputModeDescriptor {
        mode: DisplayModeId::from_raw(id),
        pixel_size: Size { width, height },
        refresh_millihz: 60_000,
        preferred,
    };
    OutputV1Snapshot {
        connection_epoch: EPOCH,
        snapshot: OutputAuthoritySnapshot {
            topology_epoch: 7,
            primary_output: OutputId::from_raw(2),
            heads: vec![
                OutputHeadDescriptor {
                    head: DisplayHeadId::from_raw(3),
                    generation: 4,
                    label: "DP-1".into(),
                    connected: true,
                    enabled: true,
                    vrr_capable: true,
                    transforms: OutputTransformSet::ALL,
                    current_mode: Some(DisplayModeId::from_raw(5)),
                    modes: vec![mode(5, 1920, 1080, true), mode(6, 1280, 720, false)],
                },
                OutputHeadDescriptor {
                    head: DisplayHeadId::from_raw(8),
                    generation: 1,
                    label: "HDMI-A-1".into(),
                    connected: true,
                    enabled: false,
                    vrr_capable: false,
                    transforms: OutputTransformSet::ALL,
                    current_mode: None,
                    modes: vec![mode(9, 1024, 768, true)],
                },
            ],
            groups: vec![OutputLogicalGroupState {
                output: OutputId::from_raw(2),
                generation: 6,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

fn proposal() -> Vec<u8> {
    let message = OutputV1Proposal {
        connection_epoch: EPOCH,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 7,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(3),
                head_generation: 4,
                mode: DisplayModeId::from_raw(6),
                transform: OutputTransform::Rotate90,
                vrr: OutputVrrPolicy::Automatic,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::from_raw(2),
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 720,
                    height: 1280,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Fit,
                }],
            }],
        },
    };
    encode_output_file_proposal(TransactionId::from_raw(11), &message).unwrap()
}

fn samples(schema: &Schema) -> Vec<Sample> {
    let header = |block: &str| vec![("header".to_owned(), 0), (block.to_owned(), 32)];
    let topology_body = encode_output_file_topology(&topology()).unwrap();
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
    let topology_record = record(OutputFileKind::Topology, 0, 0, &topology_body);
    let proposal_record = record(OutputFileKind::Proposal, 5, 0, &proposal());
    vec![
        Sample {
            name: "Limits",
            bytes: record(
                OutputFileKind::Limits,
                0,
                0,
                &encode_output_file_limits(OutputFileLimits::default()).unwrap(),
            ),
            decode: |bytes| body(bytes).is_some_and(|b| decode_output_file_limits(b).is_ok()),
            blocks: header("Limits"),
            owner_checked: false,
        },
        Sample {
            name: "Topology",
            blocks: tail("Topology", &topology_record),
            bytes: topology_record,
            decode: |bytes| {
                body(bytes).is_some_and(|b| decode_output_file_topology(b, EPOCH).is_ok())
            },
            owner_checked: false,
        },
        Sample {
            name: "Negotiated",
            bytes: record(
                OutputFileKind::Negotiated,
                0,
                1,
                &encode_output_file_negotiated(OutputV1ServerWelcome {
                    selected_revision: 1,
                    capabilities: 3,
                    connection_epoch: EPOCH,
                    max_heads: 16,
                    max_groups: 16,
                    max_modes_per_head: 128,
                    max_heads_per_group: 4,
                })
                .unwrap(),
            ),
            decode: |bytes| {
                body(bytes).is_some_and(|b| decode_output_file_negotiated(b, EPOCH).is_ok())
            },
            blocks: header("Negotiated"),
            owner_checked: false,
        },
        Sample {
            name: "Refused",
            bytes: record(
                OutputFileKind::Refused,
                0,
                1,
                &encode_output_file_refused(OutputFileRefusal::ObservationRequired),
            ),
            decode: |bytes| body(bytes).is_some_and(|b| decode_output_file_refused(b).is_ok()),
            blocks: header("Refused"),
            owner_checked: false,
        },
        Sample {
            name: "Submitted",
            bytes: record(
                OutputFileKind::Submitted,
                0,
                2,
                &encode_output_file_submitted(OutputFileSubmitted {
                    submission_id: 5,
                    candidate_kind: OutputFileKind::Proposal,
                })
                .unwrap(),
            ),
            decode: |bytes| body(bytes).is_some_and(|b| decode_output_file_submitted(b).is_ok()),
            blocks: header("Submitted"),
            owner_checked: false,
        },
        Sample {
            name: "ObjectPublished",
            bytes: record(
                OutputFileKind::ObjectPublished,
                0,
                3,
                &encode_output_file_publication(OutputFilePublication {
                    topology_epoch: 7,
                    qid_path: 90,
                })
                .unwrap(),
            ),
            decode: |bytes| body(bytes).is_some_and(|b| decode_output_file_publication(b).is_ok()),
            blocks: header("ObjectPublished"),
            owner_checked: false,
        },
        Sample {
            name: "Outcome",
            bytes: record(
                OutputFileKind::Outcome,
                0,
                4,
                &encode_output_file_outcome(
                    TransactionId::from_raw(11),
                    OutputV1Outcome {
                        connection_epoch: EPOCH,
                        topology_epoch: 7,
                        kind: OutputV1OutcomeKind::Committed,
                        reason: 0,
                    },
                )
                .unwrap(),
            ),
            decode: |bytes| {
                body(bytes).is_some_and(|b| decode_output_file_outcome(b, EPOCH).is_ok())
            },
            blocks: header("Outcome"),
            owner_checked: true,
        },
        Sample {
            name: "Negotiate",
            bytes: record(
                OutputFileKind::Negotiate,
                1,
                0,
                &encode_output_file_negotiate(OutputV1ClientHello {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    capabilities: 3,
                }),
            ),
            decode: |bytes| body(bytes).is_some_and(|b| decode_output_file_negotiate(b).is_ok()),
            blocks: header("Negotiate"),
            owner_checked: true,
        },
        Sample {
            name: "Proposal",
            blocks: tail("Proposal", &proposal_record),
            bytes: proposal_record,
            decode: |bytes| {
                body(bytes).is_some_and(|b| decode_output_file_proposal(b, EPOCH).is_ok())
            },
            owner_checked: true,
        },
        Sample {
            name: "submit",
            bytes: encode_output_file_submit(OutputFileSubmit {
                connection_epoch: EPOCH,
                submission_id: 5,
                candidate_bytes: 48,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_output_file_submit(bytes).is_ok(),
            blocks: vec![("submit".into(), 0)],
            owner_checked: false,
        },
        Sample {
            name: "ack",
            bytes: encode_output_file_ack(OutputFileAck {
                connection_epoch: EPOCH,
                sequence: 3,
            })
            .unwrap()
            .to_vec(),
            decode: |bytes| decode_output_file_ack(bytes).is_ok(),
            blocks: vec![("ack".into(), 0)],
            owner_checked: false,
        },
    ]
}

/// Expand embedded member rows. Only the first slot is used in the samples;
/// the others are unused and must stay zero.
fn placed(schema: &Schema, sample: &Sample) -> Vec<(String, usize, Field)> {
    let mut out = Vec::new();
    for (block, base) in &sample.blocks {
        for field in &schema.blocks[block].1 {
            match &field.row {
                Some((row, _)) => {
                    for inner in &schema.blocks[row].1 {
                        out.push((
                            format!("{block}.{}[0]", field.name),
                            base + field.offset + inner.offset,
                            inner.clone(),
                        ));
                    }
                }
                None => out.push((block.clone(), base + field.offset, field.clone())),
            }
        }
    }
    out
}

#[test]
fn every_block_covers_its_bytes_and_matches_the_native_sizes() {
    let schema = Schema::parse();
    for (block, size) in [
        ("header", OUTPUT_FILE_HEADER_BYTES),
        ("submit", 24),
        ("ack", 16),
        ("Limits", 40),
        ("Negotiate", 16),
        ("Negotiated", 24),
        ("Refused", 8),
        ("Submitted", 16),
        ("ObjectPublished", 24),
        ("Outcome", 24),
        ("Topology", 24),
        ("Proposal", 24),
        ("Head", 104),
        ("Mode", 24),
        ("Member", 12),
        ("Group", 84),
        ("HeadTarget", 32),
        ("ProposalGroup", 76),
    ] {
        assert_eq!(schema.size(block), size, "{block}");
    }
    let root = &schema.root;
    assert_eq!(
        integer(root, "max-record-bytes"),
        Some(OUTPUT_FILE_MAX_BYTES as i128)
    );
    assert_eq!(
        integer(root, "max-candidate-bytes"),
        Some(OUTPUT_FILE_MAX_CANDIDATE_BYTES as i128)
    );
    // Largest bodies from declared row sizes and count maxima.
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
    assert_eq!(largest("Topology"), OUTPUT_FILE_MAX_TOPOLOGY_BYTES);
    assert_eq!(largest("Topology"), 52_216);
    assert_eq!(largest("Proposal"), OUTPUT_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(
        schema.field("submit", "candidate_bytes").max,
        Some(OUTPUT_FILE_MAX_CANDIDATE_BYTES as i128)
    );
    assert_eq!(
        schema.field("submit", "candidate_bytes").min,
        Some((schema.size("header") + schema.size("Negotiate")) as i128)
    );
}

#[test]
fn kinds_vocabulary_and_namespace_match_the_contract() {
    let schema = Schema::parse();
    let native = [
        ("Limits", OutputFileKind::Limits),
        ("Topology", OutputFileKind::Topology),
        ("Negotiated", OutputFileKind::Negotiated),
        ("Refused", OutputFileKind::Refused),
        ("Submitted", OutputFileKind::Submitted),
        ("ObjectPublished", OutputFileKind::ObjectPublished),
        ("Outcome", OutputFileKind::Outcome),
        ("Negotiate", OutputFileKind::Negotiate),
        ("Proposal", OutputFileKind::Proposal),
    ];
    assert_eq!(schema.kinds.len(), native.len());
    for (name, kind) in native {
        let (class, value) = &schema.kinds[name];
        assert_eq!(*value, kind as u16, "{name}");
        let expected = match kind.class() {
            OutputFileClass::Object => "object",
            OutputFileClass::Event => "event",
            OutputFileClass::Candidate => "candidate",
        };
        assert_eq!(class, expected, "{name}");
        assert!(schema.blocks.contains_key(name), "{name} has no body");
    }
    assert_eq!(
        schema.values("capability", "bit"),
        BTreeMap::from([("configure".into(), 1), ("observe".into(), 0)])
    );
    assert_eq!(
        1u64 << schema.values("capability", "bit")["observe"],
        SOPHIA_OUTPUT_CAPABILITY_OBSERVE
    );
    assert_eq!(
        1u64 << schema.values("capability", "bit")["configure"],
        SOPHIA_OUTPUT_CAPABILITY_CONFIGURE
    );
    assert_eq!(
        schema.values("refusal", "value"),
        BTreeMap::from([
            (
                "observation_required".into(),
                OutputFileRefusal::ObservationRequired as i128
            ),
            (
                "unsupported_revision".into(),
                OutputFileRefusal::UnsupportedRevision as i128
            ),
        ])
    );
    assert_eq!(schema.values("outcome", "value").len(), 6);
    assert_eq!(schema.values("transform", "value").len(), 8);
    assert_eq!(
        schema.values("reason", "value")["invariant"],
        i128::from(SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT)
    );
    let api = schema.nodes("api").next().unwrap();
    assert_eq!(name(api), "sophia-output-files version=1\n");
    let files: Vec<_> = schema
        .nodes("file")
        .map(|node| (name(node), text(node, "access").unwrap()))
        .collect();
    assert_eq!(
        files,
        [
            ("api", "read"),
            ("limits", "read"),
            ("topology", "read"),
            ("events", "read"),
            ("transaction", "read-write"),
            ("submit", "write"),
            ("ack", "write"),
        ]
    );
}

#[test]
fn native_encoders_place_values_at_declared_offsets() {
    let schema = Schema::parse();
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
    let topology = find("Topology");
    assert_eq!(at(topology, "header", 0, "kind"), 2);
    assert_eq!(
        at(topology, "header", 0, "total_bytes"),
        topology.bytes.len() as i128
    );
    assert_eq!(at(topology, "Topology", 0, "mode_count"), 3);
    assert_eq!(at(topology, "Head", 0, "flags"), 7);
    assert_eq!(at(topology, "Head", 1, "flags"), 1);
    assert_eq!(at(topology, "Head", 1, "first_mode"), 2);
    assert_eq!(at(topology, "Head", 1, "current_mode"), 0);
    assert_eq!(at(topology, "Head", 0, "transforms"), 255);
    assert_eq!(at(topology, "Mode", 1, "width"), 1280);
    assert_eq!(at(topology, "Group", 0, "member_count"), 1);
    let proposal = find("Proposal");
    assert_eq!(at(proposal, "Proposal", 0, "transaction"), 11);
    assert_eq!(at(proposal, "Proposal", 0, "intent"), 2);
    assert_eq!(at(proposal, "HeadTarget", 0, "transform"), 2);
    assert_eq!(at(proposal, "HeadTarget", 0, "vrr"), 2);
    assert_eq!(at(proposal, "ProposalGroup", 0, "height"), 1280);
    assert_eq!(at(find("Outcome"), "Outcome", 0, "outcome"), 2);
    assert_eq!(
        at(find("ObjectPublished"), "ObjectPublished", 0, "qid_path"),
        90
    );
    assert_eq!(at(find("submit"), "submit", 0, "candidate_bytes"), 48);
    assert_eq!(at(find("Limits"), "Limits", 0, "max_modes"), 2_048);
}

#[test]
fn every_declared_constraint_is_refused_by_the_native_decoder() {
    let schema = Schema::parse();
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
    assert!(checked > 80, "only {checked} violations exercised");
}

#[test]
fn unused_member_slots_and_label_padding_must_be_zero() {
    let schema = Schema::parse();
    for sample in samples(&schema) {
        for (block, base) in &sample.blocks {
            for field in &schema.blocks[block].1 {
                let Some((row, count)) = &field.row else {
                    continue;
                };
                let slot = schema.size(row);
                for index in 1..*count {
                    let mut bytes = sample.bytes.clone();
                    bytes[base + field.offset + index * slot] = 1;
                    assert!(!(sample.decode)(&bytes), "{block} slot {index}");
                }
            }
        }
    }
    let sample = samples(&schema).remove(1);
    let (_, head) = sample
        .blocks
        .iter()
        .find(|(name, _)| name == "Head")
        .unwrap();
    let label = schema.field("Head", "label");
    let mut bytes = sample.bytes.clone();
    bytes[head + label.offset + 4] = b'X';
    assert!(!(sample.decode)(&bytes), "label padding");
}

/// Owner-checked candidates: fields without a declared rule are not syntax.
#[test]
fn unconstrained_candidate_fields_reach_the_owner() {
    let schema = Schema::parse();
    let mut checked = 0;
    for sample in samples(&schema)
        .into_iter()
        .filter(|sample| sample.owner_checked)
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
    assert!(checked >= 20, "only {checked} owner fields exercised");
}

#[test]
fn record_identity_classes_follow_the_header_rules() {
    let schema = Schema::parse();
    let header = &schema.blocks["header"].1;
    let offset = |name: &str| header.iter().find(|f| f.name == name).unwrap().offset;
    for sample in samples(&schema)
        .into_iter()
        .filter(|sample| sample.blocks[0].0 == "header")
    {
        let kind = u16::from_le_bytes(sample.bytes[6..8].try_into().unwrap());
        let (submission, sequence) = match class(kind) {
            OutputFileClass::Object => (1u64, 1u64),
            OutputFileClass::Candidate => (0, 1),
            OutputFileClass::Event => (1, 0),
        };
        for (field, value) in [("submission_id", submission), ("sequence", sequence)] {
            let mut bytes = sample.bytes.clone();
            let at = offset(field);
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            assert!(!(sample.decode)(&bytes), "{} {field}={value}", sample.name);
        }
    }
}
