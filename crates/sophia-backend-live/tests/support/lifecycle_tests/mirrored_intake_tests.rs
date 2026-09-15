use super::*;

#[test]
fn mirrored_thousand_cycles_join_intake_lowering_installation_copy_and_completion() {
    exercise_thousand(MirroredTarget::new(&outputs()));
}

#[test]
fn real_mirror_sources_survive_second_head_and_reservation_refusal_before_copy() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let id = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let source = upload(&mut store, grant(), id);
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, source.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let frame = target.queue.get(outputs[0].id).unwrap().frame;
    assert_eq!(target.queue.get(outputs[0].id).unwrap().heads.len(), 2);
    // From here the real lowered queue is the ONLY owner of the source bytes.
    drop(runtime);
    drop(source);
    retire(&mut store, grant(), id);
    assert!(released(&mut store).is_empty());
    let groups = target.groups.clone();
    target.wrong_target = Some(1);
    assert!(target.install(outputs[0].id).is_err());
    assert_eq!(target.installed_heads, 0);
    assert_eq!(target.groups, groups);
    assert!(target.cohorts.is_empty());
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, frame);
    assert!(released(&mut store).is_empty());
    target.wrong_target = None;
    target.refuse_reservation = true;
    assert_eq!(
        target.install(outputs[0].id),
        Err("mirror generation targets an unregistered output")
    );
    assert_eq!(target.installed_heads, 0);
    assert_eq!(target.groups, groups);
    assert!(target.cohorts.is_empty());
    assert!(released(&mut store).is_empty());
    target.refuse_reservation = false;
    target.install(outputs[0].id).unwrap();
    assert_eq!(target.installed_heads, 2);
    assert!(!target.queue.pending(outputs[0].id));
    assert!(target.heads[0].pending.is_some() && target.heads[1].pending.is_some());
    assert!(released(&mut store).is_empty());
    target.prepare(outputs[0].id);
    assert_eq!(released(&mut store), vec![id]); // both real source copies ended
    assert!(released(&mut store).is_empty());
    assert_eq!(target.owners.get(), 2); // copied backings remain physically owned
    target.flip(outputs[0].id, 1);
    assert_eq!(
        target.settled_checksum(outputs[0].id),
        None,
        "lagging primary remains owned"
    );
    assert!(target.groups[&outputs[0].id].completed_frame().is_none());
    // A different output progresses while the first primary remains submitted.
    target.install(outputs[1].id).unwrap();
    target.prepare(outputs[1].id);
    target.flip(outputs[1].id, 1);
    target.flip(outputs[1].id, 0);
    assert!(target.heads[0].custody.submitted().is_some());
    target.flip(outputs[0].id, 0);
    assert_eq!(target.groups[&outputs[0].id].completed_frame(), Some(frame));
    assert!(target.settled_checksum(outputs[0].id).is_some());
    target.teardown();
    assert_eq!(store.usage().retiring, 0);
}

#[test]
fn shared_reservation_refuses_before_mutation_and_preserves_singleton_startup() {
    let outputs = outputs();
    let mut target = MirroredTarget::new(&outputs);
    let installation = CompositionInstallation {
        output: outputs[0].id,
        frame: crate::LiveProductionNativeFrameId::from_raw(1),
        checksum: Some(99),
        mirrored: true,
    };
    let current = target.current_heads(installation);
    let group = target.groups.get_mut(&outputs[0].id).unwrap();
    group.begin(installation.frame);
    let before = group.clone();
    assert!(crate::reserve_composition_lifecycle(installation, &current, Some(group)).is_err());
    assert_eq!(*group, before);
    let mut uninitialized = LiveProductionMirrorGroupLifecycle::new(
        outputs[0].id,
        current.iter().map(|head| head.identity.head()),
    )
    .unwrap();
    let before = uninitialized.clone();
    assert!(
        crate::reserve_composition_lifecycle(installation, &current, Some(&mut uninitialized))
            .unwrap()
            .is_none()
    );
    assert_eq!(uninitialized, before);
    assert!(
        crate::reserve_composition_lifecycle(
            CompositionInstallation {
                mirrored: false,
                ..installation
            },
            &current[..1],
            None
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn changed_output_does_not_resubmit_a_settled_mirrored_neighbor() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let source = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    for output in outputs {
        runtime
            .set_shell_content_on_target(
                shell_frame(output, output.id.raw(), source.clone()),
                &scene,
                Some(&mut target),
            )
            .unwrap();
        target.drain();
    }
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 3, source.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.install(outputs[0].id).unwrap();
    target.prepare(outputs[0].id);
    target.flip(outputs[0].id, 0);
    assert!(target.heads[1].custody.submitted().is_some());
    assert_eq!(
        target.settled_checksum(outputs[0].id),
        None,
        "primary completion is not mirror settlement"
    );
    target.flip(outputs[0].id, 1);
    assert!(target.settled_checksum(outputs[0].id).is_some());
    let settled = target.groups[&outputs[1].id].completed_frame();
    let previous_a = target.groups[&outputs[0].id].completed_frame().unwrap();
    let displayed_b = [2, 3].map(|i| target.heads[i].custody.displayed().unwrap().correlation());
    let installs = target.installed_heads;
    let next = target.next;
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 4, source.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    assert!(target.queue.pending(outputs[0].id));
    assert_ne!(target.queue.get(outputs[0].id).unwrap().frame, previous_a);
    assert_eq!(target.next, next + 1, "only required A mints a frame");
    assert!(
        !target.queue.pending(outputs[1].id),
        "unchanged settled mirror must not submit another generation"
    );
    target.drain();
    assert_eq!(target.groups[&outputs[1].id].completed_frame(), settled);
    assert_eq!(target.installed_heads, installs + 2);
    assert_eq!(
        [2, 3].map(|i| target.heads[i].custody.displayed().unwrap().correlation()),
        displayed_b
    );
    target.teardown();
}

#[test]
fn settled_mirror_requires_every_current_head_and_never_just_equal_pixels() {
    let owner = crate::NativeFrameOwner::new();
    let other = crate::NativeFrameOwner::new();
    let output = OutputId::from_raw(1);
    let content = crate::LiveProductionScanoutContent::RetainedMixed {
        frame: crate::LiveProductionNativeFrameId::from_raw(7),
        requires_retirement: true,
        logical_content_checksum: Some(99),
        nonzero_rgb_pixels: 0,
    };
    let heads = [1, 2].map(|id| crate::SettledMirrorHead {
        head: RenderHeadId::from_raw(id),
        target_generation: 1,
        idle: true,
        presented: Some(content),
        displayed: Some(owner.frame(output, RenderHeadId::from_raw(id), 1, 7)),
    });
    let mut group = LiveProductionMirrorGroupLifecycle::new(output, heads.map(|h| h.head)).unwrap();
    let frame = content.frame();
    for head in heads {
        group.mark_initialized(head.head);
    }
    group.begin(frame);
    for head in heads {
        group.mark_submitted(head.head, frame);
        group.mark_flipped(head.head, frame);
    }
    assert_eq!(
        crate::settled_mirror_checksum(owner, output, 2, Some(&group), heads),
        Some(99)
    );
    assert_eq!(
        crate::settled_mirror_checksum(owner, output, 2, Some(&group), [heads[0]]),
        None
    );
    assert_eq!(
        crate::settled_mirror_checksum(owner, output, 1, Some(&group), heads),
        None
    );
    assert_eq!(
        crate::settled_mirror_checksum(owner, OutputId::from_raw(2), 2, Some(&group), heads),
        None
    );
    for negative in 0..10 {
        let mut changed = heads;
        match negative {
            0 => changed[1].idle = false,
            1 => changed[1].presented = None,
            2 => changed[1].displayed = None,
            3 => changed[1].displayed = Some(other.frame(output, changed[1].head, 1, 7)),
            4 => changed[1].target_generation = 2,
            5 => changed[1] = changed[0],
            6 => changed[1].displayed = Some(owner.frame(output, changed[1].head, 1, 8)),
            7..=9 => {
                let crate::LiveProductionScanoutContent::RetainedMixed {
                    frame,
                    logical_content_checksum,
                    ..
                } = changed[1].presented.as_mut().unwrap()
                else {
                    unreachable!()
                };
                match negative {
                    7 => {
                        *frame = crate::LiveProductionNativeFrameId::from_raw(8);
                        changed[1].displayed = Some(owner.frame(output, changed[1].head, 1, 8));
                    }
                    8 => *logical_content_checksum = Some(98),
                    9 => *logical_content_checksum = None,
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
        assert_eq!(
            crate::settled_mirror_checksum(owner, output, 2, Some(&group), changed),
            None,
            "negative {negative}"
        );
    }
}

// Replay real lowered owners at the ordinary-repaint adapter boundary. The
// capture does not publish or settle the originating shell candidate.
fn take_lowered(
    target: &mut MirroredTarget,
    output: OutputId,
) -> Vec<crate::LiveProductionHeadCompositionFrame> {
    let generation = target.queue.take_ready(output, None, None, None).unwrap();
    generation
        .heads
        .into_iter()
        .map(|head| crate::LiveProductionHeadCompositionFrame {
            head: head.identity.head(),
            scene_generation: head
                .frame
                .trace
                .expect("real lowerer trace")
                .scene_generation,
            target_generation: head.identity.target_generation(),
            mapping: target.heads[head.head_index].target.mapping,
            logical_content_checksum: generation.logical_content_checksum.unwrap(),
            frame: head.frame,
        })
        .collect()
}

#[test]
fn deferred_ordinary_change_back_is_not_suppressed_by_displayed_pixels() {
    let outputs = outputs();
    let output = outputs[0];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(output.size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let source = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(output, 1, source.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.drain();
    runtime
        .set_shell_content_on_target(
            shell_frame(output, 2, source.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let original = take_lowered(&mut target, output.id);
    let original_checksum = original[0].logical_content_checksum;
    for index in &target.outputs[&output.id] {
        assert_eq!(
            target.heads[*index].presented.unwrap().logical_checksum(),
            Some(original_checksum)
        );
        assert!(target.heads[*index].custody.displayed().is_some());
    }
    let mut changed = shell_frame(output, 3, source);
    changed.images[0].geometry_px.x = 1;
    runtime
        .set_shell_content_on_target(changed, &scene, Some(&mut target))
        .unwrap();
    let changed = take_lowered(&mut target, output.id);
    assert_ne!(changed[0].logical_content_checksum, original_checksum);
    target
        .queue_retained_batch(vec![(output.id, changed)], &BTreeSet::new())
        .unwrap();
    assert!(!target.queue.get(output.id).unwrap().requires_retirement());
    let newer = target.queue.get(output.id).unwrap().frame;
    let accepted = target
        .queue_retained_batch(vec![(output.id, original)], &BTreeSet::new())
        .unwrap();
    assert!(accepted.contains_key(&output.id));
    assert_ne!(accepted[&output.id], newer);
    assert_eq!(
        target
            .queue
            .get(output.id)
            .unwrap()
            .logical_content_checksum,
        Some(original_checksum)
    );
    target.drain();
    target.teardown();
}

#[test]
fn ordinary_scene_defers_protected_output_without_stopping_neighbor() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let source = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, source),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let protected = target.queue.get(outputs[0].id).unwrap().frame;
    let before = target.next;
    let frames = runtime
        .retained_output_head_composition_frames(&scene, &target)
        .unwrap();
    let states = target
        .outputs
        .iter()
        .map(|(output, indices)| {
            (
                *output,
                crate::NativeCompositionOutput {
                    targets: indices
                        .iter()
                        .map(|index| (*index, target.heads[*index].target))
                        .collect(),
                    ready: target.ready(*output),
                    protected: target.protected(*output),
                    available: true,
                    newest: [None; 4],
                    settled_mirror_checksum: None,
                },
            )
        })
        .collect();
    let result = crate::prepare_native_composition_batch(
        frames,
        &BTreeSet::new(),
        &states,
        target.owner,
        &mut target.next,
        crate::LiveProductionHeadCompositionContent::OrdinaryScene,
    );
    let generations =
        result.unwrap_or_else(|(reason, _)| panic!("ordinary repaint became fatal: {reason}"));
    assert_eq!(generations.len(), 1);
    assert_eq!(generations[0].output, outputs[1].id);
    assert_eq!(target.next, before + 1);
    target
        .queue
        .admit_batch(generations, &target.outputs.keys().copied().collect())
        .unwrap_or_else(|(reason, _)| panic!("{reason}"));
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, protected);
    assert!(
        target
            .queue
            .get(outputs[0].id)
            .unwrap()
            .requires_retirement()
    );
    target.install(outputs[1].id).unwrap();
    target.prepare(outputs[1].id);
    target.flip(outputs[1].id, 0);
    target.flip(outputs[1].id, 1);
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, protected);
}

#[test]
fn ordinary_retry_retains_one_obligation_and_recomposes_without_another_event() {
    let outputs = outputs();
    let output = outputs[0].id;
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let source = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, source),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let protected = target.queue.get(output).unwrap().frame;
    let next = target.next;
    let original_checksum = target.queue.get(output).unwrap().logical_content_checksum;
    let mut latest_checksum = None;
    for x in 0..8 {
        // A WM-owned outline is a scene update independent of shell resource
        // identity. Do not mutate accepted shell pixels or target meaning.
        runtime
            .set_floating_outline(
                Some(crate::LiveFloatingOutline {
                    surface: SurfaceId::new(71, 1),
                    geometry: Rect {
                        x: 16 + x,
                        y: 8,
                        width: 16,
                        height: 12,
                    },
                }),
                &scene,
                None,
            )
            .unwrap();
        let frames = runtime
            .retained_output_head_composition_frames(&scene, &target)
            .unwrap()
            .into_iter()
            .find(|(id, _)| *id == output)
            .unwrap()
            .1;
        latest_checksum = Some(frames[0].logical_content_checksum);
        assert_eq!(
            ordinary_repaint::admit(
                &mut runtime.ordinary_repaints_pending,
                &mut target,
                output,
                frames
            )
            .unwrap(),
            None
        );
        runtime
            .service_ordinary_repaints(&scene, &mut target)
            .unwrap();
        assert_eq!(runtime.ordinary_repaints_pending, BTreeSet::from([output]));
        assert_eq!(target.next, next);
        assert_eq!(target.queue.get(output).unwrap().frame, protected);
    }
    assert_ne!(latest_checksum, original_checksum);
    target.install(output).unwrap();
    target.prepare(output);
    target.flip(output, 0);
    runtime
        .service_ordinary_repaints(&scene, &mut target)
        .unwrap();
    assert_eq!(
        target.next, next,
        "lagging sibling still protects retirement"
    );
    target.flip(output, 1);
    // No event or new repaint offer: native service alone admits the retained
    // latest-scene obligation, through the same helper as production callers.
    runtime
        .service_ordinary_repaints(&scene, &mut target)
        .unwrap();
    assert!(runtime.ordinary_repaints_pending.is_empty());
    assert_eq!(target.next, next + 1);
    assert_ne!(target.queue.get(output).unwrap().frame, protected);
    assert!(!target.queue.get(output).unwrap().requires_retirement());
    assert_eq!(
        target.queue.get(output).unwrap().logical_content_checksum,
        latest_checksum
    );
    target.drain();
    let next = target.next;
    runtime
        .service_ordinary_repaints(&scene, &mut target)
        .unwrap();
    assert_eq!(target.next, next, "settled obligation is not replayed");
    target.teardown();
}

#[test]
fn ordinary_invalid_second_output_is_not_hidden_by_first_output_deferral() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let source = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, source),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let protected = target.queue.get(outputs[0].id).unwrap().frame;
    let next = target.next;
    let mut frames = runtime
        .retained_output_head_composition_frames(&scene, &target)
        .unwrap();
    frames[1].1[1].target_generation += 1;
    assert!(target.queue_ordinary_batch(frames).is_err());
    assert_eq!(target.next, next);
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, protected);
}
