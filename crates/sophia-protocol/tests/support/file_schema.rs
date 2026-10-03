//! Reads a native file contract's KDL: every block's fields, sizes and
//! declared constraints, for tests that bind a codec to its contract.
use std::collections::{BTreeMap, BTreeSet};

use kdl::{KdlDocument, KdlNode};

pub fn text<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.get(key).and_then(|value| value.as_string())
}

pub fn integer(node: &KdlNode, key: &str) -> Option<i128> {
    node.get(key).and_then(|value| value.as_integer())
}

pub fn name(node: &KdlNode) -> &str {
    node.get(0)
        .and_then(|value| value.as_string())
        .unwrap_or_else(|| panic!("{} has no name", node.name().value()))
}

pub fn flag(node: &KdlNode, key: &str) -> bool {
    node.get(key).and_then(|value| value.as_bool()) == Some(true)
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub offset: usize,
    pub width: usize,
    pub value: Option<i128>,
    pub min: Option<i128>,
    pub max: Option<i128>,
    pub mask: Option<i128>,
    pub required: Option<i128>,
    pub nonzero: bool,
    pub row: Option<(String, usize)>,
}

impl Field {
    pub fn constrained(&self) -> bool {
        self.value.is_some()
            || self.min.is_some()
            || self.max.is_some()
            || self.mask.is_some()
            || self.nonzero
    }
}

pub struct Schema {
    pub root: KdlNode,
    pub blocks: BTreeMap<String, (usize, Vec<Field>)>,
    pub kinds: BTreeMap<String, (String, u16)>,
}

impl Schema {
    pub fn parse(schema: &str) -> Self {
        let document = KdlDocument::parse_v2(schema).unwrap();
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

    pub fn nodes(&self, class: &str) -> impl Iterator<Item = &KdlNode> {
        self.root
            .children()
            .unwrap()
            .nodes()
            .iter()
            .filter(move |node| node.name().value() == class)
    }

    pub fn values(&self, class: &str, key: &str) -> BTreeMap<String, i128> {
        self.nodes(class)
            .map(|node| (name(node).to_owned(), integer(node, key).unwrap()))
            .collect()
    }

    pub fn size(&self, block: &str) -> usize {
        self.blocks[block].0
    }

    pub fn field(&self, block: &str, field: &str) -> &Field {
        self.blocks[block]
            .1
            .iter()
            .find(|candidate| candidate.name == field)
            .unwrap_or_else(|| panic!("{block}.{field}"))
    }

    /// The row sequence following a variable body prefix.
    pub fn tail(&self, block: &str) -> Vec<(String, String)> {
        let node = self.nodes("rows").find(|node| name(node) == block).unwrap();
        node.children()
            .unwrap()
            .nodes()
            .iter()
            .map(|row| (name(row).to_owned(), text(row, "count").unwrap().to_owned()))
            .collect()
    }
}

pub fn fields(
    node: &KdlNode,
    size: usize,
    rows: &BTreeMap<String, (usize, Vec<Field>)>,
) -> Vec<Field> {
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

pub fn put(bytes: &mut [u8], at: usize, field: &Field, value: i128) {
    let width = if field.ty == "bytes" { 1 } else { field.width };
    let raw = value.to_le_bytes();
    bytes[at..at + width].copy_from_slice(&raw[..width]);
}

pub fn get(bytes: &[u8], at: usize, field: &Field) -> i128 {
    let mut raw = [0; 16];
    raw[..field.width].copy_from_slice(&bytes[at..at + field.width]);
    let value = i128::from_le_bytes(raw);
    if field.ty == "i32" {
        i128::from(value as u32 as i32)
    } else {
        value
    }
}

pub fn fits(field: &Field, value: i128) -> bool {
    match field.ty.as_str() {
        "i32" => i32::try_from(value).is_ok(),
        "bytes" => (0..=255).contains(&value),
        _ => value >= 0 && value < 1i128 << (8 * field.width),
    }
}

pub fn satisfies(field: &Field, value: i128) -> bool {
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
pub fn violations(field: &Field) -> Vec<i128> {
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
