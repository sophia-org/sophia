#![cfg(test)]
use super::*;

fn successful(submissions: usize, retirements: usize) -> NativeEvidenceSnapshot {
    NativeEvidenceSnapshot {
        submissions,
        retirements,
        nonzero_exports: submissions,
        ..Default::default()
    }
}

#[test]
fn replacements_preserve_disjoint_history_and_immediate_exit() {
    let mut session = NativeSessionEvidence::default();
    for reason in [
        "startup",
        "seat_resume",
        "vt_rejected",
        "disable_timeout",
        "startup_recovery",
        "topology_rebuild",
    ] {
        session.open(reason);
        session.close(&successful(100, 99), reason);
    }
    session.open("seat_resume");
    session.close(&successful(2, 0), "completion");
    let totals = session.snapshot(None);
    assert_eq!((totals.submissions, totals.retirements), (602, 594));
    assert!(totals.clean());
    assert_eq!(session.epoch, 7);
}

#[test]
fn previous_failures_and_unsettled_ownership_cannot_be_repaired_by_replacement() {
    let mut session = NativeSessionEvidence::default();
    session.open("startup");
    session.close(
        &NativeEvidenceSnapshot {
            submit_failures: 1,
            callback_rejected: 2,
            in_flight: true,
            cleanup_pending: true,
            ..successful(5, 3)
        },
        "forced_detach",
    );
    session.open("seat_resume");
    session.close(&successful(200, 199), "completion");
    let totals = session.snapshot(None);
    assert!(!totals.clean());
    assert_eq!(totals.submit_failures, 1);
    assert_eq!(totals.callback_rejected, 2);
    assert_eq!(session.unsettled_owners, 1);
    assert!(totals.in_flight && totals.cleanup_pending);
}

#[test]
fn maxima_gauges_and_sample_populations_have_different_aggregation_rules() {
    let mut first = successful(5, 4);
    first.max_submit_to_page_flip = Duration::from_millis(20);
    first.resources.uploads = 20;
    first.resources.snapshot_live_entries = 10;
    first.resources.frame_slots_high_watermark = 3;
    first
        .cost
        .record_offer_to_submit(false, Duration::from_micros(1));
    let mut next = successful(2, 1);
    next.max_submit_to_page_flip = Duration::from_millis(5);
    next.resources.uploads = 3;
    next.resources.snapshot_live_entries = 2;
    next.resources.frame_slots_high_watermark = 2;
    for _ in 0..3 {
        next.cost
            .record_offer_to_submit(false, Duration::from_micros(100));
    }
    first.append(&next);
    assert_eq!(first.max_submit_to_page_flip, Duration::from_millis(20));
    assert_eq!(first.resources.uploads, 23);
    assert_eq!(first.resources.snapshot_live_entries, 2);
    assert_eq!(first.resources.frame_slots_high_watermark, 3);
    let summary = first.cost.composed.offer_to_submit.summary().unwrap();
    assert_eq!((summary.frames, summary.p50), (4, 100));
    first.append(&NativeEvidenceSnapshot::default());
    assert_eq!(first.resources.snapshot_live_entries, 0);
    assert_eq!(first.resources.uploads, 23);
}

#[test]
#[should_panic(expected = "closed twice")]
fn duplicate_close_is_not_silently_counted() {
    let mut session = NativeSessionEvidence::default();
    session.open("startup");
    session.close(&successful(1, 1), "seat_release");
    session.close(&successful(1, 1), "seat_release");
}

#[test]
fn an_abandoned_replacement_is_opened_and_closed_like_any_other_owner() {
    // The topology phase opens each adopted replacement before resuming onto
    // it; a refused resume then closes that owner with no submissions.
    let mut session = NativeSessionEvidence::default();
    session.open("startup");
    session.close(&successful(10, 10), "topology_rebuild");
    assert_eq!(session.open("topology_rebuild"), 2);
    session.close(&NativeEvidenceSnapshot::default(), "replacement_refused");
    let totals = session.snapshot(None);
    assert_eq!((totals.submissions, totals.retirements), (10, 10));
    assert_eq!(session.unsettled_owners, 0);
    assert_eq!(session.epoch, 2);
}
