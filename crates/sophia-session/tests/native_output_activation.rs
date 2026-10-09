#![cfg(feature = "native-session")]

use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopOutputCandidate, reconcile_desktop_output_candidate,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputId, Size};
use sophia_session::desktop_output_activation::{
    NativeOutputActivationDisposition, NativeOutputActivationEffect,
    NativeOutputActivationEffectExecutor, NativeOutputActivationFailure, NativeOutputActivationKey,
    NativeOutputActivationModel, NativeOutputActivationMsg, NativeOutputActivationPhase,
    NativeOutputActivationSettlement, NativeOutputEffectCompletion, NativeOutputRollbackSettlement,
    UnavailableNativeOutputExecutor, reduce_native_output_activation, run_native_output_activation,
};
use sophia_session::desktop_output_topology::{
    NativeOutputActivationPlan, prepare_native_output_activation_plan,
    project_native_output_topology,
};

fn plan(generation: u64, digest: u8) -> NativeOutputActivationPlan {
    let selected = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capability = LibdrmNativeOutputCapability::new(
        OutputId::from_raw(1),
        1,
        "DP-1",
        [selected],
        Some(selected),
        selected,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap();
    let capabilities = [capability];
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(generation),
        digest: ConfigDigest::new([digest; 32]),
        inherit_sophia: true,
        availability: sophia_config::DesktopOutputAvailability::Strict,
        fallback_policy_key: None,
        named: Vec::new(),
    };
    let reconciliation = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();
    prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap()
}

fn begin(
    plan: NativeOutputActivationPlan,
) -> sophia_session::desktop_output_activation::NativeOutputActivationUpdate {
    reduce_native_output_activation(
        &NativeOutputActivationModel::default(),
        NativeOutputActivationMsg::Begin(plan),
    )
}

#[test]
fn reducer_is_deterministic_and_does_not_replace_in_flight_work() {
    let model = NativeOutputActivationModel::default();
    let candidate = plan(2, 2);
    let first = reduce_native_output_activation(
        &model,
        NativeOutputActivationMsg::Begin(candidate.clone()),
    );
    let repeated =
        reduce_native_output_activation(&model, NativeOutputActivationMsg::Begin(candidate));
    assert_eq!(first, repeated);

    let competing =
        reduce_native_output_activation(&first.model, NativeOutputActivationMsg::Begin(plan(3, 9)));
    assert_eq!(
        competing.disposition,
        NativeOutputActivationDisposition::IgnoredBusy
    );
    assert_eq!(competing.model, first.model);
    assert!(competing.effect.is_none());
}

#[test]
fn activation_advances_only_through_matching_typed_completions() {
    let started = begin(plan(3, 3));
    assert_eq!(
        started.disposition,
        NativeOutputActivationDisposition::Started
    );
    assert_eq!(started.model.phase(), NativeOutputActivationPhase::Testing);
    let NativeOutputActivationEffect::Test { key, plan } = started.effect.unwrap() else {
        panic!("begin must emit a test effect");
    };
    assert_eq!(plan.generation(), key.generation());

    let tested = reduce_native_output_activation(
        &started.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    assert_eq!(tested.model.phase(), NativeOutputActivationPhase::Applying);
    assert!(matches!(
        tested.effect,
        Some(NativeOutputActivationEffect::Apply { key: effect_key, .. }) if effect_key == key
    ));

    let applied = reduce_native_output_activation(
        &tested.model,
        NativeOutputActivationMsg::ApplyCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    assert_eq!(
        applied.disposition,
        NativeOutputActivationDisposition::Activated
    );
    assert_eq!(
        applied.model.phase(),
        NativeOutputActivationPhase::Activated
    );
    assert_eq!(
        applied.model.settlement(),
        Some(NativeOutputActivationSettlement::Activated { key })
    );
    assert!(applied.model.pending_plan().is_none());
    assert!(applied.effect.is_none());
}

#[test]
fn apply_failure_requires_rollback_before_terminal_rejection() {
    let expected_plan = plan(4, 4);
    let started = begin(expected_plan.clone());
    let key = started.effect.as_ref().unwrap().key();
    let tested = reduce_native_output_activation(
        &started.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    let failed_apply = reduce_native_output_activation(
        &tested.model,
        NativeOutputActivationMsg::ApplyCompleted {
            key,
            completion: NativeOutputEffectCompletion::Failed(
                NativeOutputActivationFailure::TimedOut,
            ),
        },
    );

    assert_eq!(
        failed_apply.model.phase(),
        NativeOutputActivationPhase::RollingBack
    );
    let NativeOutputActivationEffect::Rollback {
        key: rollback_key,
        plan: rollback_plan,
    } = failed_apply.effect.unwrap()
    else {
        panic!("apply failure must emit rollback");
    };
    assert_eq!(rollback_key, key);
    assert_eq!(rollback_plan, expected_plan);
    assert!(failed_apply.model.settlement().is_none());

    let rolled_back = reduce_native_output_activation(
        &failed_apply.model,
        NativeOutputActivationMsg::RollbackCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    assert_eq!(
        rolled_back.model.phase(),
        NativeOutputActivationPhase::Rejected
    );
    assert_eq!(
        rolled_back.model.settlement(),
        Some(NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::TimedOut,
            rollback: NativeOutputRollbackSettlement::Succeeded,
        })
    );
    assert!(rolled_back.model.pending_plan().is_none());
}

#[test]
fn stale_duplicate_and_out_of_order_completions_cannot_advance_state() {
    let started = begin(plan(5, 5));
    let key = started.effect.as_ref().unwrap().key();
    let stale_key = begin(plan(6, 6)).effect.unwrap().key();

    for message in [
        NativeOutputActivationMsg::TestCompleted {
            key: stale_key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
        NativeOutputActivationMsg::ApplyCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
        NativeOutputActivationMsg::RollbackCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    ] {
        let ignored = reduce_native_output_activation(&started.model, message);
        assert_eq!(
            ignored.disposition,
            NativeOutputActivationDisposition::IgnoredStale
        );
        assert_eq!(ignored.model, started.model);
        assert!(ignored.effect.is_none());
    }

    let tested = reduce_native_output_activation(
        &started.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    let duplicate = reduce_native_output_activation(
        &tested.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    assert_eq!(
        duplicate.disposition,
        NativeOutputActivationDisposition::IgnoredStale
    );
    assert_eq!(duplicate.model, tested.model);
}

#[test]
fn test_rejection_discards_candidate_without_rollback() {
    let started = begin(plan(7, 7));
    let key = started.effect.as_ref().unwrap().key();
    let rejected = reduce_native_output_activation(
        &started.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Failed(
                NativeOutputActivationFailure::Rejected,
            ),
        },
    );

    assert_eq!(
        rejected.disposition,
        NativeOutputActivationDisposition::Rejected
    );
    assert_eq!(
        rejected.model.phase(),
        NativeOutputActivationPhase::Rejected
    );
    assert_eq!(
        rejected.model.settlement(),
        Some(NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::Rejected,
            rollback: NativeOutputRollbackSettlement::NotRequired,
        })
    );
    assert!(rejected.model.pending_plan().is_none());
    assert!(rejected.effect.is_none());

    let repeated = reduce_native_output_activation(
        &rejected.model,
        NativeOutputActivationMsg::Begin(plan(8, 8)),
    );
    assert_eq!(
        repeated.disposition,
        NativeOutputActivationDisposition::IgnoredTerminal
    );
    assert_eq!(repeated.model, rejected.model);
}

#[test]
fn rollback_failure_is_terminal_and_preserves_both_causes() {
    let started = begin(plan(9, 9));
    let key = started.effect.as_ref().unwrap().key();
    let tested = reduce_native_output_activation(
        &started.model,
        NativeOutputActivationMsg::TestCompleted {
            key,
            completion: NativeOutputEffectCompletion::Succeeded,
        },
    );
    let failed_apply = reduce_native_output_activation(
        &tested.model,
        NativeOutputActivationMsg::ApplyCompleted {
            key,
            completion: NativeOutputEffectCompletion::Failed(
                NativeOutputActivationFailure::Disconnected,
            ),
        },
    );
    let failed_rollback = reduce_native_output_activation(
        &failed_apply.model,
        NativeOutputActivationMsg::RollbackCompleted {
            key,
            completion: NativeOutputEffectCompletion::Failed(
                NativeOutputActivationFailure::Invalidated,
            ),
        },
    );

    assert_eq!(
        failed_rollback.disposition,
        NativeOutputActivationDisposition::RecoveryFailed
    );
    assert_eq!(
        failed_rollback.model.phase(),
        NativeOutputActivationPhase::RecoveryFailed
    );
    assert_eq!(
        failed_rollback.model.settlement(),
        Some(NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::Disconnected,
            rollback: NativeOutputRollbackSettlement::Failed(
                NativeOutputActivationFailure::Invalidated
            ),
        })
    );
}

/// Records what the driver actually asked for, so a test can assert that a
/// declined test phase never reaches apply. That is the property keeping startup
/// free of KMS mutation while the executor is absent.
#[derive(Default)]
struct RecordingExecutor {
    test: Vec<NativeOutputActivationKey>,
    apply: Vec<NativeOutputActivationKey>,
    rollback: Vec<NativeOutputActivationKey>,
    test_result: Option<NativeOutputEffectCompletion>,
    apply_result: Option<NativeOutputEffectCompletion>,
    rollback_result: Option<NativeOutputEffectCompletion>,
}

impl RecordingExecutor {
    fn succeeding() -> Self {
        Self {
            test_result: Some(NativeOutputEffectCompletion::Succeeded),
            apply_result: Some(NativeOutputEffectCompletion::Succeeded),
            rollback_result: Some(NativeOutputEffectCompletion::Succeeded),
            ..Self::default()
        }
    }
}

impl NativeOutputActivationEffectExecutor for RecordingExecutor {
    fn test(
        &mut self,
        key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        self.test.push(key);
        self.test_result
            .expect("test executed without a configured result")
    }

    fn apply(
        &mut self,
        key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        self.apply.push(key);
        self.apply_result
            .expect("apply executed without a configured result")
    }

    fn rollback(
        &mut self,
        key: NativeOutputActivationKey,
        _plan: &NativeOutputActivationPlan,
    ) -> NativeOutputEffectCompletion {
        self.rollback.push(key);
        self.rollback_result
            .expect("rollback executed without a configured result")
    }
}

#[test]
fn driver_carries_one_candidate_from_test_through_apply() {
    let candidate = plan(4, 7);
    let key = NativeOutputActivationKey::from(&candidate);
    let mut executor = RecordingExecutor::succeeding();

    let report = run_native_output_activation(candidate, &mut executor).unwrap();

    assert_eq!(
        report.settlement,
        NativeOutputActivationSettlement::Activated { key }
    );
    assert_eq!(report.model.phase(), NativeOutputActivationPhase::Activated);
    // Exactly one test then one apply, both for the same key, and no rollback.
    assert_eq!(executor.test, vec![key]);
    assert_eq!(executor.apply, vec![key]);
    assert!(executor.rollback.is_empty());
}

#[test]
fn declined_test_never_reaches_apply_or_rollback() {
    let candidate = plan(5, 9);
    let key = NativeOutputActivationKey::from(&candidate);
    let mut executor = RecordingExecutor {
        test_result: Some(NativeOutputEffectCompletion::Failed(
            NativeOutputActivationFailure::WouldBlock,
        )),
        ..RecordingExecutor::default()
    };

    let report = run_native_output_activation(candidate, &mut executor).unwrap();

    assert_eq!(
        report.settlement,
        NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::WouldBlock,
            rollback: NativeOutputRollbackSettlement::NotRequired,
        }
    );
    assert_eq!(executor.test, vec![key]);
    // The load-bearing assertion: nothing was applied, so nothing was mutated,
    // and no rollback was needed to get back to a coherent topology.
    assert!(executor.apply.is_empty());
    assert!(executor.rollback.is_empty());
}

#[test]
fn failed_apply_drives_rollback_before_settling() {
    let candidate = plan(6, 11);
    let key = NativeOutputActivationKey::from(&candidate);
    let mut executor = RecordingExecutor {
        test_result: Some(NativeOutputEffectCompletion::Succeeded),
        apply_result: Some(NativeOutputEffectCompletion::Failed(
            NativeOutputActivationFailure::Rejected,
        )),
        rollback_result: Some(NativeOutputEffectCompletion::Succeeded),
        ..RecordingExecutor::default()
    };

    let report = run_native_output_activation(candidate, &mut executor).unwrap();

    assert_eq!(
        report.settlement,
        NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::Rejected,
            rollback: NativeOutputRollbackSettlement::Succeeded,
        }
    );
    assert_eq!(executor.rollback, vec![key]);
}

#[test]
fn failed_rollback_settles_as_recovery_failed() {
    let candidate = plan(7, 13);
    let mut executor = RecordingExecutor {
        test_result: Some(NativeOutputEffectCompletion::Succeeded),
        apply_result: Some(NativeOutputEffectCompletion::Failed(
            NativeOutputActivationFailure::Disconnected,
        )),
        rollback_result: Some(NativeOutputEffectCompletion::Failed(
            NativeOutputActivationFailure::Invalidated,
        )),
        ..RecordingExecutor::default()
    };

    let report = run_native_output_activation(candidate, &mut executor).unwrap();

    assert_eq!(
        report.model.phase(),
        NativeOutputActivationPhase::RecoveryFailed
    );
}

#[test]
fn the_absent_executor_declines_without_mutating_anything() {
    let candidate = plan(8, 15);
    let key = NativeOutputActivationKey::from(&candidate);

    let report =
        run_native_output_activation(candidate, &mut UnavailableNativeOutputExecutor).unwrap();

    // This is what startup logs today: prepared, declined, no rollback required.
    assert_eq!(
        report.settlement,
        NativeOutputActivationSettlement::Rejected {
            key,
            cause: NativeOutputActivationFailure::WouldBlock,
            rollback: NativeOutputRollbackSettlement::NotRequired,
        }
    );
    assert_eq!(report.model.phase(), NativeOutputActivationPhase::Rejected);
}
