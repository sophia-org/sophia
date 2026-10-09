//! t291: the session lock cover through the production runtime, the lowered
//! owners and the mirrored native target. Every output and every mirror head
//! draws the cover and nothing else, whatever else the runtime holds, and the
//! session is proven locked only once every head has retired the cover.
use super::presentation_instances::{
    commit_cpu_surface, output_list, presentation_output, published, rect, region, shown_instance,
};
use super::*;

#[path = "resume_abandonment.rs"]
mod resume_abandonment;
#[path = "resume_viewports.rs"]
mod resume_viewports;
#[path = "session_lock_topology.rs"]
mod topology;

const FILL: sophia_engine::CompositorRgb8 = sophia_engine::CompositorRgb8 {
    red: 0x10,
    green: 0x20,
    blue: 0x30,
};

fn cover(epoch: u64) -> sophia_engine::SessionLockCover {
    sophia_engine::SessionLockCover::fill(
        sophia_engine::SessionLockEpoch::from_raw(epoch).unwrap(),
        FILL,
    )
}

/// A desktop with an application on each output and a WM presentation that
/// overlays a preview of one of them: everything a lock must hide.
fn desktop() -> (
    [HeadlessOutput; 2],
    LiveProductionVisualRuntime,
    LiveProductionCpuScene,
    MirroredTarget,
) {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let target = MirroredTarget::new(&outputs);
    let left = SurfaceId::new(5, 1);
    let right = SurfaceId::new(6, 1);
    commit_cpu_surface(&mut runtime, &mut scene, left, 55, 1, rect(0, 0, 64, 32));
    commit_cpu_surface(&mut runtime, &mut scene, right, 66, 1, rect(64, 0, 64, 32));
    let output = outputs[0].id;
    runtime.presentation_order = vec![left, right];
    runtime.surface_outputs.insert(left, output);
    runtime.surface_outputs.insert(right, outputs[1].id);
    runtime.chrome_surfaces = vec![left, right];
    runtime
        .set_policy_presentation(
            Some(published(
                1,
                vec![presentation_output(output, PolicyPresentationMode::Overlay)],
                vec![shown_instance(output, 2, 1, left, rect(4, 4, 8, 8))],
                vec![region(
                    output,
                    1,
                    0,
                    PolicyPresentationRegionRole::Backdrop,
                    rect(0, 0, 64, 32),
                    rect(0, 0, 64, 32),
                )],
            )),
            &scene,
            None,
        )
        .unwrap();
    (outputs, runtime, scene, target)
}

fn draws_an_application(list: &CompositorDisplayList) -> bool {
    list.commands
        .iter()
        .any(|command| matches!(command, CompositorDisplayCommand::Surface { .. }))
}

fn only_the_cover(list: &CompositorDisplayList, output: OutputId, epoch: u64) -> bool {
    matches!(
        list.commands.as_slice(),
        [CompositorDisplayCommand::Rect(rect)]
            if rect.node == CompositorNodeId::SessionLock { output, epoch }
                && rect.color == FILL
                && rect.opacity == u8::MAX
    )
}

/// Queues one retained frame for every output and installs it on both heads.
fn queue_all(
    runtime: &LiveProductionVisualRuntime,
    scene: &LiveProductionCpuScene,
    target: &mut MirroredTarget,
    outputs: &[HeadlessOutput],
) {
    let frames = runtime
        .retained_output_head_composition_frames(scene, &*target)
        .unwrap();
    target
        .queue_retained_batch(frames, &BTreeSet::new())
        .unwrap();
    for output in outputs {
        target.install(output.id).unwrap();
        target.prepare(output.id);
    }
}

#[test]
fn every_list_of_a_locked_runtime_is_the_cover_alone() {
    let (outputs, mut runtime, scene, _target) = desktop();
    for output in &outputs {
        let list = output_list(&runtime, output.id);
        assert!(draws_an_application(&list), "unlocked {:?}", output.id);
    }
    assert!(
        output_list(&runtime, outputs[0].id)
            .presentation_stamp()
            .is_some(),
        "the WM tier is drawn while unlocked"
    );
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(3)), &scene, None)
        .unwrap();
    for output in &outputs {
        let committed = runtime.committed_surfaces().to_vec();
        assert!(
            only_the_cover(&output_list(&runtime, output.id), output.id, 3),
            "snapshot list on {:?}",
            output.id
        );
        let viewport = runtime.outputs.logical_viewport(output.id).unwrap();
        let retained = runtime
            .display_list_for_output(output.id, viewport, &committed, &runtime.presentation_order)
            .unwrap();
        assert!(only_the_cover(&retained, output.id, 3), "retained list");
        let recovery = runtime
            .recovery_display_list_for_output(
                output.id,
                &committed,
                &runtime.presentation_order,
                None,
            )
            .unwrap();
        assert!(only_the_cover(&recovery, output.id, 3), "recovery list");
    }
}

#[test]
fn a_locked_runtime_treats_every_surface_as_hidden() {
    let (outputs, mut runtime, scene, _target) = desktop();
    let ids = outputs.map(|output| output.id);
    assert_eq!(
        runtime.present_sampling(&ids[1..]),
        LivePresentSampling::Required,
        "an unlocked application output samples its Present"
    );
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(3)), &scene, None)
        .unwrap();
    assert_eq!(
        runtime.present_sampling(&ids),
        LivePresentSampling::SessionLocked
    );
    for output in ids {
        assert!(runtime.surface_hidden(SurfaceId::new(6, 1), output));
    }
}

#[test]
fn the_session_is_proven_locked_only_once_every_head_of_every_output_retired_the_cover() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    assert_eq!(target.presented_head_frames(outputs[0].id).len(), 2);
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(4)), &scene, None)
        .unwrap();
    // Every head frame of every output samples no client and draws the
    // cover alone, before any head has shown it.
    let frames = runtime
        .retained_output_head_composition_frames(&scene, &target)
        .unwrap();
    assert_eq!(frames.len(), outputs.len());
    for (output, heads) in &frames {
        assert_eq!(heads.len(), 2, "both mirror heads of {output:?}");
        for head in heads {
            let snapshot = head.frame.output_damage_snapshot.as_ref().unwrap();
            assert_eq!(
                sophia_engine::presented_session_lock(*output, &[Some(snapshot)]),
                sophia_engine::SessionLockEpoch::from_raw(4),
                "{output:?} head {:?} drew more than the cover",
                head.head
            );
        }
    }
    queue_all(&runtime, &scene, &mut target, &outputs);

    target.flip(outputs[0].id, 0);
    target.flip(outputs[0].id, 1);
    target.flip(outputs[1].id, 0);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        None,
        "the second output's mirror head still shows the desktop"
    );
    target.flip(outputs[1].id, 1);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        sophia_engine::SessionLockEpoch::from_raw(4)
    );
    target.teardown();
}

#[test]
fn clearing_the_lock_restores_the_desktop_and_ends_the_proof() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(4)), &scene, None)
        .unwrap();
    queue_all(&runtime, &scene, &mut target, &outputs);
    for output in &outputs {
        target.flip(output.id, 0);
        target.flip(output.id, 1);
    }
    assert!(runtime.presented_session_lock_on(&target).is_some());

    runtime
        .set_session_lock_on::<MirroredTarget>(None, &scene, None)
        .unwrap();
    for output in &outputs {
        assert!(
            draws_an_application(&output_list(&runtime, output.id)),
            "{:?} draws its application again",
            output.id
        );
    }
    queue_all(&runtime, &scene, &mut target, &outputs);
    target.flip(outputs[0].id, 0);
    assert_eq!(
        runtime.presented_session_lock_on(&target),
        None,
        "one head showing the desktop ends the proof"
    );
    target.teardown();
}

/// A topology change while locked: the provisional first frames of every
/// head, including those of an output no client has ever drawn on, are the
/// cover. The cover is runtime state, not any client's content.
#[test]
fn a_hotplugged_output_shows_the_cover_on_its_first_frame() {
    let (outputs, mut runtime, scene, _target) = desktop();
    let (left, right, added) = (outputs[0].id, outputs[1].id, OutputId::from_raw(3));
    let size = outputs[0].size;
    let head = |head, output| crate::LiveOutputAuthorityHeadTarget {
        head: RenderHeadId::from_raw(head),
        target_generation: 2,
        output,
        timing: crate::LibdrmNativeOutputTiming::new(64, 32, 60_000),
        native_size: size,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Exact,
        vrr: OutputVrrPolicy::Disabled,
    };
    let resolved = crate::LiveResolvedOutputTopology {
        primary_output: left,
        primary_heads: [
            (left, RenderHeadId::from_raw(11)),
            (right, RenderHeadId::from_raw(12)),
            (added, RenderHeadId::from_raw(13)),
        ]
        .into_iter()
        .collect(),
        outputs: [left, right, added]
            .map(|id| HeadlessOutput { id, size, scale: 1 })
            .to_vec(),
        logical_viewports: [left, right, added]
            .into_iter()
            .zip(0..)
            .map(
                |(output, index)| crate::LiveOutputAuthorityLogicalViewport {
                    output,
                    logical: rect(64 * index, 0, 64, 32),
                },
            )
            .collect(),
        disabled_heads: Vec::new(),
        targets: vec![head(11, left), head(12, right), head(13, added)],
        mirror_grouping: crate::NativeMirrorGrouping::none(),
    };
    let surfaces = |frames: &[crate::LiveProductionHeadCompositionFrame]| {
        frames
            .iter()
            .map(|frame| {
                frame
                    .frame
                    .output_damage_snapshot
                    .as_ref()
                    .unwrap()
                    .surfaces
                    .len()
            })
            .collect::<Vec<_>>()
    };
    let unlocked = runtime
        .compose_output_topology_head_frames(&scene, &resolved, 9)
        .unwrap();
    assert_eq!(
        surfaces(&unlocked),
        vec![1, 1, 0],
        "unlocked, each known output draws its application"
    );

    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(7)), &scene, None)
        .unwrap();
    let locked = runtime
        .compose_output_topology_head_frames(&scene, &resolved, 10)
        .unwrap();
    assert_eq!(locked.len(), 3);
    for frame in &locked {
        let snapshot = frame.frame.output_damage_snapshot.as_ref().unwrap();
        assert_eq!(
            sophia_engine::presented_session_lock(snapshot.output.id, &[Some(snapshot)]),
            sophia_engine::SessionLockEpoch::from_raw(7),
            "head {:?} of {:?} drew more than the cover",
            frame.head,
            snapshot.output.id
        );
    }
}

/// t289 joined: background pacing and Present clock selection see what the
/// heads draw. While locked no surface is visible and every new Present binds
/// the fallback clock; unlocking makes them visible again, which is the edge
/// that releases Presents parked while locked.
#[test]
fn a_locked_session_paces_every_client_as_hidden_and_binds_the_fallback_clock() {
    let (outputs, mut runtime, scene, _target) = desktop();
    let right = SurfaceId::new(6, 1);
    let geometry = rect(64, 0, 64, 32);
    let clock = |runtime: &LiveProductionVisualRuntime| {
        runtime.present_clock_outputs([(right, geometry)], Some(outputs[0].id))
    };
    assert!(runtime.background_surface_is_visible(right, geometry, 0.0));
    assert_eq!(clock(&runtime), vec![(right, Some(outputs[1].id))]);

    runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(9)), &scene, None)
        .unwrap();
    assert!(!runtime.background_surface_is_visible(right, geometry, 0.0));
    assert_eq!(clock(&runtime), vec![(right, None)], "the fallback clock");
    assert!(
        runtime
            .background_visible_surfaces([(right, geometry)])
            .is_empty()
    );

    runtime
        .set_session_lock_on::<MirroredTarget>(None, &scene, None)
        .unwrap();
    assert!(runtime.background_surface_is_visible(right, geometry, 0.0));
    assert_eq!(clock(&runtime), vec![(right, Some(outputs[1].id))]);
}

/// Locking never rolls back: the cover is runtime state every later frame
/// consults, so a lock whose repaint could not be queued still covers.
/// Unlocking does roll back: heads that never drew the desktop must not
/// report an unlock, so a session whose unlock repaint fails stays covered.
#[test]
fn an_unlock_whose_repaint_cannot_be_queued_keeps_the_cover() {
    let (outputs, mut runtime, scene, mut target) = desktop();
    target.refuse_queue = true;
    assert!(
        runtime
            .set_session_lock_on(Some(cover(5)), &scene, Some(&mut target))
            .is_err()
    );
    assert_eq!(
        runtime.session_lock(),
        Some(cover(5)),
        "a lock whose repaint failed still covers"
    );
    assert!(
        runtime
            .set_session_lock_on(None, &scene, Some(&mut target))
            .is_err()
    );
    assert_eq!(
        runtime.session_lock(),
        Some(cover(5)),
        "a failed unlock keeps the cover"
    );
    for output in &outputs {
        assert!(
            only_the_cover(&output_list(&runtime, output.id), output.id, 5),
            "{:?} still draws the cover alone",
            output.id
        );
    }
    target.refuse_queue = false;
    assert!(
        runtime
            .set_session_lock_on(None, &scene, Some(&mut target))
            .unwrap()
    );
    assert_eq!(runtime.session_lock(), None);
    target.teardown();
}
