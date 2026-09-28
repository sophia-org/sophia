//! Parses `protocol/sophia-shell-files-v1.kdl` into the blocks and fields
//! [`super::checks`] reads bytes against, plus the codec's own kind list so
//! completeness can be checked against `ShellFileKind` directly.

use kdl::{KdlDocument, KdlNode, KdlValue};
use sophia_shell_protocol::shell_files::{ShellFileClass, ShellFileKind, shell_file_class};

#[derive(Debug, Clone)]
pub struct FieldSpec {
    pub name: String,
    pub ty: String,
    pub offset: usize,
    pub size: Option<usize>,
    pub value: Option<i128>,
    pub nonzero: bool,
    pub min: Option<i128>,
    pub max: Option<i128>,
}

impl FieldSpec {
    pub fn width(&self) -> usize {
        match self.ty.as_str() {
            "u8" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" => 4,
            "u64" => 8,
            "bytes" => self
                .size
                .unwrap_or_else(|| panic!("field `{}`: type=bytes needs size=", self.name)),
            // A length-prefixed, zero-padded text field
            // (`shell::encoding::put_text_padded`): a `u16` length, a
            // reserved `u16`, then `size` bytes of text. `size` names the
            // maximum text length, not the field's total wire width.
            "text" => {
                4 + self
                    .size
                    .unwrap_or_else(|| panic!("field `{}`: type=text needs size=", self.name))
            }
            other => panic!("field `{}`: unknown type `{other}`", self.name),
        }
    }

    pub fn is_reserved(&self) -> bool {
        self.name == "reserved" || self.name == "reserved_tail"
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub keyword: &'static str,
    pub name: String,
    pub size: usize,
    pub fields: Vec<FieldSpec>,
}

/// Every `body`, `body-prefix` and `row` node under `protocol`, plus the
/// singleton `header`, `submit` and `ack` nodes and the `object`/`event`/
/// `candidate` kind declarations.
pub struct Kdl {
    pub header: Block,
    pub submit: Block,
    pub ack: Block,
    pub bodies: Vec<Block>,
    pub prefixes: Vec<Block>,
    pub rows: Vec<Block>,
    pub kinds: Vec<(&'static str, String, u16)>,
}

fn kdl_text() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/sophia-shell-files-v1.kdl"
    ))
    .expect("read protocol/sophia-shell-files-v1.kdl")
}

fn parse_document() -> KdlDocument {
    kdl_text()
        .parse()
        .expect("protocol/sophia-shell-files-v1.kdl must be valid KDL")
}

fn protocol_children(doc: &KdlDocument) -> &KdlDocument {
    doc.nodes()
        .iter()
        .find(|node| node.name().value() == "protocol")
        .expect("a top-level `protocol` node")
        .children()
        .expect("the `protocol` node must have children")
}

fn node_int(node: &KdlNode, key: impl Into<kdl::NodeKey>) -> Option<i128> {
    node.get(key).and_then(KdlValue::as_integer)
}

fn node_str(node: &KdlNode, key: impl Into<kdl::NodeKey>) -> Option<String> {
    node.get(key)
        .and_then(KdlValue::as_string)
        .map(str::to_owned)
}

fn node_bool(node: &KdlNode, key: &str) -> bool {
    node.get(key).and_then(KdlValue::as_bool).unwrap_or(false)
}

fn parse_field(node: &KdlNode) -> FieldSpec {
    assert_eq!(node.name().value(), "field", "expected a `field` node");
    let name = node_str(node, 0).unwrap_or_else(|| panic!("field node missing its name argument"));
    let ty = node_str(node, "type").unwrap_or_else(|| panic!("field `{name}` missing type="));
    let offset: usize = node_int(node, "offset")
        .unwrap_or_else(|| panic!("field `{name}` missing offset="))
        .try_into()
        .unwrap_or_else(|_| panic!("field `{name}`: offset must be nonnegative"));
    let size = node_int(node, "size").map(|v| {
        usize::try_from(v).unwrap_or_else(|_| panic!("field `{name}`: size must be nonnegative"))
    });
    FieldSpec {
        value: node_int(node, "value"),
        nonzero: node_bool(node, "nonzero"),
        min: node_int(node, "min"),
        max: node_int(node, "max"),
        name,
        ty,
        offset,
        size,
    }
}

fn parse_fields(node: &KdlNode) -> Vec<FieldSpec> {
    node.children()
        .map(|children| children.nodes().iter().map(parse_field).collect())
        .unwrap_or_default()
}

fn block_size(node: &KdlNode, keyword: &str, name: &str) -> usize {
    node_int(node, "size")
        .unwrap_or_else(|| panic!("`{keyword} \"{name}\"` missing size="))
        .try_into()
        .unwrap_or_else(|_| panic!("`{keyword} \"{name}\"`: size must be nonnegative"))
}

pub fn parse_kdl() -> Kdl {
    let doc = parse_document();
    let children = protocol_children(&doc);
    // This branch implements the proposed descriptor codecs before enabling
    // their client role. Keep the unchanged published contract and the exact
    // proposal separate until the production/independent-peer gates pass.
    let descriptor_proposal: KdlDocument =
        include_str!("../../../../../spec/descriptor-files-proposal.kdl")
            .parse()
            .expect("descriptor proposal KDL");
    let mut bodies = Vec::new();
    let mut prefixes = Vec::new();
    let mut rows = Vec::new();
    let mut kinds = Vec::new();
    let mut header = None;
    let mut submit = None;
    let mut ack = None;
    for node in children.nodes().iter().chain(descriptor_proposal.nodes()) {
        let keyword = node.name().value();
        match keyword {
            "body" | "body-prefix" | "row" => {
                let name = node_str(node, 0)
                    .unwrap_or_else(|| panic!("`{keyword}` node missing its name argument"));
                let block = Block {
                    keyword: match keyword {
                        "body" => "body",
                        "body-prefix" => "body-prefix",
                        _ => "row",
                    },
                    size: block_size(node, keyword, &name),
                    fields: parse_fields(node),
                    name,
                };
                match keyword {
                    "body" => bodies.push(block),
                    "body-prefix" => prefixes.push(block),
                    _ => rows.push(block),
                }
            }
            "header" | "submit" | "ack" => {
                let block = Block {
                    keyword: match keyword {
                        "header" => "header",
                        "submit" => "submit",
                        _ => "ack",
                    },
                    name: String::new(),
                    size: block_size(node, keyword, "<singleton>"),
                    fields: parse_fields(node),
                };
                match keyword {
                    "header" => header = Some(block),
                    "submit" => submit = Some(block),
                    _ => ack = Some(block),
                }
            }
            "object" | "event" | "candidate" => {
                let name = node_str(node, 0)
                    .unwrap_or_else(|| panic!("`{keyword}` node missing its name argument"));
                let kind = node_int(node, "kind")
                    .unwrap_or_else(|| panic!("`{keyword} \"{name}\"` missing kind="));
                let kind = u16::try_from(kind)
                    .unwrap_or_else(|_| panic!("`{keyword} \"{name}\"`: kind out of range"));
                let class = match keyword {
                    "object" => "object",
                    "event" => "event",
                    _ => "candidate",
                };
                kinds.push((class, name, kind));
            }
            other => panic!("unknown protocol child node `{other}`"),
        }
    }
    Kdl {
        header: header.expect("a `header` node"),
        submit: submit.expect("a `submit` node"),
        ack: ack.expect("an `ack` node"),
        bodies,
        prefixes,
        rows,
        kinds,
    }
}

pub fn find_block<'a>(blocks: &'a [Block], name: &str) -> &'a Block {
    blocks
        .iter()
        .find(|block| block.name == name)
        .unwrap_or_else(|| panic!("no `{}` block named `{name}`", blocks[0].keyword))
}

pub fn all_shell_file_kinds() -> [ShellFileKind; 56] {
    [
        ShellFileKind::Limits,
        ShellFileKind::Outputs,
        ShellFileKind::Catalog,
        ShellFileKind::Indicators,
        ShellFileKind::Negotiated,
        ShellFileKind::Refused,
        ShellFileKind::Submitted,
        ShellFileKind::ObjectPublished,
        ShellFileKind::AllocationResult,
        ShellFileKind::ResourceStatus,
        ShellFileKind::ResourceReleased,
        ShellFileKind::CandidateOutcome,
        ShellFileKind::FramePermit,
        ShellFileKind::Action,
        ShellFileKind::NativeOpening,
        ShellFileKind::NativeFocus,
        ShellFileKind::NativeFocusRevoked,
        ShellFileKind::NativeInput,
        ShellFileKind::NativeActivationOutcome,
        ShellFileKind::NativeClosed,
        ShellFileKind::CatalogActivationOutcome,
        ShellFileKind::IndicatorActivationOutcome,
        ShellFileKind::Negotiate,
        ShellFileKind::AllocationRequest,
        ShellFileKind::ResourceBegin,
        ShellFileKind::ResourceEnd,
        ShellFileKind::ResourceCancel,
        ShellFileKind::ResourceRetire,
        ShellFileKind::Candidate,
        ShellFileKind::FrameDemand,
        ShellFileKind::FrameDemandCancel,
        ShellFileKind::ActionAck,
        ShellFileKind::NativeAllocationRequest,
        ShellFileKind::NativeCandidate,
        ShellFileKind::NativeInputAck,
        ShellFileKind::NativeActivate,
        ShellFileKind::CatalogCandidate,
        ShellFileKind::CatalogActivate,
        ShellFileKind::IndicatorActivate,
        ShellFileKind::Descriptors,
        ShellFileKind::Tabs,
        ShellFileKind::Shortcuts,
        ShellFileKind::DescriptorOutcome,
        ShellFileKind::DescriptorActivation,
        ShellFileKind::ReferenceRequest,
        ShellFileKind::ReferenceOutcome,
        ShellFileKind::LauncherRequest,
        ShellFileKind::LauncherOutcome,
        ShellFileKind::LauncherActivation,
        ShellFileKind::LaunchOutcome,
        ShellFileKind::DescriptorCandidate,
        ShellFileKind::DescriptorActivationAck,
        ShellFileKind::TabsCandidate,
        ShellFileKind::ReferenceCandidate,
        ShellFileKind::LauncherCandidate,
        ShellFileKind::LauncherActivationAck,
    ]
}

/// Exhaustive: a new `ShellFileKind` variant fails this match at compile
/// time, forcing `all_shell_file_kinds` and the completeness test to be
/// updated.
pub fn kind_name(kind: ShellFileKind) -> &'static str {
    match kind {
        ShellFileKind::Limits => "Limits",
        ShellFileKind::Outputs => "Outputs",
        ShellFileKind::Catalog => "Catalog",
        ShellFileKind::Indicators => "Indicators",
        ShellFileKind::Negotiated => "Negotiated",
        ShellFileKind::Refused => "Refused",
        ShellFileKind::Submitted => "Submitted",
        ShellFileKind::ObjectPublished => "ObjectPublished",
        ShellFileKind::AllocationResult => "AllocationResult",
        ShellFileKind::ResourceStatus => "ResourceStatus",
        ShellFileKind::ResourceReleased => "ResourceReleased",
        ShellFileKind::CandidateOutcome => "CandidateOutcome",
        ShellFileKind::FramePermit => "FramePermit",
        ShellFileKind::Action => "Action",
        ShellFileKind::NativeOpening => "NativeOpening",
        ShellFileKind::NativeFocus => "NativeFocus",
        ShellFileKind::NativeFocusRevoked => "NativeFocusRevoked",
        ShellFileKind::NativeInput => "NativeInput",
        ShellFileKind::NativeActivationOutcome => "NativeActivationOutcome",
        ShellFileKind::NativeClosed => "NativeClosed",
        ShellFileKind::CatalogActivationOutcome => "CatalogActivationOutcome",
        ShellFileKind::IndicatorActivationOutcome => "IndicatorActivationOutcome",
        ShellFileKind::Negotiate => "Negotiate",
        ShellFileKind::AllocationRequest => "AllocationRequest",
        ShellFileKind::ResourceBegin => "ResourceBegin",
        ShellFileKind::ResourceEnd => "ResourceEnd",
        ShellFileKind::ResourceCancel => "ResourceCancel",
        ShellFileKind::ResourceRetire => "ResourceRetire",
        ShellFileKind::Candidate => "Candidate",
        ShellFileKind::FrameDemand => "FrameDemand",
        ShellFileKind::FrameDemandCancel => "FrameDemandCancel",
        ShellFileKind::ActionAck => "ActionAck",
        ShellFileKind::NativeAllocationRequest => "NativeAllocationRequest",
        ShellFileKind::NativeCandidate => "NativeCandidate",
        ShellFileKind::NativeInputAck => "NativeInputAck",
        ShellFileKind::NativeActivate => "NativeActivate",
        ShellFileKind::CatalogCandidate => "CatalogCandidate",
        ShellFileKind::CatalogActivate => "CatalogActivate",
        ShellFileKind::IndicatorActivate => "IndicatorActivate",
        ShellFileKind::Descriptors => "Descriptors",
        ShellFileKind::Tabs => "Tabs",
        ShellFileKind::Shortcuts => "Shortcuts",
        ShellFileKind::DescriptorOutcome => "DescriptorOutcome",
        ShellFileKind::DescriptorActivation => "DescriptorActivation",
        ShellFileKind::ReferenceRequest => "ReferenceRequest",
        ShellFileKind::ReferenceOutcome => "ReferenceOutcome",
        ShellFileKind::LauncherRequest => "LauncherRequest",
        ShellFileKind::LauncherOutcome => "LauncherOutcome",
        ShellFileKind::LauncherActivation => "LauncherActivation",
        ShellFileKind::LaunchOutcome => "LaunchOutcome",
        ShellFileKind::DescriptorCandidate => "DescriptorCandidate",
        ShellFileKind::DescriptorActivationAck => "DescriptorActivationAck",
        ShellFileKind::TabsCandidate => "TabsCandidate",
        ShellFileKind::ReferenceCandidate => "ReferenceCandidate",
        ShellFileKind::LauncherCandidate => "LauncherCandidate",
        ShellFileKind::LauncherActivationAck => "LauncherActivationAck",
    }
}

pub fn kind_class(kind: ShellFileKind) -> &'static str {
    match shell_file_class(kind) {
        ShellFileClass::Object => "object",
        ShellFileClass::Event => "event",
        ShellFileClass::Candidate => "candidate",
    }
}
