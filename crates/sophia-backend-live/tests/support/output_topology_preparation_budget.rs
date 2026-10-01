//! Preparation deadline and retry pacing for one native topology transaction.
//! Pure: no device, renderer or clock is touched; instants are supplied.
use super::*;
use LiveProductionNativeTopologyPreparationPhase as Phase;
use LiveProductionNativeTopologyPreparationTurn as Turn;
use std::time::{Duration, Instant};

const PREPARING: [Phase; 2] = [Phase::PreparingCandidate, Phase::PreparingRollback];
const UNPACED: [Phase; 8] = [
    Phase::Prepared,
    Phase::Applying,
    Phase::RollingBack,
    Phase::Applied,
    Phase::CandidateInstalled,
    Phase::FirstFramesQueued,
    Phase::RolledBack,
    Phase::Failed,
];

#[test]
fn candidate_and_rollback_share_one_monotonic_limit() {
    let start = Instant::now();
    let limit = LIVE_PRODUCTION_TOPOLOGY_PREPARATION_LIMIT;
    let mut budget = LiveProductionNativeTopologyPreparationBudget::new(start);
    assert_eq!(budget.turn(Phase::PreparingCandidate, start), Turn::Service);
    // Rollback preparation does not restart the clock.
    let late = start + limit - Duration::from_millis(1);
    assert_eq!(budget.turn(Phase::PreparingRollback, late), Turn::Service);
    for phase in PREPARING {
        let mut budget = budget;
        assert_eq!(
            budget.turn(phase, start + limit),
            Turn::Expired { elapsed: limit }
        );
        assert_eq!(
            budget.turn(phase, start + limit + Duration::from_secs(3)),
            Turn::Expired {
                elapsed: limit + Duration::from_secs(3)
            }
        );
    }
}

#[test]
fn retries_are_paced_by_skipping_not_sleeping() {
    let start = Instant::now();
    let interval = LIVE_PRODUCTION_TOPOLOGY_PREPARATION_SERVICE_INTERVAL;
    for phase in [
        Phase::PreparingCandidate,
        Phase::PreparingRollback,
        Phase::Aborting,
    ] {
        let mut budget = LiveProductionNativeTopologyPreparationBudget::new(start);
        assert_eq!(budget.turn(phase, start), Turn::Service, "{phase:?}");
        assert_eq!(budget.next_service(phase), Some(start + interval));
        for early in [
            start,
            start + interval / 2,
            start + interval - Duration::from_nanos(1),
        ] {
            assert_eq!(budget.turn(phase, early), Turn::Wait, "{phase:?}");
        }
        assert_eq!(
            budget.turn(phase, start + interval),
            Turn::Service,
            "{phase:?}"
        );
        assert_eq!(budget.next_service(phase), Some(start + interval * 2));
    }
}

#[test]
fn abort_drain_is_paced_but_never_expires() {
    // The drain must keep waiting for an export a worker still owns.
    let start = Instant::now();
    let mut budget = LiveProductionNativeTopologyPreparationBudget::new(start);
    let far = start + LIVE_PRODUCTION_TOPOLOGY_PREPARATION_LIMIT * 10;
    assert_eq!(budget.turn(Phase::Aborting, far), Turn::Service);
    assert_eq!(budget.turn(Phase::Aborting, far), Turn::Wait);
    assert_eq!(
        budget.next_service(Phase::Aborting),
        Some(far + LIVE_PRODUCTION_TOPOLOGY_PREPARATION_SERVICE_INTERVAL)
    );
}

#[test]
fn later_phases_are_unpaced_and_unbounded_here() {
    let start = Instant::now();
    let far = start + LIVE_PRODUCTION_TOPOLOGY_PREPARATION_LIMIT * 10;
    for phase in UNPACED {
        let mut budget = LiveProductionNativeTopologyPreparationBudget::new(start);
        for now in [start, start, far, far] {
            assert_eq!(budget.turn(phase, now), Turn::Service, "{phase:?}");
        }
        assert_eq!(budget.next_service(phase), None, "{phase:?}");
    }
}

#[test]
fn the_owner_loop_never_waits_past_the_deadline() {
    let start = Instant::now();
    let limit = Duration::from_micros(400);
    let mut budget = LiveProductionNativeTopologyPreparationBudget::with_limit(start, limit);
    assert_eq!(budget.turn(Phase::PreparingRollback, start), Turn::Service);
    // The pacing interval would land after the deadline; the deadline wins.
    assert_eq!(
        budget.next_service(Phase::PreparingRollback),
        Some(start + limit)
    );
    assert_eq!(
        budget.turn(Phase::PreparingRollback, start + limit),
        Turn::Expired { elapsed: limit }
    );
}

#[test]
fn an_unrepresentable_limit_expires_instead_of_waiting_forever() {
    let start = Instant::now();
    let mut budget =
        LiveProductionNativeTopologyPreparationBudget::with_limit(start, Duration::MAX);
    assert_eq!(
        budget.turn(Phase::PreparingCandidate, start),
        Turn::Expired {
            elapsed: Duration::ZERO
        }
    );
}
