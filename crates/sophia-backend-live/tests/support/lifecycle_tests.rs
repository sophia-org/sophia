//! Actual shell intake, Engine planning, lowered owners and native queue with
//! simulated device facts/completions. This is not a KMS/driver proof.
use super::*;
#[path = "lifecycle_tests/component_identity.rs"]
mod component_identity;
#[path = "lifecycle_tests/component_removal.rs"]
mod component_removal;
#[path = "lifecycle_tests/policy_composition.rs"]
mod policy_composition;
#[path = "lifecycle_tests/popout_removal.rs"]
mod popout_removal;
#[path = "lifecycle_tests/resources.rs"]
mod resources;
#[path = "lifecycle_tests/target.rs"]
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
fn runtime_retirement_refuses_and_retains_real_displayed_custody_without_device_calls() {
    use std::{num::NonZeroU32, sync::Arc};
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    assert!(runtime.validate_native_retirement_disposition().is_ok());
    let bytes = Arc::new(vec![0_u8; 4096]);
    let submission = crate::LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: bytes.clone(),
        correlation: None,
        primary_plane: crate::LibdrmNativePrimaryPlaneScanoutSubmission {
            resources: crate::LibdrmNativePrimaryPlaneResourceBundle::new(
                NonZeroU32::new(10).unwrap().into(),
                None,
                outputs[0].size,
            ),
            completion_fence: None,
        },
        submitted_after_page_flip_serial: None,
        layout_witness: None,
    };
    assert!(
        runtime
            .outputs
            .values_mut()
            .next()
            .unwrap()
            .runtime
            .adopt_presented_rendered_primary_plane_scanout(submission)
    );
    for _ in 0..3 {
        assert!(runtime.validate_native_retirement_disposition().is_err());
        assert_eq!(Arc::strong_count(&bytes), 2);
    }
    // No device is supplied: this is refusal/retention, not framebuffer cleanup.
    // The terminal error carrier can keep this whole runtime until disposition.
    let retained = Box::new(runtime);
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(retained.validate_native_retirement_disposition().is_err());
    drop(retained); // Explicit terminal fallback, never a clean retirement.
    assert_eq!(Arc::strong_count(&bytes), 1);
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
    assert!(
        !runtime
            .shell_content
            .contains_key(&(outputs[1].id, LiveShellContentLayer::Shell))
    );
    assert!(runtime.retained_projection_retirements.is_empty());
    assert_eq!(
        runtime.shell_content[&(outputs[0].id, LiveShellContentLayer::Shell)]
            .frame
            .candidate_generation,
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
            runtime.shell_content_presentation_epoch(outputs[0].id, grant(), candidate),
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
                .shell_content_presentation_epoch(outputs[0].id, grant(), candidate)
                .is_some()
        );
    }
    assert_ne!(frames[0], frames[1]);
    assert_eq!(
        checksums[0], checksums[1],
        "fresh retirement must be needed even with equal logical checksums"
    );
}

trait IntegrationTarget: NativeCompositionTarget {
    fn queued(&self) -> &crate::DeferredNativeCompositions;
    fn drain(&mut self);
    fn teardown(&mut self);
    fn backing_count(&self) -> usize;
}

#[test]
fn thousand_two_output_intakes_reclaim_pixels_with_real_history_and_one_held_consumer() {
    exercise_thousand(Target::new(&outputs()));
}

fn exercise_thousand<T: IntegrationTarget>(mut target: T) {
    let outputs = outputs();
    let grant = grant();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
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
    let bytes = target.queued().get(outputs[0].id).unwrap().heads[0]
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
                runtime.shell_content_presentation_epoch(output.id, grant, candidate),
                None
            );
            target.drain();
            runtime.publish_presented_input_layers(&target);
            let epoch = runtime
                .shell_content_presentation_epoch(output.id, grant, candidate)
                .unwrap();
            let binding = runtime
                .input_projections
                .iter()
                .find(|projection| projection.output == output.id)
                .unwrap()
                .content
                .first()
                .unwrap();
            assert_eq!(binding.targets[0].action_id, candidate);
            assert_eq!(binding.targets[0].presentation_epoch, epoch);
            let viewport = runtime.outputs.logical_viewport(output.id).unwrap();
            assert_eq!(binding.transform.viewport, viewport);
            assert!(binding.authority_current);
            let global = Point {
                x: f64::from(viewport.x) + 1.0,
                y: f64::from(viewport.y) + 1.0,
            };
            assert!(matches!(
                resolve_content_pointer_event(
                    &mut capture,
                    SeatId::from_raw(1),
                    DeviceId::from_raw(1),
                    InputEventKind::PointerButton {
                        button: CHROME_PRIMARY_BUTTON,
                        pressed: true
                    },
                    Some(global),
                    Some(binding),
                    false
                ),
                ContentPointerDisposition::Captured
            ));
            assert!(
                matches!(resolve_content_pointer_event(&mut capture, SeatId::from_raw(1), DeviceId::from_raw(1), InputEventKind::PointerButton { button: CHROME_PRIMARY_BUTTON, pressed: false }, Some(global), Some(binding), false), ContentPointerDisposition::Activated(target) if target.action_id == candidate && target.presentation_epoch == epoch)
            );
            activation_count += 1;

            // Keep the actual CPU last-frame, damage and secondary-output caches alive.
            let display = CompositorDisplayList {
                output: output.id,
                commands: runtime.shell_content[&(output.id, LiveShellContentLayer::Shell)]
                    .frame
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
    assert_eq!(target.backing_count(), 0);
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
            .shell_content_presentation_epoch(outputs[1].id, grant(), 2)
            .is_some()
    );
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, grant(), 1),
        None
    );
    assert_eq!(target.queue.get(outputs[0].id).unwrap().frame, original);
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, grant(), 1)
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
        runtime.shell_content_presentation_epoch(outputs[0].id, grant(), 1),
        None
    );
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[1].id, grant(), 2)
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
        runtime.shell_content_presentation_epoch(outputs[0].id, grant(), 1),
        None
    );
    assert!(target.flip(outputs[0].id, None));
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, grant(), 1)
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
    let other_epoch = runtime.shell_content_presentation_epoch(outputs[1].id, grant(), 2);
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
            runtime.shell_content_presentation_epoch(outputs[1].id, grant(), 2),
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
            .shell_content_presentation_epoch(outputs[0].id, grant(), 2)
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
            .contains_key(&(outputs[0].id, LiveShellContentLayer::Shell))
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
            .shell_content_presentation_epoch(outputs[1].id, grant(), 3)
            .is_some()
    );
    assert_eq!(
        runtime
            .input_projections
            .iter()
            .find(|p| p.output == outputs[0].id)
            .unwrap()
            .content
            .first()
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
            .shell_content_presentation_epoch(outputs[0].id, grant(), 4)
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
            .shell_content_presentation_epoch(outputs[0].id, grant(), 1)
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
    let stale = runtime.input_projections[0].content.first().unwrap();
    assert!(!stale.authority_current);
    assert_eq!(stale.targets[0].grant, grant());
    let mut capture = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut capture,
            SeatId::from_raw(1),
            DeviceId::from_raw(1),
            InputEventKind::PointerButton {
                button: CHROME_PRIMARY_BUTTON,
                pressed: true
            },
            Some(Point { x: 1.0, y: 1.0 }),
            Some(stale),
            false
        ),
        ContentPointerDisposition::Consumed
    );
    assert_eq!(
        runtime.shell_content_presentation_epoch(outputs[0].id, next_grant, 1),
        None
    );
    runtime.input_projections[0].content.clear();
    runtime.publish_presented_input_layers(&target);
    let unknown = runtime.input_projections[0].content.first().unwrap();
    assert!(!unknown.authority_current);
    assert!(unknown.targets.is_empty());
    assert_eq!(
        resolve_content_pointer_event(
            &mut capture,
            SeatId::from_raw(1),
            DeviceId::from_raw(1),
            InputEventKind::PointerButton {
                button: CHROME_PRIMARY_BUTTON,
                pressed: false
            },
            Some(Point { x: 1.0, y: 1.0 }),
            Some(unknown),
            false
        ),
        ContentPointerDisposition::Consumed
    );
    assert_eq!(
        resolve_content_pointer_event(
            &mut capture,
            SeatId::from_raw(1),
            DeviceId::from_raw(1),
            InputEventKind::PointerButton {
                button: CHROME_PRIMARY_BUTTON,
                pressed: true
            },
            Some(Point { x: 1.0, y: 1.0 }),
            Some(unknown),
            false
        ),
        ContentPointerDisposition::Consumed
    );
    target.complete(outputs[0].id);
    runtime.publish_presented_input_layers(&target);
    assert!(
        runtime
            .shell_content_presentation_epoch(outputs[0].id, next_grant, 1)
            .is_some()
    );
    assert_eq!(
        runtime.input_projections[0]
            .content
            .first()
            .unwrap()
            .targets[0]
            .grant,
        next_grant
    );
}

#[test]
fn topology_change_cannot_reinterpret_old_presented_pixels_with_a_new_origin() {
    let outputs = outputs();
    let output = outputs[0];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(output.size);
    let mut target = Target::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let pixels = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 1,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(shell_frame(output, 1, pixels), &scene, Some(&mut target))
        .unwrap();
    target.drain();
    runtime.publish_presented_input_layers(&target);
    let old = runtime.input_projections[0]
        .content
        .first()
        .unwrap()
        .clone();
    // Supply a committed topology transition without constructing a DRM owner.
    // Publication/intake below are the real production methods, completion fake.
    let mut viewports = runtime.outputs.logical_viewports().collect::<Vec<_>>();
    viewports
        .iter_mut()
        .find(|(id, _)| *id == output.id)
        .unwrap()
        .1
        .x = -1920;
    runtime
        .outputs
        .replace_logical_viewports(&viewports)
        .unwrap();
    runtime.content_layout_generation += 1;
    for retained_metadata in [true, false] {
        if !retained_metadata {
            runtime.input_projections[0].content.clear();
        }
        runtime.publish_presented_input_layers(&target);
        let stale = runtime.input_projections[0].content.first().unwrap();
        assert_eq!(stale.transform, old.transform);
        assert!(!stale.authority_current);
        let mut capture = ContentCaptureState::default();
        assert_eq!(
            resolve_content_pointer_event(
                &mut capture,
                SeatId::from_raw(1),
                DeviceId::from_raw(1),
                InputEventKind::PointerButton {
                    button: CHROME_PRIMARY_BUTTON,
                    pressed: true
                },
                Some(Point { x: -1919.0, y: 1.0 }),
                Some(stale),
                false
            ),
            ContentPointerDisposition::Consumed
        );
    }
    let next = upload(
        &mut store,
        grant(),
        ContentResourceId {
            id: 2,
            generation: 1,
        },
    );
    runtime
        .set_shell_content_on_target(shell_frame(output, 2, next), &scene, Some(&mut target))
        .unwrap();
    runtime.publish_presented_input_layers(&target);
    assert!(
        !runtime.input_projections[0]
            .content
            .first()
            .unwrap()
            .authority_current
    );
    target.drain();
    runtime.publish_presented_input_layers(&target);
    let current = runtime.input_projections[0].content.first().unwrap();
    assert!(current.authority_current);
    assert_eq!(current.transform.viewport.x, -1920);
    assert_eq!(
        current.transform.layout_generation,
        old.transform.layout_generation + 1
    );
    let mut capture = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut capture,
            SeatId::from_raw(1),
            DeviceId::from_raw(1),
            InputEventKind::PointerButton {
                button: CHROME_PRIMARY_BUTTON,
                pressed: true
            },
            Some(Point { x: -1919.0, y: 1.0 }),
            Some(current),
            false
        ),
        ContentPointerDisposition::Captured
    );
}

#[test]
fn revoked_suspend_keeps_displayed_custody_for_owned_retirement() {
    use std::{num::NonZeroU32, sync::Arc};
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    assert!(runtime.validate_native_retirement_disposition().is_ok());
    let bytes = Arc::new(vec![0_u8; 4096]);
    let submission = crate::LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: bytes.clone(),
        correlation: None,
        primary_plane: crate::LibdrmNativePrimaryPlaneScanoutSubmission {
            resources: crate::LibdrmNativePrimaryPlaneResourceBundle::new(
                NonZeroU32::new(10).unwrap().into(),
                None,
                outputs[0].size,
            ),
            completion_fence: None,
        },
        submitted_after_page_flip_serial: None,
        layout_witness: None,
    };
    assert!(
        runtime
            .outputs
            .values_mut()
            .next()
            .unwrap()
            .runtime
            .adopt_presented_rendered_primary_plane_scanout(submission)
    );
    assert_eq!(Arc::strong_count(&bytes), 2);
    assert!(runtime.validate_native_retirement_disposition().is_err());
    let report = runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    assert_eq!(
        report.outcome,
        crate::LiveProductionNativeSuspendOutcome::ForcedDetachRevoked
    );
    assert!(runtime.validate_native_retirement_disposition().is_err());
    assert!(runtime.native_suspended);
    assert!(
        runtime
            .input_projections
            .iter()
            .all(|projection| projection.content.is_empty() && projection.layers.is_empty())
    );
    // No native owner, worker, or device is constructed here. This reaches
    // the production logical-runtime transition before any worker join or
    // outside-loop retirement transfer can have occurred.
    assert_eq!(
        Arc::strong_count(&bytes),
        2,
        "revoked suspend dropped displayed custody before the owned retirement transition"
    );
    for _ in 0..3 {
        runtime.suspend_revoked_native_scanout(&outputs).unwrap();
        assert!(runtime.validate_native_retirement_disposition().is_err());
        assert_eq!(Arc::strong_count(&bytes), 2);
    }
}

#[test]
fn revoked_suspend_without_affine_owners_remains_disposed() {
    let outputs = outputs();
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    for _ in 0..3 {
        runtime.suspend_revoked_native_scanout(&outputs).unwrap();
        assert!(runtime.validate_native_retirement_disposition().is_ok());
        assert_eq!(runtime.outputs.output_count(), outputs.len());
    }
}
