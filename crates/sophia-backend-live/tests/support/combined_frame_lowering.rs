//! G3-3 closes G3-2's D1: one real scene lowered conventionally and
//! provisionally, the provisional occurrences mapped by the production
//! `CombinedPlaceholder::of_composition` and filled by the real private
//! binder, then the bound frame compared with the conventional one layer for
//! layer. Only the candidate's pixel storage and version may differ. No GPU.

use super::super::super::super::PendingRenderedFrame;
use super::{BYTES, CombinedPlaceholder, bind, validated};
use sophia_engine::{
    CompositorContentImage, CompositorImageSource, CompositorNodeId, CompositorRgb8,
    DirectScanoutVerdict, HeadCompositionPlan, HeadCompositorCommand, HeadCompositorContentImage,
    HeadCompositorRect, HeadLogicalTransform, RenderHeadId,
};
use sophia_protocol::{
    ContentGrant, ContentLimits, ContentResourceBegin, ContentResourceId, OutputHeadMapping,
    OutputId, OutputTransform, Rect, Region, Size, TransactionId,
};
use sophia_renderer_live::{
    CompositorTextRasterCache, IndicatorStripRasterCache, LiveCompositionPlacement,
    LiveCpuPixelStorage, LiveOwnedMixedCompositionLayer, lower_head_composition_plan,
    lower_provisional_head_composition_plan,
};
use std::sync::Arc;

const VIEW: Size = Size {
    width: 64,
    height: 32,
};

/// A resident 4x2 resource holding `BYTES`, the pixels the job validated.
fn lease() -> (
    sophia_runtime::ContentResourceStore,
    sophia_runtime::ContentResourceLease,
) {
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let resource = ContentResourceId {
        id: 77,
        generation: 1,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    store
        .begin_isb(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 4,
                height_px: 2,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    store
        .complete_isb(grant, resource, &mut Some(Ok(validated(&BYTES))), 0)
        .unwrap();
    while store.take_event().is_some() {}
    let lease = store.lease(grant, resource).unwrap();
    (store, lease)
}

fn content(
    lease: &sophia_runtime::ContentResourceLease,
    placement: u16,
    x: i32,
    clipped: bool,
    provisional: bool,
) -> HeadCompositorCommand {
    let geometry = Rect {
        x,
        y: 0,
        width: 4,
        height: 2,
    };
    HeadCompositorCommand::ContentImage(HeadCompositorContentImage {
        image: CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                grant: lease.description().grant,
                output: OutputId::from_raw(1),
                candidate: 1,
                surface: 0,
                placement,
            },
            generation: lease.description().resource.generation,
            output_size_px: VIEW,
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
        clip: if clipped { Rect::default() } else { geometry },
    })
}

/// Two consecutive occurrences, a retained opaque rect, one clipped to
/// nothing, and one after the rect.
fn plan(lease: &sophia_runtime::ContentResourceLease, provisional: bool) -> HeadCompositionPlan {
    let rect = HeadCompositorCommand::Rect(HeadCompositorRect {
        opacity: 255,
        node: CompositorNodeId::IndicatorStrip {
            output: OutputId::from_raw(1),
        },
        generation: 1,
        geometry: Rect {
            x: 0,
            y: 8,
            width: 16,
            height: 4,
        },
        color: CompositorRgb8 {
            red: 1,
            green: 2,
            blue: 3,
        },
    });
    HeadCompositionPlan {
        output: OutputId::from_raw(1),
        scene_generation: 1,
        head: RenderHeadId::from_raw(1),
        target_generation: 1,
        native_size: VIEW,
        target_transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
        transform: HeadLogicalTransform {
            source: VIEW,
            projected_scene: Rect {
                x: 0,
                y: 0,
                width: VIEW.width,
                height: VIEW.height,
            },
        },
        layers: Vec::new(),
        compositor: vec![
            content(lease, 0, 0, false, provisional),
            content(lease, 1, 4, false, provisional),
            rect,
            content(lease, 2, 12, true, provisional),
            content(lease, 3, 8, false, provisional),
        ],
        cursor: None,
        repaint: Region::empty(),
        logical_content_checksum: 0,
        direct_scanout: DirectScanoutVerdict::default(),
    }
}

/// Every placement field, as bits.
fn placement_bits(
    placement: &LiveCompositionPlacement,
) -> (
    Rect,
    Option<Rect>,
    [u32; 9],
    u32,
    sophia_engine::HeadSamplingClass,
) {
    (
        placement.target,
        placement.clip,
        placement.transform.matrix.map(f32::to_bits),
        placement.alpha.to_bits(),
        placement.sampling,
    )
}

#[test]
fn the_bound_provisional_frame_is_the_conventional_frame_layer_for_layer() {
    let (_store, lease) = lease();
    let conventional = lower_head_composition_plan(&plan(&lease, false), &[]).unwrap();
    let (indicator, text) = (
        IndicatorStripRasterCache::default(),
        CompositorTextRasterCache::default(),
    );
    let provisional =
        lower_provisional_head_composition_plan(&plan(&lease, true), &[], &indicator, &text)
            .unwrap();
    let placeholders = CombinedPlaceholder::of_composition(&provisional.images);
    assert_eq!(
        placeholders
            .iter()
            .map(|placeholder| placeholder.index)
            .collect::<Vec<_>>(),
        [0, 1, 3],
        "consecutive, then past the rect; none for the clipped occurrence"
    );
    let pixels = Arc::new(validated(&BYTES));
    let Ok(bound) = bind(
        PendingRenderedFrame::Mixed(provisional.frame, None),
        &placeholders,
        &pixels,
    ) else {
        panic!("every final index fits");
    };
    let PendingRenderedFrame::Mixed(bound, _) = bound.frame() else {
        panic!("the bound frame is Mixed");
    };
    assert_eq!(conventional.layers.len(), 4);
    assert_eq!(bound.layers.len(), conventional.layers.len());
    for (index, pair) in bound.layers.iter().zip(&conventional.layers).enumerate() {
        match pair {
            (
                LiveOwnedMixedCompositionLayer::Cpu {
                    buffer: ours,
                    placement: our_placement,
                },
                LiveOwnedMixedCompositionLayer::Cpu {
                    buffer: theirs,
                    placement: their_placement,
                },
            ) => {
                assert_eq!(
                    placement_bits(our_placement),
                    placement_bits(their_placement),
                    "layer {index}: every placement field"
                );
                assert_eq!(
                    (
                        ours.handle,
                        ours.size,
                        ours.stride,
                        ours.format,
                        ours.generation
                    ),
                    (
                        theirs.handle,
                        theirs.size,
                        theirs.stride,
                        theirs.format,
                        theirs.generation
                    ),
                    "layer {index}: every descriptor field"
                );
                assert_eq!(&*ours.bytes, &*theirs.bytes, "layer {index}: the bytes");
                // The one exception: the bound layer holds the validated
                // pixels and no version, so it always uploads.
                assert!(matches!(ours.bytes, LiveCpuPixelStorage::Validated(_)));
                assert_eq!(ours.pixel_version, None, "layer {index}");
            }
            (
                LiveOwnedMixedCompositionLayer::Solid {
                    geometry: ours,
                    color: our_color,
                },
                LiveOwnedMixedCompositionLayer::Solid {
                    geometry: theirs,
                    color: their_color,
                },
            ) => assert_eq!(
                (ours, our_color),
                (theirs, their_color),
                "layer {index}: the retained rect"
            ),
            _ => panic!("layer {index}: the kinds differ"),
        }
    }
    assert!(
        matches!(
            bound.layers[2],
            LiveOwnedMixedCompositionLayer::Solid { .. }
        ),
        "the rect separates the repetitions"
    );
}
