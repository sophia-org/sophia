//! The owner thread's liveness poll counts as receipt rows, one per grid
//! cell, printed beside the other owner receipts on the resource cadence.
//! The grid is copied once before any row is printed, and every row carries
//! the timestamp of the receipts it is paired with. Cumulative, never reset.
use sophia_runtime::liveness_counts::{self, Boundary, Counts, Entry, Field};

pub(super) fn emit(owner_tid: i32, observed_monotonic_usec: u64) {
    let snapshot = liveness_counts::snapshot();
    for (entry, boundary, counts) in snapshot.rows() {
        crate::session_println!(
            "{}",
            row(owner_tid, observed_monotonic_usec, entry, boundary, counts)
        );
    }
}

/// One row: the identity and timestamp, the cell's two words, then every
/// counter in `Field::ALL` order, always present even when zero.
pub(super) fn row(
    owner_tid: i32,
    observed_monotonic_usec: u64,
    entry: Entry,
    boundary: Boundary,
    counts: Counts,
) -> String {
    let mut row = format!(
        "sophia_liveness_counts schema=1 owner_tid={owner_tid} observed_monotonic_usec={observed_monotonic_usec} entry={} boundary={}",
        entry.name(),
        boundary.name(),
    );
    for field in Field::ALL {
        row.push_str(&format!(" {}={}", field.name(), counts.get(field)));
    }
    row
}

mod tests;
