use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId, CompositorRect,
    CompositorRgb8, HeadlessOutput,
};
use sophia_protocol::{OutputId, Rect, Size};
use sophia_renderer_live::LiveProductionCpuScene;

fn output(id: u64, width: i32, height: i32) -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(id),
        size: Size { width, height },
        scale: 1,
    }
}

fn compose_first(scene: &mut LiveProductionCpuScene, outputs: &[HeadlessOutput]) {
    let first = outputs[0];
    let list = CompositorDisplayList {
        output: first.id,
        commands: vec![CompositorDisplayCommand::Rect(CompositorRect {
            node: CompositorNodeId::IndicatorStrip { output: first.id },
            generation: 1,
            opacity: 255,
            geometry: Rect {
                x: 0,
                y: 0,
                width: first.size.width,
                height: first.size.height,
            },
            color: CompositorRgb8 {
                red: 13,
                green: 29,
                blue: 47,
            },
        })],
    };
    scene
        .compose_display_list(first, &[], &list, None)
        .expect("a topology change must keep the CPU scene matched to its composition descriptor");
    let frames = scene.frames_for_outputs(outputs).unwrap();
    assert_eq!(frames.len(), outputs.len());
    for (frame, descriptor) in frames.iter().zip(outputs) {
        assert_eq!(frame.frame.size, descriptor.size);
    }
    assert_eq!(&frames[0].frame.bytes[0..4], &[47, 29, 13, 255]);
}

#[test]
fn descriptor_reconfiguration_matches_composition_across_apply_and_rollback() {
    let initial = [output(1, 16, 9), output(2, 12, 8)];
    // Output 2 may be primary for focus/placement. Composition still receives
    // the ID-ordered descriptors, with output 1 first, as in the native failure.
    let candidate = [output(1, 20, 10), output(2, 12, 8)];
    let mut scene = LiveProductionCpuScene::new(initial[0].size);
    compose_first(&mut scene, &initial);
    // Changing only the policy primary leaves descriptor order and sizes
    // intact. This was the native startup trigger, before any client appeared.
    let changed = scene.reconfigure_output_descriptors(&initial).unwrap();
    compose_first(&mut scene, &initial);
    assert!(!changed);
    assert!(scene.reconfigure_output_descriptors(&candidate).unwrap());
    assert!(
        scene.frames_for_outputs(&candidate).is_err(),
        "old-size frames must be invalidated"
    );
    compose_first(&mut scene, &candidate);
    assert!(!scene.reconfigure_output_descriptors(&candidate).unwrap());
    compose_first(&mut scene, &candidate);
    assert!(scene.reconfigure_output_descriptors(&initial).unwrap());
    compose_first(&mut scene, &initial);
    // A removed first output changes which descriptor is composed next.
    assert!(scene.reconfigure_output_descriptors(&initial[1..]).unwrap());
    compose_first(&mut scene, &initial[1..]);
}

#[test]
fn invalid_replacement_descriptors_leave_the_composed_frame_available() {
    let outputs = [output(1, 16, 9), output(2, 12, 8)];
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    compose_first(&mut scene, &outputs);
    let checksum = scene.frames_for_outputs(&outputs).unwrap()[0].checksum;
    assert!(scene.reconfigure_output_descriptors(&[]).is_err());
    assert!(
        scene
            .reconfigure_output_descriptors(&[output(1, 0, 9)])
            .is_err()
    );
    assert_eq!(
        scene.frames_for_outputs(&outputs).unwrap()[0].checksum,
        checksum
    );
}
