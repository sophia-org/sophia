//! Design checks only: these layouts have no implemented file codecs/export.
//! Check their actual KDL, including field coverage and custody arithmetic.
use std::collections::{BTreeMap, BTreeSet};

use kdl::{KdlDocument, KdlNode};

const BROKER: &str = include_str!("../../../protocol/sophia-broker-files-v1.kdl");
const PORTAL: &str = include_str!("../../../protocol/sophia-portal-files-v1.kdl");

fn number(node: &KdlNode, name: &str) -> Result<usize, String> {
    node.get(name)
        .and_then(|value| value.as_integer())
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("{} missing unsigned {name}", node.name().value()))
}

fn name(node: &KdlNode) -> Result<&str, String> {
    node.get(0)
        .and_then(|value| value.as_string())
        .ok_or_else(|| format!("{} missing name", node.name().value()))
}

struct Layout {
    sizes: BTreeMap<String, usize>,
    kinds: BTreeMap<String, usize>,
    document: KdlDocument,
}

impl Layout {
    fn parse(text: &str) -> Result<Self, String> {
        let document = KdlDocument::parse_v2(text).map_err(|error| error.to_string())?;
        let [root] = document.nodes() else {
            return Err("exactly one protocol required".into());
        };
        if root.name().value() != "protocol" {
            return Err("expected protocol".into());
        }
        let children = root.children().ok_or("protocol children missing")?;
        let mut sizes = BTreeMap::new();
        let mut row_sizes = BTreeMap::new();
        let mut kinds = BTreeMap::new();
        let mut used_kinds = BTreeSet::new();
        for node in children.nodes() {
            match node.name().value() {
                "body" | "row" => {
                    let key = name(node)?.to_owned();
                    let size = number(node, "size")?;
                    if sizes.insert(key.clone(), size).is_some() {
                        return Err(format!("duplicate layout {key}"));
                    }
                    if node.name().value() == "row" {
                        row_sizes.insert(key, size);
                    }
                }
                "header" | "submit" | "ack" => {
                    let key = node.name().value().to_owned();
                    if sizes.insert(key.clone(), number(node, "size")?).is_some() {
                        return Err(format!("duplicate layout {key}"));
                    }
                }
                "object" | "event" | "candidate" => {
                    let kind = number(node, "kind")?;
                    let allowed = match node.name().value() {
                        "object" => 1..=15,
                        "event" => 16..=255,
                        _ => 256..=u16::MAX as usize,
                    };
                    if !allowed.contains(&kind)
                        || !used_kinds.insert(kind)
                        || kinds.insert(name(node)?.to_owned(), kind).is_some()
                    {
                        return Err("duplicate or misclassified kind".into());
                    }
                }
                "rejection" => {}
                other => return Err(format!("unexpected declaration {other}")),
            }
        }
        for key in kinds.keys() {
            if !sizes.contains_key(key) || row_sizes.contains_key(key) {
                return Err(format!("missing body {key}"));
            }
        }
        for node in children.nodes().iter().filter(|n| n.children().is_some()) {
            let key = match node.name().value() {
                "body" | "row" => name(node)?,
                other => other,
            };
            let size = *sizes.get(key).ok_or("unknown sized block")?;
            if !(1..=65_536).contains(&size) {
                return Err("invalid block size".into());
            }
            let mut covered = vec![false; size];
            let mut fields = BTreeSet::new();
            for field in node.children().expect("filtered").nodes() {
                if field.name().value() != "field" || !fields.insert(name(field)?) {
                    return Err("invalid or repeated field".into());
                }
                let ty = field
                    .get("type")
                    .and_then(|v| v.as_string())
                    .ok_or("type")?;
                let width = match ty {
                    "u8" => 1,
                    "u16" => 2,
                    "u32" => 4,
                    "u64" => 8,
                    "bytes" => number(field, "size")?,
                    ty => row_sizes
                        .get(ty.strip_prefix("row:").ok_or("unknown type")?)
                        .ok_or("unknown row")?
                        .checked_mul(number(field, "count")?)
                        .ok_or("row size overflow")?,
                };
                let start = number(field, "offset")?;
                let end = start.checked_add(width).ok_or("field end overflow")?;
                if width == 0 || end > size || covered[start..end].iter().any(|v| *v) {
                    return Err(format!("overlap or overflow in {key}"));
                }
                covered[start..end].fill(true);
                if field.get("min").is_some()
                    && field.get("max").is_some()
                    && number(field, "min")? > number(field, "max")?
                {
                    return Err("reversed interval".into());
                }
            }
            if covered.iter().any(|v| !v) {
                return Err(format!("gap in {key}"));
            }
        }
        Ok(Self {
            sizes,
            kinds,
            document,
        })
    }

    fn field(&self, body: &str, field: &str, property: &str) -> usize {
        let nodes = self.document.nodes()[0].children().unwrap().nodes();
        let body = nodes
            .iter()
            .find(|n| n.name().value() == "body" && name(n).ok() == Some(body))
            .unwrap();
        let field = body
            .children()
            .unwrap()
            .nodes()
            .iter()
            .find(|n| name(n).ok() == Some(field))
            .unwrap();
        number(field, property).unwrap()
    }

    fn record(&self, body: &str) -> usize {
        self.sizes["header"] + self.sizes[body]
    }
}

#[test]
fn broker_and_portal_layouts_cover_every_byte_and_declared_kind() {
    for (text, kinds, largest, limits) in [(BROKER, 7, 448, 72), (PORTAL, 11, 344, 80)] {
        let layout = Layout::parse(text).unwrap();
        assert_eq!(layout.sizes["header"], 32);
        assert_eq!(layout.sizes["submit"], 24);
        assert_eq!(layout.sizes["ack"], 16);
        assert_eq!(layout.kinds.len(), kinds);
        assert_eq!(layout.record("Limits"), limits);
        assert_eq!(
            layout
                .kinds
                .iter()
                .filter(|(_, k)| **k >= 256)
                .map(|(n, _)| layout.record(n))
                .max(),
            Some(largest)
        );
    }
}

#[test]
fn broker_keeps_all_rejections_and_bounds_the_complete_response_set() {
    let layout = Layout::parse(BROKER).unwrap();
    let rejections: BTreeMap<_, _> = layout.document.nodes()[0]
        .children()
        .unwrap()
        .nodes()
        .iter()
        .filter(|n| n.name().value() == "rejection")
        .map(|n| (name(n).unwrap(), number(n, "value").unwrap()))
        .collect();
    assert_eq!(
        rejections,
        BTreeMap::from([
            ("unknown_surface", 1),
            ("stale_generation", 2),
            ("capacity_exhausted", 3),
            ("disclosure_exceeded", 4),
            ("invalid_connection_epoch", 5),
        ])
    );
    assert_eq!(layout.field("ResponseSet", "rows", "count"), 2);
    assert_eq!(layout.record("ResponseSet"), 448);
    assert_eq!(layout.field("Limits", "staging_bytes", "min"), 448);
    assert_eq!(layout.field("Limits", "staging_bytes", "max"), 512);
    let retained = layout.record("Submitted") * 2
        + layout.record("Negotiated")
        + layout.record("BrokerRequest");
    assert_eq!(retained, 352);
    assert_eq!(layout.field("Limits", "journal_records", "min"), 4);
    assert_eq!(layout.field("Limits", "journal_bytes", "min"), retained);
}

#[test]
fn portal_history_payload_and_terminal_reservations_have_distinct_bounds() {
    let l = Layout::parse(PORTAL).unwrap();
    let active = l.field("Limits", "max_transfers", "max");
    assert_eq!(active, 64);
    assert_eq!(l.field("Limits", "max_retained_transfers", "max"), 4096);
    assert_eq!(l.field("Limits", "upload_slots", "value"), 1);
    assert_eq!(l.field("PayloadBegin", "slot", "value"), 0);
    assert_eq!(l.field("Limits", "max_payload_bytes", "max"), 65_536);
    assert_eq!(
        l.field("Limits", "staging_bytes", "value"),
        l.record("TransferRequest")
    );
    let terminal = l.record("TransferOutcome");
    assert_eq!(active * terminal, 3072);
    let lifecycle = 3 * l.record("Submitted") + l.record("Decision") + terminal;
    assert_eq!(lifecycle, 280);
    let handshake = l.record("Submitted") + l.record("Negotiated");
    assert_eq!(active * lifecycle + handshake, 18_016);
    // Byte room does not imply record room. Admission must backpressure
    // before all 64 unacknowledged complete lifecycles accumulate.
    assert!(active * 5 + 2 > l.field("Limits", "journal_records", "max"));
    assert!(active * lifecycle + handshake < l.field("Limits", "journal_bytes", "max"));
    for text in [BROKER, PORTAL] {
        let l = Layout::parse(text).unwrap();
        assert_eq!(l.field("Limits", "ack_progress_timeout_ms", "max"), 2000);
        assert_eq!(l.field("Limits", "assembly_timeout_ms", "max"), 12_000);
    }
}

#[test]
fn corrupted_layouts_are_refused_instead_of_validating_their_own_arithmetic() {
    for (text, from, to) in [
        (BROKER, "offset=72 size=128", "offset=71 size=128"),
        (BROKER, "offset=72 size=128", "offset=73 size=127"),
        (BROKER, "offset=16 count=2", "offset=16 count=3"),
        (BROKER, "kind=257", "kind=256"),
        (PORTAL, "kind=33", "kind=300"),
        (PORTAL, "body \"Limits\" size=48", "body \"Limits\" size=47"),
        (PORTAL, "offset=40 nonzero=#true", "offset=39 nonzero=#true"),
        (PORTAL, "min=4 max=256", "min=257 max=256"),
    ] {
        assert!(text.contains(from), "mutation did not match: {from}");
        assert!(
            Layout::parse(&text.replacen(from, to, 1)).is_err(),
            "survived: {from}"
        );
    }
}
