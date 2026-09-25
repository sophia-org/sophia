use super::*;
#[test]
fn read_only_reference_requires_its_exact_retired_projection_without_hit_targets() {
    let output = HeadlessOutput::deterministic();
    let geometry = Rect {
        x: 10,
        y: 10,
        width: 300,
        height: 100,
    };
    let panel = |projection| {
        CompositorDisplayCommand::Rect(sophia_engine::CompositorRect {
            opacity: 221,
            node: sophia_engine::CompositorNodeId::DescriptorOverlay {
                projection,
                slot: u16::MAX,
                role: sophia_engine::DescriptorOverlayNodeRole::Panel,
            },
            generation: projection,
            geometry,
            color: sophia_engine::CompositorRgb8 {
                red: 17,
                green: 19,
                blue: 24,
            },
        })
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    runtime.descriptor_overlay = Some(sophia_engine::DescriptorOverlayProjection {
        output: output.id,
        generation: 9,
        geometry,
        commands: vec![panel(5)],
        targets: Vec::new(),
    });
    runtime.tab_frames.insert(
        output.id,
        CompositorDisplayList {
            output: output.id,
            commands: vec![panel(4)],
        }
        .into(),
    );
    runtime.replace_presented_input_projection(
        0,
        crate::production_visual_runtime::projection::PresentedPolicyFrameEvidence::default(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Some(geometry),
        Vec::new(),
    );
    assert_eq!(
        runtime.descriptor_overlay_presentation_epoch(output.id, 9, true),
        None
    );
    runtime.tab_frames.insert(
        output.id,
        CompositorDisplayList {
            output: output.id,
            commands: vec![panel(5)],
        }
        .into(),
    );
    runtime.replace_presented_input_projection(
        0,
        crate::production_visual_runtime::projection::PresentedPolicyFrameEvidence::default(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        Some(geometry),
        Vec::new(),
    );
    assert!(
        runtime
            .descriptor_overlay_presentation_epoch(output.id, 9, true)
            .is_some()
    );
    assert!(runtime.input_projections()[0].descriptor_targets.is_empty());
    runtime.descriptor_overlay = None;
    assert_eq!(
        runtime.descriptor_overlay_presentation_epoch(output.id, 9, false),
        None
    );
    runtime.tab_frames.remove(&output.id);
    runtime.replace_presented_input_projection(
        0,
        crate::production_visual_runtime::projection::PresentedPolicyFrameEvidence::default(),
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        None,
        Vec::new(),
    );
    assert!(
        runtime
            .descriptor_overlay_presentation_epoch(output.id, 9, false)
            .is_some()
    );
}
