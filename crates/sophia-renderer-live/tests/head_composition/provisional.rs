//! G3-3: the metadata lowering of a combined frame job's candidate images,
//! against the conventional lowering of the same plan.
use super::*;
use sophia_engine::{
    CompositorImageSource, CompositorRgb8, HeadCompositorRect, HeadCompositorText,
};
use sophia_renderer_live::{
    CompositorTextRasterCache, IndicatorStripRasterCache, LiveImageOccurrence,
    LiveProvisionalDecline, lower_provisional_head_composition_plan,
};

fn content(
    lease: &sophia_runtime::ContentResourceLease,
    placement: u16,
    x: i32,
    provisional: bool,
) -> HeadCompositorCommand {
    let geometry = Rect {
        x,
        y: 0,
        width: 4,
        height: 2,
    };
    HeadCompositorCommand::ContentImage(sophia_engine::HeadCompositorContentImage {
        image: CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                grant: lease.description().grant,
                output: OutputId::from_raw(1),
                candidate: 1,
                surface: 0,
                placement,
            },
            generation: lease.description().resource.generation,
            output_size_px: Size {
                width: 1_920,
                height: 1_080,
            },
            geometry_px: geometry,
            size_px: Size {
                width: 4,
                height: 2,
            },
            stride: 16,
            format: u32::from_le_bytes(*b"AR24"),
            resource: if provisional {
                CompositorImageSource::Provisional(lease.description().clone())
            } else {
                lease.clone().into()
            },
        },
        geometry,
        clip: geometry,
    })
}

/// Two consecutive occurrences, the plan's surface between, a third after,
/// and one clipped to nothing.
fn commands(
    lease: &sophia_runtime::ContentResourceLease,
    provisional: bool,
) -> Vec<HeadCompositorCommand> {
    let surface = SurfaceId::new(3, 1);
    let mut clipped = content(lease, 3, 40, provisional);
    let HeadCompositorCommand::ContentImage(image) = &mut clipped else {
        unreachable!();
    };
    image.clip = Rect::default();
    vec![
        content(lease, 0, 0, provisional),
        content(lease, 1, 8, provisional),
        HeadCompositorCommand::Surface { surface },
        clipped,
        content(lease, 2, 16, provisional),
    ]
}

fn plan_with(commands: Vec<HeadCompositorCommand>) -> HeadCompositionPlan {
    HeadCompositionPlan {
        compositor: commands,
        ..plan()
    }
}

fn sources() -> Vec<LiveOwnedHeadCompositionSource> {
    let source = source(42);
    vec![LiveOwnedHeadCompositionSource {
        surface: source.surface,
        source: BufferSource::CpuBuffer { handle: 42 },
        kind: LiveOwnedHeadCompositionSourceKind::Cpu(source.buffer.into()),
    }]
}

fn caches() -> (IndicatorStripRasterCache, CompositorTextRasterCache) {
    (Default::default(), Default::default())
}

#[test]
fn a_provisional_image_never_lowers_conventionally() {
    let (_store, lease) = content_resource();
    assert_eq!(
        lower_head_composition_plan(&plan_with(vec![content(&lease, 0, 0, true)]), &sources())
            .err(),
        Some(LiveHeadCompositionLoweringError::UnavailableContentImage),
        "no pixels and no lease: conventional lowering refuses it"
    );
}

#[test]
fn placeholders_take_their_final_layer_indices() {
    let (_store, lease) = content_resource();
    let (indicator, text) = caches();
    let lowered = lower_provisional_head_composition_plan(
        &plan_with(commands(&lease, true)),
        &sources(),
        &indicator,
        &text,
    )
    .unwrap();
    assert_eq!(lowered.frame.layers.len(), 1, "only the surface lowers");
    assert_eq!(
        lowered
            .images
            .iter()
            .map(|image| (image.index, image.provisional))
            .collect::<Vec<_>>(),
        [(0, true), (1, true), (3, true)],
        "consecutive occurrences take 0 and 1, the surface 2, the last 3; the clipped one none"
    );
}

fn descriptor(
    layer: &LiveOwnedMixedCompositionLayer,
) -> (u64, Size, u32, u32, u64, Rect, Option<Rect>) {
    let LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } = layer else {
        panic!("a content image lowers to a CPU layer");
    };
    (
        buffer.handle,
        buffer.size,
        buffer.stride,
        buffer.format,
        buffer.generation,
        placement.target,
        placement.clip,
    )
}

fn occurrence(image: &LiveImageOccurrence) -> (u64, Size, u32, u32, u64, Rect, Option<Rect>) {
    (
        image.handle,
        image.size,
        image.stride,
        image.format,
        image.generation,
        image.placement.target,
        image.placement.clip,
    )
}

#[test]
fn the_placeholders_match_the_conventional_lowering_layer_for_layer() {
    let (_store, lease) = content_resource();
    let conventional =
        lower_head_composition_plan(&plan_with(commands(&lease, false)), &sources()).unwrap();
    let (indicator, text) = caches();
    let provisional = lower_provisional_head_composition_plan(
        &plan_with(commands(&lease, true)),
        &sources(),
        &indicator,
        &text,
    )
    .unwrap();
    assert_eq!(conventional.layers.len(), 4);
    let mut retained = provisional.frame.layers.iter();
    for (index, layer) in conventional.layers.iter().enumerate() {
        match provisional.images.iter().find(|image| image.index == index) {
            Some(image) => {
                assert_eq!(
                    occurrence(image),
                    descriptor(layer),
                    "occurrence {index} carries the conventional layer's descriptor"
                );
                assert_eq!(image.placement.alpha, 1.0);
                assert_eq!(
                    image.resource,
                    sophia_engine::CompositorImageSourceIdentity::Shell(
                        lease.description().clone()
                    )
                );
            }
            None => {
                let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = layer else {
                    panic!("the surface lowers to a CPU layer");
                };
                let Some(LiveOwnedMixedCompositionLayer::Cpu { buffer: kept, .. }) =
                    retained.next()
                else {
                    panic!("the retained layer keeps its order");
                };
                assert_eq!(
                    (kept.handle, kept.generation),
                    (buffer.handle, buffer.generation)
                );
            }
        }
    }
    assert!(retained.next().is_none());
}

fn text_command() -> HeadCompositorCommand {
    HeadCompositorCommand::Text(HeadCompositorText {
        node: CompositorNodeId::IndicatorStrip {
            output: OutputId::from_raw(1),
        },
        generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 12,
        },
        text: "dock".into(),
        font_size_millis: 10_000,
        color: CompositorRgb8 {
            red: 200,
            green: 200,
            blue: 200,
        },
    })
}

#[test]
fn an_unreadable_raster_key_is_ineligible_without_rasterizing() {
    let (indicator, mut text) = caches();
    let plan = plan_with(vec![text_command()]);
    let cold = text.stats();
    assert_eq!(
        lower_provisional_head_composition_plan(&plan, &sources(), &indicator, &text).err(),
        Some(LiveHeadCompositionLoweringError::ProvisionalIneligible(
            LiveProvisionalDecline::ColdTextRaster
        ))
    );
    assert_eq!(
        text.stats(),
        cold,
        "a cold key is not rasterized or counted"
    );

    // Warm: the conventional lowering fills the cache once.
    let HeadCompositorCommand::Text(command) = text_command() else {
        unreachable!();
    };
    text.raster_for(&command).unwrap();
    let warm = text.stats();
    let versions = (0..2)
        .map(|_| {
            let lowered =
                lower_provisional_head_composition_plan(&plan, &sources(), &indicator, &text)
                    .unwrap();
            let [LiveOwnedMixedCompositionLayer::Cpu { buffer, .. }] = &lowered.frame.layers[..]
            else {
                panic!("one text layer");
            };
            (buffer.handle, buffer.pixel_version)
        })
        .collect::<Vec<_>>();
    assert_eq!(versions[0], versions[1], "the same metadata twice");
    assert_eq!(text.stats(), warm, "a warm read moves and counts nothing");
}

#[test]
fn a_translucent_rect_declines_and_ordinary_lowering_is_unchanged() {
    let rect = HeadCompositorCommand::Rect(HeadCompositorRect {
        opacity: 128,
        node: CompositorNodeId::IndicatorStrip {
            output: OutputId::from_raw(1),
        },
        generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        },
        color: CompositorRgb8 {
            red: 1,
            green: 2,
            blue: 3,
        },
    });
    let plan = plan_with(vec![rect]);
    let (indicator, text) = caches();
    assert_eq!(
        lower_provisional_head_composition_plan(&plan, &sources(), &indicator, &text).err(),
        Some(LiveHeadCompositionLoweringError::ProvisionalIneligible(
            LiveProvisionalDecline::TranslucentRect
        )),
        "its buffer would be new pixels under a new version"
    );
    let conventional = lower_head_composition_plan(&plan, &sources()).unwrap();
    assert!(matches!(
        conventional.layers[..],
        [LiveOwnedMixedCompositionLayer::Cpu { .. }]
    ));
}
