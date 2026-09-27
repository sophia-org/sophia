//! Byte-level verification: reads the fields [`super::kdl_model`] parsed
//! back out of an encoded record and compares them against a fixture's own
//! field values.

use super::kdl_model::{Block, FieldSpec};
use sophia_shell_protocol::shell_files::{
    ShellFileHeader, ShellFileKind, ShellFileTransactionRecord,
};
use std::collections::{BTreeMap, BTreeSet};

fn read_field(bytes: &[u8], base: usize, spec: &FieldSpec) -> i128 {
    let start = base + spec.offset;
    let width = spec.width();
    let end = start + width;
    let slice = bytes.get(start..end).unwrap_or_else(|| {
        panic!(
            "field `{}`: offset {start}..{end} exceeds the encoded body's {} bytes",
            spec.name,
            bytes.len()
        )
    });
    match spec.ty.as_str() {
        "u8" => i128::from(slice[0]),
        "u16" => i128::from(u16::from_le_bytes(slice.try_into().unwrap())),
        "u32" => i128::from(u32::from_le_bytes(slice.try_into().unwrap())),
        "u64" => i128::from(u64::from_le_bytes(slice.try_into().unwrap())),
        "i16" => i128::from(i16::from_le_bytes(slice.try_into().unwrap())),
        "i32" => i128::from(i32::from_le_bytes(slice.try_into().unwrap())),
        "bytes" => slice
            .iter()
            .fold(0i128, |acc, b| (acc << 8) | i128::from(*b)),
        other => panic!("field `{}`: unknown type `{other}`", spec.name),
    }
}

/// Checks a fixed-size field block against `bytes` starting at `base`:
/// every non-reserved field must have a mapped fixture value and match the
/// encoded bytes; every fixture value must correspond to a declared field
/// (set equality, so an unmapped field on either side fails); `value=`,
/// `nonzero=`, `min=` and `max=` are all checked against the encoded bytes.
pub fn verify_block(
    bytes: &[u8],
    base: usize,
    label: &str,
    fields: &[FieldSpec],
    expected: &BTreeMap<&str, i128>,
) {
    let mut mapped: BTreeSet<&str> = BTreeSet::new();
    for spec in fields {
        // Text fields carry a string, not an integer; a dedicated caller
        // checks their bytes with `verify_text_field`, so they take no part
        // in the numeric expected-value bookkeeping here.
        if spec.ty == "text" {
            continue;
        }
        let actual = read_field(bytes, base, spec);
        if spec.is_reserved() {
            assert_eq!(
                spec.value,
                Some(0),
                "{label}.{}: a reserved field must declare value=0",
                spec.name
            );
        } else {
            mapped.insert(spec.name.as_str());
            let expected_value = *expected.get(spec.name.as_str()).unwrap_or_else(|| {
                panic!(
                    "{label}.{}: no fixture value is mapped for this KDL field \
                     (every non-reserved KDL field must have one)",
                    spec.name
                )
            });
            assert_eq!(
                actual, expected_value,
                "{label}.{} at offset {}: encoded bytes do not match the fixture's field value",
                spec.name, spec.offset
            );
        }
        if let Some(v) = spec.value {
            assert_eq!(
                actual, v,
                "{label}.{}: declared value={v} but encoded as {actual}",
                spec.name
            );
        }
        if spec.nonzero {
            assert_ne!(
                actual, 0,
                "{label}.{}: declared nonzero=true but encoded as 0",
                spec.name
            );
        }
        if let Some(min) = spec.min {
            assert!(
                actual >= min,
                "{label}.{} = {actual} is below its declared min={min}",
                spec.name
            );
        }
        if let Some(max) = spec.max {
            assert!(
                actual <= max,
                "{label}.{} = {actual} is above its declared max={max}",
                spec.name
            );
        }
    }
    let expected_names: BTreeSet<&str> = expected.keys().copied().collect();
    assert_eq!(
        expected_names, mapped,
        "{label}: the KDL fields and the fixture's fields disagree (see the set difference above); \
         every non-reserved KDL field needs a fixture value and every fixture field needs a KDL field"
    );
}

/// Sorts `fields` by offset and checks the fixed part covers exactly
/// `0..size` with no gap and no overlap.
pub fn assert_no_gaps_or_overlaps(label: &str, size: usize, fields: &[FieldSpec]) {
    let mut sorted: Vec<&FieldSpec> = fields.iter().collect();
    sorted.sort_by_key(|f| f.offset);
    let mut cursor = 0usize;
    for spec in sorted {
        assert_eq!(
            spec.offset, cursor,
            "{label}.{}: gap or overlap (expected the next field at offset {cursor}, got offset {})",
            spec.name, spec.offset
        );
        cursor += spec.width();
    }
    assert_eq!(
        cursor, size,
        "{label}: declared fields cover {cursor} bytes but size={size}"
    );
}

/// Checks one length-prefixed, zero-padded text field
/// (`shell::encoding::put_text_padded`/`take_text_padded`): the `u16`
/// length at `spec`'s offset, a reserved `u16`, then `spec.size` bytes of
/// UTF-8 text zero-padded past the length, against `expected`.
pub fn verify_text_field(bytes: &[u8], base: usize, label: &str, spec: &FieldSpec, expected: &str) {
    assert_eq!(
        spec.ty, "text",
        "{label}.{}: not a `type=\"text\"` field",
        spec.name
    );
    let max = spec
        .size
        .unwrap_or_else(|| panic!("field `{}`: type=text needs size=", spec.name));
    assert!(
        expected.len() <= max,
        "{label}.{}: fixture text is {} bytes, over the declared max={max}",
        spec.name,
        expected.len()
    );
    let start = base + spec.offset;
    let prefix = bytes.get(start..start + 4).unwrap_or_else(|| {
        panic!(
            "{label}.{}: length/reserved prefix at offset {start} exceeds the encoded body's {} bytes",
            spec.name,
            bytes.len()
        )
    });
    let len = u16::from_le_bytes([prefix[0], prefix[1]]) as usize;
    assert_eq!(
        len,
        expected.len(),
        "{label}.{}: length prefix disagrees with the fixture text",
        spec.name
    );
    assert_eq!(
        [prefix[2], prefix[3]],
        [0, 0],
        "{label}.{}: the reserved u16 after the length must be zero",
        spec.name
    );
    let text_start = start + 4;
    let text_bytes = bytes.get(text_start..text_start + max).unwrap_or_else(|| {
        panic!(
            "{label}.{}: text field at offset {text_start} exceeds the encoded body's {} bytes",
            spec.name,
            bytes.len()
        )
    });
    assert_eq!(
        &text_bytes[..len],
        expected.as_bytes(),
        "{label}.{}: text bytes disagree with the fixture",
        spec.name
    );
    assert!(
        text_bytes[len..].iter().all(|b| *b == 0),
        "{label}.{}: the padding tail beyond the length must be zero",
        spec.name
    );
}

pub fn verify_rows(
    bytes: &[u8],
    base: usize,
    row_size: usize,
    label: &str,
    row_fields: &[FieldSpec],
    rows_expected: &[BTreeMap<&str, i128>],
) {
    for (index, expected) in rows_expected.iter().enumerate() {
        verify_block(
            bytes,
            base + index * row_size,
            &format!("{label}[{index}]"),
            row_fields,
            expected,
        );
    }
}

/// Checks a full-body encode: the declared `size=` matches the real length,
/// then every field in it.
pub fn checked(body: Vec<u8>, block: &Block, label: &str, expected: &BTreeMap<&str, i128>) {
    assert_eq!(
        body.len(),
        block.size,
        "{label}: encoded body length disagrees with the KDL size="
    );
    verify_block(&body, 0, label, &block.fields, expected);
}

/// Encodes a single-payload transaction record and returns just its body,
/// asserting the transaction encoder picked the kind the test expects.
pub fn transaction_body(kind: ShellFileKind, tx_record: &ShellFileTransactionRecord) -> Vec<u8> {
    let (body_kind, body) =
        sophia_shell_protocol::shell_files::encode_shell_file_transaction_body(tx_record).unwrap();
    assert_eq!(body_kind, kind);
    body
}

/// The header field-value table for one encoded record: `total_bytes` is the
/// real encoded length, `api_version` is the fixed constant, the rest come
/// straight from the header the caller encoded with.
pub fn header_expected(header: ShellFileHeader, total_bytes: u64) -> BTreeMap<&'static str, i128> {
    BTreeMap::from([
        ("total_bytes", total_bytes as i128),
        ("api_version", 1i128),
        ("kind", (header.kind as u16) as i128),
        ("connection_epoch", header.connection_epoch as i128),
        ("submission_id", header.submission_id as i128),
        ("sequence", header.sequence as i128),
    ])
}
