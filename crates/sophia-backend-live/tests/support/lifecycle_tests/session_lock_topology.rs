//! Cover continuity through the production rebind/resume runtime transitions.
//! Device setup and completions are simulated; every cover proof uses the
//! actual lowered frames retired by the mirror fixture's native custody.
use super::*;

fn rebind(
    runtime: &mut LiveProductionVisualRuntime,
    target: &mut MirroredTarget,
    outputs: &[HeadlessOutput],
) {
    target.teardown();
    *target = MirroredTarget::new(outputs);
    let viewports = outputs
        .iter()
        .enumerate()
        .map(|(i, output)| (output.id, rect(i as i32 * 64, 0, 64, 32)))
        .collect::<Vec<_>>();
    runtime
        .rebind_applied_topology_on(target, outputs, &viewports)
        .unwrap();
}

fn retire_all(target: &mut MirroredTarget, outputs: &[HeadlessOutput]) {
    for output in outputs {
        target.flip(output.id, 0);
        target.flip(output.id, 1);
    }
}

fn assert_first_frames_covered(
    runtime: &LiveProductionVisualRuntime,
    scene: &LiveProductionCpuScene,
    target: &MirroredTarget,
    epoch: u64,
) {
    let frames = runtime
        .retained_output_head_composition_frames(scene, target)
        .unwrap();
    assert_eq!(frames.len(), target.outputs.len());
    for (output, heads) in frames {
        assert_eq!(heads.len(), 2);
        for head in heads {
            assert_eq!(
                sophia_engine::presented_session_lock(
                    output,
                    &[head.frame.output_damage_snapshot.as_ref()]
                ),
                sophia_engine::SessionLockEpoch::from_raw(epoch),
            );
        }
    }
}

#[test]
fn locked_output_loss_and_return_keep_the_cover_but_require_new_head_retirement() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(4)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    retire_all(&mut target, &outputs);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(4)
    );

    rebind(&mut runtime, &mut target, &outputs[..1]);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    assert_first_frames_covered(&runtime, &scene, &target, 4);
    queue_all(&runtime, &scene, &mut target, &outputs[..1]);
    retire_all(&mut target, &outputs[..1]);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(4)
    );

    rebind(&mut runtime, &mut target, &outputs);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    assert_first_frames_covered(&runtime, &scene, &target, 4);
    queue_all(&runtime, &scene, &mut target, &outputs);
    target.flip(outputs[0].id, 0);
    target.flip(outputs[0].id, 1);
    target.flip(outputs[1].id, 0);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        None,
        "the returned mirror still owes a head"
    );
    target.flip(outputs[1].id, 1);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(4)
    );
    target.teardown();
}

#[test]
fn loss_before_lock_coverage_is_complete_cannot_turn_prepared_frames_into_proof() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(5)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    target.flip(outputs[0].id, 0);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    // Device completion/withdrawal belongs to the target. The runtime still
    // executes the same guarded rebind called after a native topology commit.
    rebind(&mut runtime, &mut target, &outputs[..1]);
    assert_first_frames_covered(&runtime, &scene, &target, 5);
    queue_all(&runtime, &scene, &mut target, &outputs[..1]);
    target.flip(outputs[0].id, 0);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    target.flip(outputs[0].id, 1);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(5)
    );
    target.teardown();
}

#[test]
fn all_heads_absent_proves_nothing_and_resumed_first_frames_remain_covered() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(6)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    retire_all(&mut target, &outputs);
    target.teardown();
    target = MirroredTarget::new(&[]);
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    assert_eq!(runtime.session_lock(), Some(cover(6)));

    target = MirroredTarget::new(&outputs);
    let prepared =
        LiveProductionOutputRuntimeSet::new(&outputs, runtime.committed_surfaces(), None).unwrap();
    runtime
        .resume_prepared_outputs_on(&mut target, prepared, &scene)
        .unwrap();
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        None,
        "resume prepared frames are not retirement"
    );
    retire_all(&mut target, &outputs);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(6)
    );
    target.teardown();
}

#[test]
fn unlocked_rebind_and_resume_do_not_invent_a_lock_cover() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    rebind(&mut runtime, &mut target, &outputs);
    queue_all(&runtime, &scene, &mut target, &outputs);
    retire_all(&mut target, &outputs);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    assert!(draws_an_application(&output_list(&runtime, outputs[0].id)));
    target.teardown();
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    target = MirroredTarget::new(&outputs);
    let prepared =
        LiveProductionOutputRuntimeSet::new(&outputs, runtime.committed_surfaces(), None).unwrap();
    runtime
        .resume_prepared_outputs_on(&mut target, prepared, &scene)
        .unwrap();
    retire_all(&mut target, &outputs);
    assert_eq!(runtime.presented_session_lock_on(&target), None);
    assert!(runtime.session_lock().is_none());
    target.teardown();
}

#[test]
fn coverage_diagnostic_requires_current_epoch_distinct_heads_and_retired_frames() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    assert!(runtime.session_lock_coverage_on(&target).is_none());
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(4)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    assert!(runtime.session_lock_coverage_on(&target).is_none());
    retire_all(&mut target, &outputs);
    assert_eq!(
        runtime.session_lock_coverage_on(&target),
        Some(LiveSessionLockCoverage {
            epoch: SessionLockEpoch::from_raw(4).unwrap(),
            outputs: 2,
            heads: 4,
        })
    );
    let actual = target.heads[1].target.head;
    target.heads[1].target.head = target.heads[0].target.head;
    assert!(
        runtime.session_lock_coverage_on(&target).is_none(),
        "a duplicate is not another covered head"
    );
    target.heads[1].target.head = actual;
    let output = target.heads[1].target.output;
    target.heads[1].target.output = outputs[1].id;
    assert!(
        runtime.session_lock_coverage_on(&target).is_none(),
        "the head target must belong to the covered output"
    );
    target.heads[1].target.output = output;
    target.frame_service_available = false;
    assert!(
        runtime.session_lock_coverage_on(&target).is_none(),
        "retained frames cannot prove an unavailable frame service"
    );
    target.frame_service_available = true;
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(5)), &scene, None)
        .unwrap();
    assert!(
        runtime.session_lock_coverage_on(&target).is_none(),
        "the previous lock is still on screen"
    );
    queue_all(&runtime, &scene, &mut target, &outputs);
    target.flip(outputs[0].id, 0);
    target.flip(outputs[0].id, 1);
    target.flip(outputs[1].id, 0);
    assert!(runtime.session_lock_coverage_on(&target).is_none());
    target.flip(outputs[1].id, 1);
    assert_eq!(
        runtime.session_lock_coverage_on(&target).unwrap().epoch,
        SessionLockEpoch::from_raw(5).unwrap()
    );
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    assert!(
        runtime.session_lock_coverage_on(&target).is_none(),
        "revocation makes retained snapshots insufficient"
    );
    target.teardown();
}
