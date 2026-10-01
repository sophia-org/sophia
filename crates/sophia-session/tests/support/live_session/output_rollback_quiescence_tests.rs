//! The rollback gate: the blocking reverse apply runs only after candidate
//! presentation ownership has settled. Pure; instants and owners are supplied.
use super::super::{
    OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_INTERVAL, OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_TIMEOUT,
    OutputTopologyRollbackQuiescence, OutputTopologyRollbackStep,
};
use std::time::{Duration, Instant};

/// Supplied owners: how many turns ownership stays busy, and what ran.
#[derive(Default)]
struct Owners {
    busy_turns: usize,
    quiesce_calls: usize,
    applies: usize,
    fail_quiesce: bool,
}

fn quiesce(owners: &mut Owners) -> Result<bool, Box<dyn std::error::Error>> {
    owners.quiesce_calls += 1;
    if owners.fail_quiesce {
        return Err("candidate drain refused".into());
    }
    if owners.busy_turns == 0 {
        return Ok(true);
    }
    owners.busy_turns -= 1;
    Ok(false)
}

fn apply(owners: &mut Owners) -> Result<&'static str, Box<dyn std::error::Error>> {
    owners.applies += 1;
    Ok("reverse_apply")
}

fn turn(
    wait: &mut OutputTopologyRollbackQuiescence,
    owners: &mut Owners,
    now: Instant,
) -> Result<OutputTopologyRollbackStep<&'static str>, Box<dyn std::error::Error>> {
    wait.turn(now, owners, quiesce, apply)
}

#[test]
fn settled_ownership_applies_on_the_first_turn() {
    let now = Instant::now();
    let mut wait = OutputTopologyRollbackQuiescence::new(now);
    let mut owners = Owners::default();
    assert_eq!(
        turn(&mut wait, &mut owners, now).unwrap(),
        OutputTopologyRollbackStep::Effect("reverse_apply")
    );
    assert_eq!((owners.quiesce_calls, owners.applies), (1, 1));
    assert_eq!(wait.next_wake(), None);
}

#[test]
fn the_reverse_apply_waits_until_ownership_settles_and_turns_are_paced() {
    let start = Instant::now();
    let interval = OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_INTERVAL;
    let mut wait = OutputTopologyRollbackQuiescence::new(start);
    let mut owners = Owners {
        busy_turns: 2,
        ..Owners::default()
    };
    assert_eq!(
        turn(&mut wait, &mut owners, start).unwrap(),
        OutputTopologyRollbackStep::Pending
    );
    assert_eq!(wait.next_wake(), Some(start + interval));
    // Not due yet: neither quiescence nor the apply is touched.
    for early in [start, start + interval / 2] {
        assert_eq!(
            turn(&mut wait, &mut owners, early).unwrap(),
            OutputTopologyRollbackStep::Pending
        );
    }
    assert_eq!((owners.quiesce_calls, owners.applies), (1, 0));
    assert_eq!(
        turn(&mut wait, &mut owners, start + interval).unwrap(),
        OutputTopologyRollbackStep::Pending
    );
    assert_eq!((owners.quiesce_calls, owners.applies), (2, 0));
    assert_eq!(
        turn(&mut wait, &mut owners, start + interval * 2).unwrap(),
        OutputTopologyRollbackStep::Effect("reverse_apply")
    );
    assert_eq!((owners.quiesce_calls, owners.applies), (3, 1));
}

#[test]
fn readiness_latches_for_later_rollback_turns() {
    // A multi-card rollback takes several apply turns; ownership is not
    // re-examined once it has settled, and nothing new can be submitted.
    let now = Instant::now();
    let mut wait = OutputTopologyRollbackQuiescence::new(now);
    let mut owners = Owners::default();
    for _ in 0..3 {
        turn(&mut wait, &mut owners, now).unwrap();
    }
    assert_eq!((owners.quiesce_calls, owners.applies), (1, 3));
}

#[test]
fn a_quiescence_error_never_reaches_the_reverse_apply() {
    let now = Instant::now();
    let mut wait = OutputTopologyRollbackQuiescence::new(now);
    let mut owners = Owners {
        fail_quiesce: true,
        ..Owners::default()
    };
    let error = turn(&mut wait, &mut owners, now).unwrap_err().to_string();
    assert!(error.contains("candidate drain refused"), "{error}");
    assert_eq!(owners.applies, 0);
    assert_eq!(wait.next_wake(), Some(now), "the wait is left as it was");
}

#[test]
fn the_deadline_fails_by_name_without_applying() {
    let start = Instant::now();
    let timeout = OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_TIMEOUT;
    let mut wait = OutputTopologyRollbackQuiescence::new(start);
    let mut owners = Owners {
        busy_turns: usize::MAX,
        ..Owners::default()
    };
    assert_eq!(
        turn(
            &mut wait,
            &mut owners,
            start + timeout - Duration::from_millis(1)
        )
        .unwrap(),
        OutputTopologyRollbackStep::Pending
    );
    // The next turn is never scheduled past the deadline.
    assert_eq!(wait.next_wake(), Some(start + timeout));
    let error = turn(&mut wait, &mut owners, start + timeout)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("rollback quiescence timed out after 2000 ms"),
        "{error}"
    );
    assert_eq!((owners.quiesce_calls, owners.applies), (1, 0));
}

#[test]
fn a_late_ready_report_cannot_admit_the_reverse_apply() {
    // Ownership would report settled, but only after the deadline: the
    // deadline is checked first and neither closure runs.
    let start = Instant::now();
    let timeout = OUTPUT_TOPOLOGY_ROLLBACK_QUIESCENCE_TIMEOUT;
    let mut wait = OutputTopologyRollbackQuiescence::new(start);
    let mut owners = Owners {
        busy_turns: 1,
        ..Owners::default()
    };
    assert_eq!(
        turn(&mut wait, &mut owners, start).unwrap(),
        OutputTopologyRollbackStep::Pending
    );
    let error = turn(&mut wait, &mut owners, start + timeout)
        .unwrap_err()
        .to_string();
    assert!(error.contains("rollback quiescence timed out"), "{error}");
    assert_eq!((owners.quiesce_calls, owners.applies), (1, 0));
}

#[test]
fn completion_reuses_its_existing_deadline() {
    let start = Instant::now();
    let deadline = start + Duration::from_millis(300);
    let mut wait = OutputTopologyRollbackQuiescence::until(start, deadline);
    let mut owners = Owners {
        busy_turns: usize::MAX,
        ..Owners::default()
    };
    turn(&mut wait, &mut owners, start).unwrap();
    assert!(turn(&mut wait, &mut owners, deadline).is_err());
    assert_eq!(owners.applies, 0);
}
