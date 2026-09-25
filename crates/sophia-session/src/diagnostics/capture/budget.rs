//! What one record name may write before it starts evicting the rest.
//!
//! Rotation is by bytes, so the fastest-arriving kind decides what survives:
//! one client's Present burst wrote 64,712 records in 836 milliseconds and
//! rotated every input-routing record out of the history before the check they
//! were produced for could read them. This bounds each name's share of a
//! segment, which leaves the sparse records that explain a session somewhere
//! to land.
//!
//! The accounting lives on the capture worker thread, so it needs no locking
//! and can be reset exactly at rotation -- the bound is "per segment" rather
//! than a rate, and a burst small enough to fit is never touched.

use crate::diagnostics::SEGMENT_LIMIT;

#[path = "../../../tests/support/diagnostic_capture_budget.rs"]
mod tests;

/// How much of one event segment a single record name may write.
///
/// Rotation is by bytes, so a kind that arrives fast enough owns every
/// segment: one client's Present burst wrote 64,712 records in 836ms and
/// rotated out every input-routing record a check had been run to read. A
/// quarter each means two flooding kinds still leave half a segment for the
/// sparse records that explain a session, while a burst small enough to fit
/// is untouched.
pub(super) const NAME_SEGMENT_SHARE: u64 = SEGMENT_LIMIT / 4;

/// How many distinct record names one segment's budget tracks.
///
/// The vocabulary is a fixed set of `sophia_*` names; this is that set with
/// room to spare. A name arriving past the cap is written without a share,
/// which is the safe direction: only a name common enough to be counted can
/// flood, and a name that first appears once the map is full is by
/// construction not one of those.
pub(super) const BUDGET_NAMES: usize = 256;

/// Per-name byte accounting for the segment currently being written.
///
/// Lives on the worker thread and is reset at rotation, so the bound is "per
/// segment" rather than a rate: a burst is admitted in full until its share is
/// spent, and gets a fresh share in the next segment.
#[derive(Debug, Default)]
pub(super) struct SegmentBudget {
    bytes: std::collections::BTreeMap<String, u64>,
    suppressed: std::collections::BTreeMap<String, u64>,
    /// Every record refused since the capture started, across segments.
    ///
    /// The per-name map above is per segment and is what rotation reports;
    /// this is what the health record reports, and it must not fall back to
    /// zero at a rotation or a session that suppressed tens of thousands of
    /// records reads as one that suppressed none.
    suppressed_total: u64,
}

/// What the budget decided about one entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Admission {
    Written,
    Suppressed,
    /// The first record this name has lost in this segment.
    ///
    /// Distinguished so a truncated kind is reported when the truncation
    /// starts, not only when the segment closes. Most sessions never fill a
    /// segment and so never rotate: an onscreen client at a hundred and
    /// eighteen frames a second spends this share in half a minute, and a
    /// rotation-only account would leave the busiest kind silently cut for the
    /// rest of the session with nothing but a total to say so.
    SuppressedFirst,
}

impl SegmentBudget {
    /// Whether this entry may be written, charging it when it may.
    pub(super) fn admit(&mut self, name: &str, len: u64) -> Admission {
        if let Some(spent) = self.bytes.get_mut(name) {
            if *spent >= NAME_SEGMENT_SHARE {
                let suppressed = self.suppressed.entry(name.to_owned()).or_default();
                let first = *suppressed == 0;
                *suppressed = suppressed.saturating_add(1);
                self.suppressed_total = self.suppressed_total.saturating_add(1);
                return if first {
                    Admission::SuppressedFirst
                } else {
                    Admission::Suppressed
                };
            }
            *spent = spent.saturating_add(len);
            return Admission::Written;
        }
        if self.bytes.len() < BUDGET_NAMES {
            self.bytes.insert(name.to_owned(), len);
        }
        Admission::Written
    }

    /// The names suppressed in the segment just closed, and how many records
    /// each lost. Clears the accounting for the segment now being opened.
    pub(super) fn rotate(&mut self) -> Vec<(String, u64)> {
        self.bytes.clear();
        std::mem::take(&mut self.suppressed).into_iter().collect()
    }

    pub(super) fn total_suppressed(&self) -> u64 {
        self.suppressed_total
    }
}

/// The record name an entry is charged to: the first token of the message.
///
/// The same token `reduced_record` already validated as a `sophia_*` name, and
/// the same one the tracing layer dispatches on. Reading it back off the
/// formatted entry keeps the budget out of the producer threads.
pub(super) fn budget_name(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or(line)
}
