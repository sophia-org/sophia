#![cfg(feature = "experimental-native-frame-transaction")]

use sophia_protocol::*;
use sophia_runtime::{
    ContentCandidateError as Error, ContentFrameRequestAdmission as Admission, *,
};
use std::cell::Cell;

#[path = "support/frame_stream.rs"]
#[expect(
    dead_code,
    reason = "intake cases use a subset of the shared lifecycle fixture"
)]
mod fixture;
use fixture::*;

fn open(f: &mut Fixture) {
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
}

#[test]
fn no_reply_capacity_leaves_input_and_generation_untouched() {
    let mut f = Fixture::new(64);
    open(&mut f);
    let taken = Cell::new(0);
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), begin(1, 1), now, 0, || taken.set(taken.get() + 1)),
        Ok(Admission::Deferred)
    );
    assert_eq!(taken.get(), 0);
    assert_eq!(f.candidates().pending_event_count(), 0);
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), begin(1, 1), now, 2, || taken.set(taken.get() + 1)),
        Ok(Admission::Accepted)
    );
    assert_eq!(taken.get(), 1);
}

#[test]
fn one_slot_reports_budget_without_spending_credit_or_faking_a_terminal() {
    let mut f = Fixture::new(64);
    open(&mut f);
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), begin(1, 1), now, 1, || {}),
        Ok(Admission::Refused(Error::Budget))
    );
    assert!(f.candidates().pending_event().is_none());
    let refused = f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(refused.transaction, tx(11));
    assert!(matches!(
        refused.record,
        ContentFrameStreamRecord::Refused {
            request_kind: sophia_protocol::shell_files::ShellFileKind::FrameCommit,
            candidate_generation: 1,
            reason: ContentReason::Budget,
            ..
        }
    ));
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(12), begin(1, 1), now, 2, || {}),
        Ok(Admission::Refused(Error::Stale))
    );
    f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(13), begin(2, 1), now, 2, || {}),
        Ok(Admission::Accepted)
    );
}

#[test]
fn closure_between_peek_and_consume_is_nonfatal_and_preserves_identity() {
    let mut f = Fixture::new(64);
    open(&mut f);
    let request = begin(1, 1);
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(99), key(1), now)
        .unwrap();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), request, now, 1, || {}),
        Ok(Admission::Refused(Error::Stale))
    );
    assert!(matches!(
        f.candidates().take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Closed(_)
    ));
    assert!(matches!(
        f.candidates().take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Refused {
            request_kind: sophia_protocol::shell_files::ShellFileKind::FrameCommit,
            reason: ContentReason::Stale,
            ..
        }
    ));
    f.open_with_credits(2, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(12), begin(1, 2), now, 2, || {}),
        Ok(Admission::Accepted)
    );
}

#[test]
fn accepted_continuations_progress_at_the_exact_full_reservation_limit() {
    let mut f = Fixture::new(3);
    open(&mut f);
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), begin(1, 1), now, 2, || {}),
        Ok(Admission::Accepted)
    );
    // Lifetime close + Prepared + terminal fill the candidate store's budget.
    assert_eq!(f.pool.accounting().response_records, 6); // plus three resource release credits
    f.finish_frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    f.drain_outcomes();
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(12), begin(2, 1), now, 1, || {}),
        Ok(Admission::Refused(Error::Budget))
    );
    assert_eq!(f.candidates().submitted_candidate_count(), 1);
    assert_eq!(f.pool.accounting().response_records, 6);
}

#[test]
fn foreign_grant_fails_before_consumption_even_when_a_reply_slot_is_free() {
    let mut f = Fixture::new(64);
    open(&mut f);
    let mut request = begin(1, 1);
    request.grant.connection_epoch += 1;
    let now = f.tick();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), request, now, 2, || panic!("foreign input consumed")),
        Err(Error::Stale)
    );
    assert_eq!(f.candidates().pending_event_count(), 0);
}

#[test]
fn stale_stream_input_never_consumes_a_legacy_permit_with_the_same_number() {
    let mut f = Fixture::new(64);
    let now = f.tick();
    f.candidates()
        .grant_permit(tx(10), output(), 1, 1, now)
        .unwrap();
    f.candidates().take_event().unwrap();
    assert_eq!(
        f.candidates()
            .admit_frame_begin(tx(11), begin(1, 1), now, 1, || {}),
        Ok(Admission::Refused(Error::Stale))
    );
    f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(f.candidates().begin(tx(12), begin(1, 1), now), Ok(()));
}
