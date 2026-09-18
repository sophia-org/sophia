mod shell_schema;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as IoWrite;
use std::path::Path;
use std::process::{Command, Stdio};

use kdl::{KdlDocument, KdlNode};

mod control;

const SCHEMA_PATH: &str = "protocol/sophia-wm-v1.kdl";
const SHELL_SCHEMA_PATH: &str = "protocol/sophia-shell-v1.kdl";
const RUST_PATH: &str = "crates/sophia-protocol/src/ipc/wm_v1.rs";
const C_HEADER_PATH: &str = "bindings/c/sophia_wm_v1.h";
const C_SOURCE_PATH: &str = "bindings/c/sophia_wm_v1.c";
const DOC_PATH: &str = "docs/generated/sophia-wm-v1-wire.md";
const GOLDEN_PATH: &str = "protocol/golden/sophia-wm-v1.frames";
const MALFORMED_PATH: &str = "protocol/golden/sophia-wm-v1-malformed.frames";
const RECORD_GOLDEN_PATH: &str = "protocol/golden/sophia-wm-v1.records";

/// First record kind reserved for capability-gated extension chunks. Ordinary
/// records are allocated sequentially from 1 and must stay below it; see the
/// forward-compatibility rule in `docs/sophia-policy-ipc.md`.
const EXTENSION_RECORD_KIND_FLOOR: u64 = 0xFF00;
const SMT_FACTS_PATH: &str = "validation/architecture/generated/sophia-wm-v1-facts.smt2";

#[derive(Clone, Debug)]
struct Protocol {
    name: String,
    frame_version: u64,
    interface_major: u64,
    interface_revision: u64,
    max_outputs: u64,
    max_surfaces: u64,
    max_bindings: u64,
    capabilities: Vec<NamedValue>,
    outcomes: Vec<NamedValue>,
    records: Vec<Record>,
    extension_records: Vec<ExtensionRecord>,
    messages: Vec<Message>,
}

#[derive(Clone, Debug)]
struct NamedValue {
    name: String,
    value: u64,
}

#[derive(Clone, Debug)]
struct Message {
    name: String,
    kind: u64,
    direction: String,
    transaction: TransactionRule,
    fields: Vec<Field>,
}

#[derive(Clone, Debug)]
struct Record {
    name: String,
    transfer: String,
    kind: u64,
    max: u64,
    fields: Vec<Field>,
}

/// A capability-gated record riding an uncounted chunk after the ordinary
/// prefix. It shares the ordinary record shape but is never counted by a
/// `*Begin` message and never generates codec machinery: the frozen counted
/// path must not grow, which is the whole reason this shape exists.
#[derive(Clone, Debug)]
struct ExtensionRecord {
    record: Record,
    gate: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionRule {
    Zero,
    Required,
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    kind: FieldKind,
    reserved: bool,
    max: Option<u64>,
    sample: Sample,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldKind {
    U16,
    U32,
    U64,
    I32,
    Bytes,
    /// A fixed-length octet run. Records must stay fixed width, so bounded
    /// text belongs here rather than in `Bytes`, which carries a length and is
    /// therefore only legal as a message's final field.
    FixedBytes(u64),
}

#[derive(Clone, Debug)]
enum Sample {
    Integer(u64),
    Bytes(Vec<u8>),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("sophia-policy-protocol-gen: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let check = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err("usage: sophia-policy-protocol-gen [--check]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("generator path has no repository root")?;
    let schema_text = fs::read_to_string(root.join(SCHEMA_PATH))
        .map_err(|error| format!("read {SCHEMA_PATH}: {error}"))?;
    let schema = parse_schema(&schema_text)?;
    let shell_schema_text = fs::read_to_string(root.join(SHELL_SCHEMA_PATH))
        .map_err(|error| format!("read {SHELL_SCHEMA_PATH}: {error}"))?;
    shell_schema::validate(&shell_schema_text)?;
    let mut outputs = render_outputs(&schema)?;
    let control_text = fs::read_to_string(root.join("protocol/sophia-control-v1.kdl"))
        .map_err(|error| format!("read control schema: {error}"))?;
    outputs.extend(control::outputs(&control_text)?);

    let mut stale = Vec::new();
    for (relative, content) in outputs {
        let path = root.join(relative);
        if check {
            if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
                stale.push(relative);
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("create {}: {error}", parent.display()))?;
            }
            fs::write(&path, content)
                .map_err(|error| format!("write {}: {error}", path.display()))?;
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!("generated files are stale: {}", stale.join(", ")))
    }
}

fn parse_schema(text: &str) -> Result<Protocol, String> {
    let document: KdlDocument = text.parse().map_err(|error: kdl::KdlError| {
        let details = error
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        format!("parse schema: {details}")
    })?;
    let protocol = document
        .get("protocol")
        .ok_or("schema must contain one protocol node")?;
    let name = string_arg(protocol, 0)?;
    let children = protocol
        .children()
        .ok_or("protocol node must have children")?;
    let mut capabilities = Vec::new();
    let mut outcomes = Vec::new();
    let mut records = Vec::new();
    let mut extension_records = Vec::new();
    let mut messages = Vec::new();
    for node in children.nodes() {
        match node.name().value() {
            "capability" => capabilities.push(NamedValue {
                name: string_arg(node, 0)?,
                value: integer_property(node, "bit")?,
            }),
            "outcome" => outcomes.push(NamedValue {
                name: string_arg(node, 0)?,
                value: integer_property(node, "value")?,
            }),
            "record" => records.push(parse_record(node)?),
            "extension-record" => extension_records.push(ExtensionRecord {
                gate: string_property(node, "gate")?,
                record: parse_record(node)?,
            }),
            "message" => messages.push(parse_message(node)?),
            other => return Err(format!("unknown protocol node `{other}`")),
        }
    }
    let parsed = Protocol {
        name,
        frame_version: integer_property(protocol, "frame-version")?,
        interface_major: integer_property(protocol, "interface-major")?,
        interface_revision: integer_property(protocol, "interface-revision")?,
        max_outputs: integer_property(protocol, "max-outputs")?,
        max_surfaces: integer_property(protocol, "max-surfaces")?,
        max_bindings: integer_property(protocol, "max-bindings")?,
        capabilities,
        outcomes,
        records,
        extension_records,
        messages,
    };
    validate_schema(&parsed)?;
    Ok(parsed)
}

fn parse_record(node: &KdlNode) -> Result<Record, String> {
    let mut fields = Vec::new();
    let children = node.children().ok_or_else(|| {
        format!(
            "record `{}` must have fields",
            string_arg(node, 0).unwrap_or_default()
        )
    })?;
    for field in children.nodes() {
        if field.name().value() != "field" {
            return Err(format!("unknown record child `{}`", field.name().value()));
        }
        fields.push(parse_field(field)?);
    }
    Ok(Record {
        name: string_arg(node, 0)?,
        transfer: string_property(node, "transfer")?,
        kind: integer_property(node, "kind")?,
        max: integer_property(node, "max")?,
        fields,
    })
}

fn parse_message(node: &KdlNode) -> Result<Message, String> {
    let transaction = match string_property(node, "transaction")?.as_str() {
        "zero" => TransactionRule::Zero,
        "required" => TransactionRule::Required,
        other => return Err(format!("unknown transaction rule `{other}`")),
    };
    let mut fields = Vec::new();
    if let Some(children) = node.children() {
        for field in children.nodes() {
            if field.name().value() != "field" {
                return Err(format!("unknown message child `{}`", field.name().value()));
            }
            fields.push(parse_field(field)?);
        }
    }
    Ok(Message {
        name: string_arg(node, 0)?,
        kind: integer_property(node, "kind")?,
        direction: string_property(node, "direction")?,
        transaction,
        fields,
    })
}

fn parse_field(node: &KdlNode) -> Result<Field, String> {
    let kind = match string_property(node, "type")?.as_str() {
        "u16" => FieldKind::U16,
        "u32" => FieldKind::U32,
        "u64" => FieldKind::U64,
        "i32" => FieldKind::I32,
        "bytes" => FieldKind::Bytes,
        "u8" => {
            let count = integer_property(node, "count")?;
            if count == 0 || count > 256 {
                return Err(format!(
                    "field `{}` count must be between 1 and 256",
                    string_arg(node, 0).unwrap_or_default()
                ));
            }
            FieldKind::FixedBytes(count)
        }
        other => return Err(format!("unknown field type `{other}`")),
    };
    let reserved = node
        .get("reserved")
        .and_then(kdl::KdlValue::as_bool)
        .unwrap_or(false);
    let max = node
        .get("max")
        .and_then(kdl::KdlValue::as_integer)
        .map(|value| u64::try_from(value).map_err(|_| "negative max".to_string()))
        .transpose()?;
    let sample_value = node.get("sample").ok_or_else(|| {
        format!(
            "field `{}` lacks sample",
            string_arg(node, 0).unwrap_or_default()
        )
    })?;
    let sample = match kind {
        FieldKind::Bytes | FieldKind::FixedBytes(_) => Sample::Bytes(decode_hex(
            sample_value
                .as_string()
                .ok_or("bytes sample must be a hex string")?,
        )?),
        _ => Sample::Integer(
            u64::try_from(
                sample_value
                    .as_integer()
                    .ok_or("integer field sample must be an integer")?,
            )
            .map_err(|_| "integer sample must be nonnegative")?,
        ),
    };
    Ok(Field {
        name: string_arg(node, 0)?,
        kind,
        reserved,
        max,
        sample,
    })
}

fn validate_schema(protocol: &Protocol) -> Result<(), String> {
    if protocol.frame_version != 1 || protocol.interface_major == 0 {
        return Err("frame version must be 1 and interface major must be nonzero".into());
    }
    if protocol.messages.is_empty() {
        return Err("protocol must define messages".into());
    }
    let mut kinds = BTreeSet::new();
    let mut names = BTreeSet::new();
    for message in &protocol.messages {
        if message.kind > u16::MAX as u64 || !kinds.insert(message.kind) {
            return Err(format!(
                "message kind {} is invalid or duplicated",
                message.kind
            ));
        }
        if !names.insert(message.name.as_str()) {
            return Err(format!("message name `{}` is duplicated", message.name));
        }
        let mut fixed_len = 0_u64;
        let mut saw_bytes = false;
        let mut field_names = BTreeSet::new();
        for field in &message.fields {
            if !field_names.insert(field.name.as_str()) {
                return Err(format!(
                    "duplicate field `{}` in `{}`",
                    field.name, message.name
                ));
            }
            if saw_bytes {
                return Err(format!(
                    "bytes must be the final field in `{}`",
                    message.name
                ));
            }
            match field.kind {
                FieldKind::U16 => validate_integer_sample(field, u16::MAX as u64)?,
                FieldKind::U32 => validate_integer_sample(field, u32::MAX as u64)?,
                FieldKind::U64 => validate_integer_sample(field, u64::MAX)?,
                FieldKind::I32 => validate_integer_sample(field, i32::MAX as u64)?,
                FieldKind::Bytes => {
                    saw_bytes = true;
                    let max = field
                        .max
                        .ok_or_else(|| format!("bytes field `{}` must declare max", field.name))?;
                    if fixed_len + max > 64 * 1024 {
                        return Err(format!("payload `{}` exceeds 64 KiB", message.name));
                    }
                    if field.reserved {
                        return Err("bytes fields cannot be reserved".into());
                    }
                }
                FieldKind::FixedBytes(count) => {
                    if field.reserved {
                        return Err(format!(
                            "fixed octet field `{}` cannot be reserved",
                            field.name
                        ));
                    }
                    match &field.sample {
                        Sample::Bytes(bytes) if bytes.len() as u64 == count => {}
                        Sample::Bytes(bytes) => {
                            return Err(format!(
                                "field `{}` sample is {} bytes but declares {count}",
                                field.name,
                                bytes.len()
                            ));
                        }
                        Sample::Integer(_) => {
                            return Err(format!(
                                "field `{}` sample must be a hex string",
                                field.name
                            ));
                        }
                    }
                }
            }
            fixed_len += field_width(field.kind);
            if field.reserved && !matches!(field.sample, Sample::Integer(0)) {
                return Err(format!(
                    "reserved field `{}` sample must be zero",
                    field.name
                ));
            }
        }
        if fixed_len > 64 * 1024 {
            return Err(format!("payload `{}` exceeds 64 KiB", message.name));
        }
    }
    let mut record_names = BTreeSet::new();
    let mut record_keys = BTreeSet::new();
    for record in &protocol.records {
        // `0xFF00`-`0xFFFF` belongs to capability-gated extension chunks, which is
        // what lets a frozen revision carry new facts at all. An ordinary record
        // allocated here would collide with a future extension, and the collision
        // would surface as a frozen client rejecting a transfer in the field.
        // Ordinary kinds are allocated sequentially from 1, so nothing legitimate
        // reaches this range by accident -- but the rule was review-time only, and
        // a review-time rule about a number is one typo from being broken.
        if record.kind >= EXTENSION_RECORD_KIND_FLOOR {
            return Err(format!(
                "record `{}` claims kind {:#06x}, which is inside the reserved \
extension range {:#06x}-0xFFFF",
                record.name, record.kind, EXTENSION_RECORD_KIND_FLOOR
            ));
        }
        if !record_names.insert(record.name.as_str())
            || !record_keys.insert((record.transfer.as_str(), record.kind))
        {
            return Err(format!("record `{}` is duplicated", record.name));
        }
        validate_record_shape(record)?;
    }
    let capability_names: BTreeSet<&str> = protocol
        .capabilities
        .iter()
        .map(|capability| capability.name.as_str())
        .collect();
    for extension in &protocol.extension_records {
        let record = &extension.record;
        // An extension record is the one shape a frozen revision can still
        // gain. Its kind must sit inside the range the ordinary allocator
        // refuses, and it must name the capability that gates it: the layout,
        // the kind, and the gate are one contract, and a schema that states
        // only two of the three is how this record went undiscoverable for an
        // entire revision.
        if record.kind < EXTENSION_RECORD_KIND_FLOOR {
            return Err(format!(
                "extension record `{}` claims kind {:#06x}, below the reserved \
extension range floor {:#06x}",
                record.name, record.kind, EXTENSION_RECORD_KIND_FLOOR
            ));
        }
        if !capability_names.contains(extension.gate.as_str()) {
            return Err(format!(
                "extension record `{}` is gated on unknown capability `{}`",
                record.name, extension.gate
            ));
        }
        if !record_names.insert(record.name.as_str())
            || !record_keys.insert((record.transfer.as_str(), record.kind))
        {
            return Err(format!("record `{}` is duplicated", record.name));
        }
        validate_record_shape(record)?;
    }
    Ok(())
}

fn validate_record_shape(record: &Record) -> Result<(), String> {
    if record.max == 0 || record.max > u32::MAX as u64 {
        return Err(format!("record `{}` has invalid max", record.name));
    }
    if !matches!(record.transfer.as_str(), "snapshot" | "projection") {
        return Err(format!("record `{}` has invalid transfer", record.name));
    }
    if record.kind == 0 || record.kind > u16::MAX as u64 {
        return Err(format!("record `{}` has invalid kind", record.name));
    }
    let mut field_names = BTreeSet::new();
    for field in &record.fields {
        if field.kind == FieldKind::Bytes || field.max.is_some() {
            return Err(format!("record `{}` must be fixed width", record.name));
        }
        if !field_names.insert(field.name.as_str()) {
            return Err(format!(
                "duplicate field `{}` in `{}`",
                field.name, record.name
            ));
        }
        match field.kind {
            FieldKind::U16 => validate_integer_sample(field, u16::MAX as u64)?,
            FieldKind::U32 => validate_integer_sample(field, u32::MAX as u64)?,
            FieldKind::U64 => validate_integer_sample(field, u64::MAX)?,
            FieldKind::I32 => validate_integer_sample(field, i32::MAX as u64)?,
            FieldKind::Bytes => unreachable!(),
            // A fixed run stays fixed width, so it is legal here. The
            // sample must fill it exactly; a short sample would encode a
            // different width than the record declares.
            FieldKind::FixedBytes(count) => {
                if field.reserved {
                    return Err(format!(
                        "fixed octet field `{}` cannot be reserved",
                        field.name
                    ));
                }
                match &field.sample {
                    Sample::Bytes(bytes) if bytes.len() as u64 == count => {}
                    Sample::Bytes(bytes) => {
                        return Err(format!(
                            "field `{}` sample is {} bytes but declares {count}",
                            field.name,
                            bytes.len()
                        ));
                    }
                    Sample::Integer(_) => {
                        return Err(format!(
                            "field `{}` sample must be a hex string",
                            field.name
                        ));
                    }
                }
            }
        }
        if field.reserved && !matches!(field.sample, Sample::Integer(0)) {
            return Err(format!(
                "reserved field `{}` sample must be zero",
                field.name
            ));
        }
    }
    Ok(())
}

fn validate_integer_sample(field: &Field, maximum: u64) -> Result<(), String> {
    match field.sample {
        Sample::Integer(value) if value <= maximum => Ok(()),
        Sample::Integer(_) => Err(format!("sample for `{}` is out of range", field.name)),
        Sample::Bytes(_) => Err(format!("sample for `{}` is not an integer", field.name)),
    }
}

fn render_outputs(protocol: &Protocol) -> Result<BTreeMap<&'static str, String>, String> {
    let mut outputs = BTreeMap::new();
    outputs.insert(RUST_PATH, format_rust(&render_rust(protocol))?);
    outputs.insert(C_HEADER_PATH, render_c_header(protocol));
    outputs.insert(C_SOURCE_PATH, render_c_source(protocol));
    outputs.insert(DOC_PATH, render_docs(protocol));
    outputs.insert(GOLDEN_PATH, render_golden(protocol)?);
    outputs.insert(MALFORMED_PATH, render_malformed(protocol)?);
    outputs.insert(RECORD_GOLDEN_PATH, render_record_golden(protocol)?);
    outputs.insert(SMT_FACTS_PATH, render_smt_facts(protocol));
    Ok(outputs)
}

fn render_smt_facts(protocol: &Protocol) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "; @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "; Source: {SCHEMA_PATH}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_frame_version () Int {})",
        protocol.frame_version
    )
    .unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_interface_major () Int {})",
        protocol.interface_major
    )
    .unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_interface_revision () Int {})",
        protocol.interface_revision
    )
    .unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_max_outputs () Int {})",
        protocol.max_outputs
    )
    .unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_max_surfaces () Int {})",
        protocol.max_surfaces
    )
    .unwrap();
    writeln!(
        out,
        "(define-fun wm_v1_max_bindings () Int {})",
        protocol.max_bindings
    )
    .unwrap();

    for record in &protocol.records {
        let name = snake(&record.name);
        writeln!(out).unwrap();
        writeln!(
            out,
            "(define-fun {name}_record_width () Int {})",
            record_width(record)
        )
        .unwrap();
        writeln!(out, "(define-fun {name}_record_max () Int {})", record.max).unwrap();
    }

    for message in &protocol.messages {
        let name = snake(&message.name);
        let variable_max = message
            .fields
            .iter()
            .find(|field| field.kind == FieldKind::Bytes)
            .and_then(|field| field.max)
            .unwrap_or(0);
        let fixed = fixed_prefix_len(message);
        writeln!(out).unwrap();
        writeln!(
            out,
            "(define-fun {name}_fixed_payload_bytes () Int {fixed})"
        )
        .unwrap();
        writeln!(
            out,
            "(define-fun {name}_variable_payload_max () Int {variable_max})"
        )
        .unwrap();
        writeln!(
            out,
            "(define-fun {name}_max_payload_bytes () Int {})",
            fixed + variable_max
        )
        .unwrap();
        for field in &message.fields {
            writeln!(
                out,
                "(define-fun {name}_field_{}_width () Int {})",
                field.name,
                field_width(field.kind)
            )
            .unwrap();
            if let Some(max) = field.max {
                writeln!(
                    out,
                    "(define-fun {name}_field_{}_max () Int {max})",
                    field.name
                )
                .unwrap();
            }
        }
    }
    out
}

fn format_rust(source: &str) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start rustfmt: {error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("rustfmt stdin is unavailable")?
        .write_all(source.as_bytes())
        .map_err(|error| format!("write rustfmt input: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for rustfmt: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("rustfmt output is not UTF-8: {error}"))
}

fn render_rust(protocol: &Protocol) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "// Source: {SCHEMA_PATH}\n").unwrap();
    writeln!(out, "use crate::TransactionId;").unwrap();
    writeln!(
        out,
        "use super::cursor::{{Cursor, push_i32, push_u16, push_u32, push_u64}};"
    )
    .unwrap();
    writeln!(out, "use super::frame::{{decode_frame, encode_frame}};").unwrap();
    writeln!(
        out,
        "use super::types::{{IpcCodecError, IpcMessageKind}};\n"
    )
    .unwrap();
    writeln!(
        out,
        "pub const SOPHIA_WM_INTERFACE_MAJOR: u16 = {};",
        protocol.interface_major
    )
    .unwrap();
    writeln!(
        out,
        "pub const SOPHIA_WM_INTERFACE_REVISION: u16 = {};",
        protocol.interface_revision
    )
    .unwrap();
    writeln!(
        out,
        "pub const SOPHIA_WM_MAX_OUTPUTS: usize = {};",
        protocol.max_outputs
    )
    .unwrap();
    writeln!(
        out,
        "pub const SOPHIA_WM_MAX_SURFACES: usize = {};",
        protocol.max_surfaces
    )
    .unwrap();
    writeln!(
        out,
        "pub const SOPHIA_WM_MAX_BINDINGS: usize = {};\n",
        protocol.max_bindings
    )
    .unwrap();
    for capability in &protocol.capabilities {
        writeln!(
            out,
            "pub const SOPHIA_WM_CAPABILITY_{}: u64 = 1 << {};",
            screaming(&capability.name),
            capability.value
        )
        .unwrap();
    }
    writeln!(out).unwrap();
    for outcome in &protocol.outcomes {
        writeln!(
            out,
            "pub const SOPHIA_WM_OUTCOME_{}: u16 = {};",
            screaming(&outcome.name),
            outcome.value
        )
        .unwrap();
    }
    writeln!(out).unwrap();
    for record in &protocol.records {
        render_rust_record(record, &mut out);
    }
    for message in &protocol.messages {
        render_rust_message(protocol, message, &mut out);
    }
    out
}

fn render_rust_record(record: &Record, out: &mut String) {
    let rust_name = format!("WmV1{}Record", record.name);
    let snake = snake(&record.name);
    let constant = screaming(&snake);
    let width = record_width(record);
    writeln!(
        out,
        "pub const {constant}_RECORD_KIND: u16 = {};",
        record.kind
    )
    .unwrap();
    writeln!(out, "pub const {constant}_RECORD_SIZE: usize = {width};").unwrap();
    writeln!(
        out,
        "pub const {constant}_RECORD_MAX: usize = {};\n",
        record.max
    )
    .unwrap();
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]").unwrap();
    writeln!(out, "pub struct {rust_name} {{").unwrap();
    for field in &record.fields {
        if !field.reserved {
            writeln!(out, "    pub {}: {},", field.name, rust_type(field.kind)).unwrap();
        }
    }
    writeln!(out, "}}\n").unwrap();
    writeln!(out, "pub fn encode_wm_v1_{snake}_records(records: &[{rust_name}]) -> Result<Vec<u8>, IpcCodecError> {{").unwrap();
    writeln!(out, "    if records.len() > {} {{", record.max).unwrap();
    writeln!(
        out,
        "        return Err(IpcCodecError::CountTooLarge {{ count: records.len(), max: {} }});",
        record.max
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    let mut data = Vec::with_capacity(records.len() * {width});"
    )
    .unwrap();
    writeln!(out, "    for record in records {{").unwrap();
    for field in &record.fields {
        if let FieldKind::FixedBytes(_) = field.kind {
            writeln!(
                out,
                "        data.extend_from_slice(&record.{});",
                field.name
            )
            .unwrap();
        } else if field.reserved {
            writeln!(out, "        {}(&mut data, 0);", rust_push(field.kind)).unwrap();
        } else {
            writeln!(
                out,
                "        {}(&mut data, record.{});",
                rust_push(field.kind),
                field.name
            )
            .unwrap();
        }
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    Ok(data)\n}}\n").unwrap();
    writeln!(out, "pub fn decode_wm_v1_{snake}_records(data: &[u8], item_count: u32) -> Result<Vec<{rust_name}>, IpcCodecError> {{").unwrap();
    writeln!(out, "    let count = item_count as usize;").unwrap();
    writeln!(
        out,
        "    if count > {} {{ return Err(IpcCodecError::CountTooLarge {{ count, max: {} }}); }}",
        record.max, record.max
    )
    .unwrap();
    writeln!(out, "    let expected = count.checked_mul({width}).ok_or(IpcCodecError::CountTooLarge {{ count, max: {} }})?;", record.max).unwrap();
    writeln!(
        out,
        "    if data.len() < expected {{ return Err(IpcCodecError::Truncated); }}"
    )
    .unwrap();
    writeln!(out, "    if data.len() > expected {{ return Err(IpcCodecError::TrailingBytes(data.len() - expected)); }}").unwrap();
    writeln!(out, "    let mut cursor = Cursor::new(data);").unwrap();
    writeln!(out, "    let mut records = Vec::with_capacity(count);").unwrap();
    writeln!(out, "    for _ in 0..count {{").unwrap();
    for field in &record.fields {
        if let FieldKind::FixedBytes(count) = field.kind {
            writeln!(out, "        let mut {} = [0u8; {count}];", field.name).unwrap();
            writeln!(
                out,
                "        {}.copy_from_slice(cursor.slice({count})?);",
                field.name
            )
            .unwrap();
        } else if field.reserved {
            writeln!(
                out,
                "        let reserved = cursor.{}()?;",
                rust_cursor(field.kind)
            )
            .unwrap();
            let cast = if field.kind == FieldKind::U32 {
                ""
            } else {
                " as u32"
            };
            writeln!(out, "        if reserved != 0 {{ return Err(IpcCodecError::ReservedNonZero(reserved{cast})); }}").unwrap();
        } else {
            writeln!(
                out,
                "        let {} = cursor.{}()?;",
                field.name,
                rust_cursor(field.kind)
            )
            .unwrap();
        }
    }
    writeln!(out, "        records.push({rust_name} {{").unwrap();
    for field in &record.fields {
        if !field.reserved {
            writeln!(out, "            {},", field.name).unwrap();
        }
    }
    writeln!(out, "        }});").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    cursor.finish()?;").unwrap();
    writeln!(out, "    Ok(records)\n}}\n").unwrap();
}

fn render_rust_message(_protocol: &Protocol, message: &Message, out: &mut String) {
    let rust_name = format!("WmV1{}", message.name);
    let snake = snake(&message.name);
    writeln!(out, "#[derive(Clone, Debug, Eq, PartialEq)]").unwrap();
    writeln!(out, "pub struct {rust_name} {{").unwrap();
    for field in &message.fields {
        if !field.reserved {
            writeln!(out, "    pub {}: {},", field.name, rust_type(field.kind)).unwrap();
        }
    }
    writeln!(out, "}}\n").unwrap();

    let transaction_arg = if message.transaction == TransactionRule::Required {
        "transaction: TransactionId, "
    } else {
        ""
    };
    writeln!(out, "pub fn encode_wm_v1_{snake}_frame({transaction_arg}message: &{rust_name}) -> Result<Vec<u8>, IpcCodecError> {{").unwrap();
    if message.transaction == TransactionRule::Required {
        writeln!(
            out,
            "    if !transaction.is_valid() {{ return Err(IpcCodecError::InvalidTransaction(0)); }}"
        )
        .unwrap();
    }
    for field in &message.fields {
        if field.kind == FieldKind::Bytes {
            writeln!(
                out,
                "    if message.{}.len() > {} {{",
                field.name,
                field.max.unwrap()
            )
            .unwrap();
            writeln!(out, "        return Err(IpcCodecError::FieldTooLarge {{ field: \"{}\", len: message.{}.len(), max: {} }});", field.name, field.name, field.max.unwrap()).unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "    let mut payload = Vec::new();").unwrap();
    for field in &message.fields {
        if field.reserved {
            writeln!(out, "    {}(&mut payload, 0);", rust_push(field.kind)).unwrap();
        } else if matches!(field.kind, FieldKind::Bytes | FieldKind::FixedBytes(_)) {
            writeln!(
                out,
                "    payload.extend_from_slice(&message.{});",
                field.name
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "    {}(&mut payload, message.{});",
                rust_push(field.kind),
                field.name
            )
            .unwrap();
        }
    }
    let transaction = if message.transaction == TransactionRule::Required {
        "transaction"
    } else {
        "TransactionId::INVALID"
    };
    writeln!(
        out,
        "    encode_frame(IpcMessageKind::WmV1{}, {transaction}, &payload)",
        message.name
    )
    .unwrap();
    writeln!(out, "}}\n").unwrap();

    let return_type = if message.transaction == TransactionRule::Required {
        format!("(TransactionId, {rust_name})")
    } else {
        rust_name.clone()
    };
    writeln!(
        out,
        "pub fn decode_wm_v1_{snake}_frame(frame: &[u8]) -> Result<{return_type}, IpcCodecError> {{"
    )
    .unwrap();
    writeln!(out, "    let (header, payload) = decode_frame(frame)?;").unwrap();
    writeln!(
        out,
        "    if header.message_kind != IpcMessageKind::WmV1{} {{",
        message.name
    )
    .unwrap();
    writeln!(out, "        return Err(IpcCodecError::InvalidEnum {{ field: \"message_kind\", value: header.message_kind as u32 }});").unwrap();
    writeln!(out, "    }}").unwrap();
    match message.transaction {
        TransactionRule::Zero => {
            writeln!(out, "    if header.transaction.is_valid() {{").unwrap();
            writeln!(
                out,
                "        return Err(IpcCodecError::InvalidTransaction(header.transaction.raw()));"
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
        TransactionRule::Required => {
            writeln!(out, "    if !header.transaction.is_valid() {{").unwrap();
            writeln!(
                out,
                "        return Err(IpcCodecError::InvalidTransaction(0));"
            )
            .unwrap();
            writeln!(out, "    }}").unwrap();
        }
    }
    writeln!(out, "    let mut cursor = Cursor::new(payload);").unwrap();
    for field in &message.fields {
        if field.reserved {
            writeln!(
                out,
                "    let reserved = cursor.{}()?;",
                rust_cursor(field.kind)
            )
            .unwrap();
            writeln!(out, "    if reserved != 0 {{ return Err(IpcCodecError::ReservedNonZero(reserved as u32)); }}").unwrap();
        } else if field.kind == FieldKind::Bytes {
            writeln!(
                out,
                "    let len = payload.len().saturating_sub({});",
                fixed_prefix_len(message)
            )
            .unwrap();
            writeln!(out, "    if len > {} {{ return Err(IpcCodecError::FieldTooLarge {{ field: \"{}\", len, max: {} }}); }}", field.max.unwrap(), field.name, field.max.unwrap()).unwrap();
            writeln!(out, "    let {} = cursor.slice(len)?.to_vec();", field.name).unwrap();
        } else if let FieldKind::FixedBytes(count) = field.kind {
            writeln!(out, "    let mut {} = [0u8; {count}];", field.name).unwrap();
            writeln!(
                out,
                "    {}.copy_from_slice(cursor.slice({count})?);",
                field.name
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "    let {} = cursor.{}()?;",
                field.name,
                rust_cursor(field.kind)
            )
            .unwrap();
        }
    }
    writeln!(out, "    cursor.finish()?;").unwrap();
    writeln!(out, "    let message = {rust_name} {{").unwrap();
    for field in &message.fields {
        if !field.reserved {
            writeln!(out, "        {},", field.name).unwrap();
        }
    }
    writeln!(out, "    }};").unwrap();
    if message.transaction == TransactionRule::Required {
        writeln!(out, "    Ok((header.transaction, message))").unwrap();
    } else {
        writeln!(out, "    Ok(message)").unwrap();
    }
    writeln!(out, "}}\n").unwrap();
}

fn render_c_header(protocol: &Protocol) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "/* @generated by sophia-policy-protocol-gen; do not edit. */"
    )
    .unwrap();
    writeln!(out, "#ifndef SOPHIA_WM_V1_H\n#define SOPHIA_WM_V1_H\n").unwrap();
    writeln!(out, "#include <stddef.h>\n#include <stdint.h>\n").unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_INTERFACE_MAJOR {}u",
        protocol.interface_major
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_INTERFACE_REVISION {}u",
        protocol.interface_revision
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_MAX_OUTPUTS {}u",
        protocol.max_outputs
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_MAX_SURFACES {}u",
        protocol.max_surfaces
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_MAX_BINDINGS {}u\n",
        protocol.max_bindings
    )
    .unwrap();
    for capability in &protocol.capabilities {
        writeln!(
            out,
            "#define SOPHIA_WM_CAPABILITY_{} (UINT64_C(1) << {})",
            screaming(&capability.name),
            capability.value
        )
        .unwrap();
    }
    writeln!(out).unwrap();
    for outcome in &protocol.outcomes {
        writeln!(
            out,
            "#define SOPHIA_WM_OUTCOME_{} {}u",
            screaming(&outcome.name),
            outcome.value
        )
        .unwrap();
    }
    writeln!(out, "\nenum sophia_wm_v1_status {{").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_OK = 0,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_TRUNCATED = 1,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_BAD_MAGIC = 2,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_UNSUPPORTED_FRAME_VERSION = 3,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_WRONG_MESSAGE_KIND = 4,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_PAYLOAD_TOO_LARGE = 5,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_RESERVED_NONZERO = 6,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_TRAILING_BYTES = 7,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_INVALID_TRANSACTION = 8,").unwrap();
    writeln!(out, "    SOPHIA_WM_V1_FIELD_TOO_LARGE = 9\n}};\n").unwrap();
    for record in &protocol.records {
        render_c_record_header(record, &mut out);
    }
    for message in &protocol.messages {
        let c_name = snake(&message.name);
        writeln!(out, "struct sophia_wm_v1_{c_name} {{").unwrap();
        for field in &message.fields {
            if field.reserved {
                continue;
            }
            if field.kind == FieldKind::Bytes {
                writeln!(out, "    const uint8_t *{};", field.name).unwrap();
                writeln!(out, "    size_t {}_len;", field.name).unwrap();
            } else if let FieldKind::FixedBytes(count) = field.kind {
                writeln!(out, "    uint8_t {}[{count}];", field.name).unwrap();
            } else {
                writeln!(out, "    {} {};", c_type(field.kind), field.name).unwrap();
            }
        }
        writeln!(out, "}};").unwrap();
        let tx_arg = if message.transaction == TransactionRule::Required {
            "uint64_t transaction, "
        } else {
            ""
        };
        writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_encode_{c_name}({tx_arg}const struct sophia_wm_v1_{c_name} *message, uint8_t *out, size_t capacity, size_t *written);").unwrap();
        let tx_out = if message.transaction == TransactionRule::Required {
            "uint64_t *transaction, "
        } else {
            ""
        };
        writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_decode_{c_name}(const uint8_t *frame, size_t frame_len, {tx_out}struct sophia_wm_v1_{c_name} *message);\n").unwrap();
    }
    writeln!(out, "#endif").unwrap();
    out
}

fn render_c_record_header(record: &Record, out: &mut String) {
    let c_name = snake(&record.name);
    let constant = screaming(&c_name);
    writeln!(
        out,
        "#define SOPHIA_WM_V1_{constant}_RECORD_KIND {}u",
        record.kind
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_V1_{constant}_RECORD_SIZE {}u",
        record_width(record)
    )
    .unwrap();
    writeln!(
        out,
        "#define SOPHIA_WM_V1_{constant}_RECORD_MAX {}u",
        record.max
    )
    .unwrap();
    writeln!(out, "struct sophia_wm_v1_{c_name}_record {{").unwrap();
    for field in &record.fields {
        if field.reserved {
            continue;
        }
        if let FieldKind::FixedBytes(count) = field.kind {
            writeln!(out, "    uint8_t {}[{count}];", field.name).unwrap();
        } else {
            writeln!(out, "    {} {};", c_type(field.kind), field.name).unwrap();
        }
    }
    writeln!(out, "}};").unwrap();
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_encode_{c_name}_record(const struct sophia_wm_v1_{c_name}_record *record, uint8_t *out, size_t capacity);").unwrap();
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_decode_{c_name}_record(const uint8_t *data, size_t data_len, size_t index, struct sophia_wm_v1_{c_name}_record *record);\n").unwrap();
}

fn render_c_source(protocol: &Protocol) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "/* @generated by sophia-policy-protocol-gen; do not edit. */"
    )
    .unwrap();
    writeln!(out, "#include \"sophia_wm_v1.h\"\n").unwrap();
    writeln!(out, "#define SOPHIA_IPC_MAGIC UINT32_C(0x48504f53)").unwrap();
    writeln!(
        out,
        "#define SOPHIA_IPC_FRAME_VERSION {}u",
        protocol.frame_version
    )
    .unwrap();
    writeln!(out, "#define SOPHIA_IPC_HEADER_LEN 24u").unwrap();
    writeln!(out, "#define SOPHIA_IPC_MAX_PAYLOAD_LEN 65536u\n").unwrap();
    out.push_str(C_HELPERS);
    for record in &protocol.records {
        render_c_record_source(record, &mut out);
    }
    for message in &protocol.messages {
        render_c_message(message, &mut out);
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn render_c_record_source(record: &Record, out: &mut String) {
    let c_name = snake(&record.name);
    let width = record_width(record);
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_encode_{c_name}_record(const struct sophia_wm_v1_{c_name}_record *record, uint8_t *out, size_t capacity) {{").unwrap();
    writeln!(
        out,
        "    if (capacity < {width}u) return SOPHIA_WM_V1_TRUNCATED;"
    )
    .unwrap();
    let mut offset = 0;
    for field in &record.fields {
        if let FieldKind::FixedBytes(count) = field.kind {
            writeln!(
                out,
                "    put_bytes(out + {offset}, record->{}, {count}u);",
                field.name
            )
            .unwrap();
            offset += count;
            continue;
        }
        let value = if field.reserved {
            "0".to_string()
        } else {
            format!("record->{}", field.name)
        };
        writeln!(out, "    {}(out + {offset}, {value});", c_put(field.kind)).unwrap();
        offset += field_width(field.kind);
    }
    writeln!(out, "    return SOPHIA_WM_V1_OK;\n}}\n").unwrap();
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_decode_{c_name}_record(const uint8_t *data, size_t data_len, size_t index, struct sophia_wm_v1_{c_name}_record *record) {{").unwrap();
    writeln!(out, "    if (data_len % {width}u != 0) return data_len < {width}u ? SOPHIA_WM_V1_TRUNCATED : SOPHIA_WM_V1_TRAILING_BYTES;").unwrap();
    writeln!(out, "    size_t count = data_len / {width}u;").unwrap();
    writeln!(
        out,
        "    if (count > {}u) return SOPHIA_WM_V1_FIELD_TOO_LARGE;",
        record.max
    )
    .unwrap();
    writeln!(
        out,
        "    if (index >= count) return SOPHIA_WM_V1_TRUNCATED;"
    )
    .unwrap();
    writeln!(out, "    const uint8_t *cursor = data + index * {width}u;").unwrap();
    let mut offset = 0;
    for field in &record.fields {
        if let FieldKind::FixedBytes(count) = field.kind {
            writeln!(
                out,
                "    get_bytes(cursor + {offset}, record->{}, {count}u);",
                field.name
            )
            .unwrap();
            offset += count;
            continue;
        }
        if field.reserved {
            writeln!(
                out,
                "    if ({}(cursor + {offset}) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;",
                c_get(field.kind)
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "    record->{} = {}(cursor + {offset});",
                field.name,
                c_get(field.kind)
            )
            .unwrap();
        }
        offset += field_width(field.kind);
    }
    writeln!(out, "    return SOPHIA_WM_V1_OK;\n}}\n").unwrap();
}

fn render_c_message(message: &Message, out: &mut String) {
    let c_name = snake(&message.name);
    let fixed_len = fixed_prefix_len(message);
    let bytes_field = message
        .fields
        .iter()
        .find(|field| field.kind == FieldKind::Bytes);
    let tx_arg = if message.transaction == TransactionRule::Required {
        "uint64_t transaction, "
    } else {
        ""
    };
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_encode_{c_name}({tx_arg}const struct sophia_wm_v1_{c_name} *message, uint8_t *out, size_t capacity, size_t *written) {{").unwrap();
    if message.transaction == TransactionRule::Required {
        writeln!(
            out,
            "    if (transaction == 0) return SOPHIA_WM_V1_INVALID_TRANSACTION;"
        )
        .unwrap();
    }
    if let Some(field) = bytes_field {
        writeln!(
            out,
            "    if (message->{}_len > {}u) return SOPHIA_WM_V1_FIELD_TOO_LARGE;",
            field.name,
            field.max.unwrap()
        )
        .unwrap();
        writeln!(
            out,
            "    size_t payload_len = {fixed_len}u + message->{}_len;",
            field.name
        )
        .unwrap();
    } else {
        writeln!(out, "    size_t payload_len = {fixed_len}u;").unwrap();
    }
    let tx = if message.transaction == TransactionRule::Required {
        "transaction"
    } else {
        "UINT64_C(0)"
    };
    writeln!(out, "    enum sophia_wm_v1_status status = write_header({}u, {tx}, payload_len, out, capacity, written);", message.kind).unwrap();
    writeln!(out, "    if (status != SOPHIA_WM_V1_OK) return status;").unwrap();
    writeln!(out, "    uint8_t *cursor = out + SOPHIA_IPC_HEADER_LEN;").unwrap();
    let mut offset = 0;
    for field in &message.fields {
        match field.kind {
            FieldKind::FixedBytes(count) => {
                writeln!(
                    out,
                    "    put_bytes(cursor + {offset}, message->{}, {count}u);",
                    field.name
                )
                .unwrap();
            }
            FieldKind::U16 => {
                let value = if field.reserved {
                    "0".to_string()
                } else {
                    format!("message->{}", field.name)
                };
                writeln!(out, "    put_u16(cursor + {offset}, {value});").unwrap();
            }
            FieldKind::U32 => {
                let value = if field.reserved {
                    "0".to_string()
                } else {
                    format!("message->{}", field.name)
                };
                writeln!(out, "    put_u32(cursor + {offset}, {value});").unwrap();
            }
            FieldKind::U64 => {
                let value = if field.reserved {
                    "0".to_string()
                } else {
                    format!("message->{}", field.name)
                };
                writeln!(out, "    put_u64(cursor + {offset}, {value});").unwrap();
            }
            FieldKind::I32 => {
                let value = if field.reserved {
                    "0".to_string()
                } else {
                    format!("message->{}", field.name)
                };
                writeln!(out, "    put_i32(cursor + {offset}, {value});").unwrap();
            }
            FieldKind::Bytes => {
                writeln!(out, "    for (size_t index = 0; index < message->{}_len; ++index) cursor[{offset}u + index] = message->{}[index];", field.name, field.name).unwrap();
            }
        }
        offset += field_width(field.kind);
    }
    writeln!(out, "    return SOPHIA_WM_V1_OK;\n}}\n").unwrap();

    let tx_out = if message.transaction == TransactionRule::Required {
        "uint64_t *transaction, "
    } else {
        ""
    };
    writeln!(out, "enum sophia_wm_v1_status sophia_wm_v1_decode_{c_name}(const uint8_t *frame, size_t frame_len, {tx_out}struct sophia_wm_v1_{c_name} *message) {{").unwrap();
    writeln!(out, "    uint64_t frame_transaction = 0;").unwrap();
    writeln!(out, "    size_t payload_len = 0;").unwrap();
    writeln!(out, "    enum sophia_wm_v1_status status = read_header(frame, frame_len, {}u, &frame_transaction, &payload_len);", message.kind).unwrap();
    writeln!(out, "    if (status != SOPHIA_WM_V1_OK) return status;").unwrap();
    match message.transaction {
        TransactionRule::Zero => writeln!(
            out,
            "    if (frame_transaction != 0) return SOPHIA_WM_V1_INVALID_TRANSACTION;"
        )
        .unwrap(),
        TransactionRule::Required => {
            writeln!(
                out,
                "    if (frame_transaction == 0) return SOPHIA_WM_V1_INVALID_TRANSACTION;"
            )
            .unwrap();
            writeln!(out, "    *transaction = frame_transaction;").unwrap();
        }
    }
    if let Some(field) = bytes_field {
        writeln!(
            out,
            "    if (payload_len < {fixed_len}u) return SOPHIA_WM_V1_TRUNCATED;"
        )
        .unwrap();
        writeln!(out, "    size_t bytes_len = payload_len - {fixed_len}u;").unwrap();
        writeln!(
            out,
            "    if (bytes_len > {}u) return SOPHIA_WM_V1_FIELD_TOO_LARGE;",
            field.max.unwrap()
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "    if (payload_len < {fixed_len}u) return SOPHIA_WM_V1_TRUNCATED;"
        )
        .unwrap();
        writeln!(
            out,
            "    if (payload_len > {fixed_len}u) return SOPHIA_WM_V1_TRAILING_BYTES;"
        )
        .unwrap();
    }
    writeln!(
        out,
        "    const uint8_t *cursor = frame + SOPHIA_IPC_HEADER_LEN;"
    )
    .unwrap();
    let mut offset = 0;
    for field in &message.fields {
        match field.kind {
            FieldKind::FixedBytes(count) => {
                writeln!(
                    out,
                    "    get_bytes(cursor + {offset}, message->{}, {count}u);",
                    field.name
                )
                .unwrap();
            }
            FieldKind::U16 if field.reserved => writeln!(
                out,
                "    if (get_u16(cursor + {offset}) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;"
            )
            .unwrap(),
            FieldKind::U32 if field.reserved => writeln!(
                out,
                "    if (get_u32(cursor + {offset}) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;"
            )
            .unwrap(),
            FieldKind::U64 if field.reserved => writeln!(
                out,
                "    if (get_u64(cursor + {offset}) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;"
            )
            .unwrap(),
            FieldKind::I32 if field.reserved => writeln!(
                out,
                "    if (get_i32(cursor + {offset}) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;"
            )
            .unwrap(),
            FieldKind::U16 => writeln!(
                out,
                "    message->{} = get_u16(cursor + {offset});",
                field.name
            )
            .unwrap(),
            FieldKind::U32 => writeln!(
                out,
                "    message->{} = get_u32(cursor + {offset});",
                field.name
            )
            .unwrap(),
            FieldKind::U64 => writeln!(
                out,
                "    message->{} = get_u64(cursor + {offset});",
                field.name
            )
            .unwrap(),
            FieldKind::I32 => writeln!(
                out,
                "    message->{} = get_i32(cursor + {offset});",
                field.name
            )
            .unwrap(),
            FieldKind::Bytes => {
                writeln!(out, "    message->{} = cursor + {offset};", field.name).unwrap();
                writeln!(out, "    message->{}_len = bytes_len;", field.name).unwrap();
            }
        }
        offset += field_width(field.kind);
    }
    writeln!(out, "    return SOPHIA_WM_V1_OK;\n}}\n").unwrap();
}

const C_HELPERS: &str = r#"static void put_u16(uint8_t *out, uint16_t value) {
    out[0] = (uint8_t)value;
    out[1] = (uint8_t)(value >> 8);
}

static void put_u32(uint8_t *out, uint32_t value) {
    for (size_t index = 0; index < 4; ++index) out[index] = (uint8_t)(value >> (8 * index));
}

static void put_u64(uint8_t *out, uint64_t value) {
    for (size_t index = 0; index < 8; ++index) out[index] = (uint8_t)(value >> (8 * index));
}

static void put_i32(uint8_t *out, int32_t value) {
    put_u32(out, (uint32_t)value);
}

static uint16_t get_u16(const uint8_t *in) {
    return (uint16_t)in[0] | ((uint16_t)in[1] << 8);
}

static uint32_t get_u32(const uint8_t *in) {
    uint32_t value = 0;
    for (size_t index = 0; index < 4; ++index) value |= ((uint32_t)in[index]) << (8 * index);
    return value;
}

static uint64_t get_u64(const uint8_t *in) {
    uint64_t value = 0;
    for (size_t index = 0; index < 8; ++index) value |= ((uint64_t)in[index]) << (8 * index);
    return value;
}

static int32_t get_i32(const uint8_t *in) {
    return (int32_t)get_u32(in);
}

static void put_bytes(uint8_t *out, const uint8_t *value, size_t len) {
    for (size_t index = 0; index < len; ++index) out[index] = value[index];
}

static void get_bytes(const uint8_t *in, uint8_t *value, size_t len) {
    for (size_t index = 0; index < len; ++index) value[index] = in[index];
}

static enum sophia_wm_v1_status write_header(uint16_t kind, uint64_t transaction, size_t payload_len, uint8_t *out, size_t capacity, size_t *written) {
    if (payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN) return SOPHIA_WM_V1_PAYLOAD_TOO_LARGE;
    if (capacity < SOPHIA_IPC_HEADER_LEN + payload_len) return SOPHIA_WM_V1_TRUNCATED;
    put_u32(out, SOPHIA_IPC_MAGIC);
    put_u16(out + 4, SOPHIA_IPC_FRAME_VERSION);
    put_u16(out + 6, kind);
    put_u64(out + 8, transaction);
    put_u32(out + 16, (uint32_t)payload_len);
    put_u32(out + 20, 0);
    *written = SOPHIA_IPC_HEADER_LEN + payload_len;
    return SOPHIA_WM_V1_OK;
}

static enum sophia_wm_v1_status read_header(const uint8_t *frame, size_t frame_len, uint16_t expected_kind, uint64_t *transaction, size_t *payload_len) {
    if (frame_len < SOPHIA_IPC_HEADER_LEN) return SOPHIA_WM_V1_TRUNCATED;
    if (get_u32(frame) != SOPHIA_IPC_MAGIC) return SOPHIA_WM_V1_BAD_MAGIC;
    if (get_u16(frame + 4) != SOPHIA_IPC_FRAME_VERSION) return SOPHIA_WM_V1_UNSUPPORTED_FRAME_VERSION;
    if (get_u16(frame + 6) != expected_kind) return SOPHIA_WM_V1_WRONG_MESSAGE_KIND;
    if (get_u32(frame + 20) != 0) return SOPHIA_WM_V1_RESERVED_NONZERO;
    *transaction = get_u64(frame + 8);
    *payload_len = get_u32(frame + 16);
    if (*payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN) return SOPHIA_WM_V1_PAYLOAD_TOO_LARGE;
    if (frame_len < SOPHIA_IPC_HEADER_LEN + *payload_len) return SOPHIA_WM_V1_TRUNCATED;
    if (frame_len > SOPHIA_IPC_HEADER_LEN + *payload_len) return SOPHIA_WM_V1_TRAILING_BYTES;
    return SOPHIA_WM_V1_OK;
}

"#;

fn render_docs(protocol: &Protocol) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "<!-- @generated by sophia-policy-protocol-gen; do not edit. -->"
    )
    .unwrap();
    writeln!(out, "# `{}` Wire Tables\n", protocol.name).unwrap();
    writeln!(out, "The common frame is 24-byte, little-endian Sophia IPC frame version {}. The interface version is {}.{}.\n", protocol.frame_version, protocol.interface_major, protocol.interface_revision).unwrap();
    writeln!(
        out,
        "| Message | Kind | Direction | Transaction | Payload bytes |"
    )
    .unwrap();
    writeln!(out, "| --- | ---: | --- | --- | ---: |").unwrap();
    for message in &protocol.messages {
        let suffix = message
            .fields
            .iter()
            .find(|field| field.kind == FieldKind::Bytes)
            .map(|field| format!("..{}", fixed_prefix_len(message) + field.max.unwrap()))
            .unwrap_or_else(|| fixed_prefix_len(message).to_string());
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            message.name,
            message.kind,
            message.direction,
            transaction_name(message.transaction),
            suffix
        )
        .unwrap();
    }
    for message in &protocol.messages {
        writeln!(out, "\n## `{}`\n", message.name).unwrap();
        writeln!(out, "| Offset | Field | Type | Rule |").unwrap();
        writeln!(out, "| ---: | --- | --- | --- |").unwrap();
        let mut offset = 0;
        for field in &message.fields {
            let rule = if field.reserved {
                "must be zero".to_string()
            } else if let Some(max) = field.max {
                format!("at most {max} bytes; consumes payload tail")
            } else if matches!(field.kind, FieldKind::FixedBytes(_)) {
                "fixed octet run".to_string()
            } else {
                "little-endian".to_string()
            };
            writeln!(
                out,
                "| {offset} | `{}` | `{}` | {} |",
                field.name,
                field_name(field.kind),
                rule
            )
            .unwrap();
            offset += field_width(field.kind);
        }
    }
    writeln!(out, "\n# Transfer Records").unwrap();
    for record in &protocol.records {
        writeln!(out, "\n## `{}` record\n", record.name).unwrap();
        writeln!(
            out,
            "Transfer: `{}`; record kind: {}; maximum records: {}; fixed size: {} bytes.\n",
            record.transfer,
            record.kind,
            record.max,
            record_width(record)
        )
        .unwrap();
        writeln!(out, "| Offset | Field | Type | Rule |").unwrap();
        writeln!(out, "| ---: | --- | --- | --- |").unwrap();
        let mut offset = 0;
        for field in &record.fields {
            let rule = if field.reserved {
                "must be zero"
            } else if matches!(field.kind, FieldKind::FixedBytes(_)) {
                "octet run, zero padded"
            } else {
                "little-endian"
            };
            writeln!(
                out,
                "| {offset} | `{}` | `{}` | {rule} |",
                field.name,
                field_name(field.kind)
            )
            .unwrap();
            offset += field_width(field.kind);
        }
    }
    if !protocol.extension_records.is_empty() {
        writeln!(out, "\n# Extension Records").unwrap();
        writeln!(
            out,
            "\nExtension records ride uncounted chunks appended after the ordinary \
prefix: `*Begin.chunk_count` excludes them, their ordinals continue the dense \
sequence, and a sender emits one only when the gating capability was \
negotiated. Kinds `0xFF00`-`0xFFFF` are reserved for them, and no codec is \
generated: the counted record path is frozen, and these exist so a frozen \
revision can still carry new facts."
        )
        .unwrap();
        for extension in &protocol.extension_records {
            let record = &extension.record;
            writeln!(out, "\n## `{}` extension record\n", record.name).unwrap();
            writeln!(
                out,
                "Transfer: `{}`; record kind: {:#06X}; gated on capability \
`{}`; maximum records: {}; fixed size: {} bytes.\n",
                record.transfer,
                record.kind,
                extension.gate,
                record.max,
                record_width(record)
            )
            .unwrap();
            writeln!(out, "| Offset | Field | Type | Rule |").unwrap();
            writeln!(out, "| ---: | --- | --- | --- |").unwrap();
            let mut offset = 0;
            for field in &record.fields {
                let rule = if field.reserved {
                    "must be zero"
                } else if matches!(field.kind, FieldKind::FixedBytes(_)) {
                    "octet run, zero padded"
                } else {
                    "little-endian"
                };
                writeln!(
                    out,
                    "| {offset} | `{}` | `{}` | {rule} |",
                    field.name,
                    field_name(field.kind)
                )
                .unwrap();
                offset += field_width(field.kind);
            }
        }
    }
    out
}

fn record_sample_bytes(record: &Record) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for field in &record.fields {
        match (field.kind, &field.sample) {
            (FieldKind::U16, Sample::Integer(value)) => {
                bytes.extend_from_slice(&(*value as u16).to_le_bytes())
            }
            (FieldKind::U32, Sample::Integer(value)) => {
                bytes.extend_from_slice(&(*value as u32).to_le_bytes())
            }
            (FieldKind::U64, Sample::Integer(value)) => {
                bytes.extend_from_slice(&value.to_le_bytes())
            }
            (FieldKind::I32, Sample::Integer(value)) => {
                bytes.extend_from_slice(&(*value as i32).to_le_bytes())
            }
            (FieldKind::FixedBytes(count), Sample::Bytes(sample)) => {
                if sample.len() as u64 != count {
                    return Err(format!(
                        "record sample for `{}` is {} bytes but declares {count}",
                        field.name,
                        sample.len()
                    ));
                }
                bytes.extend_from_slice(sample)
            }
            _ => {
                return Err(format!("record sample type mismatch for `{}`", field.name));
            }
        }
    }
    Ok(bytes)
}

fn render_record_golden(protocol: &Protocol) -> Result<String, String> {
    let mut out = String::new();
    writeln!(
        out,
        "# @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "# record-name|record-hex").unwrap();
    for record in &protocol.records {
        let bytes = record_sample_bytes(record)?;
        writeln!(out, "{}|{}", snake(&record.name), encode_hex(&bytes)).unwrap();
    }
    // Extension records join the corpus so an independent client can prove its
    // decoder against the same bytes, even though no codec is generated for
    // them here.
    for extension in &protocol.extension_records {
        let bytes = record_sample_bytes(&extension.record)?;
        writeln!(
            out,
            "{}|{}",
            snake(&extension.record.name),
            encode_hex(&bytes)
        )
        .unwrap();
    }
    Ok(out)
}

fn render_golden(protocol: &Protocol) -> Result<String, String> {
    let mut out = String::new();
    writeln!(
        out,
        "# @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "# name|transaction|frame-hex").unwrap();
    for message in &protocol.messages {
        let transaction = if message.transaction == TransactionRule::Required {
            0x0102_0304_0506_0708
        } else {
            0
        };
        let payload = sample_payload(message)?;
        let frame = raw_frame(protocol.frame_version, message.kind, transaction, &payload);
        writeln!(
            out,
            "{}|{transaction}|{}",
            snake(&message.name),
            encode_hex(&frame)
        )
        .unwrap();
    }
    Ok(out)
}

fn render_malformed(protocol: &Protocol) -> Result<String, String> {
    let sample = |name: &str| -> Result<Vec<u8>, String> {
        let message = protocol
            .messages
            .iter()
            .find(|message| snake(&message.name) == name)
            .ok_or_else(|| format!("missing malformed-corpus message `{name}`"))?;
        let transaction = if message.transaction == TransactionRule::Required {
            0x0102_0304_0506_0708
        } else {
            0
        };
        Ok(raw_frame(
            protocol.frame_version,
            message.kind,
            transaction,
            &sample_payload(message)?,
        ))
    };
    let mut cases = Vec::new();

    let hello = sample("client_hello")?;
    cases.push((
        "truncated_header",
        "client_hello",
        "truncated",
        hello[..23].to_vec(),
    ));
    let mut bad_magic = hello.clone();
    bad_magic[0] = 0;
    cases.push(("bad_magic", "client_hello", "bad_magic", bad_magic));
    let mut bad_version = hello.clone();
    bad_version[4..6].copy_from_slice(&2_u16.to_le_bytes());
    cases.push((
        "bad_frame_version",
        "client_hello",
        "unsupported_frame_version",
        bad_version,
    ));
    let mut bad_kind = hello.clone();
    bad_kind[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
    cases.push((
        "unknown_message_kind",
        "client_hello",
        "wrong_message_kind",
        bad_kind,
    ));
    let mut excessive = hello[..24].to_vec();
    excessive[16..20].copy_from_slice(&65_537_u32.to_le_bytes());
    cases.push((
        "payload_too_large",
        "client_hello",
        "payload_too_large",
        excessive,
    ));
    let mut header_reserved = hello.clone();
    header_reserved[20] = 1;
    cases.push((
        "header_reserved_nonzero",
        "client_hello",
        "reserved_nonzero",
        header_reserved,
    ));
    let mut trailing = hello.clone();
    trailing.push(0);
    cases.push((
        "trailing_frame_byte",
        "client_hello",
        "trailing_bytes",
        trailing,
    ));
    let mut hello_transaction = hello;
    hello_transaction[8] = 1;
    cases.push((
        "hello_nonzero_transaction",
        "client_hello",
        "invalid_transaction",
        hello_transaction,
    ));

    let mut welcome_reserved = sample("server_welcome")?;
    welcome_reserved[26] = 1;
    cases.push((
        "welcome_reserved_nonzero",
        "server_welcome",
        "reserved_nonzero",
        welcome_reserved,
    ));

    let mut snapshot_zero = sample("snapshot_begin")?;
    snapshot_zero[8..16].fill(0);
    cases.push((
        "snapshot_zero_transaction",
        "snapshot_begin",
        "invalid_transaction",
        snapshot_zero,
    ));

    let chunk = protocol
        .messages
        .iter()
        .find(|message| snake(&message.name) == "snapshot_chunk")
        .ok_or("missing snapshot_chunk")?;
    let short_chunk = raw_frame(
        protocol.frame_version,
        chunk.kind,
        0x0102_0304_0506_0708,
        &[0; 8],
    );
    cases.push((
        "chunk_short_prefix",
        "snapshot_chunk",
        "truncated",
        short_chunk,
    ));

    let mut out = String::new();
    writeln!(
        out,
        "# @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "# case|decoder|expected-error|frame-hex").unwrap();
    for (name, decoder, expected, frame) in cases {
        writeln!(out, "{name}|{decoder}|{expected}|{}", encode_hex(&frame)).unwrap();
    }
    Ok(out)
}

fn sample_payload(message: &Message) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for field in &message.fields {
        match (&field.kind, &field.sample) {
            (FieldKind::U16, Sample::Integer(value)) => {
                out.extend_from_slice(&(*value as u16).to_le_bytes())
            }
            (FieldKind::U32, Sample::Integer(value)) => {
                out.extend_from_slice(&(*value as u32).to_le_bytes())
            }
            (FieldKind::U64, Sample::Integer(value)) => out.extend_from_slice(&value.to_le_bytes()),
            (FieldKind::I32, Sample::Integer(value)) => {
                out.extend_from_slice(&(*value as i32).to_le_bytes())
            }
            (FieldKind::Bytes | FieldKind::FixedBytes(_), Sample::Bytes(bytes)) => {
                out.extend_from_slice(bytes)
            }
            _ => return Err(format!("sample type mismatch for `{}`", field.name)),
        }
    }
    Ok(out)
}

fn raw_frame(version: u64, kind: u64, transaction: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x4850_4f53_u32.to_le_bytes());
    out.extend_from_slice(&(version as u16).to_le_bytes());
    out.extend_from_slice(&(kind as u16).to_le_bytes());
    out.extend_from_slice(&transaction.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn string_arg(node: &KdlNode, index: usize) -> Result<String, String> {
    node.get(index)
        .and_then(kdl::KdlValue::as_string)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "`{}` argument {index} must be a string",
                node.name().value()
            )
        })
}

fn string_property(node: &KdlNode, name: &str) -> Result<String, String> {
    node.get(name)
        .and_then(kdl::KdlValue::as_string)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "`{}` property `{name}` must be a string",
                node.name().value()
            )
        })
}

fn integer_property(node: &KdlNode, name: &str) -> Result<u64, String> {
    let value = node
        .get(name)
        .and_then(kdl::KdlValue::as_integer)
        .ok_or_else(|| {
            format!(
                "`{}` property `{name}` must be an integer",
                node.name().value()
            )
        })?;
    u64::try_from(value).map_err(|_| format!("`{name}` must be nonnegative"))
}

fn field_width(kind: FieldKind) -> u64 {
    match kind {
        FieldKind::U16 => 2,
        FieldKind::U32 => 4,
        FieldKind::U64 => 8,
        FieldKind::I32 => 4,
        FieldKind::Bytes => 0,
        FieldKind::FixedBytes(count) => count,
    }
}

fn record_width(record: &Record) -> u64 {
    record
        .fields
        .iter()
        .map(|field| field_width(field.kind))
        .sum()
}

fn fixed_prefix_len(message: &Message) -> u64 {
    message
        .fields
        .iter()
        .map(|field| field_width(field.kind))
        .sum()
}

fn rust_type(kind: FieldKind) -> String {
    match kind {
        FieldKind::U16 => "u16".to_string(),
        FieldKind::U32 => "u32".to_string(),
        FieldKind::U64 => "u64".to_string(),
        FieldKind::I32 => "i32".to_string(),
        FieldKind::Bytes => "Vec<u8>".to_string(),
        FieldKind::FixedBytes(count) => format!("[u8; {count}]"),
    }
}

fn c_type(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::U16 => "uint16_t",
        FieldKind::U32 => "uint32_t",
        FieldKind::U64 => "uint64_t",
        FieldKind::I32 => "int32_t",
        FieldKind::Bytes => "const uint8_t *",
        // A fixed run declares its extent after the member name in C, so the
        // declarator is assembled at the call site rather than here.
        FieldKind::FixedBytes(_) => "uint8_t",
    }
}

fn rust_push(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::U16 => "push_u16",
        FieldKind::U32 => "push_u32",
        FieldKind::U64 => "push_u64",
        FieldKind::I32 => "push_i32",
        FieldKind::Bytes => unreachable!(),
        FieldKind::FixedBytes(_) => unreachable!(),
    }
}

fn rust_cursor(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::U16 => "u16",
        FieldKind::U32 => "u32",
        FieldKind::U64 => "u64",
        FieldKind::I32 => "i32",
        FieldKind::Bytes => unreachable!(),
        FieldKind::FixedBytes(_) => unreachable!(),
    }
}

fn field_name(kind: FieldKind) -> String {
    match kind {
        FieldKind::U16 => "u16".to_string(),
        FieldKind::U32 => "u32".to_string(),
        FieldKind::U64 => "u64".to_string(),
        FieldKind::I32 => "i32".to_string(),
        FieldKind::Bytes => "bytes".to_string(),
        FieldKind::FixedBytes(count) => format!("u8[{count}]"),
    }
}

fn c_put(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::U16 => "put_u16",
        FieldKind::U32 => "put_u32",
        FieldKind::U64 => "put_u64",
        FieldKind::I32 => "put_i32",
        FieldKind::Bytes => unreachable!(),
        FieldKind::FixedBytes(_) => unreachable!(),
    }
}

fn c_get(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::U16 => "get_u16",
        FieldKind::U32 => "get_u32",
        FieldKind::U64 => "get_u64",
        FieldKind::I32 => "get_i32",
        FieldKind::Bytes => unreachable!(),
        FieldKind::FixedBytes(_) => unreachable!(),
    }
}

fn transaction_name(rule: TransactionRule) -> &'static str {
    match rule {
        TransactionRule::Zero => "must be zero",
        TransactionRule::Required => "must be nonzero",
    }
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            out.push('_');
        }
        out.push(character.to_ascii_lowercase());
    }
    out
}

fn screaming(name: &str) -> String {
    name.chars()
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("hex sample must have an even length".into());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| "hex sample is not ASCII")?;
            u8::from_str_radix(pair, 16).map_err(|_| format!("invalid hex byte `{pair}`"))
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

#[cfg(test)]
#[path = "../tests/support/shell_schema.rs"]
mod shell_schema_tests;
