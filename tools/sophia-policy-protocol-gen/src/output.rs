//! Describe the existing output codec without making it depend on generation.
//! Samples are assembled from the schema, never from Sophia's Rust encoder.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use kdl::{KdlDocument, KdlNode};

use super::{integer_property, string_arg, string_property};

type Mutation = (String, usize, Vec<u8>);

pub(super) fn outputs(text: &str) -> Result<BTreeMap<&'static str, String>, String> {
    let document: KdlDocument = text.parse().map_err(|e| format!("output KDL: {e}"))?;
    let protocol = document.get("protocol").ok_or("missing output protocol")?;
    if string_arg(protocol, 0)? != "sophia_output_v1"
        || integer_property(protocol, "frame-version")? != 1
        || integer_property(protocol, "interface-major")? != 1
        || integer_property(protocol, "interface-revision")? != 1
        || integer_property(protocol, "max-payload")? != 65536
    {
        return Err("unsupported output envelope or revision".into());
    }
    let nodes = children(protocol)?;
    let mut records = BTreeMap::new();
    for node in nodes.iter().filter(|n| n.name().value() == "record") {
        if records.insert(string_arg(node, 0)?, node).is_some() {
            return Err("duplicate output record".into());
        }
    }
    let mut doc = String::from(
        "# sophia_output_v1 wire tables\n\nGenerated from `protocol/sophia-output-v1.kdl`; do not edit.\n\n\
         Experimental major 1 revision 1. [Normative lifecycle](../sophia-output-v1.md).\n\n\
         Fields occur in the listed order, packed and little endian. Offsets are relative to the\n\
         enclosing payload or record; variable terms are byte lengths, not sample offsets.\n",
    );
    let mut valid = String::from("# Schema samples: name hex; not an ordered conversation.\n");
    let mut malformed = String::from("# Schema-derived malformed frames: message.case hex.\n");
    let mut values = String::from("# Schema assignments: family name value.\n");
    let mut kinds = BTreeSet::new();
    let mut names = BTreeSet::new();
    for node in nodes {
        let name = string_arg(node, 0)?;
        match node.name().value() {
            "record" => render_table(node, &mut doc)?,
            "message" => {
                let kind = u16::try_from(integer_property(node, "kind")?)
                    .map_err(|_| "output kind overflow")?;
                if !kinds.insert(kind) || !names.insert(name.clone()) {
                    return Err("duplicate output message".into());
                }
                let rule = string_property(node, "transaction")?;
                let transaction = match rule.as_str() {
                    "zero" => 0_u64,
                    "required" => 1,
                    _ => return Err("invalid output transaction rule".into()),
                };
                writeln!(values, "message {name} {kind}").unwrap();
                render_table(node, &mut doc)?;
                let mut payload = Vec::new();
                let mut mutations = Vec::new();
                sample(
                    node,
                    &records,
                    &mut Vec::new(),
                    &mut payload,
                    &mut mutations,
                )?;
                let mut frame = b"SOPH".to_vec();
                frame.extend_from_slice(&1_u16.to_le_bytes());
                frame.extend_from_slice(&kind.to_le_bytes());
                frame.extend_from_slice(&transaction.to_le_bytes());
                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                frame.extend_from_slice(&0_u32.to_le_bytes());
                frame.extend_from_slice(&payload);
                line(&mut valid, &name, &frame);
                for (case, offset, bytes) in mutations {
                    let mut bad = frame.clone();
                    bad[24 + offset..24 + offset + bytes.len()].copy_from_slice(&bytes);
                    line(&mut malformed, &format!("{name}.{case}-{offset}"), &bad);
                }
                for (case, offset, bytes) in [
                    ("magic", 0, vec![0]),
                    ("version", 4, vec![2]),
                    ("kind", 6, vec![255, 255]),
                    ("reserved", 20, vec![1]),
                    ("oversize", 16, 65537_u32.to_le_bytes().to_vec()),
                    ("transaction", 8, (1 - transaction).to_le_bytes().to_vec()),
                ] {
                    let mut bad = frame.clone();
                    bad[offset..offset + bytes.len()].copy_from_slice(&bytes);
                    line(&mut malformed, &format!("{name}.{case}"), &bad);
                }
            }
            family => {
                let value = integer_property(node, "value")?;
                writeln!(values, "{family} {name} {value}").unwrap();
                writeln!(doc, "\n- {family} `{name}` = {value}").unwrap();
            }
        }
    }
    Ok(BTreeMap::from([
        ("docs/generated/sophia-output-v1-wire.md", doc),
        ("protocol/golden/sophia-output-v1.frames", valid),
        (
            "protocol/golden/sophia-output-v1-malformed.frames",
            malformed,
        ),
        ("protocol/golden/sophia-output-v1.values", values),
    ]))
}

fn children(node: &KdlNode) -> Result<&[KdlNode], String> {
    node.children()
        .map(|d| d.nodes())
        .ok_or_else(|| "missing output fields".into())
}

fn scalar_width(kind: &str) -> Result<usize, String> {
    match kind {
        "u16" => Ok(2),
        "u32" | "i32" => Ok(4),
        "u64" => Ok(8),
        _ => Err(format!("unsupported output scalar {kind}")),
    }
}

fn render_table(node: &KdlNode, doc: &mut String) -> Result<(), String> {
    writeln!(doc, "\n## {}\n", string_arg(node, 0)?).unwrap();
    if node.name().value() == "message" {
        writeln!(
            doc,
            "Kind {}; {}; transaction `{}`.\n",
            integer_property(node, "kind")?,
            string_property(node, "direction")?,
            string_property(node, "transaction")?
        )
        .unwrap();
    }
    doc.push_str("| Offset | Field | Wire type |\n| --- | --- | --- |\n");
    let mut fixed = 0;
    let mut variable = String::new();
    for field in children(node)? {
        let name = string_arg(field, 0)?;
        let offset = format!("{fixed}{variable}");
        if field.name().value() == "repeated" {
            let count = string_property(field, "count")?;
            let record = string_property(field, "record")?;
            writeln!(
                doc,
                "| {offset} | `{name}` | `{count}` packed `{record}` records |"
            )
            .unwrap();
            write!(variable, " + bytes({name})").unwrap();
        } else {
            let kind = string_property(field, "type")?;
            let mut detail = kind.clone();
            for key in ["max", "reserved", "enum", "flags", "zero", "length"] {
                if let Some(value) = field.get(key) {
                    write!(detail, "; {key}={value}").unwrap();
                }
            }
            writeln!(doc, "| {offset} | `{name}` | {detail} |").unwrap();
            if kind == "utf8" {
                write!(variable, " + {}", string_property(field, "length")?).unwrap();
            } else {
                fixed += scalar_width(&kind)?;
            }
        }
    }
    writeln!(doc, "\nTotal size: {fixed}{variable} bytes.").unwrap();
    Ok(())
}

fn sample(
    node: &KdlNode,
    records: &BTreeMap<String, &KdlNode>,
    stack: &mut Vec<String>,
    bytes: &mut Vec<u8>,
    mutations: &mut Vec<Mutation>,
) -> Result<(), String> {
    let fields = children(node)?;
    for field in fields {
        if field.name().value() == "repeated" {
            let count = bounded_count(fields, &string_property(field, "count")?)?;
            let record_name = string_property(field, "record")?;
            if stack.contains(&record_name) {
                return Err("recursive output record".into());
            }
            let record = records.get(&record_name).ok_or("unknown output record")?;
            stack.push(record_name);
            for _ in 0..count {
                sample(record, records, stack, bytes, mutations)?;
            }
            stack.pop();
        } else if field.name().value() == "field" {
            let kind = string_property(field, "type")?;
            if kind == "utf8" {
                let value = string_property(field, "sample")?;
                if value.len() as u64 != bounded_count(fields, &string_property(field, "length")?)?
                {
                    return Err("output sample string length differs".into());
                }
                if !value.is_empty() {
                    mutations.push(("utf8".into(), bytes.len(), vec![255]));
                }
                bytes.extend_from_slice(value.as_bytes());
            } else {
                let width = scalar_width(&kind)?;
                let value = if kind == "i32" {
                    let signed = field
                        .get("sample")
                        .and_then(kdl::KdlValue::as_integer)
                        .ok_or("missing signed output sample")?;
                    u64::from(i32::try_from(signed).map_err(|_| "i32 sample overflow")? as u32)
                } else {
                    integer_property(field, "sample")?
                };
                if width < 8 && value >= 1_u64 << (width * 8) {
                    return Err("output sample integer overflow".into());
                }
                if field.get("reserved").and_then(kdl::KdlValue::as_bool) == Some(true) {
                    if value != 0 {
                        return Err("nonzero reserved output sample".into());
                    }
                    mutations.push(("reserved".into(), bytes.len(), vec![1]));
                }
                if field.get("max").is_some() {
                    let max = integer_property(field, "max")?;
                    if value > max || max >= 65536 {
                        return Err("output sample exceeds bound".into());
                    }
                    mutations.push((
                        "count".into(),
                        bytes.len(),
                        (max + 1).to_le_bytes()[..width].to_vec(),
                    ));
                }
                if field.get("enum").is_some() || field.get("flags").is_some() {
                    mutations.push(("enum".into(), bytes.len(), vec![255; width]));
                }
                bytes.extend_from_slice(&value.to_le_bytes()[..width]);
            }
        } else {
            return Err("unknown output layout node".into());
        }
        if bytes.len() > 65536 {
            return Err("output sample exceeds frame bound".into());
        }
    }
    Ok(())
}

fn bounded_count(fields: &[KdlNode], name: &str) -> Result<u64, String> {
    let field = fields
        .iter()
        .find(|n| string_arg(n, 0).ok().as_deref() == Some(name))
        .ok_or("missing output count field")?;
    let max = integer_property(field, "max")?;
    let value = integer_property(field, "sample")?;
    if value > max || max > 65536 {
        return Err("invalid output count bound".into());
    }
    Ok(value)
}

fn line(output: &mut String, name: &str, bytes: &[u8]) {
    write!(output, "{name} ").unwrap();
    for byte in bytes {
        write!(output, "{byte:02x}").unwrap();
    }
    output.push('\n');
}
