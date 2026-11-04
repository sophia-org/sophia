#![cfg(feature = "experimental-native-frame-transaction")]

use sophia_protocol::{ContentAllocationId, ContentGrant, ContentOutputId, ContentReason};
use sophia_runtime::{ContentCandidateError as Error, frame_credits::*};

fn key() -> FrameStreamKey {
    FrameStreamKey {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 2,
        },
        output: ContentOutputId {
            id: 1,
            generation: 3,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 4,
        },
        facts_generation: 5,
        stream_id: 1,
    }
}

fn opened() -> FrameCredits {
    let mut ledger = FrameCredits::new(key().grant, 250).unwrap();
    ledger.open(key(), 2, 0, || Ok(())).unwrap();
    ledger
}

#[test]
fn open_reservation_failure_grants_nothing_and_does_not_spend_stream_id() {
    let mut ledger = FrameCredits::new(key().grant, 250).unwrap();
    assert_eq!(
        ledger.open(key(), 2, 0, || Err(Error::Budget)),
        Err(Error::Budget)
    );
    assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
    assert_eq!(ledger.available(), 0);
    ledger.open(key(), 2, 0, || Ok(())).unwrap();
    assert_eq!(
        ledger.open(key(), 2, 0, || panic!("duplicate open reserved")),
        Err(Error::Stale)
    );
}

#[test]
fn budget_refusal_spends_identity_but_preserves_credit_and_outstanding_candidate() {
    let mut ledger = opened();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    assert_eq!(
        ledger.admit(key(), 2, 2, || Err(Error::Budget)),
        Err(Error::Budget)
    );
    assert_eq!((ledger.available(), ledger.outstanding()), (1, 1));
    assert_eq!(
        ledger.admit(key(), 2, 3, || panic!("replayed refusal")),
        Err(Error::Stale)
    );
    ledger.admit(key(), 3, 3, || Ok(())).unwrap();
    assert_eq!((ledger.available(), ledger.outstanding()), (0, 2));
}

#[test]
fn exhausted_credit_is_budget_not_stale_and_cannot_exceed_two_outstanding() {
    let mut ledger = opened();
    for candidate in 1..=2 {
        ledger.admit(key(), candidate, 1, || Ok(())).unwrap();
    }
    assert_eq!(
        ledger.admit(key(), 3, 2, || panic!("exhausted stream reserved")),
        Err(Error::Budget)
    );
    assert_eq!(ledger.phase(), FrameStreamPhase::Open);
    assert_eq!(ledger.outstanding(), 2);
    ledger.terminal(key(), 1, 3, |_| Ok(())).unwrap();
    assert_eq!(
        ledger.admit(key(), 3, 4, || panic!("replayed exhausted identity")),
        Err(Error::Stale)
    );
    ledger.admit(key(), 4, 4, || Ok(())).unwrap();
}

#[test]
fn terminal_is_committed_once_and_failed_publication_returns_no_credit() {
    let mut ledger = opened();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    assert_eq!(
        ledger.terminal(key(), 1, 2, |_| Err(Error::Budget)),
        Err(Error::Budget)
    );
    assert_eq!((ledger.available(), ledger.outstanding()), (1, 1));
    ledger
        .terminal(key(), 1, 3, |deadline| {
            assert_eq!(deadline, Some(253));
            Ok(())
        })
        .unwrap();
    assert_eq!((ledger.available(), ledger.outstanding()), (2, 0));
    assert_eq!(
        ledger.terminal(key(), 1, 4, |_| panic!("duplicate terminal published")),
        Err(Error::Stale)
    );
}

#[test]
fn every_tuple_field_and_stream_id_are_bound() {
    let mut ledger = opened();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    let mut variants = [key(); 8];
    variants[0].grant.connection_epoch += 1;
    variants[1].grant.content_grant_epoch += 1;
    variants[2].output.id += 1;
    variants[3].output.generation += 1;
    variants[4].allocation.id += 1;
    variants[5].allocation.generation += 1;
    variants[6].facts_generation += 1;
    variants[7].stream_id += 1;
    for wrong in variants {
        assert_eq!(
            ledger.admit(wrong, 2, 2, || panic!("foreign admission")),
            Err(Error::Stale)
        );
        assert_eq!(
            ledger.terminal(wrong, 1, 2, |_| panic!("foreign settlement")),
            Err(Error::Stale)
        );
        assert_eq!(
            ledger.close(wrong, ContentReason::Cancelled),
            Err(Error::Stale)
        );
    }
    assert_eq!((ledger.available(), ledger.outstanding()), (1, 1));
    ledger.admit(key(), 2, 2, || Ok(())).unwrap();
}

#[test]
fn oldest_credit_is_spent_first_and_client_delay_cannot_renew_it() {
    let mut ledger = opened();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    ledger
        .terminal(key(), 1, 100, |deadline| {
            assert_eq!(deadline, Some(350));
            Ok(())
        })
        .unwrap();
    ledger.admit(key(), 2, 200, || Ok(())).unwrap(); // spends original deadline 250
    ledger.expire(250).unwrap();
    assert_eq!(ledger.phase(), FrameStreamPhase::Open);
    ledger.expire(350).unwrap();
    assert_eq!(ledger.phase(), FrameStreamPhase::Closing);
    assert_eq!(
        ledger.pending_close().unwrap().reason,
        ContentReason::Timeout
    );
    ledger
        .terminal(key(), 2, 400, |deadline| {
            assert_eq!(deadline, None);
            Ok(())
        })
        .unwrap();
    assert_eq!(ledger.available(), 0);
}

#[test]
fn idle_expiry_closes_once_even_if_notice_is_temporarily_blocked() {
    let mut ledger = opened();
    ledger.expire(250).unwrap();
    assert_eq!(
        ledger.publish_close(|_| Err(Error::Budget)),
        Err(Error::Budget)
    );
    ledger.expire(500).unwrap();
    let notice = ledger.pending_close().unwrap();
    assert_eq!(notice.reason, ContentReason::Timeout);
    ledger
        .publish_close(|actual| {
            assert_eq!(actual, notice);
            Ok(())
        })
        .unwrap();
    assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
    assert_eq!(
        ledger.publish_close(|_| panic!("second close notice")),
        Err(Error::Stale)
    );
}

#[test]
fn close_notice_and_admitted_terminals_must_both_drain_before_reopen() {
    for notice_first in [false, true] {
        let mut ledger = opened();
        ledger.admit(key(), 1, 1, || Ok(())).unwrap();
        ledger.close(key(), ContentReason::Cancelled).unwrap();
        let next = FrameStreamKey {
            stream_id: 2,
            ..key()
        };
        assert_eq!(
            ledger.open(next, 2, 2, || panic!("reopen before drain")),
            Err(Error::Stale)
        );
        if notice_first {
            ledger.publish_close(|_| Ok(())).unwrap();
        }
        ledger
            .terminal(key(), 1, 2, |deadline| {
                assert_eq!(deadline, None);
                Ok(())
            })
            .unwrap();
        if !notice_first {
            assert_eq!(ledger.phase(), FrameStreamPhase::Closing);
            ledger.publish_close(|_| Ok(())).unwrap();
        }
        assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
        assert_eq!(
            ledger.open(key(), 2, 2, || panic!("replayed stream")),
            Err(Error::Stale)
        );
        ledger.open(next, 2, 2, || Ok(())).unwrap();
        assert_eq!(
            ledger.terminal(key(), 1, 3, |_| panic!("old stream returned credit")),
            Err(Error::Stale)
        );
        assert_eq!(
            ledger.admit(next, 1, 3, || panic!("old candidate replay")),
            Err(Error::Stale)
        );
    }
}

#[test]
fn clock_regression_closes_admission_but_prior_settlements_can_drain() {
    let mut ledger = opened();
    ledger.admit(key(), 1, 100, || Ok(())).unwrap();
    assert_eq!(ledger.expire(99), Err(Error::ClockRegression));
    assert_eq!(
        ledger.admit(key(), 2, 101, || panic!("poisoned admission")),
        Err(Error::ClockRegression)
    );
    ledger
        .terminal(key(), 1, 101, |deadline| {
            assert_eq!(deadline, None);
            Ok(())
        })
        .unwrap();
    ledger.publish_close(|_| Ok(())).unwrap();
    assert_eq!(
        ledger.open(
            FrameStreamKey {
                stream_id: 2,
                ..key()
            },
            2,
            102,
            || panic!("poisoned reopen")
        ),
        Err(Error::ClockRegression)
    );
}

#[test]
fn deadline_overflow_refuses_before_reservation() {
    let mut ledger = FrameCredits::new(key().grant, 250).unwrap();
    assert_eq!(
        ledger.open(key(), 2, u64::MAX, || panic!("overflow reserved")),
        Err(Error::Malformed)
    );
    assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
}

#[test]
fn replenishment_overflow_preserves_settlement_until_explicit_close() {
    let mut ledger = opened();
    for candidate in 1..=2 {
        ledger.admit(key(), candidate, 1, || Ok(())).unwrap();
    }
    assert_eq!(
        ledger.terminal(key(), 1, u64::MAX, |_| panic!("overflow published")),
        Err(Error::Malformed)
    );
    assert_eq!((ledger.available(), ledger.outstanding()), (0, 2));
    ledger.close(key(), ContentReason::Cancelled).unwrap();
    for candidate in 1..=2 {
        ledger
            .terminal(key(), candidate, u64::MAX, |deadline| {
                assert_eq!(deadline, None);
                Ok(())
            })
            .unwrap();
    }
    ledger.publish_close(|_| Ok(())).unwrap();
    assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
}

#[test]
fn caller_cannot_open_zero_or_more_than_two_credits() {
    let mut ledger = FrameCredits::new(key().grant, 250).unwrap();
    for count in [0, 3, u8::MAX] {
        assert_eq!(
            ledger.open(key(), count, 0, || panic!("bad count reserved")),
            Err(Error::Malformed)
        );
    }
    ledger.open(key(), 1, 0, || Ok(())).unwrap();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    assert_eq!(
        ledger.admit(key(), 2, 2, || panic!("one-credit over-issue")),
        Err(Error::Budget)
    );
}

#[test]
fn structural_bound_does_not_claim_a_temporal_rate_limit() {
    let mut ledger = opened();
    for generation in 1..=1000 {
        ledger.admit(key(), generation, 1, || Ok(())).unwrap();
        ledger.terminal(key(), generation, 1, |_| Ok(())).unwrap();
        assert_eq!((ledger.available(), ledger.outstanding()), (2, 0));
    }
}

#[test]
fn every_stream_key_field_is_checked_before_spend_or_terminal_commit() {
    let original = key();
    let mut wrong = [original; 8];
    wrong[0].grant.connection_epoch += 1;
    wrong[1].grant.content_grant_epoch += 1;
    wrong[2].output.id += 1;
    wrong[3].output.generation += 1;
    wrong[4].allocation.id += 1;
    wrong[5].allocation.generation += 1;
    wrong[6].facts_generation += 1;
    wrong[7].stream_id += 1;

    for foreign in wrong {
        let mut ledger = FrameCredits::new(original.grant, 250).unwrap();
        ledger.open(original, 1, 0, || Ok(())).unwrap();
        assert_eq!(
            ledger.admit(foreign, 1, 1, || panic!("foreign admission reserved")),
            Err(Error::Stale)
        );
        assert_eq!((ledger.available(), ledger.outstanding()), (1, 0));
        ledger.admit(original, 1, 1, || Ok(())).unwrap();
        assert_eq!(
            ledger.terminal(foreign, 1, 2, |_| panic!("foreign terminal published")),
            Err(Error::Stale)
        );
        assert_eq!((ledger.available(), ledger.outstanding()), (0, 1));
        ledger.terminal(original, 1, 2, |_| Ok(())).unwrap();
        assert_eq!((ledger.available(), ledger.outstanding()), (1, 0));
    }
}

#[test]
fn owner_drain_refusal_keeps_reopen_identity_and_old_completion_cannot_replenish_it() {
    let mut ledger = FrameCredits::new(key().grant, 250).unwrap();
    ledger.open(key(), 1, 0, || Ok(())).unwrap();
    ledger.admit(key(), 1, 1, || Ok(())).unwrap();
    ledger.close(key(), ContentReason::Cancelled).unwrap();
    ledger
        .terminal(key(), 1, 2, |credit| {
            assert_eq!(credit, None);
            Ok(())
        })
        .unwrap();
    ledger.publish_close(|_| Ok(())).unwrap();

    let successor = FrameStreamKey {
        stream_id: 2,
        ..key()
    };
    // Logical ledger close does not prove the owner's source/journal drain.
    assert_eq!(
        ledger.open(successor, 1, 3, || Err(Error::Budget)),
        Err(Error::Budget)
    );
    assert_eq!(ledger.phase(), FrameStreamPhase::Closed);
    assert_eq!(ledger.available(), 0);
    ledger.open(successor, 1, 4, || Ok(())).unwrap();
    assert_eq!(
        ledger.terminal(key(), 1, 4, |_| panic!("old completion published")),
        Err(Error::Stale)
    );
    assert_eq!((ledger.available(), ledger.outstanding()), (1, 0));
    assert_eq!(
        ledger.admit(successor, 1, 4, || panic!("generation replay reserved")),
        Err(Error::Stale)
    );
    ledger.admit(successor, 2, 4, || Ok(())).unwrap();
}
