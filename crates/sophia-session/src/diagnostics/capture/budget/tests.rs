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
    assert_eq!(
        budget.total_suppressed(),
        4,
        "the health total survives rotation; only the per-name accounting resets"
    );
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

/// What the reference host actually writes, measured from the post-repair
/// session `00000001789950095665-f150c787` on 2026-09-20.
///
/// A `sophia_x_present_delivery` entry is 239 bytes with its sequence,
/// timestamp and monotonic columns: that session wrote 3,932,186 bytes of
/// them in 16,443 records before its share was spent. Eight are emitted per
/// presented frame -- four statuses for each of two kinds -- so an onscreen
/// client at 118 frames a second offers 944 of them a second.
const DELIVERY_ENTRY: u64 = 239;
const DELIVERY_PER_FRAME: u64 = 8;
const REFERENCE_FPS: u64 = 118;

#[test]
fn the_share_buys_the_routing_records_a_whole_segment_at_the_reference_rate() {
    // t118's judgement, at the operating point the shake produces rather than
    // the one an ordinary desktop does. The real post-repair session settles
    // that routing records survive at all -- 411 and 1,434 of them per
    // segment, against 203,263 and 177,412 delivery records refused -- but it
    // ran at about 157 delivery records a second. The shake's client offers
    // six times that, and the question is whether the share still leaves the
    // sparse kinds a segment to land in when it does.
    let mut budget = SegmentBudget::default();
    let per_second = DELIVERY_PER_FRAME * REFERENCE_FPS;

    let mut admitted = 0u64;
    while budget.admit("sophia_x_present_delivery", DELIVERY_ENTRY) == Admission::Written {
        admitted += 1;
        assert!(
            admitted < 1_000_000,
            "the share must be spent, not unbounded"
        );
    }

    // Spent in seconds, not minutes: the flood is cut early in the segment and
    // everything that follows it in that segment is somebody else's.
    let seconds_to_spend = admitted / per_second;
    assert_eq!(
        (admitted, seconds_to_spend),
        (16_453, 17),
        "the reference client spends the share in seventeen seconds"
    );

    // The segment is ~23 minutes at the rate that session rotated at, so the
    // routing records the check exists to read arrive long after the flood was
    // cut. They must still be written -- this is the whole repair.
    for _ in 0..4_096 {
        assert_eq!(
            budget.admit("sophia_live_session_input_routing", 512),
            Admission::Written,
            "a routing record must outlive the flood that used to evict it"
        );
    }
    assert_eq!(
        budget.admit("sophia_live_session_pointer", 512),
        Admission::Written
    );

    // And the flood is still being refused while they land, which is what the
    // health total reported as 457,843 for that session.
    assert_eq!(
        budget.admit("sophia_x_present_delivery", DELIVERY_ENTRY),
        Admission::Suppressed
    );
}

#[test]
fn a_second_flooding_kind_does_not_take_the_routing_records_room() {
    // Both Present kinds hit their share in that session -- delivery in every
    // segment, submission in two of three. Two shares is half the segment, so
    // the sparse kinds still have the other half; this is that bound driven
    // through the accounting rather than asserted of the constant.
    let mut budget = SegmentBudget::default();
    for name in ["sophia_x_present_delivery", "sophia_x_present_submission"] {
        while budget.admit(name, DELIVERY_ENTRY) == Admission::Written {}
    }

    let mut room = 0u64;
    while budget.admit("sophia_live_session_input_routing", 512) == Admission::Written {
        room += 512;
        assert!(room <= SEGMENT_LIMIT, "a third name is bounded too");
    }
    assert_eq!(
        room, NAME_SEGMENT_SHARE,
        "a third kind gets a full share of its own after two floods"
    );
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
