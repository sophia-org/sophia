//! Native resume at a resolved layout (t306, t310): the head plans a resume
//! presents first are lowered at the viewports given, before any later
//! rebind, and the ordinary resume keeps the row.
use super::*;

/// Where output 2's head plans put each CPU layer, by the planner the resume
/// presents its first frames from.
fn first_targets(
    runtime: &LiveProductionVisualRuntime,
    scene: &LiveProductionCpuScene,
    target: &MirroredTarget,
    output: OutputId,
) -> Vec<Vec<Rect>> {
    runtime
        .retained_output_head_composition_frames(scene, target)
        .unwrap()
        .into_iter()
        .find(|(planned, _)| *planned == output)
        .unwrap()
        .1
        .iter()
        .map(|head| {
            head.frame
                .layers
                .iter()
                .filter_map(|layer| match layer {
                    sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu {
                        placement, ..
                    } => Some(placement.target),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

fn resume(
    viewports: Option<&[(OutputId, Rect)]>,
) -> (
    [HeadlessOutput; 2],
    LiveProductionVisualRuntime,
    LiveProductionCpuScene,
    MirroredTarget,
) {
    let (outputs, mut runtime, scene, mut target) = desktop();
    target.teardown();
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    target = MirroredTarget::new(&outputs);
    let prepared = runtime
        .resumed_output_set(&outputs, None, viewports)
        .unwrap();
    runtime
        .resume_prepared_outputs_on(&mut target, prepared, &scene)
        .unwrap();
    (outputs, runtime, scene, target)
}

#[test]
fn a_resume_at_a_resolved_layout_plans_its_first_frames_at_those_viewports() {
    // The second output resumes 32 pixels into the first instead of beside
    // it, so the application at root x 64 lands at x 32 of its head, and
    // the first output's application shows on its left half.
    let outputs = super::outputs();
    let viewports = [
        (outputs[0].id, rect(0, 0, 64, 32)),
        (outputs[1].id, rect(32, 0, 64, 32)),
    ];
    let (outputs, runtime, scene, mut target) = resume(Some(&viewports));
    for head in first_targets(&runtime, &scene, &target, outputs[1].id) {
        assert!(head.contains(&rect(32, 0, 64, 32)), "{head:?}");
        assert!(!head.contains(&rect(0, 0, 64, 32)), "{head:?}");
    }
    target.teardown();

    let (outputs, runtime, scene, mut target) = resume(None);
    for head in first_targets(&runtime, &scene, &target, outputs[1].id) {
        assert!(head.contains(&rect(0, 0, 64, 32)), "{head:?}");
    }
    target.teardown();
}

#[test]
fn a_resume_refuses_viewports_that_do_not_cover_its_outputs() {
    let (outputs, mut runtime, _scene, mut target) = desktop();
    target.teardown();
    runtime.suspend_revoked_native_scanout(&outputs).unwrap();
    let partial = [(outputs[0].id, rect(0, 0, 64, 32))];
    assert!(
        runtime
            .resumed_output_set(&outputs, None, Some(&partial))
            .is_err()
    );
}
