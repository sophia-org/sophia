use sophia_session::diagnostics::{
    SessionCompletionFailure, SessionFailurePhase, reduced_record, session_failure_record,
};

#[test]
fn unclocked_records_keep_the_failure_errno_and_only_scalar_counts() {
    let line = "sophia_present_unclocked schema=1 head=2 owner=3 incarnation=4 reason=sequence_unsupported errno=95 minimum_period_usec=16667";
    assert_eq!(reduced_record(line).as_deref(), Some(line));
    assert_eq!(
        reduced_record(&format!(
            "{line} payload=secret errno=private reason=private"
        ))
        .as_deref(),
        Some(line)
    );
    let counters =
        "sophia_present_clock_service schema=1 unclocked_bound=5 unclocked_notify_settled=2";
    assert_eq!(reduced_record(counters).as_deref(), Some(counters));
}

#[test]
fn timed_present_execution_retains_only_the_scalar_request_join() {
    let line = "sophia_x_present_execution schema=1 request_transaction=7 execution_transaction=19 client=2 accepted=1";
    assert_eq!(reduced_record(line).as_deref(), Some(line));
    assert_eq!(
        reduced_record(&format!(
            "{line} window=secret payload=secret request_transaction=secret"
        ))
        .as_deref(),
        Some(line)
    );
}
use sophia_session::{input_delivery::InputDeliveryError, session_control::SessionControlFailure};

#[test]
fn recovery_records_preserve_only_identity_timing_and_fixed_outcomes() {
    let record = reduced_record("sophia_live_session_input_recovery schema=1 status=revoked delivery=42 client=3 surface=11 generation=4 seat=1 control_epoch=7 age_msec=6000 release_barrier=true reason=delivery_deadline content=redacted text=password keycode=30 payload=secret arbitrary=123 client_title=secret").unwrap();
    assert!(record.contains("delivery=42 client=3 surface=11 generation=4"));
    assert!(record.contains("reason=delivery_deadline"));
    for forbidden in ["password", "keycode", "secret", "arbitrary", "title"] {
        assert!(!record.contains(forbidden));
    }
    let spoof = reduced_record(
        "sophia_live_session_input_recovery reason=password client=secret status=secret",
    )
    .unwrap();
    assert!(!spoof.contains("password"));
    assert!(!spoof.contains("secret"));
}

#[test]
fn typed_control_input_and_completion_causes_survive_recording() {
    let cases: Vec<(Box<dyn std::error::Error>, &str)> = vec![
        (
            Box::new(InputDeliveryError::ProofTimeout),
            "input_proof_timeout",
        ),
        (
            Box::new(SessionControlFailure::UnexpectedAcknowledgement),
            "control_unexpected_ack",
        ),
        (
            Box::new(SessionCompletionFailure::IncompleteLayoutRecovery),
            "completion_layout_recovery",
        ),
        (
            Box::new(SessionCompletionFailure::PendingWork(1 << 6)),
            "completion_pending_input",
        ),
        (
            Box::new(SessionCompletionFailure::PendingWork(1)),
            "completion_pending_layout",
        ),
    ];
    for (error, expected) in cases {
        let line = session_failure_record(SessionFailurePhase::Control, error.as_ref());
        let captured = reduced_record(&line).unwrap();
        assert!(
            captured.contains(&format!("failure_code={expected}")),
            "{captured}"
        );
        assert!(!captured.contains("unclassified"));
    }
}

#[test]
fn empty_active_output_focus_clear_has_a_distinct_sanitized_reason() {
    let captured = reduced_record("sophia_live_session_focus schema=1 status=cleared reason=active_output_empty output=2 surface=4 generation=1 transaction=99 title=secret").unwrap();
    assert!(captured.contains("reason=active_output_empty"));
    assert!(captured.contains("output=2"));
    assert!(!captured.contains("secret"));
}

#[test]
fn a_refused_control_keeps_its_kind_and_typed_outcome_in_the_archive() {
    let line = "sophia_live_session_control schema=1 status=control_refused kind=FocusSurface transaction=148 surface=6291460 generation=1 failure_code=control_rejected outcome=target_not_viewable";
    assert_eq!(reduced_record(line).as_deref(), Some(line));
    let spoof = reduced_record("sophia_live_session_control schema=1 status=secret kind=secret outcome=secret failure_code=secret title=secret").unwrap();
    assert!(!spoof.contains("secret"));
    assert_eq!(reduced_record("sophia_live_session_control schema=1 status=stale_target_retired kind=ConfigureSurface transaction=2 surface=3").unwrap(),
        "sophia_live_session_control schema=1 status=stale_target_retired kind=ConfigureSurface transaction=2 surface=3");
}

#[test]
fn aggregate_present_records_keep_numeric_counts_and_observation_times() {
    for line in [
        "sophia_present_evidence schema=1 mode=full",
        "sophia_present_evidence schema=1 mode=aggregate",
        "sophia_x_present_work schema=1 attempted_count=10000 emitted_count=128 coalesced_count=9872 accepted_count=1000 ready_count=2000 queued_count=2000 write_started_count=2500 written_count=2500",
        "sophia_live_present_work schema=1 retired_count=1000 attempted_count=2000 emitted_count=64 coalesced_count=1936",
        "sophia_x_present_delivery schema=1 client=1 transaction=6 sequence=1 window_token=4 subscription_token=5 pixmap_token=0 serial=8 kind=complete status=written observed_monotonic_usec=123456",
        "sophia_live_session_present schema=2 status=retired transaction=6 surface=8 clip=none unit_scale=true ust=123456 msc=89",
    ] {
        assert_eq!(reduced_record(line).as_deref(), Some(line));
    }
    assert_eq!(
        reduced_record("sophia_present_evidence schema=1 mode=secret title=private"),
        Some("sophia_present_evidence schema=1".to_owned())
    );
    for record in ["sophia_x_present_work", "sophia_live_present_work"] {
        let line = format!(
            "{record} schema=1 attempted_count=secret emitted_count=+2 coalesced_count=-1 title=private bytes=private"
        );
        let reduced = reduced_record(&line).unwrap();
        for forbidden in ["secret", "private", "+2", "-1"] {
            assert!(!reduced.contains(forbidden), "{reduced}");
        }
    }
}

#[test]
fn native_readiness_counters_survive_reduction_without_fd_identities() {
    let line = "sophia_present_clock_service schema=1 native_ready=8 native_ready_consumed=6 native_ready_idle=2 native_errors=1 native_event_waits=9 native_short_waits=3 native_service_waits=4";
    assert_eq!(reduced_record(line).as_deref(), Some(line));
    assert_eq!(
        reduced_record(&format!("{line} native_fd=37 path=/dev/secret")).as_deref(),
        Some(line)
    );
}
