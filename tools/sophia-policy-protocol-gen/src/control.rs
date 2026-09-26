//! Control artifacts are derived separately: they must not register a live
//! service or alter the frozen WM/shell codecs.
use std::collections::BTreeMap;
use std::fmt::Write as _;

use kdl::{KdlDocument, KdlNode};

use super::{integer_property, string_arg, string_property};

pub(super) fn outputs(text: &str) -> Result<BTreeMap<&'static str, String>, String> {
    let document: KdlDocument = text
        .parse()
        .map_err(|error| format!("control KDL: {error}"))?;
    let protocol = document.get("protocol").ok_or("missing control protocol")?;
    if string_arg(protocol, 0)? != "sophia_control_v1"
        || integer_property(protocol, "frame-version")? != 1
        || integer_property(protocol, "interface-major")? != 1
        || integer_property(protocol, "interface-revision")? != 2
        || integer_property(protocol, "max-payload")? != 65536
        || integer_property(protocol, "max-commands")? != 259
        || integer_property(protocol, "max-name-bytes")? != 128
    {
        return Err("control envelope/revision bounds drifted".into());
    }
    let mut doc = String::from(
        "# sophia_control_v1 wire tables\n\nGenerated from `protocol/sophia-control-v1.kdl`; do not edit.\n\n\
         Experimental major 1, revision 2 (revision 1 remains negotiable). [Normative semantics](../sophia-control-v1.md).\n\n\
         All offsets are payload-relative; integers are little endian, with no alignment padding.\n",
    );
    let mut valid =
        String::from("# Generated schema samples: name hex (not one ordered conversation).\n");
    let mut values = String::from("# Generated symbolic assignments: family name value.\n");
    let mut frames = BTreeMap::new();
    for node in children(protocol)? {
        if node.name().value() != "message" {
            writeln!(
                values,
                "{} {} {}",
                node.name().value(),
                string_arg(node, 0)?,
                integer_property(node, "value")?
            )
            .unwrap();
            writeln!(
                doc,
                "\n- {} `{}` = {}",
                node.name().value(),
                string_arg(node, 0)?,
                integer_property(node, "value")?
            )
            .unwrap();
            continue;
        }
        let name = string_arg(node, 0)?;
        let kind = u16::try_from(integer_property(node, "kind")?).map_err(|_| "kind overflow")?;
        writeln!(values, "message {name} {kind}").unwrap();
        let transaction: u64 = match string_property(node, "transaction")?.as_str() {
            "zero" | "zero-or-offending" => 0,
            "required" => 1,
            _ => return Err("unknown control transaction rule".into()),
        };
        writeln!(doc, "\n## {name}\n\nKind {kind}; {}; transaction `{}`.\n\n| Offset | Field | Wire type |\n| --- | --- | --- |", string_property(node, "direction")?, string_property(node, "transaction")?).unwrap();
        let mut payload = Vec::new();
        let mut offset = 0;
        for field in children(node)? {
            if field.name().value() == "repeated" {
                let count_name = string_property(field, "count")?;
                let count_node = children(node)?
                    .iter()
                    .find(|n| string_arg(n, 0).ok().as_deref() == Some(&count_name))
                    .ok_or("missing count field")?;
                let count = integer_property(count_node, "sample")?;
                if count > integer_property(field, "max")? {
                    return Err("sample count overflow".into());
                }
                writeln!(
                    doc,
                    "| {offset} | `{}` | `{count_name}` entries, max {} |",
                    string_arg(field, 0)?,
                    integer_property(field, "max")?
                )
                .unwrap();
                doc.push_str(
                    "\nEntry offsets:\n\n| Offset | Field | Wire type |\n| --- | --- | --- |\n",
                );
                let mut entry = Vec::new();
                let mut entry_offset = 0;
                for item in children(field)? {
                    render_field(item, &mut entry, &mut entry_offset, &mut doc)?;
                }
                for _ in 0..count {
                    payload.extend_from_slice(&entry);
                }
                writeln!(
                    doc,
                    "\nPayload size: {offset} + {entry_offset} × `{count_name}` bytes."
                )
                .unwrap();
            } else {
                render_field(field, &mut payload, &mut offset, &mut doc)?;
            }
        }
        if payload.len() > 65536 {
            return Err("control sample too large".into());
        }
        if !children(node)?
            .iter()
            .any(|field| field.name().value() == "repeated")
        {
            writeln!(doc, "\nPayload size: {} bytes.", payload.len()).unwrap();
        }
        let mut frame = Vec::new();
        frame.extend_from_slice(b"SOPH");
        frame.extend_from_slice(&1_u16.to_le_bytes());
        frame.extend_from_slice(&kind.to_le_bytes());
        frame.extend_from_slice(&transaction.to_le_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&0_u32.to_le_bytes());
        frame.extend_from_slice(&payload);
        line(&mut valid, &name, &frame);
        if frames.insert(name, frame).is_some() {
            return Err("duplicate control message".into());
        }
    }
    let hello = frames.get("ClientHello").ok_or("missing hello")?;
    let mut malformed = String::from(
        "# Generated malformed frames: name hex; every line must fail strict decoding.\n",
    );
    for (name, offset, bytes) in [
        ("bad-magic", 0, vec![0]),
        ("bad-frame-version", 4, vec![2]),
        ("unknown-kind", 6, vec![255, 255]),
        ("reserved-header", 20, vec![1]),
        ("oversize", 16, 65537_u32.to_le_bytes().to_vec()),
        ("hello-nonzero-id", 8, vec![1]),
    ] {
        let mut frame = hello.clone();
        frame[offset..offset + bytes.len()].copy_from_slice(&bytes);
        line(&mut malformed, name, &frame);
    }
    line(&mut malformed, "truncated-header", &hello[..23]);
    line(
        &mut malformed,
        "truncated-payload",
        &hello[..hello.len() - 1],
    );
    let mut extra = hello.clone();
    extra.push(0);
    extra[16..20].copy_from_slice(&13_u32.to_le_bytes());
    line(&mut malformed, "trailing-payload", &extra);
    Ok(BTreeMap::from([
        ("docs/generated/sophia-control-v1-wire.md", doc),
        ("protocol/golden/sophia-control-v1.frames", valid),
        ("protocol/golden/sophia-control-v1.values", values),
        (
            "protocol/golden/sophia-control-v1-malformed.frames",
            malformed,
        ),
    ]))
}

fn children(node: &KdlNode) -> Result<&[KdlNode], String> {
    node.children()
        .map(|d| d.nodes())
        .ok_or_else(|| "missing control node children".into())
}

fn render_field(
    node: &KdlNode,
    bytes: &mut Vec<u8>,
    offset: &mut usize,
    doc: &mut String,
) -> Result<(), String> {
    if node.name().value() != "field" {
        return Err("unknown control layout node".into());
    }
    let kind = string_property(node, "type")?;
    let width = match kind.as_str() {
        "u16" => 2,
        "u32" => 4,
        "u64" => 8,
        "u8" => usize::try_from(integer_property(node, "count")?).map_err(|_| "array overflow")?,
        _ => return Err("unknown control field type".into()),
    };
    if width > 65536 {
        return Err("array too large".into());
    }
    if kind == "u8" {
        let sample = string_property(node, "sample")?;
        if sample.len() > width {
            return Err("control string sample too large".into());
        }
        bytes.extend_from_slice(sample.as_bytes());
        bytes.resize(bytes.len() + width - sample.len(), 0);
    } else {
        let sample = integer_property(node, "sample")?;
        if (width < 8 && sample >= 1_u64 << (width * 8))
            || node.get("max").is_some() && sample > integer_property(node, "max")?
            || node.get("reserved").and_then(kdl::KdlValue::as_bool) == Some(true) && sample != 0
        {
            return Err("control integer sample out of range".into());
        }
        bytes.extend_from_slice(&sample.to_le_bytes()[..width]);
    }
    let reserved = if node.get("reserved").is_some() {
        "; must be zero"
    } else {
        ""
    };
    writeln!(
        doc,
        "| {} | `{}` | {} ({} bytes){} |",
        offset,
        string_arg(node, 0)?,
        kind,
        width,
        reserved
    )
    .unwrap();
    *offset += width;
    Ok(())
}

fn line(output: &mut String, name: &str, bytes: &[u8]) {
    write!(output, "{name} ").unwrap();
    for byte in bytes {
        write!(output, "{byte:02x}").unwrap();
    }
    output.push('\n');
}
