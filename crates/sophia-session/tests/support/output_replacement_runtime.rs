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
fn unavailable_outputs_keep_a_slow_probe_after_the_short_series_without_a_new_notice() {
    let mut elapsed = Duration::ZERO;
    let mut probe_times = Vec::new();
    for attempt in 0..8 {
        elapsed += runtime_output_waiting_delay(attempt)
            .expect("an unavailable output still owes a probe");
        probe_times.push(elapsed);
    }
    assert_eq!(
        probe_times,
        [250, 1250, 5250, 10250, 15250, 20250, 25250, 30250].map(Duration::from_millis)
    );
    // A cable returned long after the last short retry must still get a
    // discovery call, even when the sleeping device emits no hotplug notice.
    let connected_at = Duration::from_secs(19);
    let detected_at = probe_times
        .into_iter()
        .find(|time| *time >= connected_at)
        .unwrap();
    assert_eq!(detected_at, Duration::from_millis(20250));
    assert_eq!(
        runtime_output_waiting_delay(usize::MAX),
        Some(Duration::from_secs(5))
    );
    assert_eq!(runtime_output_retry_delay(usize::MAX), None);
    let phase = include_str!("../../src/live_session/owner_loop/topology_phase.rs");
    assert!(phase.contains("output_replacement::runtime_output_retry_after_observation("));
    assert!(phase.contains("seat_state == sophia_backend_live::LiveSeatState::Active"));
    assert!(phase.contains("&& session_quiescence.is_none()"));
    assert!(phase.contains("if !probe_allowed {"));
    let cancelled = phase.split("if !probe_allowed {").nth(1).unwrap();
    assert!(
        cancelled.find("output_topology_retry_at = None;").unwrap()
            < cancelled.find("let retry_due").unwrap()
    );
}

#[test]
fn a_return_after_long_wait_keeps_one_finite_activation_allowance() {
    let profile = super::super::tests::profile();
    let mut recovery = OutputRecovery::Desired;
    let mut waiting = true;
    let mut attempts = 100;
    runtime_output_observe_availability(Some(true), &mut waiting, &mut attempts);
    assert!(!waiting);
    assert_eq!(attempts, 0);
    for millis in [250, 1000, 4000] {
        // Every successful preflight may still fail during resume. Seeing an
        // available output again must not reset that failure count.
        runtime_output_observe_availability(Some(true), &mut waiting, &mut attempts);
        assert_eq!(
            runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts),
            Some(Duration::from_millis(millis))
        );
    }
    runtime_output_observe_availability(Some(true), &mut waiting, &mut attempts);
    assert_eq!(
        runtime_output_retry_after_failure(&mut recovery, &profile, &mut attempts),
        None
    );
    assert_eq!(recovery, OutputRecovery::Exhausted);
    assert_eq!(attempts, 4);
    // A genuinely absent output starts the waiting series at its first step.
    runtime_output_observe_availability(Some(false), &mut waiting, &mut attempts);
    assert!(waiting);
    assert_eq!(attempts, 0);
    assert_eq!(
        runtime_output_waiting_delay(attempts),
        Some(Duration::from_millis(250))
    );
}

#[test]
fn failed_probes_keep_a_slow_retry_without_spending_or_replenishing_activation() {
    let profile = super::super::tests::profile();
    let mut recovery = OutputRecovery::Desired;
    let mut waiting = true;
    let mut failures = 0;
    let mut waiting_attempts = 0;
    for index in 0..10 {
        let stage = if index % 2 == 0 { "probe" } else { "seat" };
        let error = RuntimeOutputRefusal::new(stage, &io::Error::from_raw_os_error(16));
        assert_eq!(error.observed_availability(), None);
        runtime_output_observe_availability(
            error.observed_availability(),
            &mut waiting,
            &mut failures,
        );
        let (attempt, retry, report) = runtime_output_retry_after_observation(
            error.observed_availability(),
            &mut recovery,
            &profile,
            &mut failures,
            &mut waiting_attempts,
        );
        assert_eq!(attempt, index);
        assert_eq!(retry, runtime_output_waiting_delay(index));
        assert_eq!(report, index < 4);
        assert!(waiting);
        // A successful probe that still finds no monitor must not erase a
        // preceding probe failure; the long Waiting count is independent.
        runtime_output_observe_availability(Some(false), &mut waiting, &mut failures);
        assert_eq!(failures, 0);
        assert_eq!(recovery, OutputRecovery::Desired);
    }
    assert_eq!(waiting_attempts, 10);
    let error = RuntimeOutputRefusal::new("construction", &io::Error::from_raw_os_error(22));
    assert_eq!(error.observed_availability(), Some(true));
    runtime_output_observe_availability(Some(true), &mut waiting, &mut failures);
    for index in 0..4 {
        let (attempt, retry, report) = runtime_output_retry_after_observation(
            error.observed_availability(),
            &mut recovery,
            &profile,
            &mut failures,
            &mut waiting_attempts,
        );
        assert_eq!(attempt, index);
        assert_eq!(retry, runtime_output_retry_delay(index));
        assert!(report);
        assert_eq!(waiting_attempts, 0);
    }
    assert_eq!(recovery, OutputRecovery::Exhausted);
}

#[test]
fn long_waiting_records_the_short_series_and_one_slow_probe() {
    assert_eq!(
        (0..1000)
            .filter(|attempt| runtime_output_report_waiting(*attempt))
            .collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert!(!runtime_output_report_waiting(usize::MAX));
}

#[test]
fn complete_validation_errno_survives_the_runtime_refusal_boundary() {
    for (validation, errno) in [
        ("busy", 11),
        ("busy", 16),
        ("rejected", 22),
        ("rejected", 1),
        ("unbuildable", 0),
    ] {
        let refusal = RuntimeOutputRefusal::validation(validation, errno);
        assert_eq!(refusal.stage, "validation");
        assert_eq!(refusal.validation, validation);
        assert_eq!(refusal.errno, errno as u32);
    }
    assert_eq!(RuntimeOutputRefusal::validation("unresolved", -1).errno, 0);
}

fn strict_profile() -> DesktopOutputCandidate {
    let mut profile = super::super::tests::profile();
    profile.availability = sophia_config::DesktopOutputAvailability::Strict;
    profile.fallback_policy_key = None;
    profile.inherit_sophia = true;
    profile
}

#[test]
fn strict_runtime_waits_for_a_return_without_relaxing_settings_or_startup() {
    let mut profile = strict_profile();
    let mut absent = super::super::tests::probe("DP-1");
    absent.connected = false;
    absent.usable = false;
    absent.modes.clear();
    for probes in [Vec::new(), vec![absent]] {
        assert!(matches!(
            resolve_runtime_probe_policy(&probes, &profile, None).unwrap(),
            DesktopOutputResolution::Waiting { .. }
        ));
        assert!(
            resolve_probe_policy(&probes, &profile, None).is_err(),
            "startup/reload policy stays strict"
        );
    }
    let returned = super::super::tests::probe("DP-1");
    let DesktopOutputResolution::Active(resolved) =
        resolve_runtime_probe_policy(std::slice::from_ref(&returned), &profile, None).unwrap()
    else {
        panic!("returned monitor must resolve")
    };
    assert_eq!(resolved.outputs.len(), 1);
    assert_eq!(resolved.outputs[0].mode, timing(returned.modes[0]));
    // Candidate errors remain errors even with no monitor attached.
    profile.fallback_policy_key = Some(1);
    assert!(resolve_runtime_probe_policy(&[], &profile, None).is_err());
}

#[test]
fn strict_missing_named_connector_waits_but_an_unsupported_mode_refuses() {
    let mut profile = strict_profile();
    profile
        .named
        .push(sophia_config::DesktopNamedOutputCandidate {
            connector: "DP-1".into(),
            policy_key: Some(1),
            enabled: Some(true),
            mode: Some(sophia_config::DesktopOutputMode::Exact {
                width: 1920,
                height: 1080,
                refresh_millihz: 60_000,
            }),
            scale: None,
            position: None,
            transform: None,
            focus_at_startup: None,
            vrr: None,
            mirror_fit: None,
            mirror: Vec::new(),
        });
    assert!(matches!(
        resolve_runtime_probe_policy(&[super::super::tests::probe("DP-2")], &profile, None)
            .unwrap(),
        DesktopOutputResolution::Waiting { .. }
    ));
    let probe = super::super::tests::probe("DP-1");
    assert!(matches!(
        resolve_runtime_probe_policy(std::slice::from_ref(&probe), &profile, None).unwrap(),
        DesktopOutputResolution::Active(_)
    ));
    profile.named[0].mode = Some(sophia_config::DesktopOutputMode::Exact {
        width: 2560,
        height: 1440,
        refresh_millihz: 120_000,
    });
    let error = resolve_runtime_probe_policy(&[probe], &profile, None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<sophia_config::DesktopOutputReconcileError>(),
        Some(sophia_config::DesktopOutputReconcileError::ModeUnavailable(
            _
        ))
    ));
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
