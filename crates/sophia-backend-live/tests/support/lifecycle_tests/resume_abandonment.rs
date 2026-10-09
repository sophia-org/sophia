//! A resume that fails partway (t306, t310): the runtime returns to suspension
//! without presentation or input authority, keeps its cover and retained
//! images, and the next owner proves the cover only by its own retirements.
use super::*;
use crate::{
    LiveProductionNativeResumeAbandonment as Abandonment, LiveProductionNativeSuspendError,
    LiveProductionNativeSuspendOutcome as Outcome, LiveProductionNativeSuspendReport,
};

fn retire_all(target: &mut MirroredTarget, outputs: &[HeadlessOutput]) {
    for output in outputs {
        target.flip(output.id, 0);
        target.flip(output.id, 1);
    }
}

fn epoch(raw: u64) -> Option<sophia_engine::SessionLockEpoch> {
    sophia_engine::SessionLockEpoch::from_raw(raw)
}

/// A locked desktop whose owner was revoked, as before a replacement resume.
fn suspended_locked_desktop() -> (
    [HeadlessOutput; 2],
    LiveProductionVisualRuntime,
    LiveProductionCpuScene,
) {
    let (outputs, mut runtime, scene, mut target) = desktop();
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(7)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    retire_all(&mut target, &outputs);
    assert_eq!(runtime.presented_session_lock_on(&target), epoch(7));
    target.teardown();
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    (outputs, runtime, scene)
}

#[test]
fn a_partial_resume_returns_to_suspension_and_the_next_owner_must_retire_the_cover() {
    let (outputs, mut runtime, scene) = suspended_locked_desktop();
    let retained = runtime.retained_renderer_image_ids();

    // The replacement presents its first output and refuses the second.
    let mut failed = MirroredTarget::new(&outputs);
    failed.refuse_queue_after = Some(1);
    let prepared = runtime.resumed_output_set(&outputs, None, None).unwrap();
    assert!(
        runtime
            .resume_prepared_outputs_on(&mut failed, prepared, &scene)
            .is_err()
    );
    assert_eq!(failed.queued_batches, 1, "one output was presented first");

    let abandonment = runtime
        .abandon_native_resume_with(&outputs, |runtime| {
            runtime.suspend_revoked_native_scanout(&outputs)
        })
        .unwrap();
    assert!(matches!(
        abandonment,
        Abandonment::Suspended(report) if report.outcome == Outcome::ForcedDetachRevoked
    ));
    assert_eq!(runtime.presented_session_lock_on(&failed), None);
    assert_eq!(runtime.session_lock(), Some(cover(7)));
    assert_eq!(runtime.retained_renderer_image_ids(), retained);
    // Suspended again: a second abandonment has nothing to detach.
    assert_eq!(
        runtime
            .abandon_native_resume_with(&outputs, |_| panic!("already suspended"))
            .unwrap(),
        Abandonment::BeforeInstall
    );
    failed.teardown();

    let mut next = MirroredTarget::new(&outputs);
    let prepared = runtime.resumed_output_set(&outputs, None, None).unwrap();
    runtime
        .resume_prepared_outputs_on(&mut next, prepared, &scene)
        .unwrap();
    assert_eq!(
        runtime.presented_session_lock_on(&next),
        None,
        "the failed owner's presentation is not the next owner's proof"
    );
    retire_all(&mut next, &outputs);
    assert_eq!(runtime.presented_session_lock_on(&next), epoch(7));
    next.teardown();
}

#[test]
fn a_failure_before_installation_leaves_the_suspended_runtime_alone() {
    let (outputs, mut runtime, _scene) = suspended_locked_desktop();
    let retained = runtime.retained_renderer_image_ids();
    assert_eq!(
        runtime
            .abandon_native_resume_with(&outputs, |_| panic!("nothing was installed"))
            .unwrap(),
        Abandonment::BeforeInstall
    );
    assert_eq!(runtime.session_lock(), Some(cover(7)));
    assert_eq!(runtime.retained_renderer_image_ids(), retained);
}

#[test]
fn a_drain_failure_whose_forced_detach_completed_is_not_detached_again() {
    let (outputs, mut runtime, _scene, mut target) = desktop();
    let abandonment = runtime
        .abandon_native_resume_with(&outputs, |runtime| {
            let detached = runtime.suspend_revoked_native_scanout(&outputs)?;
            Err(Box::new(LiveProductionNativeSuspendError {
                drain_error: "drain timed out".into(),
                detach_report: Some(LiveProductionNativeSuspendReport {
                    outcome: Outcome::ForcedDetachDrainError,
                    ..detached
                }),
                detach_error: None,
            }))
        })
        .unwrap();
    // A second, revoked detach would have reported ForcedDetachRevoked.
    assert!(matches!(
        abandonment,
        Abandonment::Suspended(report) if report.outcome == Outcome::ForcedDetachDrainError
    ));
    target.teardown();
}

#[test]
fn an_incomplete_detach_falls_back_to_forced_revocation() {
    for failure in [
        Box::new(LiveProductionNativeSuspendError {
            drain_error: "drain failed".into(),
            detach_report: None,
            detach_error: Some("detach failed".into()),
        }) as Box<dyn std::error::Error>,
        "not a suspension error".into(),
    ] {
        let (outputs, mut runtime, _scene, mut target) = desktop();
        let mut failure = Some(failure);
        let abandonment = runtime
            .abandon_native_resume_with(&outputs, |_| Err(failure.take().unwrap()))
            .unwrap();
        assert!(matches!(
            abandonment,
            Abandonment::Suspended(report) if report.outcome == Outcome::ForcedDetachRevoked
        ));
        assert_eq!(
            runtime
                .abandon_native_resume_with(&outputs, |_| panic!("already suspended"))
                .unwrap(),
            Abandonment::BeforeInstall
        );
        target.teardown();
    }
}
