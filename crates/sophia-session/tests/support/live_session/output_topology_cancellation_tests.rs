//! Exercise the cancellation function called by wm_phase with supplied native
//! results. These prove dispatch order and retained recovery state, not KMS
//! effects or restoration of physical outputs.
use super::super::{
    LiveOutputTopologyExecutionPhase as Execution, NativeOutputCancellationRequest as Request,
    cancel_output_topology_execution,
};
use sophia_backend_live::LiveProductionNativeTopologyPreparationPhase as Native;
use std::cell::RefCell;

#[test]
fn cancellation_before_native_work_and_during_rollback_adds_no_effect() {
    for mut phase in [Execution::WaitingForQuiescence, Execution::RollingBack] {
        let original = phase;
        cancel_output_topology_execution(
            &mut phase,
            |_| panic!("no native request is owed in {original:?}"),
            || panic!("no additional policy rejection is owed in {original:?}"),
        )
        .unwrap();
        assert_eq!(phase, original);
    }
}

#[test]
fn preparation_abort_defers_policy_settlement_until_resource_cleanup() {
    let mut phase = Execution::Preparing;
    cancel_output_topology_execution(
        &mut phase,
        |request| {
            assert_eq!(request, Request::AbortPreparation);
            Ok(Some(Native::Aborting))
        },
        || panic!("preparation resources have not drained"),
    )
    .unwrap();
    assert_eq!(phase, Execution::Preparing);
}

#[test]
fn cancelled_apply_without_card_changes_returns_to_preparation_cleanup() {
    let mut phase = Execution::Applying;
    cancel_output_topology_execution(
        &mut phase,
        |request| {
            assert_eq!(request, Request::AbortPreparation);
            Ok(Some(Native::Failed))
        },
        || panic!("native resources still require preparation cleanup"),
    )
    .unwrap();
    assert_eq!(phase, Execution::Preparing);
}

#[test]
fn rollback_is_requested_before_policy_observes_cancellation() {
    for (original, expected_request) in [
        (Execution::Applying, Request::AbortPreparation),
        (Execution::AwaitingFirstPresentation, Request::Rollback),
        (Execution::Reconciling, Request::Rollback),
    ] {
        let mut phase = original;
        let effects = RefCell::new(Vec::new());
        cancel_output_topology_execution(
            &mut phase,
            |request| {
                assert_eq!(request, expected_request);
                effects.borrow_mut().push("native rollback accepted");
                Ok(Some(Native::RollingBack))
            },
            || {
                assert_eq!(*effects.borrow(), ["native rollback accepted"]);
                effects.borrow_mut().push("policy rejected");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(phase, Execution::RollingBack, "from {original:?}");
        assert_eq!(
            effects.into_inner(),
            ["native rollback accepted", "policy rejected"]
        );
        cancel_output_topology_execution(
            &mut phase,
            |_| panic!("an accepted rollback is not requested twice"),
            || panic!("policy is not rejected twice"),
        )
        .unwrap();
    }
}

#[test]
fn native_refusal_never_rejects_policy_or_advances_execution() {
    for original in [
        Execution::Preparing,
        Execution::Applying,
        Execution::AwaitingFirstPresentation,
        Execution::Reconciling,
    ] {
        let mut phase = original;
        let error = cancel_output_topology_execution(
            &mut phase,
            |_| Err("native cancellation refused".into()),
            || panic!("native cancellation was not accepted"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "native cancellation refused");
        assert_eq!(phase, original);
    }
}

#[test]
fn policy_error_retains_the_accepted_physical_rollback() {
    for original in [
        Execution::Applying,
        Execution::AwaitingFirstPresentation,
        Execution::Reconciling,
    ] {
        let mut phase = original;
        let error = cancel_output_topology_execution(
            &mut phase,
            |_| Ok(Some(Native::RollingBack)),
            || Err("policy observation failed".into()),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "policy observation failed");
        assert_eq!(phase, Execution::RollingBack, "from {original:?}");
    }
}

#[test]
fn unexpected_native_apply_abort_reports_never_settle_policy() {
    for report in [
        None,
        Some(Native::PreparingCandidate),
        Some(Native::PreparingRollback),
        Some(Native::Prepared),
        Some(Native::Applying),
        Some(Native::Applied),
        Some(Native::CandidateInstalled),
        Some(Native::FirstFramesQueued),
        Some(Native::RolledBack),
        Some(Native::Aborting),
    ] {
        let mut phase = Execution::Applying;
        let error = cancel_output_topology_execution(
            &mut phase,
            |_| Ok(report),
            || panic!("unexpected native report {report:?} cannot settle policy"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid native phase"));
        assert_eq!(phase, Execution::Applying);
    }
}
