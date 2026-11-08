//! Owner liveness poll receipts: the entry and boundary words come from the
//! runtime's own vocabulary and every counter is an unsigned integer; any
//! other field is dropped from the row.
use sophia_runtime::liveness_counts::{Boundary, Entry, Field};

const RECORD: &str = "sophia_liveness_counts";
/// More fields than the emitter prints; the rest are not read.
const FIELD_LIMIT: usize = 16;

pub(super) fn record(name: &str) -> bool {
    name == RECORD
}

pub(super) fn reduce<'a>(name: &str, fields: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut result = name.to_owned();
    for field in fields.take(FIELD_LIMIT) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if admitted(key, value) {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

fn admitted(key: &str, value: &str) -> bool {
    match key {
        "entry" => Entry::ALL.iter().any(|entry| entry.name() == value),
        "boundary" => Boundary::ALL
            .iter()
            .any(|boundary| boundary.name() == value),
        "schema" | "owner_tid" | "observed_monotonic_usec" => unsigned(value),
        _ => Field::ALL.iter().any(|field| field.name() == key) && unsigned(value),
    }
}

fn unsigned(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) && value.parse::<u64>().is_ok()
}
