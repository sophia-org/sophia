use sophia_session::diagnostics::{
    SessionCompletionFailure, SessionFailurePhase, reduced_record, session_failure_record,
};
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
