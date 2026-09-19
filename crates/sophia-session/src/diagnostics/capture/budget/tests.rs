#![cfg(test)]

//! What one record name may write before it starts evicting the rest.
//!
//! The incident these bound: one client's Present burst wrote 64,712 records
//! in 836 milliseconds and rotated every input-routing record out of the log
//! before anyone could read it. The share is per segment rather than a rate,
//! so a burst small enough to fit is untouched and a flood is cut where it
//! would otherwise take the whole segment.

use super::{
    Admission, BUDGET_NAMES, NAME_SEGMENT_SHARE, SEGMENT_LIMIT, SegmentBudget, budget_name,
};

#[test]
fn a_name_writes_until_its_share_is_spent() {
    let mut budget = SegmentBudget::default();
    let entry = NAME_SEGMENT_SHARE / 4;

    // The first four fill the share exactly; charging happens on admission, so
    // the fourth is admitted and the fifth finds nothing left.
    for _ in 0..4 {
        assert_eq!(
            budget.admit("sophia_x_present_delivery", entry),
            Admission::Written
        );
    }
    // The first loss is distinguished so it can be reported where it happens.
    assert_eq!(
        budget.admit("sophia_x_present_delivery", entry),
        Admission::SuppressedFirst
    );
    assert_eq!(
        budget.admit("sophia_x_present_delivery", entry),
        Admission::Suppressed,
        "only the first loss announces itself; the rest are counted"
    );
    assert_eq!(budget.total_suppressed(), 2);
}

#[test]
fn a_flood_does_not_silence_the_records_that_explain_the_session() {
    // The whole point: the sparse record must still be written while the
    // flooding one is being refused.
    let mut budget = SegmentBudget::default();
    for _ in 0..8 {
        budget.admit("sophia_x_present_delivery", NAME_SEGMENT_SHARE / 2);
    }
    assert_ne!(
        budget.admit("sophia_x_present_delivery", 64),
        Admission::Written
    );
    assert_eq!(
        budget.admit("sophia_live_input_route", 64),
        Admission::Written
    );
    assert_eq!(
        budget.admit("sophia_live_session_cursor", 64),
        Admission::Written
    );
}

#[test]
fn two_flooding_names_still_leave_half_the_segment() {
    // Two shares is the worst case worth stating: whatever else arrives has
    // half a segment to land in, which is what makes the log readable after a
    // burst rather than merely smaller.
    assert_eq!(NAME_SEGMENT_SHARE.saturating_mul(2), SEGMENT_LIMIT / 2);
}

#[test]
fn rotation_reports_what_the_closed_segment_refused_and_starts_clean() {
    let mut budget = SegmentBudget::default();
    budget.admit("sophia_x_present_delivery", NAME_SEGMENT_SHARE);
    for _ in 0..3 {
        budget.admit("sophia_x_present_delivery", 64);
    }
    budget.admit("sophia_x_present_submission", NAME_SEGMENT_SHARE);
    budget.admit("sophia_x_present_submission", 64);
    // Three lost for the first name and one for the second, whatever order
    // they arrived in.

    let suppressed = budget.rotate();

    assert_eq!(
        suppressed,
        [
            ("sophia_x_present_delivery".to_owned(), 3),
            ("sophia_x_present_submission".to_owned(), 1),
        ],
        "each refused name is accounted for by its own count"
    );
    assert_eq!(budget.total_suppressed(), 0);
    assert_eq!(
        budget.admit("sophia_x_present_delivery", NAME_SEGMENT_SHARE),
        Admission::Written,
        "a new segment grants a new share"
    );
}

#[test]
fn an_unbounded_vocabulary_cannot_grow_the_budget() {
    // The map is state on the worker thread, so its size is bounded even
    // against a name space it does not control. Names past the cap are written
    // rather than refused: only a name common enough to be counted can flood,
    // and one that first appears when the map is full is not that name.
    let mut budget = SegmentBudget::default();
    for index in 0..BUDGET_NAMES * 2 {
        assert_eq!(
            budget.admit(&format!("sophia_kind_{index}"), NAME_SEGMENT_SHARE),
            Admission::Written
        );
    }
    // The first names filled the map and are each held to their share.
    assert_eq!(
        budget.admit("sophia_kind_0", 64),
        Admission::SuppressedFirst,
        "a name the budget is tracking is held to its share"
    );
    // One that arrived past the cap is untracked, so it is written rather than
    // refused -- the safe direction for a name too rare to have been counted.
    let overflowed = format!("sophia_kind_{}", BUDGET_NAMES * 2 - 1);
    assert_eq!(
        budget.admit(&overflowed, NAME_SEGMENT_SHARE),
        Admission::Written
    );
    assert_eq!(budget.total_suppressed(), 1);
}

#[test]
fn a_record_is_charged_to_its_own_name() {
    // The same first token the reduction validated and the tracing layer
    // dispatches on, so the budget and the admission agree on what a kind is.
    assert_eq!(
        budget_name("sophia_x_present_delivery schema=1 client=3 kind=idle"),
        "sophia_x_present_delivery"
    );
    assert_eq!(budget_name("sophia_live_session"), "sophia_live_session");
    assert_eq!(budget_name(""), "");
}
