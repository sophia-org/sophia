// t244: mirror-copy damage for WM presentation content: policy targets,
// region borders and stamp coverage project as the head plan draws them.

/// t244: a mirror head's damage for WM presentation content is where that
/// head draws it. Surface instances, region backdrops (rects) and the
/// presentation stamp's coverage project with the scene, as borders do.
#[test]
fn mirror_damage_projection_places_instances_regions_and_stamp_coverage() {
    use sophia_engine::{
        CompositorBorder, CompositorContentImage, CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId,
        CompositorPresentationStamp, CompositorRect, CompositorRgb8, CompositorSurfaceInstance,
        HeadlessOutput, OutputFrameDamageSnapshot,
    };
    use sophia_protocol::{OutputId, Size};
    let output = OutputId::from_raw(3);
    let source = Size {
        width: 2560,
        height: 1440,
    };
    let destination = HeadlessOutput {
        id: output,
        size: Size {
            width: 1280,
            height: 720,
        },
        scale: 1,
    };
    let whole = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    let preview = Rect {
        x: 200,
        y: 100,
        width: 400,
        height: 300,
    };
    let snapshot = OutputFrameDamageSnapshot {
        output: HeadlessOutput {
            id: output,
            size: source,
            scale: 1,
        },
        surfaces: Vec::new(),
        compositor_display_list: CompositorDisplayList::<CompositorContentImage> {
            output,
            commands: vec![
                CompositorDisplayCommand::PresentationStamp(CompositorPresentationStamp {
                    owner_epoch: 7,
                    publication_generation: 2,
                    output,
                    output_generation: 1,
                    coverage: whole,
                }),
                CompositorDisplayCommand::Rect(CompositorRect {
                    opacity: 255,
                    node: CompositorNodeId::PolicyRegion {
                        owner_epoch: 7,
                        id: 1,
                    },
                    generation: 2,
                    geometry: whole,
                    color: CompositorRgb8 {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                }),
                CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
                    owner_epoch: 7,
                    id: 2,
                    generation: 3,
                    source: SurfaceId::new(9, 1),
                    source_generation: 4,
                    destination: preview,
                    clip: preview,
                    opacity_millis: 1_000,
                }),
                // A tiny frame whose hole vanishes at half scale (solid),
                // and a thin emphasis ring: each keeps drawn damage.
                CompositorDisplayCommand::Border(CompositorBorder {
                    node: CompositorNodeId::PolicyRegion {
                        owner_epoch: 7,
                        id: 4,
                    },
                    generation: 2,
                    outer: Rect {
                        x: 10,
                        y: 2,
                        width: 3,
                        height: 3,
                    },
                    inner: Rect {
                        x: 11,
                        y: 3,
                        width: 1,
                        height: 1,
                    },
                    color: CompositorRgb8 {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                }),
                CompositorDisplayCommand::Border(CompositorBorder {
                    node: CompositorNodeId::PolicyRegion {
                        owner_epoch: 7,
                        id: 5,
                    },
                    generation: 2,
                    outer: Rect {
                        x: 20,
                        y: 4,
                        width: 6,
                        height: 6,
                    },
                    inner: Rect {
                        x: 22,
                        y: 6,
                        width: 2,
                        height: 2,
                    },
                    color: CompositorRgb8 {
                        red: 1,
                        green: 2,
                        blue: 3,
                    },
                }),
                // One logical pixel at an even offset: half of it is less
                // than a pixel, and a policy target still keeps one.
                CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
                    owner_epoch: 7,
                    id: 3,
                    generation: 1,
                    source: SurfaceId::new(9, 1),
                    source_generation: 4,
                    destination: Rect {
                        x: 2,
                        y: 2,
                        width: 1,
                        height: 1,
                    },
                    clip: Rect {
                        x: 2,
                        y: 2,
                        width: 1,
                        height: 1,
                    },
                    opacity_millis: 1_000,
                }),
            ],
        }
        .into(),
        software_cursor: None,
    };
    let projected =
        project_mirror_output_damage_snapshot(&snapshot, source, destination, NativeMirrorFit::Fit)
            .expect("presentation damage projects");
    let half = Rect {
        x: 100,
        y: 50,
        width: 200,
        height: 150,
    };
    let screen = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    };
    let list = &projected.compositor_display_list;
    assert_eq!(list.presentation_stamp().unwrap().coverage, screen);
    assert!(matches!(&list.commands[1], CompositorDisplayCommand::Rect(rect) if rect.geometry == screen));
    let instance = list.surface_instances().next().unwrap();
    assert_eq!((instance.destination, instance.clip), (half, half));
    assert_eq!((instance.id, instance.generation, instance.source_generation), (2, 3, 4));
    // Policy borders damage exactly where the head plan draws them: the
    // Engine's shared policy border geometry, never an empty ring.
    let shared = sophia_engine::HeadLogicalTransform {
        source,
        projected_scene: screen,
    };
    for (id, outer, inner) in [
        (
            4,
            Rect {
                x: 10,
                y: 2,
                width: 3,
                height: 3,
            },
            Rect {
                x: 11,
                y: 3,
                width: 1,
                height: 1,
            },
        ),
        (
            5,
            Rect {
                x: 20,
                y: 4,
                width: 6,
                height: 6,
            },
            Rect {
                x: 22,
                y: 6,
                width: 2,
                height: 2,
            },
        ),
    ] {
        let border = list
            .commands
            .iter()
            .find_map(|command| match command {
                CompositorDisplayCommand::Border(border)
                    if border.node == (CompositorNodeId::PolicyRegion { owner_epoch: 7, id }) =>
                {
                    Some(*border)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (border.outer, border.inner),
            shared.project_local_policy_border(outer, inner),
            "region {id} damages as the head plan draws it"
        );
        assert!(
            sophia_engine::compositor_border_bands(border)
                .iter()
                .any(|band| !band.geometry.is_empty()),
            "region {id} keeps drawn damage"
        );
    }
    let tiny = list.surface_instances().find(|instance| instance.id == 3).unwrap();
    assert_eq!(
        tiny.visible(),
        Rect {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        }
    );
}

/// A Cover mirror copy that crops a WM policy target away entirely neither
/// damages nor lists it, so the whole output's drawn membership cannot
/// include a target that head never showed (t244).
#[test]
fn a_cover_mirror_copy_drops_policy_targets_it_crops_away() {
    use sophia_engine::{
        CompositorContentImage, CompositorDisplayCommand, CompositorDisplayList,
        CompositorPresentationStamp, CompositorSurfaceInstance, HeadlessOutput,
        OutputFrameDamageSnapshot,
    };
    use sophia_protocol::{OutputId, Size};
    let output = OutputId::from_raw(3);
    let source = Size {
        width: 2560,
        height: 1440,
    };
    let instance = |id: u64, x: i32| {
        let rect = Rect {
            x,
            y: 600,
            width: 200,
            height: 200,
        };
        CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
            owner_epoch: 7,
            id,
            generation: 1,
            source: SurfaceId::new(9, 1),
            source_generation: 1,
            destination: rect,
            clip: rect,
            opacity_millis: 1_000,
        })
    };
    let snapshot = OutputFrameDamageSnapshot {
        output: HeadlessOutput {
            id: output,
            size: source,
            scale: 1,
        },
        surfaces: Vec::new(),
        compositor_display_list: CompositorDisplayList::<CompositorContentImage> {
            output,
            commands: vec![
                CompositorDisplayCommand::PresentationStamp(CompositorPresentationStamp {
                    owner_epoch: 7,
                    publication_generation: 1,
                    output,
                    output_generation: 1,
                    coverage: Rect {
                        x: 0,
                        y: 0,
                        width: 2560,
                        height: 1440,
                    },
                }),
                // A 720x720 Cover mirror keeps the middle 1440 source columns.
                instance(1, 100),
                instance(2, 1200),
            ],
        }
        .into(),
        software_cursor: None,
    };
    let projected = project_mirror_output_damage_snapshot(
        &snapshot,
        source,
        HeadlessOutput {
            id: output,
            size: Size {
                width: 720,
                height: 720,
            },
            scale: 1,
        },
        NativeMirrorFit::Cover,
    )
    .unwrap();
    let ids = projected
        .compositor_display_list
        .surface_instances()
        .map(|instance| instance.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![2], "the cropped preview is not listed on this head");
}
