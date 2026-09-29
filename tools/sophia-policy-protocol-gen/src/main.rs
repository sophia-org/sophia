use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as IoWrite;
use std::path::Path;
use std::process::{Command, Stdio};

use kdl::KdlNode;

mod control;
mod output;
mod wm_rows;

const RUST_ROWS_PATH: &str = "crates/sophia-protocol/src/wm_rows.rs";
const RECORD_GOLDEN_PATH: &str = "protocol/golden/sophia-wm-v1.records";

/// First record kind reserved for capability-gated extension chunks. Ordinary
/// records are allocated sequentially from 1 and must stay below it in the
/// WM file contract's row layouts.
const EXTENSION_RECORD_KIND_FLOOR: u64 = 0xFF00;
const SMT_FACTS_PATH: &str = "validation/architecture/generated/sophia-wm-file-rows-facts.smt2";

#[derive(Clone, Debug, Eq, PartialEq)]
struct NamedValue {
    name: String,
    value: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    name: String,
    transfer: String,
    kind: u64,
    max: u64,
    fields: Vec<Field>,
}

/// A capability-gated row in the file contract's reserved extension range.
/// Its layout and samples are shared with independent clients; typed extension
/// codecs live with their semantic owners rather than the ordinary row generator.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtensionRecord {
    record: Record,
    gate: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// therefore rejected by the fixed-row validator.
    FixedBytes(u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    let file_rows = wm_rows::read(root)?;
    let mut outputs = BTreeMap::new();
    outputs.insert(RECORD_GOLDEN_PATH, render_record_golden(&file_rows)?);
    outputs.insert(SMT_FACTS_PATH, render_smt_facts(&file_rows));
    outputs.insert(RUST_ROWS_PATH, format_rust(&render_rust_rows(&file_rows))?);
    let control_text = fs::read_to_string(root.join("protocol/sophia-control-v1.kdl"))
        .map_err(|error| format!("read control schema: {error}"))?;
    outputs.extend(control::outputs(&control_text)?);
    let output_text = fs::read_to_string(root.join("protocol/sophia-output-v1.kdl"))
        .map_err(|error| format!("read output schema: {error}"))?;
    outputs.extend(output::outputs(&output_text)?);

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

fn validate_rows(
    capabilities: &[NamedValue],
    records: &[Record],
    extension_records: &[ExtensionRecord],
) -> Result<(), String> {
    let mut record_names = BTreeSet::new();
    let mut record_keys = BTreeSet::new();
    for record in records {
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
    let capability_names: BTreeSet<&str> = capabilities
        .iter()
        .map(|capability| capability.name.as_str())
        .collect();
    for extension in extension_records {
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

fn render_smt_facts(protocol: &wm_rows::Rows) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "; @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "; Source: {} (row-layouts)", wm_rows::SCHEMA_PATH).unwrap();
    writeln!(out).unwrap();
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

    for record in protocol.records.iter().chain(
        protocol
            .extension_records
            .iter()
            .map(|extension| &extension.record),
    ) {
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

    let records = protocol.records.iter().chain(
        protocol
            .extension_records
            .iter()
            .map(|extension| &extension.record),
    );
    let max_width = records.clone().map(record_width).max().unwrap_or(0);
    let max_count = records.map(|record| record.max).max().unwrap_or(0);
    writeln!(out, "(define-fun max_record_width () Int {max_width})").unwrap();
    writeln!(out, "(define-fun max_record_count () Int {max_count})").unwrap();

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

fn render_rust_rows(protocol: &wm_rows::Rows) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "// @generated by sophia-policy-protocol-gen; do not edit."
    )
    .unwrap();
    writeln!(out, "// Source: {} (row-layouts)\n", wm_rows::SCHEMA_PATH).unwrap();
    writeln!(out, "use crate::BinaryCodecError;").unwrap();
    writeln!(
        out,
        "use crate::byte_cursor::{{Cursor, push_i32, push_u16, push_u32, push_u64}};\n"
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
    writeln!(out, "pub fn encode_wm_v1_{snake}_records(records: &[{rust_name}]) -> Result<Vec<u8>, BinaryCodecError> {{").unwrap();
    writeln!(out, "    if records.len() > {} {{", record.max).unwrap();
    writeln!(
        out,
        "        return Err(BinaryCodecError::CountTooLarge {{ count: records.len(), max: {} }});",
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
    writeln!(out, "pub fn decode_wm_v1_{snake}_records(data: &[u8], item_count: u32) -> Result<Vec<{rust_name}>, BinaryCodecError> {{").unwrap();
    writeln!(out, "    let count = item_count as usize;").unwrap();
    writeln!(
        out,
        "    if count > {} {{ return Err(BinaryCodecError::CountTooLarge {{ count, max: {} }}); }}",
        record.max, record.max
    )
    .unwrap();
    writeln!(out, "    let expected = count.checked_mul({width}).ok_or(BinaryCodecError::CountTooLarge {{ count, max: {} }})?;", record.max).unwrap();
    writeln!(
        out,
        "    if data.len() < expected {{ return Err(BinaryCodecError::Truncated); }}"
    )
    .unwrap();
    writeln!(out, "    if data.len() > expected {{ return Err(BinaryCodecError::TrailingBytes(data.len() - expected)); }}").unwrap();
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
            writeln!(out, "        if reserved != 0 {{ return Err(BinaryCodecError::ReservedNonZero(reserved{cast})); }}").unwrap();
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

fn render_record_golden(protocol: &wm_rows::Rows) -> Result<String, String> {
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
#[path = "../tests/support/wm_rows.rs"]
mod wm_rows_tests;
