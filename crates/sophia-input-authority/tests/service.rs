//! Boundary and lifecycle controls for the service owner, without sleeping.

use sophia_input_authority::{
    CleanupReadiness, ExecutionPhase, ExecutionWatchdog, ServiceAccountingError, ServiceBudget,
    ServiceLimits, ServiceStartRefusal, ServiceWork, WatchdogError, WatchdogObservation,
};
use std::time::Duration;

const NEW: ServiceWork = ServiceWork::NewWork;
const CLEANUP: ServiceWork = ServiceWork::Cleanup;
const ELIGIBLE: CleanupReadiness = CleanupReadiness::Eligible;
const EMPTY: CleanupReadiness = CleanupReadiness::NoneEligible;

fn us(value: u64) -> Duration {
    Duration::from_micros(value)
}

fn run(budget: &mut ServiceBudget, begin: u64, end: u64, work: ServiceWork) {
    budget
        .start(us(begin), work, ELIGIBLE)
        .unwrap()
        .finish(us(end))
        .unwrap();
}

#[test]
fn planned_limits_are_the_approved_contract() {
    let limits = ServiceBudget::planned(Duration::ZERO).limits();
    assert_eq!(limits.interval, Duration::from_millis(16));
    assert_eq!(limits.starts, 32);
    assert_eq!(limits.charge, us(2000));
    assert_eq!(limits.cleanup_starts, 4);
    assert_eq!(limits.cleanup_charge, us(500));
    assert_eq!(ExecutionWatchdog::DEADLINE, Duration::from_millis(250));
}

#[test]
fn invalid_limits_refuse_before_any_start() {
    let planned = ServiceLimits::PLANNED;
    for limits in [
        ServiceLimits {
            interval: Duration::ZERO,
            ..planned
        },
        ServiceLimits {
            starts: 0,
            ..planned
        },
        ServiceLimits {
            charge: Duration::ZERO,
            ..planned
        },
        ServiceLimits {
            charge: Duration::from_millis(17),
            ..planned
        },
        ServiceLimits {
            cleanup_starts: 33,
            ..planned
        },
        ServiceLimits {
            cleanup_charge: us(2001),
            ..planned
        },
    ] {
        assert!(ServiceBudget::new(Duration::ZERO, limits).is_err());
    }
}

#[test]
fn cleanup_keeps_four_starts_when_new_work_saturates() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    for _ in 0..28 {
        run(&mut budget, 0, 0, NEW);
    }
    assert_eq!(
        budget.start(Duration::ZERO, NEW, ELIGIBLE).unwrap_err(),
        ServiceStartRefusal::CleanupStartsReserved {
            retry_after: us(16000)
        }
    );
    assert_eq!(budget.usage().starts, 28);
    for _ in 0..4 {
        run(&mut budget, 0, 0, CLEANUP);
    }
    assert_eq!(budget.usage().cleanup_starts, 4);
    assert_eq!(
        budget.start(Duration::ZERO, CLEANUP, ELIGIBLE).unwrap_err(),
        ServiceStartRefusal::StartsExhausted {
            retry_after: us(16000)
        }
    );
}

#[test]
fn donation_is_per_decision_and_never_a_sticky_mode() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    for _ in 0..28 {
        run(&mut budget, 0, 0, NEW);
    }
    budget
        .start(Duration::ZERO, NEW, EMPTY)
        .unwrap()
        .finish(Duration::ZERO)
        .unwrap();
    assert!(matches!(
        budget.start(Duration::ZERO, NEW, ELIGIBLE),
        Err(ServiceStartRefusal::CleanupStartsReserved { .. })
    ));
    for _ in 0..3 {
        run(&mut budget, 0, 0, CLEANUP);
    }
    assert_eq!(budget.usage().starts, 32);
    assert_eq!(budget.usage().cleanup_starts, 3);
}

#[test]
fn cleanup_keeps_elapsed_allowance_and_consumes_it_once() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    run(&mut budget, 0, 1500, NEW);
    assert_eq!(
        budget.start(us(1500), NEW, ELIGIBLE).unwrap_err(),
        ServiceStartRefusal::CleanupTimeReserved {
            retry_after: us(14500)
        }
    );
    run(&mut budget, 1500, 2000, CLEANUP);
    assert_eq!(budget.usage().cleanup_charged, us(500));
    assert_eq!(
        budget.start(us(2000), CLEANUP, ELIGIBLE).unwrap_err(),
        ServiceStartRefusal::TimeExhausted {
            retry_after: us(14000)
        }
    );
}

#[test]
fn cleanup_done_first_leaves_the_rest_for_new_work() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    for index in 0..4 {
        run(&mut budget, index * 125, (index + 1) * 125, CLEANUP);
    }
    run(&mut budget, 500, 1999, NEW);
    assert_eq!(budget.usage().charged, us(1999));
    run(&mut budget, 1999, 2000, NEW);
    assert!(matches!(
        budget.start(us(2000), NEW, ELIGIBLE),
        Err(ServiceStartRefusal::TimeExhausted { .. })
    ));
}

#[test]
fn donated_elapsed_reservation_is_not_reported_as_a_reservation_overrun() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    let charge = budget
        .start(Duration::ZERO, NEW, EMPTY)
        .unwrap()
        .finish(us(2000))
        .unwrap();
    assert_eq!(charge.cleanup_reservation_overrun, Duration::ZERO);
    assert_eq!(charge.allowance_overrun, Duration::ZERO);
}

#[test]
fn indivisible_operation_overruns_are_reported_not_hidden() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    let charge = budget
        .start(Duration::ZERO, NEW, ELIGIBLE)
        .unwrap()
        .finish(us(2500))
        .unwrap();
    assert_eq!(charge.elapsed, us(2500));
    assert_eq!(charge.interval_charge, us(2500));
    assert_eq!(charge.cleanup_reservation_overrun, us(1000));
    assert_eq!(charge.allowance_overrun, us(500));
    assert_eq!(charge.interval_boundary_overrun, Duration::ZERO);
    assert!(matches!(
        budget.start(us(2500), CLEANUP, ELIGIBLE),
        Err(ServiceStartRefusal::TimeExhausted { .. })
    ));
}

#[test]
fn interval_boundary_is_exact_and_idle_intervals_do_not_accumulate() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    for _ in 0..32 {
        budget
            .start(Duration::ZERO, NEW, EMPTY)
            .unwrap()
            .finish(Duration::ZERO)
            .unwrap();
    }
    assert_eq!(
        budget.start(us(15999), NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::StartsExhausted { retry_after: us(1) }
    );
    run(&mut budget, 16000, 16000, NEW);
    assert_eq!(budget.usage().starts, 1);
    run(&mut budget, 1600000, 1600000, NEW);
    assert_eq!(budget.usage().starts, 1);
    for _ in 1..32 {
        budget
            .start(us(1600000), NEW, EMPTY)
            .unwrap()
            .finish(us(1600000))
            .unwrap();
    }
    assert!(matches!(
        budget.start(us(1600000), CLEANUP, ELIGIBLE),
        Err(ServiceStartRefusal::StartsExhausted { .. })
    ));
}

#[test]
fn a_straddling_operation_is_accounted_before_the_next_window() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    let charge = budget
        .start(us(15900), NEW, ELIGIBLE)
        .unwrap()
        .finish(us(17900))
        .unwrap();
    assert_eq!(charge.elapsed, us(2000));
    assert_eq!(charge.interval_charge, us(2000));
    assert_eq!(charge.interval_boundary_overrun, us(1900));
    assert_eq!(budget.usage().charged, us(2000));
    run(&mut budget, 17900, 18000, NEW);
    assert_eq!(budget.usage().starts, 1);
    assert_eq!(budget.usage().charged, us(100));
}

#[test]
fn unwind_retains_the_start_and_clock_and_permanently_closes_the_budget() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _run = budget.start(us(1000), NEW, ELIGIBLE).unwrap();
        panic!("an effect interrupted its executor");
    }));
    assert!(panic.is_err());
    assert!(budget.is_interrupted());
    assert_eq!(budget.usage().starts, 1);
    assert_eq!(budget.usage().charged, Duration::ZERO);
    assert_eq!(
        budget.start(us(32000), NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::Interrupted
    );
    let charge = budget.account_interrupted(us(32000)).unwrap();
    assert_eq!(charge.elapsed, us(31000));
    assert_eq!(charge.interval_boundary_overrun, us(16000));
    assert_eq!(budget.usage().starts, 1);
    assert_eq!(budget.usage().charged, us(31000));
    assert_eq!(
        budget.account_interrupted(us(32000)),
        Err(ServiceAccountingError::NothingUnaccounted)
    );
    assert_eq!(
        budget.start(us(64000), NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::Interrupted
    );
}

#[test]
fn a_forgotten_guard_cannot_be_overwritten_by_a_new_interval() {
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    std::mem::forget(budget.start(us(1000), NEW, EMPTY).unwrap());
    assert_eq!(
        budget.start(us(32000), CLEANUP, EMPTY).unwrap_err(),
        ServiceStartRefusal::Interrupted
    );
    assert_eq!(budget.usage().starts, 1);
    assert_eq!(
        budget.account_interrupted(us(32000)).unwrap().elapsed,
        us(31000)
    );
}

#[test]
fn a_failed_finish_keeps_the_original_start_for_failure_accounting() {
    let mut budget = ServiceBudget::planned(us(100));
    assert_eq!(
        budget.start(us(200), NEW, EMPTY).unwrap().finish(us(199)),
        Err(ServiceAccountingError::ClockRegressed)
    );
    assert!(budget.is_interrupted());
    assert_eq!(budget.account_interrupted(us(250)).unwrap().elapsed, us(50));
    assert_eq!(budget.usage().charged, us(50));
}

#[test]
fn a_backwards_start_clock_does_not_replenish_the_allowance() {
    let mut budget = ServiceBudget::planned(us(100));
    run(&mut budget, 200, 300, NEW);
    assert_eq!(
        budget.start(us(299), NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::ClockRegressed
    );
    assert_eq!(budget.usage().starts, 1);
    assert_eq!(budget.usage().charged, us(100));
    run(&mut budget, 300, 400, NEW);
    assert_eq!(budget.usage().charged, us(200));
}

#[test]
fn the_clock_end_neither_wraps_nor_creates_a_fresh_interval() {
    let end = Duration::MAX;
    let begin = end - us(1000);
    let mut budget = ServiceBudget::planned(begin);
    let charge = budget
        .start(begin, NEW, EMPTY)
        .unwrap()
        .finish(end)
        .unwrap();
    assert_eq!(charge.elapsed, us(1000));
    for _ in 1..32 {
        budget.start(end, NEW, EMPTY).unwrap().finish(end).unwrap();
    }
    assert_eq!(
        budget.start(end, NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::StartsExhausted {
            retry_after: us(15000)
        }
    );
    assert_eq!(
        budget.start(Duration::ZERO, NEW, EMPTY).unwrap_err(),
        ServiceStartRefusal::ClockRegressed
    );
}

#[test]
fn fair_cursors_can_progress_every_grant_without_spending_cleanup_reservation() {
    // The runner owns both cursors; this workload checks that the fixed start
    // and time allowances can sustain their progress with cleanup saturated.
    let mut budget = ServiceBudget::planned(Duration::ZERO);
    let mut sources = [0_u32; 16];
    let mut debts = [0_u32; 16];
    let mut source_cursor = 0;
    let mut debt_cursor = 0;
    for interval in 0..8 {
        let mut now = interval * 16000;
        for _ in 0..4 {
            run(&mut budget, now, now + 100, CLEANUP);
            now += 100;
            debts[debt_cursor] += 1;
            debt_cursor = (debt_cursor + 1) % debts.len();
        }
        for _ in 0..28 {
            run(&mut budget, now, now + 50, NEW);
            now += 50;
            sources[source_cursor] += 1;
            source_cursor = (source_cursor + 1) % sources.len();
        }
        assert_eq!(budget.usage().starts, 32);
        assert_eq!(budget.usage().charged, us(1800));
    }
    assert_eq!(sources, [14; 16]);
    assert_eq!(debts, [2; 16]);
}

#[test]
fn watchdog_starts_at_dequeue_and_covers_lock_acquisition() {
    let mut watchdog = ExecutionWatchdog::dequeued(us(1_000_000));
    assert_eq!(
        watchdog.observe(us(1_249_999)),
        Ok(WatchdogObservation::Running {
            phase: ExecutionPhase::BeforeGuards,
            remaining: us(1)
        })
    );
    assert_eq!(
        watchdog.observe(us(1_250_000)),
        Ok(WatchdogObservation::Expired {
            phase: ExecutionPhase::BeforeGuards,
            overdue: Duration::ZERO
        })
    );
}

#[test]
fn watchdog_phases_never_restart_the_deadline() {
    let mut watchdog = ExecutionWatchdog::dequeued(Duration::ZERO);
    assert_eq!(watchdog.committed(), Err(WatchdogError::InvalidTransition));
    watchdog.applying().unwrap();
    assert_eq!(watchdog.applying(), Err(WatchdogError::InvalidTransition));
    assert_eq!(
        watchdog.observe(us(250_001)),
        Ok(WatchdogObservation::Expired {
            phase: ExecutionPhase::Applying,
            overdue: us(1)
        })
    );
    watchdog.committed().unwrap();
    assert_eq!(
        watchdog.observe(us(251_000)),
        Ok(WatchdogObservation::Expired {
            phase: ExecutionPhase::Committed,
            overdue: us(1000)
        })
    );
    assert_eq!(
        watchdog.finish(us(251_000)),
        Ok(WatchdogObservation::Finished {
            phase: ExecutionPhase::Committed,
            elapsed: us(251_000),
            exceeded_deadline: true
        })
    );
    assert_eq!(watchdog.committed(), Err(WatchdogError::InvalidTransition));
    assert_eq!(
        watchdog.finish(us(252_000)),
        Err(WatchdogError::InvalidTransition)
    );
}

#[test]
fn a_returned_pre_effect_refusal_is_finished_without_inventing_a_commit() {
    let mut watchdog = ExecutionWatchdog::dequeued(us(100));
    let outcome = WatchdogObservation::Finished {
        phase: ExecutionPhase::BeforeGuards,
        elapsed: us(50),
        exceeded_deadline: false,
    };
    assert_eq!(watchdog.finish(us(150)), Ok(outcome));
    assert_eq!(watchdog.observe(us(1_000_000)), Ok(outcome));
    assert_eq!(watchdog.applying(), Err(WatchdogError::InvalidTransition));
}

#[test]
fn watchdog_clock_end_does_not_wrap_the_deadline() {
    let begin = Duration::MAX - us(250_000);
    let mut watchdog = ExecutionWatchdog::dequeued(begin);
    assert_eq!(
        watchdog.observe(Duration::MAX),
        Ok(WatchdogObservation::Expired {
            phase: ExecutionPhase::BeforeGuards,
            overdue: Duration::ZERO
        })
    );
    assert_eq!(
        watchdog.observe(Duration::ZERO),
        Err(WatchdogError::ClockRegressed)
    );
}
