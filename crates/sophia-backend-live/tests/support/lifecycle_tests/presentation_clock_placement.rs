use super::*;

#[test]
fn admission_placement_respects_output_ownership_and_replacing_tiers() {
    let mut scene = present_scene();
    let surface = SurfaceId::new(499, 1); // no pixels, route or presentation order
    let select = |scene: &PresentScene, placement| {
        scene
            .runtime
            .present_clock_outputs_for_placements([(surface, Some(placement))], Some(scene.output))
            [0]
        .1
    };
    // An off-edge column must not borrow the adjacent output's clock.
    let placement = LivePresentClockPlacement {
        geometry: rect(70, 0, 16, 16),
        output: Some(scene.output),
    };
    assert_eq!(select(&scene, placement), None);
    assert_eq!(
        select(
            &scene,
            LivePresentClockPlacement {
                output: None,
                ..placement
            }
        ),
        Some(OutputId::from_raw(2))
    );
    let visible = LivePresentClockPlacement {
        geometry: rect(0, 0, 16, 16),
        ..placement
    };
    assert_eq!(select(&scene, visible), Some(scene.output));
    scene.replace_with(vec![]);
    assert_eq!(select(&scene, visible), None);
}

#[test]
fn hidden_ordinary_placement_still_uses_an_actual_preview() {
    let mut scene = present_scene();
    let instance = shown_instance(scene.output, 501, 0, scene.previewed, rect(0, 0, 16, 16));
    scene.replace_with(vec![instance]);
    assert_eq!(
        scene.runtime.present_clock_outputs_for_placements(
            [(scene.previewed, None), (scene.application, None)],
            Some(scene.output),
        ),
        vec![
            (scene.application, None),
            (scene.previewed, Some(scene.output))
        ]
    );
}
