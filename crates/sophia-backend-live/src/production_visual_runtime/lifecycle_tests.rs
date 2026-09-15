//! Actual shell intake, Engine planning, lowered owners and native queue with
//! simulated device facts/completions. This is not a KMS/driver proof.
use super::*;
mod resources;
mod target;
use resources::*;
use target::Target;

fn outputs() -> [HeadlessOutput; 2] {
    [1, 2].map(|id| HeadlessOutput {
        id: OutputId::from_raw(id),
        size: Size {
            width: 64,
            height: 32,
        },
        scale: 1,
    })
}
fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    }
}

#[test]
fn actual_intake_refusal_rolls_back_candidate_without_partial_native_batch() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    assert!(
        runtime
            .set_shell_content_on_target(
                shell_frame(outputs[0], 1, lease.clone()),
                &scene,
                Some(&mut target)
            )
            .unwrap()
    );
    let first = target.queue.get(outputs[0].id).unwrap().frame;
    let next = target.next;
    target.reject_output = Some(outputs[1].id);
    assert!(
        runtime
            .set_shell_content_on_target(
                shell_frame(outputs[1], 2, lease.clone()),
                &scene,
                Some(&mut target)
            )
            .is_err()
    );
    assert_eq!(
        target.next, next,
        "invalid batch cannot mint any native identity"
    );
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, first);
    assert!(!runtime.shell_content.contains_key(&outputs[1].id));
    assert!(runtime.retained_projection_retirements.is_empty());
    assert_eq!(
        runtime.shell_content[&outputs[0].id].candidate_generation,
        1
    );
    target.reject_output = None;
    assert!(
        runtime
            .set_shell_content_on_target(
                shell_frame(outputs[1], 2, lease),
                &scene,
                Some(&mut target)
            )
            .unwrap()
    );
    assert!(target.queue.pending(outputs[0].id) && target.queue.pending(outputs[1].id));
}

#[test]
fn identical_shell_pixels_require_a_distinct_queue_and_exact_presentation() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    let mut frames = Vec::new();
    let mut checksums = Vec::new();
    for candidate in 1..=2 {
        runtime
            .set_shell_content_on_target(
                shell_frame(outputs[0], candidate, lease.clone()),
                &scene,
                Some(&mut target),
            )
            .unwrap();
        assert_eq!(
            runtime.shell_content_presentation_epoch(outputs[0].id, candidate),
            None
        );
        frames.push(target.queue.get(outputs[0].id).unwrap().frame);
        checksums.push(
            target
                .queue
                .get(outputs[0].id)
                .unwrap()
                .logical_checksum()
                .unwrap(),
        );
        target.complete(outputs[0].id);
        runtime.publish_presented_input_layers(&target);
        assert!(
            runtime
                .shell_content_presentation_epoch(outputs[0].id, candidate)
                .is_some()
        );
    }
    assert_ne!(frames[0], frames[1]);
    assert_eq!(
        checksums[0], checksums[1],
        "fresh retirement must be needed even with equal logical checksums"
    );
}

#[test]
fn thousand_two_output_intakes_reclaim_pixels_with_real_history_and_one_held_consumer() {
    let outputs = outputs();
    let grant = grant();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    let held_id = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let held = upload(&mut store, grant, held_id);
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, held.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    // Hold a real byte consumer from the actual Engine-lowered native queue.
    let bytes = target.queue.get(outputs[0].id).unwrap().heads[0]
        .frame
        .layers
        .iter()
        .find_map(|layer| {
            if let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = layer {
                (buffer.size
                    == Size {
                        width: 4,
                        height: 2,
                    }
                    && buffer.bytes.len() == 32)
                    .then(|| buffer.bytes.clone())
            } else {
                None
            }
        })
        .expect("native queue must contain the submitted 4x2 source pixels");
    target.drain();
    let stable_id = ContentResourceId {
        id: 2,
        generation: 1,
    };
    let stable = upload(&mut store, grant, stable_id);
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 2, stable.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.drain();
    retire(&mut store, grant, held_id);
    drop(held);
    assert!(released(&mut store).is_empty());
    assert_eq!(store.usage().retiring, 32);

    let mut capture = ContentCaptureState::default();
    let mut activation_count = 0;
    let mut previous = None;
    let mut released_count = 0;
    let mut max_bytes = 0;
    for iteration in 0..1000_u64 {
        let id = ContentResourceId {
            id: 3 + iteration % 2,
            generation: 1 + iteration / 2,
        };
        let changing = upload(&mut store, grant, id);
        let peak = store.usage();
        assert!(peak.resident + peak.retiring <= 128);
        if iteration > 0 {
            assert_eq!(peak.resident + peak.retiring, 128);
        }
        for (output, resource) in [(outputs[0], stable.clone()), (outputs[1], changing)] {
            let candidate = 3 + iteration * 2 + output.id.raw();
            runtime
                .set_shell_content_on_target(
                    shell_frame(output, candidate, resource),
                    &scene,
                    Some(&mut target),
                )
                .unwrap();
            assert_eq!(
                runtime.shell_content_presentation_epoch(output.id, candidate),
                None
            );
            target.drain();
            runtime.publish_presented_input_layers(&target);
            let epoch = runtime
                .shell_content_presentation_epoch(output.id, candidate)
                .unwrap();
            let binding = runtime
                .input_projections
                .iter()
                .find(|projection| projection.output == output.id)
                .unwrap()
                .content
                .as_ref()
                .unwrap();
            assert_eq!(binding.targets[0].action_id, candidate);
            assert_eq!(binding.targets[0].presentation_epoch, epoch);
            assert!(matches!(
                resolve_content_pointer_event(
                    &mut capture,
                    SeatId::from_raw(1),
                    DeviceId::from_raw(1),
                    InputEventKind::PointerButton {
                        button: CHROME_PRIMARY_BUTTON,
                        pressed: true
                    },
                    Some(Point { x: 1.0, y: 1.0 }),
                    Some(binding),
                    false
                ),
                ContentPointerDisposition::Captured
            ));
            assert!(
                matches!(resolve_content_pointer_event(&mut capture, SeatId::from_raw(1), DeviceId::from_raw(1), InputEventKind::PointerButton { button: CHROME_PRIMARY_BUTTON, pressed: false }, Some(Point { x: 1.0, y: 1.0 }), Some(binding), false), ContentPointerDisposition::Activated(target) if target.action_id == candidate && target.presentation_epoch == epoch)
            );
            activation_count += 1;

            // Keep the actual CPU last-frame, damage and secondary-output caches alive.
            let display = CompositorDisplayList {
                output: output.id,
                commands: runtime.shell_content[&output.id]
                    .images
                    .iter()
                    .cloned()
                    .map(CompositorDisplayCommand::ContentImage)
                    .collect(),
            };
            scene
                .compose_display_list(output, &[], &display, None)
                .unwrap();
            scene.frames_for_outputs(&outputs).unwrap();
        }
        if let Some(old) = previous.replace(id) {
            retire(&mut store, grant, old);
            assert_eq!(released(&mut store), vec![old]);
            released_count += 1;
        }
        let usage = store.usage();
        assert_eq!(
            usage.retiring, 32,
            "only the deliberately held consumer may retain old source bytes"
        );
        assert_eq!((usage.staging, usage.reserved_resident), (0, 0));
        max_bytes = max_bytes.max(usage.resident + usage.retiring);
        assert!(
            max_bytes <= 96,
            "two current resources plus one held old resource"
        );
        assert_eq!(store.grant(), grant);
    }
    assert_eq!(released_count, 999);
    assert_eq!(activation_count, 2000);
    drop(bytes);
    assert_eq!(released(&mut store), vec![held_id]);
    assert_eq!(store.usage().retiring, 0);
    assert!(released(&mut store).is_empty());
    runtime.shell_content.clear();
    drop(stable);
    target.teardown();
    assert_eq!(target.backing_owners.get(), 0);
    retire(&mut store, grant, stable_id);
    retire(&mut store, grant, previous.unwrap());
    let final_releases = released(&mut store);
    assert_eq!(final_releases.len(), 2);
    assert!(final_releases.contains(&stable_id));
    assert!(final_releases.contains(&previous.unwrap()));
    let usage = store.usage();
    assert_eq!(
        (
            usage.resident,
            usage.retiring,
            usage.staging,
            usage.backing,
            usage.reserved_resident
        ),
        (0, 0, 0, 0, 0)
    );
    // Historical/input/worker metadata intentionally still lives at this point.
    assert!(runtime.tab_frames.len() == 2);
    assert!(released(&mut store).is_empty());
}

#[test]
fn ordinary_projection_preserves_staged_retirement_while_other_output_advances() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, lease.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let original = target.queue.get(outputs[0].id).unwrap().frame;
    assert!(target.queue.protected(outputs[0].id));
    // There are no runtime claim keys left: ownership transferred to native.
    assert!(runtime.retained_projection_retirements.is_empty());
    runtime
        .queue_retained_projection(&scene, &mut target)
        .unwrap();
    assert!(runtime.retained_projection_pending);
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, original);
    runtime
        .set_shell_content_on_target(shell_frame(outputs[1], 2, lease), &scene, Some(&mut target))
        .unwrap();
    target.complete(outputs[1].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[1].id, 2)
            .is_some()
    );
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, 1),
        None
    );
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, original);
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 1)
            .is_some()
    );
    runtime
        .queue_retained_projection(&scene, &mut target)
        .unwrap();
    assert!(!runtime.retained_projection_pending);
}

#[test]
fn delayed_worker_and_wrong_flip_keep_exact_candidate_unpublished() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, lease.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.begin_render(outputs[0].id);
    runtime
        .queue_retained_projection(&scene, &mut target)
        .unwrap();
    assert!(runtime.retained_projection_pending);
    assert!(!target.queue.pending(outputs[0].id));
    // The second output progresses while the first owns a real pending Mixed.
    runtime
        .set_shell_content_on_target(shell_frame(outputs[1], 2, lease), &scene, Some(&mut target))
        .unwrap();
    target.complete(outputs[1].id);
    runtime.publish_presented_input_layers(&target);
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, 1),
        None
    );
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[1].id, 2)
            .is_some()
    );
    target.finish_render(outputs[0].id);
    runtime
        .queue_retained_projection(&scene, &mut target)
        .unwrap();
    assert!(!target.queue.pending(outputs[0].id));
    let wrong =
        crate::NativeFrameOwner::new().frame(outputs[0].id, RenderHeadId::from_raw(1), 1, 1);
    assert!(!target.flip(outputs[0].id, Some(wrong)));
    runtime.publish_presented_input_layers(&target);
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, 1),
        None
    );
    assert!(target.flip(outputs[0].id, None));
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 1)
            .is_some()
    );
}

#[test]
fn one_outputs_new_candidate_does_not_submit_the_unchanged_other_output() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
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
                shell_frame(output, output.id.raw(), lease.clone()),
                &scene,
                Some(&mut target),
            )
            .unwrap();
        target.drain();
    }
    runtime.publish_presented_input_layers(&target);
    let other_epoch = runtime.shell_content_presentation_epoch(outputs[1].id, 2);
    assert!(other_epoch.is_some());
    let before = target.next;
    assert!(
        !runtime
            .queue_retained_projection(&scene, &mut target)
            .unwrap()
    );
    assert_eq!(target.next, before);
    for candidate in 3..13 {
        let before = target.next;
        runtime
            .set_shell_content_on_target(
                shell_frame(outputs[0], candidate, lease.clone()),
                &scene,
                Some(&mut target),
            )
            .unwrap();
        assert!(target.queue.pending(outputs[0].id));
        assert!(!target.queue.pending(outputs[1].id));
        assert_eq!(target.next, before + 1);
        target.complete(outputs[0].id);
        runtime.publish_presented_input_layers(&target);
        assert_eq!(
            runtime.shell_content_presentation_epoch(outputs[1].id, 2),
            other_epoch
        );
    }
}

#[test]
fn successor_publishes_while_old_backing_cleanup_remains_owned() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let lease = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 1, lease.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    let old_frame = target.queue.get(outputs[0].id).unwrap().frame.raw();
    target.drain();
    target.fail_cleanup(Some(u32::try_from(old_frame).unwrap()));
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 2, lease.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 2)
            .is_some()
    );
    assert_eq!(target.retry_cleanup(outputs[0].id), Some(false));
    runtime
        .set_shell_content_on_target(
            shell_frame(outputs[0], 4, lease.clone()),
            &scene,
            Some(&mut target),
        )
        .unwrap();
    assert!(!target.queue.pending(outputs[0].id));
    assert!(
        runtime
            .retained_projection_retirements
            .contains_key(&outputs[0].id)
    );
    // New pixels are already presented. Failed destruction of their
    // predecessor neither retracts them nor prevents the other output.
    runtime
        .set_shell_content_on_target(shell_frame(outputs[1], 3, lease), &scene, Some(&mut target))
        .unwrap();
    target.complete(outputs[1].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[1].id, 3)
            .is_some()
    );
    assert_eq!(
        runtime
            .input_projections
            .iter()
            .find(|p| p.output == outputs[0].id)
            .unwrap()
            .content
            .as_ref()
            .unwrap()
            .candidate_generation,
        2,
        "waiting candidate must not erase or replace the still-presented target"
    );
    assert_eq!(
        target.backing_owners.get(),
        3,
        "two displayed copies plus old failed cleanup"
    );
    target.fail_cleanup(None);
    assert_eq!(target.retry_cleanup(outputs[0].id), Some(true));
    assert_eq!(target.retry_cleanup(outputs[0].id), None);
    assert_eq!(target.backing_owners.get(), 2);
    runtime
        .queue_retained_projection(&scene, &mut target)
        .unwrap();
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 4)
            .is_some()
    );
    assert!(runtime.retained_projection_retirements.is_empty());
    target.teardown();
    assert_eq!(target.backing_owners.get(), 0);
}

#[test]
fn reconnect_reusing_candidate_numbers_cannot_publish_old_pixels_as_new_grant() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let old = upload(&mut store, grant(), resource);
    runtime
        .set_shell_content_on_target(shell_frame(outputs[0], 1, old), &scene, Some(&mut target))
        .unwrap();
    target.drain();
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 1)
            .is_some()
    );
    let next_grant = ContentGrant {
        connection_epoch: 8,
        content_grant_epoch: 10,
    };
    let mut next_store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(next_grant)).unwrap();
    let new = upload(&mut next_store, next_grant, resource);
    runtime
        .set_shell_content_on_target(shell_frame(outputs[0], 1, new), &scene, Some(&mut target))
        .unwrap();
    runtime.publish_presented_input_layers(&target);
    assert!(runtime.input_projections[0].content.is_none());
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, 1),
        None
    );
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, 1)
            .is_some()
    );
    assert_eq!(
        runtime.input_projections[0]
            .content
            .as_ref()
            .unwrap()
            .targets[0]
            .grant,
        next_grant
    );
}
