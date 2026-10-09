#![cfg(test)]

use super::*;

#[test]
fn a_runtime_rescan_series_is_the_startup_series_and_then_ends() {
    let series = (0..5).map(runtime_output_retry_delay).collect::<Vec<_>>();
    assert_eq!(
        series,
        [
            Some(Duration::from_millis(250)),
            Some(Duration::from_millis(1_000)),
            Some(Duration::from_millis(4_000)),
            None,
            None,
        ]
    );
}

#[test]
fn reconnect_notice_burst_has_one_bounded_window_before_destructive_rebuild() {
    let start = Instant::now();
    let mut deadline = runtime_output_notice_deadline(start, None);
    assert_eq!(deadline, start + Duration::from_millis(250));
    // Notices 3-6 in the physical incident arrived within 120 ms. None may
    // immediately tear down the replacement, or extend the first deadline.
    for elapsed in [40, 80, 120] {
        let now = start + Duration::from_millis(elapsed);
        deadline = runtime_output_notice_deadline(now, Some(deadline));
        assert!(now < deadline);
        assert_eq!(deadline, start + Duration::from_millis(250));
    }
    assert!(start + Duration::from_millis(250) >= deadline);
    // An old four-second retry cannot delay a new cable notice.
    assert_eq!(
        runtime_output_notice_deadline(start, Some(start + Duration::from_secs(4))),
        deadline
    );
    let phase = include_str!("../../src/live_session/owner_loop/topology_phase.rs");
    assert!(phase.contains("let rebuild_requested = retry_due"));
    assert!(!phase.contains("monitor_notice.is_some() || retry_due"));
}

#[test]
fn transient_runtime_refusals_leave_time_for_the_link_to_settle() {
    let profile = super::super::tests::profile();
    let mut recovery = OutputRecovery::Desired;
    let mut attempts = 0;
    let start = Instant::now();
    let mut now = start;
    for millis in [250, 1000, 4000] {
        let delay =
            runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts).unwrap();
        assert_eq!(delay, Duration::from_millis(millis));
        assert_eq!(recovery, OutputRecovery::Conservative);
        now += delay;
    }
    assert_eq!(now.duration_since(start), Duration::from_millis(5250));
    // The fourth refusal ends the series; idle passes cannot replenish it.
    for _ in 0..10 {
        assert_eq!(
            runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts),
            None
        );
        assert_eq!(recovery, OutputRecovery::Exhausted);
    }
    assert_eq!(attempts, 4);
    // Only a successfully resumed owner resets this counter. An Active
    // preflight followed by resume abandonment must still spend a retry.
    let phase = include_str!("../../src/live_session/owner_loop/topology_phase.rs");
    let active = phase
        .split("RuntimeOutputReplacement::Active(replacement, realization, policy_layout) =>")
        .nth(1)
        .unwrap();
    assert!(
        active.find("ResumeAttempt::Abandoned").unwrap()
            < active.find("output_topology_retry_attempts = 0").unwrap()
    );
}

#[test]
fn strict_runtime_retries_preserve_settings_and_still_end() {
    let mut profile = super::super::tests::profile();
    profile.availability = sophia_config::DesktopOutputAvailability::Strict;
    let mut recovery = OutputRecovery::Desired;
    let mut attempts = 0;
    for _ in 0..3 {
        assert!(
            runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts).is_some()
        );
        assert_eq!(recovery, OutputRecovery::Desired);
    }
    assert!(runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts).is_none());
    assert_eq!(recovery, OutputRecovery::Exhausted);
}

#[test]
fn refusal_preserves_only_bounded_diagnostic_identity() {
    let error =
        sophia_config::DesktopOutputReconcileError::UnknownConnector("private-connector".into());
    let refusal = RuntimeOutputRefusal::new("resolution", &error);
    assert_eq!(refusal.code, "output_profile_unknown_connector");
    assert_eq!(refusal.errno, 0);
    let refusal = RuntimeOutputRefusal::new("probe", &io::Error::from_raw_os_error(16));
    assert_eq!(refusal.errno, 16);
    assert_eq!(refusal.validation, "not_attempted");
}

#[test]
fn contextual_errors_keep_their_typed_source_and_cyclic_sources_are_bounded() {
    #[derive(Debug)]
    struct Context(Box<dyn Error>);
    impl std::fmt::Display for Context {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("private contextual message")
        }
    }
    impl Error for Context {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self.0.as_ref())
        }
    }
    let error = Context(Box::new(io::Error::from_raw_os_error(16)));
    assert_eq!(RuntimeOutputRefusal::new("resume", &error).errno, 16);
    let error = Context(Box::new(
        sophia_config::DesktopOutputReconcileError::ModeUnavailable("private".into()),
    ));
    assert_eq!(
        RuntimeOutputRefusal::new("resolution", &error).code,
        "output_profile_mode_unavailable"
    );

    #[derive(Debug)]
    struct Cycle(std::cell::Cell<usize>);
    impl std::fmt::Display for Cycle {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("cycle")
        }
    }
    impl Error for Cycle {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.0.set(self.0.get() + 1);
            Some(self)
        }
    }
    let cycle = Cycle(std::cell::Cell::new(0));
    assert_eq!(
        RuntimeOutputRefusal::new("resume", &cycle).code,
        "unclassified"
    );
    assert_eq!(cycle.0.get(), 16);
}
