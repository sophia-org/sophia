#![cfg(feature = "experimental-native-frame-transaction")]

use ContentCandidateError as Error;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/frame_stream.rs"]
mod fixture;
use fixture::*;

#[test]
fn queued_terminal_does_not_return_credit_before_output_transfer() {
    let mut f = Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    f.frame(1, 1, None).unwrap();
    let _bundle = f.submit(1);
    f.drain_outcomes();
    f.candidates()
        .presented_at(output(), 1, 40, 20, 30, 100)
        .unwrap();
    assert_eq!(
        f.candidates().begin(tx(12), begin(2, 1), 100),
        Err(Error::Budget)
    );
    assert!(
        f.candidates().take_event().is_none(),
        "untimed reader consumed stream terminal"
    );
    let held = f.candidates().pending_event().cloned();
    assert_eq!(
        {
            let (resources, candidates) = f.pool.active_pair_mut().unwrap();
            candidates.transfer_frame_event_at(resources, 150, |_| Err(Error::Budget))
        },
        Err(Error::Budget)
    );
    assert_eq!(f.candidates().pending_event().cloned(), held);
    assert_eq!(
        {
            let (resources, candidates) = f.pool.active_pair_mut().unwrap();
            candidates.transfer_frame_event_at(resources, 175, |_| Err(Error::Malformed))
        },
        Err(Error::Malformed)
    );
    assert!(f.candidates().pending_frame_stream_event().is_none());
    f.now = 200;
    let transferred = f.drain_outcomes();
    assert_eq!(transferred[0].frame_credit.unwrap().expires_msec, Some(450));
    assert_eq!(f.candidates().begin(tx(13), begin(3, 1), 400), Ok(()));
}

fn outcome(event: &ContentCandidateEvent) -> &ContentCandidateOutcome {
    let ShellContentRecord::CandidateOutcome(value) = &event.record else {
        panic!("expected outcome")
    };
    value
}

#[test]
fn control_budget_three_reserves_open_reply_and_lifetime_close_before_begin() {
    let mut f = Fixture::new(3);
    f.open(1).unwrap();
    assert_eq!(f.pool.accounting().response_records, 5);
    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(11), begin(1, 1), now),
        Err(Error::Budget)
    );
    assert_eq!(f.pool.accounting().response_records, 5);
    f.candidates().take_frame_stream_event().unwrap();
    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(11), begin(1, 1), now),
        Err(Error::Stale)
    );
    f.frame(2, 1, None).unwrap();
    assert_eq!(f.pool.accounting().response_records, 6);
}

#[test]
fn prepared_does_not_replenish_and_commit_time_is_fresh_and_explicit() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _bundle = f.submit(1);
    let prepared = f.drain_outcomes();
    assert_eq!(prepared.len(), 1);
    assert_eq!(outcome(&prepared[0]).kind, 1);
    assert_eq!(prepared[0].frame_credit, None);
    assert_eq!(
        f.candidates().presented(output(), 1, 40, 20, 30),
        Err(Error::Stale)
    );
    assert_eq!(
        f.candidates().renderer_failed(output(), 1),
        Err(Error::Stale)
    );
    assert_eq!(f.candidates().submitted_candidate_count(), 1);
    assert_eq!(
        f.candidates().presented_at(output(), 2, 40, 20, 30, 90),
        Err(Error::Stale)
    );
    assert_eq!(
        f.candidates().renderer_failed_at(output(), 2, 90),
        Err(Error::Stale)
    );
    assert_eq!(f.candidates().pending_event_count(), 0);
    assert_eq!(f.candidates().submitted_candidate_count(), 1);
    f.candidates()
        .presented_at(output(), 1, 40, 20, 30, 100)
        .unwrap();
    f.now = 100;
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].frame_credit.unwrap().expires_msec, Some(350));
    assert_eq!(
        f.candidates().presented_at(output(), 1, 40, 20, 30, 101),
        Err(Error::Stale)
    );
    assert_eq!(f.candidates().pending_event_count(), 0);
}

#[test]
fn zero_credit_refuses_without_superseding_the_pending_candidate() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    f.drain_outcomes();
    f.frame(2, 2, Some(retire(1))).unwrap();
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 8);
    let count = f.pool.accounting().response_records;
    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(11), begin(3, 1), now),
        Err(Error::Budget)
    );
    assert_eq!(f.candidates().next_pending_candidate(), Some((output(), 2)));
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 8);
    assert_eq!(f.pool.accounting().response_records, count);
    f.present(1);
    f.drain_outcomes();
    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(11), begin(3, 1), now),
        Err(Error::Stale)
    );
    f.frame(4, 3, None).unwrap();
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (
            outcome(&events[0]).kind,
            outcome(&events[0]).candidate_generation
        ),
        (4, 2)
    );
    assert!(events[0].frame_credit.unwrap().expires_msec.is_some());
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 0);
    assert!(f.pool.active().unwrap().lease(grant(), resource(1)).is_ok());
}

#[test]
fn failed_successor_keeps_w2_until_renderer_failure_settles_once() {
    let mut f = Fixture::new(3);
    f.open_drained();
    f.frame(1, 3, Some(retire(1))).unwrap();
    let now = f.tick();
    // With max=3, lifetime close + Superseded + successor's pair would require 4.
    assert_eq!(
        f.candidates().begin(tx(11), begin(2, 1), now),
        Err(Error::Budget)
    );
    assert_eq!(f.candidates().next_pending_candidate(), Some((output(), 1)));
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 8);
    let _bundle = f.submit(1);
    f.drain_outcomes();
    f.candidates().renderer_failed_at(output(), 1, 100).unwrap();
    f.now = 100;
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 1);
    assert_eq!(outcome(&events[0]).kind, 3);
    assert_eq!(events[0].frame_credit.unwrap().expires_msec, Some(350));
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 0);
    assert_eq!(
        f.candidates().renderer_failed_at(output(), 1, 101),
        Err(Error::Stale)
    );
}

#[test]
fn full_control_budget_can_queue_close_once_and_fifo_cannot_skip_it() {
    let mut f = Fixture::new(3);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    assert_eq!(f.pool.accounting().response_records, 6);
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(99), key(1), now)
        .unwrap();
    f.candidates().renderer_failed_at(output(), 1, now).unwrap();
    assert_eq!(f.candidates().pending_event_count(), 3);
    assert!(f.candidates().take_frame_stream_event().is_none());
    assert_eq!(outcome(&f.candidates().take_event().unwrap()).kind, 1);
    assert!(f.candidates().pending_event().is_none());
    assert!(f.candidates().take_event().is_none());
    let notice = *f.candidates().pending_frame_stream_event().unwrap();
    assert_eq!(notice.transaction, tx(99));
    assert_eq!(f.candidates().take_frame_stream_event(), Some(notice));
    assert!(f.candidates().take_frame_stream_event().is_none());
    assert_eq!(
        f.open(2),
        Err(Error::Stale),
        "terminal behind close has not drained"
    );
    let terminal = f.drain_outcomes().remove(0);
    assert_eq!(terminal.frame_credit.unwrap().expires_msec, None);
    assert_eq!(f.pool.accounting().response_records, 3);
    f.open(2).unwrap();
}

#[test]
fn stream_head_blocks_ordinary_events_without_touching_the_presented_retire_owner() {
    let mut f = Fixture::new(64);
    f.open(1).unwrap(); // deliberately keep Opened at the FIFO head
    f.frame(1, 3, Some(retire(1))).unwrap();
    let _held = f.submit(1);
    f.present(1);
    assert_eq!(f.candidates().pending_event_count(), 3);
    assert!(f.candidates().pending_event().is_none());
    assert!(f.candidates().take_event().is_none());
    assert_eq!(f.pool.active().unwrap().reserved_retiring(), 8);
    f.candidates().take_frame_stream_event().unwrap();
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 2);
    assert!(events[1].retire.is_some());
    assert!(
        f.pool
            .active()
            .unwrap()
            .lease(grant(), resource(1))
            .is_err()
    );
}

#[test]
fn w2_release_still_waits_for_last_renderer_lease_after_stream_close() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let held = f.submit(1);
    f.present(1);
    f.drain_outcomes();
    f.frame(2, 2, Some(retire(1))).unwrap();
    let _shown = f.submit(2);
    f.present(2);
    f.drain_outcomes();
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(99), key(1), now)
        .unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(f.pool.active().unwrap().usage().retiring, 8);
    assert!(f.pool.active_mut().unwrap().take_event().is_none());
    drop(held);
    f.pool.collect();
    let event = f.pool.active_mut().unwrap().take_event().unwrap();
    assert!(
        matches!(event.record, ShellContentRecord::ResourceReleased(v) if v.resource == resource(1))
    );
    assert!(f.pool.active_mut().unwrap().take_event().is_none());
}

#[test]
fn disconnect_clears_stream_ownership_so_backend_drop_uses_the_legacy_drain() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 3, Some(retire(1))).unwrap();
    let held = f.submit(1);
    f.pool.disconnect();
    assert_eq!(f.pool.accounting().response_records, 1);
    assert_eq!(
        f.pool
            .finish_after_backend_drop(held)
            .unwrap_or_else(|_| panic!("backend cleanup refused")),
        1
    );
    assert_eq!(f.pool.accounting().candidates, 0);
}

#[test]
fn clock_fault_closes_stream_even_when_ledger_clock_was_older_than_store_clock() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    let last = f.now; // prepared advanced the store, but not the pacing ledger
    assert_eq!(f.candidates().expire(last - 1), Err(Error::ClockRegression));
    f.candidates()
        .renderer_failed_at(output(), 1, last + 1)
        .unwrap();
    f.now = last + 1;
    f.drain_outcomes();
    let close = f.candidates().take_frame_stream_event().unwrap();
    assert!(matches!(close.record, ContentFrameStreamRecord::Closed(_)));
    let terminal = f.drain_outcomes().remove(0);
    assert_eq!(terminal.frame_credit.unwrap().expires_msec, None);
    f.now = last + 2;
    assert_eq!(f.open(2), Err(Error::ClockRegression));
}

#[test]
fn expiry_rejects_later_admission_and_never_renews_from_wakes() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.candidates().expire(251).unwrap();
    f.candidates().expire(999).unwrap();
    assert_eq!(f.candidates().pending_event_count(), 1);
    assert!(
        matches!(f.candidates().take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Closed(v) if v.reason == ContentReason::Timeout)
    );
    assert_eq!(
        f.candidates().begin(tx(11), begin(1, 1), 1000),
        Err(Error::Stale)
    );
}

#[test]
fn demand_permit_and_stream_modes_are_exclusive_without_consuming_a_refused_open() {
    let mut f = Fixture::new(64);
    let demand = ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId::default(),
        demand_id: 1,
        reason: 1,
    };
    f.candidates()
        .demand(tx(8), demand.clone(), &[output()], &[])
        .unwrap();
    assert_eq!(f.open(1), Err(Error::Stale));
    f.candidates()
        .cancel_demand(
            tx(9),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 1,
                permit_id: 0,
            },
        )
        .unwrap();
    while f.candidates().take_event().is_some() {}
    f.open_drained();
    assert_eq!(
        f.candidates().demand(tx(8), demand, &[output()], &[]),
        Err(Error::Stale)
    );
    assert_eq!(
        f.candidates().cancel_demand(
            tx(9),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 1,
                permit_id: 0,
            }
        ),
        Err(Error::Stale)
    );
    let now = f.tick();
    assert_eq!(
        f.candidates().grant_permit(tx(9), output(), 1, 1, now),
        Err(Error::Stale)
    );
    assert_eq!(f.open(2), Err(Error::Stale));
    f.frame(1, 1, None).unwrap();

    let mut other = Fixture::new(64);
    other
        .candidates()
        .grant_permit(tx(9), output(), 1, 1, 0)
        .unwrap();
    assert_eq!(other.open(1), Err(Error::Stale));
    assert!(matches!(
        other.candidates().take_event().unwrap().record,
        ShellContentRecord::FramePermit(_)
    ));
    assert!(other.candidates().pending_frame_stream_event().is_none());
}

#[test]
fn small_budgets_allow_atomic_supersession_without_double_reserving_the_old_pair() {
    for budget in [4, 5, 6] {
        let mut f = Fixture::new(budget);
        f.open_drained();
        f.frame(1, 3, Some(retire(1))).unwrap();
        f.frame(2, 2, None).unwrap();
        // Three resource release credits + close + new pair + Superseded.
        assert_eq!(f.pool.accounting().response_records, 7);
        let events = f.drain_outcomes();
        assert_eq!(events.len(), 1);
        assert_eq!(outcome(&events[0]).kind, 4);
        assert!(events[0].frame_credit.unwrap().expires_msec.is_some());
        assert_eq!(f.pool.active().unwrap().reserved_retiring(), 0);
        assert_eq!(f.candidates().next_pending_candidate(), Some((output(), 2)));
    }
}

#[test]
fn wrong_begin_tuple_is_stale_without_spending_the_candidate_identity() {
    let mut f = Fixture::new(64);
    f.open_drained();
    for field in 0..4 {
        let mut request = begin(1, 1);
        match field {
            0 => request.grant.connection_epoch += 1,
            1 => request.output.generation += 1,
            2 => request.facts_generation += 1,
            _ => request.pacing_permit += 1,
        }
        let now = f.tick();
        assert_eq!(
            f.candidates().begin(tx(11), request, now),
            Err(Error::Stale)
        );
        assert_eq!(f.candidates().pending_event_count(), 0);
    }
    f.frame(1, 1, None).unwrap();
}

#[test]
fn allocation_or_facts_change_closes_once_and_submitted_work_settles_without_credit() {
    for change_facts in [false, true] {
        let mut f = Fixture::new(64);
        f.open_drained();
        f.frame(1, 1, None).unwrap();
        let _held = f.submit(1);
        f.drain_outcomes();
        let mut allocations = f.allocations.clone();
        if !change_facts {
            allocations[0].allocation.generation += 1;
        }
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: if change_facts { 7 } else { 6 },
            interaction_generation: 8,
            allocations: &allocations,
        };
        let now = f.tick();
        f.candidates()
            .reconcile_frame_streams(&[context], now)
            .unwrap();
        assert!(
            matches!(f.candidates().take_frame_stream_event().unwrap().record,
            ContentFrameStreamRecord::Closed(v) if v.reason == ContentReason::AllocationLost)
        );
        f.candidates().reconcile_frame_streams(&[], now).unwrap();
        assert!(f.candidates().take_frame_stream_event().is_none());
        f.present(1);
        let events = f.drain_outcomes();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].frame_credit.unwrap().expires_msec, None);
    }
}

#[test]
fn end_rechecks_the_streams_explicit_allocation_and_refuses_a_changed_generation() {
    let mut f = Fixture::new(64);
    f.open_drained();
    let now = f.tick();
    f.candidates().begin(tx(11), begin(1, 1), now).unwrap();
    f.allocations[0].allocation.generation += 1;
    assert_eq!(f.finish_frame(1, 1, None), Err(Error::Stale));
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 1);
    assert_eq!(outcome(&events[0]).kind, 3);
    assert!(events[0].frame_credit.unwrap().expires_msec.is_some());
    assert!(f.candidates().next_pending_candidate().is_none());
}

#[test]
fn all_credits_outstanding_still_drain_when_the_unfinished_candidate_times_out() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    f.drain_outcomes();
    let now = f.tick();
    f.candidates().begin(tx(11), begin(2, 1), now).unwrap();
    f.candidates().expire(now + 999).unwrap();
    assert_eq!(
        f.candidates().pending_event_count(),
        0,
        "no available credit has expired"
    );
    f.candidates().expire(now + 1000).unwrap();
    f.now = now + 1000;
    let events = f.drain_outcomes();
    assert_eq!(events.len(), 1);
    assert_eq!(outcome(&events[0]).reason, ContentReason::Timeout as u16);
    assert_eq!(
        events[0].frame_credit.unwrap().expires_msec,
        Some(now + 1250)
    );
    f.candidates()
        .renderer_failed_at(output(), 1, now + 1000)
        .unwrap();
    assert_eq!(f.drain_outcomes().len(), 1);
}

#[test]
fn stream_reopen_cannot_replay_a_spent_candidate_or_a_prior_stream_id() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(7, 1, None).unwrap();
    let _held = f.submit(7);
    f.present(7);
    f.drain_outcomes();
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(99), key(1), now)
        .unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    assert_eq!(f.open(1), Err(Error::Stale));
    f.open(2).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    let now = f.tick();
    assert_eq!(
        f.candidates().begin(tx(11), begin(7, 2), now),
        Err(Error::Stale)
    );
    f.candidates().begin(tx(11), begin(8, 2), now).unwrap();
    f.finish_frame(8, 1, None).unwrap();
}

#[test]
fn replenishment_clock_overflow_closes_but_does_not_strand_the_real_terminal() {
    let mut f = Fixture::new(64);
    f.open_drained();
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    f.drain_outcomes();
    // Spend the other credit so expiry cannot close before replenishment overflows.
    let now = f.tick();
    f.candidates().begin(tx(11), begin(2, 1), now).unwrap();
    f.candidates()
        .presented_at(output(), 1, 40, 20, 30, u64::MAX)
        .unwrap();
    // Overflow is detected at transfer; the close follows this queued terminal.
    assert!(f.candidates().pending_frame_stream_event().is_none());
    f.now = u64::MAX;
    let events = f.drain_outcomes();
    assert!(
        matches!(f.candidates().take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Closed(v) if v.reason == ContentReason::Malformed)
    );
    assert_eq!(events.len(), 1);
    assert_eq!(outcome(&events[0]).kind, 2);
    assert_eq!(events[0].frame_credit.unwrap().expires_msec, None);
    f.candidates().expire(u64::MAX).unwrap();
    assert_eq!(f.drain_outcomes().len(), 1);
    assert_eq!(f.pool.accounting().candidates, 0);
}

#[test]
fn begin_on_a_closed_or_expired_stream_does_not_spend_identity_before_reopen() {
    for expired in [false, true] {
        let mut f = Fixture::new(64);
        f.open_drained();
        f.now = 251;
        if expired {
            f.candidates().expire(251).unwrap();
        } else {
            f.candidates()
                .close_frame_stream(tx(99), key(1), 2)
                .unwrap();
        }
        assert_eq!(
            f.candidates().begin(tx(11), begin(7, 1), 251),
            Err(Error::Stale)
        );
        assert_eq!(
            f.candidates().close_frame_stream(tx(99), key(1), 251),
            Err(Error::Stale)
        );
        f.candidates().take_frame_stream_event().unwrap();
        f.open(2).unwrap();
        f.candidates().take_frame_stream_event().unwrap();
        let now = f.tick();
        f.candidates().begin(tx(11), begin(7, 2), now).unwrap();
        f.finish_frame(7, 1, None).unwrap();
    }
}

#[test]
fn a_second_begin_during_assembly_is_stale_and_does_not_spend_its_identity() {
    let mut f = Fixture::new(64);
    f.open_drained();
    let now = f.tick();
    f.candidates().begin(tx(11), begin(1, 1), now).unwrap();
    let before = f.pool.accounting().response_records;
    assert_eq!(
        f.candidates().begin(tx(11), begin(2, 1), now),
        Err(Error::Stale)
    );
    assert_eq!(f.pool.accounting().response_records, before);
    f.finish_frame(1, 1, None).unwrap();
    f.frame(2, 2, None).unwrap();
    assert_eq!(f.candidates().next_pending_candidate(), Some((output(), 2)));
}

#[test]
fn streams_and_legacy_permits_share_one_open_slot_ceiling() {
    let other = ContentOutputId {
        id: 3,
        generation: 3,
    };
    let mut f = Fixture::with_slots(64, 1);
    f.candidates().grant_permit(tx(9), other, 1, 1, 0).unwrap();
    assert_eq!(f.open(1), Err(Error::Budget));
    f.candidates().take_event().unwrap();
    f.candidates().expire(250).unwrap();
    f.candidates().take_event().unwrap();
    f.now = 250;
    f.open_drained();
    let now = f.tick();
    assert_eq!(
        f.candidates().grant_permit(tx(9), other, 2, 2, now),
        Err(Error::Budget)
    );
    // The stream's assembly uses its already-held slot, not a second slot.
    f.frame(1, 1, None).unwrap();
    let _held = f.submit(1);
    f.present(1);
    f.drain_outcomes();
    let now = f.tick();
    f.candidates()
        .close_frame_stream(tx(99), key(1), now)
        .unwrap();
    assert_eq!(
        f.candidates().grant_permit(tx(9), other, 2, 2, now),
        Err(Error::Budget)
    );
    f.candidates().take_frame_stream_event().unwrap();
    f.candidates()
        .grant_permit(tx(9), other, 2, 2, now)
        .unwrap();
}
