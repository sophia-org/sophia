//! Fixed rows belong to the file contract. The frozen socket schema is only
//! checked for compatibility while that adapter remains in the repository.
use std::path::Path;

use kdl::KdlDocument;

use crate::{
    ExtensionRecord, NamedValue, Protocol, Record, integer_property, parse_record, string_arg,
    string_property, validate_rows,
};

pub(super) const SCHEMA_PATH: &str = "protocol/sophia-wm-files-v1.kdl";

pub(super) struct Rows {
    pub interface_major: u64,
    pub interface_revision: u64,
    pub max_outputs: u64,
    pub max_surfaces: u64,
    pub max_bindings: u64,
    pub capabilities: Vec<NamedValue>,
    pub outcomes: Vec<NamedValue>,
    pub records: Vec<Record>,
    pub extension_records: Vec<ExtensionRecord>,
}

pub(super) fn read(root: &Path) -> Result<Rows, String> {
    let text = std::fs::read_to_string(root.join(SCHEMA_PATH))
        .map_err(|error| format!("read {SCHEMA_PATH}: {error}"))?;
    parse(&text)
}

pub(super) fn parse(text: &str) -> Result<Rows, String> {
    let document =
        KdlDocument::parse(text).map_err(|error| format!("parse {SCHEMA_PATH}: {error}"))?;
    let protocol = document.get("protocol").ok_or("missing WM file protocol")?;
    if string_arg(protocol, 0)? != "sophia_wm_fs_v1" {
        return Err("wrong WM file protocol".into());
    }
    let children = protocol.children().ok_or("missing WM file children")?;
    let layouts = children
        .nodes()
        .iter()
        .filter(|node| node.name().value() == "row-layouts")
        .collect::<Vec<_>>();
    let [layout] = layouts.as_slice() else {
        return Err("WM file contract requires exactly one row-layouts node".into());
    };
    let mut rows = Rows {
        interface_major: integer_property(layout, "interface-major")?,
        interface_revision: integer_property(layout, "interface-revision")?,
        max_outputs: integer_property(layout, "max-outputs")?,
        max_surfaces: integer_property(layout, "max-surfaces")?,
        max_bindings: integer_property(layout, "max-bindings")?,
        capabilities: Vec::new(),
        outcomes: Vec::new(),
        records: Vec::new(),
        extension_records: Vec::new(),
    };
    for node in layout.children().ok_or("missing WM row layouts")?.nodes() {
        match node.name().value() {
            "capability" => rows.capabilities.push(NamedValue {
                name: string_arg(node, 0)?,
                value: integer_property(node, "bit")?,
            }),
            "outcome" => rows.outcomes.push(NamedValue {
                name: string_arg(node, 0)?,
                value: integer_property(node, "value")?,
            }),
            "record" => rows.records.push(parse_record(node)?),
            "extension-record" => rows.extension_records.push(ExtensionRecord {
                gate: string_property(node, "gate")?,
                record: parse_record(node)?,
            }),
            other => return Err(format!("unexpected WM row layout node `{other}`")),
        }
    }
    if rows.interface_major == 0 || rows.records.is_empty() {
        return Err("WM row layouts require a nonzero interface and records".into());
    }
    validate_rows(&rows.capabilities, &rows.records, &rows.extension_records)?;
    Ok(rows)
}

pub(super) fn check_legacy(rows: &Rows, legacy: &Protocol) -> Result<(), String> {
    if rows.interface_major != legacy.interface_major
        || rows.interface_revision != legacy.interface_revision
        || rows.max_outputs != legacy.max_outputs
        || rows.max_surfaces != legacy.max_surfaces
        || rows.max_bindings != legacy.max_bindings
        || rows.capabilities != legacy.capabilities
        || rows.outcomes != legacy.outcomes
        || rows.records != legacy.records
        || rows.extension_records != legacy.extension_records
    {
        return Err("frozen WM socket rows differ from the authoritative WM file rows".into());
    }
    Ok(())
}
