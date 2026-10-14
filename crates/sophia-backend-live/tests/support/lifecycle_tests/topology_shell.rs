//! KVM-style output replacement through the real runtime and CPU compositor.
//! Native devices and completion events come from the deterministic target.
use super::*;

fn settle(
    runtime: &mut LiveProductionVisualRuntime,
    target: &mut MirroredTarget,
    outputs: &[HeadlessOutput],
) {
    for output in outputs {
        if !target.queue.pending(output.id) {
            continue;
        }
        target.install(output.id).unwrap();
        target.prepare(output.id);
        target.flip(output.id, 0);
        target.flip(output.id, 1);
    }
    runtime.publish_presented_input_layers(target);
}

#[test]
fn resized_output_withdraws_old_shell_pixels_before_resume_repaint() {
    resized_repaint(true, true);
}

#[test]
fn resized_output_withdraws_old_shell_pixels_before_rebind_repaint() {
    resized_repaint(false, true);
}

#[test]
fn kvm_loss_keeps_the_direct_monitor_after_its_logical_output_is_renumbered() {
    resized_repaint(true, false);
}

fn resized_repaint(resume: bool, keep_neighbor: bool) {
    let mut outputs = outputs();
    if !keep_neighbor {
        // Two different monitors: the preferred KVM monitor goes away; the
        // directly connected monitor becomes the sole logical output 1.
        outputs[1].size = Size {
            width: 48,
            height: 24,
        };
    }
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let mut target = MirroredTarget::new(&outputs);
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    for (index, output) in outputs.iter().enumerate() {
        let source = upload(
            &mut store,
            grant(),
            ContentResourceId {
                id: index as u64 + 1,
                generation: 1,
            },
        );
        runtime
            .set_shell_content_on_target(shell_frame(*output, 1, source), &scene, Some(&mut target))
            .unwrap();
        settle(&mut runtime, &mut target, &outputs);
    }
    assert_eq!(runtime.shell_content.len(), 2);
    let mut replacement = outputs.to_vec();
    replacement[0].size = Size {
        width: 48,
        height: 24,
    };
    if !keep_neighbor {
        replacement.truncate(1);
    }
    target.teardown();
    runtime.revoke_shell_content_retirement_claims(grant());
    if resume {
        runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    }
    target = MirroredTarget::new(&replacement);
    scene.reconfigure_output_descriptors(&replacement).unwrap();
    if resume {
        let prepared = runtime
            .resumed_output_set(&replacement, None, None)
            .unwrap();
        runtime
            .resume_prepared_outputs_on(&mut target, prepared, &scene)
            .unwrap();
    } else {
        let viewports = replacement
            .iter()
            .enumerate()
            .map(|(i, output)| {
                (
                    output.id,
                    Rect {
                        x: i as i32 * 64,
                        y: 0,
                        width: output.size.width,
                        height: output.size.height,
                    },
                )
            })
            .collect::<Vec<_>>();
        runtime
            .rebind_applied_topology_on(&mut target, &replacement, &viewports)
            .unwrap();
    }
    let list = runtime.display_list(&[], &[]).unwrap();
    // This is the composition that ended the installed session. Retaining
    // the old image makes the production compositor reject its old size.
    scene
        .compose_display_list(replacement[0], &[], &list, None)
        .unwrap();
    assert_eq!(list.content_images().count(), 0);
    assert!(
        !runtime
            .shell_content
            .contains_key(&(outputs[0].id, LiveShellContentLayer::Shell))
    );
    assert_eq!(
        runtime
            .shell_content
            .contains_key(&(outputs[1].id, LiveShellContentLayer::Shell)),
        keep_neighbor,
        "an unchanged neighbor retains its pixels; an absent output cannot"
    );
    assert!(
        runtime
            .input_projections
            .iter()
            .all(|projection| projection.content.is_empty())
    );
    target.teardown();
}

#[test]
fn resized_output_cannot_discard_an_unrevoked_shell_claim() {
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
    let mut replacement = outputs;
    replacement[0].size.width = 48;
    assert!(runtime.retain_shell_content_outputs(&replacement).is_err());
    assert_eq!(runtime.shell_content.len(), 1);
    runtime.revoke_shell_content_retirement_claims(grant());
    assert_eq!(
        runtime.retain_shell_content_outputs(&replacement).unwrap(),
        1
    );
    // The queued native frames still own their sources until explicit teardown.
    assert!(target.queue.pending(outputs[0].id));
    target.teardown();
}
