#![cfg(feature = "experimental-native-frame-transaction")]

use sophia_protocol::{ContentReason, shell_files::ShellFileKind};
use sophia_runtime::{
    ContentCandidateContext, ContentCandidateError as Error,
    ContentFrameRequestAdmission as Admission, ContentFrameStreamRecord,
    frame_credits::FrameStreamKey,
};
use std::cell::Cell;

#[path = "support/frame_stream.rs"]
#[expect(
    dead_code,
    reason = "control tests use a subset of the lifecycle fixture"
)]
mod support;
use support::*;

fn open(
    f: &mut Fixture,
    requested: FrameStreamKey,
    now: u64,
    room: usize,
    taken: &Cell<usize>,
) -> Result<Admission, Error> {
    f.pool.active_candidates_mut().unwrap().admit_frame_open(
        tx(10),
        requested,
        Some(ContentCandidateContext {
            output: output(),
            facts_generation: 6,
            interaction_generation: 8,
            allocations: &f.allocations,
        }),
        now,
        room,
        || taken.set(taken.get() + 1),
    )
}

fn refused(f: &mut Fixture, request_kind: ShellFileKind, reason: ContentReason) {
    let event = f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(
        event.record,
        ContentFrameStreamRecord::Refused {
            request_kind,
            grant: grant(),
            output: output(),
            stream_id: 1,
            candidate_generation: 0,
            reason,
        }
    );
}

#[test]
fn open_reserves_reply_and_lifetime_close_before_spending_the_stream_id() {
    let mut f = Fixture::new(64);
    let taken = Cell::new(0);
    let before = f.pool.accounting().response_records;
    assert_eq!(open(&mut f, key(1), 1, 0, &taken), Ok(Admission::Deferred));
    assert_eq!(taken.get(), 0);
    assert_eq!(f.pool.accounting().response_records, before);
    assert_eq!(
        open(&mut f, key(1), 1, 1, &taken),
        Ok(Admission::Refused(Error::Budget))
    );
    assert_eq!(taken.get(), 1);
    assert_eq!(f.pool.accounting().response_records, before + 1);
    refused(
        &mut f,
        ShellFileKind::FrameStreamOpen,
        ContentReason::Budget,
    );
    assert_eq!(open(&mut f, key(1), 1, 2, &taken), Ok(Admission::Accepted));
    assert_eq!(taken.get(), 2);
    assert_eq!(f.pool.accounting().response_records, before + 2);
    assert!(matches!(
        f.candidates().take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Opened {
            credits: 1,
            ttl_ms: 250,
            ..
        }
    ));
    assert_eq!(f.pool.accounting().response_records, before + 1);
}

#[test]
fn local_capacity_still_bounds_open_even_when_the_connection_has_room() {
    let mut f = Fixture::new(1);
    let taken = Cell::new(0);
    assert_eq!(
        open(&mut f, key(1), 1, 64, &taken),
        Ok(Admission::Refused(Error::Budget))
    );
    assert_eq!(open(&mut f, key(1), 1, 64, &taken), Ok(Admission::Deferred));
    assert_eq!(taken.get(), 1);
    assert_eq!(f.candidates().pending_event_count(), 1);
    refused(
        &mut f,
        ShellFileKind::FrameStreamOpen,
        ContentReason::Budget,
    );
}

#[test]
fn close_uses_its_lifetime_reservation_but_still_needs_room_to_refuse() {
    let mut f = Fixture::new(64);
    let taken = Cell::new(0);
    assert_eq!(open(&mut f, key(1), 1, 2, &taken), Ok(Admission::Accepted));
    f.candidates().take_frame_stream_event().unwrap();
    let before = f.pool.accounting().response_records;
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(20), key(1), 2, 0, || panic!("deferred")),
        Ok(Admission::Deferred)
    );
    assert_eq!(f.pool.accounting().response_records, before);
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(20), key(1), 2, 1, || taken.set(taken.get() + 1)),
        Ok(Admission::Accepted)
    );
    assert_eq!(taken.get(), 2);
    assert_eq!(f.pool.accounting().response_records, before);
    let event = f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(event.transaction, tx(20));
    assert!(
        matches!(event.record, ContentFrameStreamRecord::Closed(v) if v.reason == ContentReason::Cancelled)
    );
    assert_eq!(f.pool.accounting().response_records, before - 1);
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(21), key(1), 2, 1, || {}),
        Ok(Admission::Refused(Error::Stale))
    );
    refused(
        &mut f,
        ShellFileKind::FrameStreamClose,
        ContentReason::Stale,
    );
    assert!(f.candidates().pending_frame_stream_event().is_none());
    assert_eq!(open(&mut f, key(2), 3, 2, &taken), Ok(Admission::Accepted));
}

#[test]
fn expiry_winning_close_is_a_nonfatal_refusal_after_the_single_timeout_notice() {
    let mut f = Fixture::new(64);
    let taken = Cell::new(0);
    assert_eq!(open(&mut f, key(1), 1, 2, &taken), Ok(Admission::Accepted));
    f.candidates().take_frame_stream_event().unwrap();
    let before = f.pool.accounting().response_records;
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(20), key(1), 251, 1, || taken.set(taken.get() + 1)),
        Ok(Admission::Refused(Error::Stale))
    );
    assert_eq!(taken.get(), 2);
    assert_eq!(f.pool.accounting().response_records, before + 1);
    let closed = f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(
        closed.transaction,
        tx(10),
        "automatic close belongs to Open"
    );
    assert!(
        matches!(closed.record, ContentFrameStreamRecord::Closed(v) if v.reason == ContentReason::Timeout)
    );
    refused(
        &mut f,
        ShellFileKind::FrameStreamClose,
        ContentReason::Stale,
    );
    assert!(f.candidates().pending_frame_stream_event().is_none());
    assert_eq!(
        open(&mut f, key(2), 251, 2, &taken),
        Ok(Admission::Accepted)
    );
}

#[test]
fn stale_context_and_duplicate_open_never_spend_identity_or_add_a_lifetime_credit() {
    let mut f = Fixture::new(64);
    let taken = Cell::new(0);
    let before = f.pool.accounting().response_records;
    let mut stale = key(1);
    stale.facts_generation += 1;
    assert_eq!(
        open(&mut f, stale, 1, 1, &taken),
        Ok(Admission::Refused(Error::Stale))
    );
    refused(&mut f, ShellFileKind::FrameStreamOpen, ContentReason::Stale);
    assert_eq!(f.pool.accounting().response_records, before);
    assert_eq!(open(&mut f, key(1), 1, 2, &taken), Ok(Admission::Accepted));
    f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(
        open(&mut f, key(1), 1, 2, &taken),
        Ok(Admission::Refused(Error::Stale))
    );
    refused(&mut f, ShellFileKind::FrameStreamOpen, ContentReason::Stale);
    assert_eq!(f.pool.accounting().response_records, before + 1);
}

#[test]
fn invalid_authority_and_malformed_identity_never_consume_control_input() {
    let mut f = Fixture::new(64);
    let taken = Cell::new(0);
    for (bad, expected) in [
        (
            FrameStreamKey {
                grant: sophia_protocol::ContentGrant {
                    connection_epoch: 99,
                    ..grant()
                },
                ..key(1)
            },
            Error::Stale,
        ),
        (
            FrameStreamKey {
                stream_id: 0,
                ..key(1)
            },
            Error::Malformed,
        ),
        (
            FrameStreamKey {
                allocation: sophia_protocol::ContentAllocationId::default(),
                ..key(1)
            },
            Error::Malformed,
        ),
    ] {
        assert_eq!(open(&mut f, bad, 1, 2, &taken), Err(expected));
        assert_eq!(
            f.candidates()
                .admit_frame_close(tx(20), bad, 1, 1, || panic!("invalid input")),
            Err(expected)
        );
    }
    assert_eq!(taken.get(), 0);
    assert_eq!(f.candidates().pending_event_count(), 0);
    assert_eq!(open(&mut f, key(1), 10, 2, &taken), Ok(Admission::Accepted));
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(20), key(1), 9, 1, || panic!("regressed clock")),
        Err(Error::ClockRegression)
    );
}

#[test]
fn occupied_candidate_budget_defers_control_without_consuming_or_cancelling_it() {
    let mut f = Fixture::new(3);
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    f.frame(1, 1, None).unwrap();
    let now = f.now;
    let before = f.pool.accounting().response_records;
    let taken = Cell::new(0);
    assert_eq!(
        open(&mut f, key(2), now, 64, &taken),
        Ok(Admission::Deferred)
    );
    assert_eq!(
        f.candidates()
            .admit_frame_close(tx(20), key(1), now, 64, || panic!("full budget")),
        Ok(Admission::Deferred)
    );
    assert_eq!(taken.get(), 0);
    assert_eq!(f.pool.accounting().response_records, before);
    assert_eq!(f.candidates().next_pending_candidate(), Some((output(), 1)));
    let _display = f.submit(1);
    f.drain_outcomes();
    f.present(1);
    assert!(
        f.drain_outcomes()[0]
            .frame_credit
            .unwrap()
            .expires_msec
            .is_some()
    );
}
