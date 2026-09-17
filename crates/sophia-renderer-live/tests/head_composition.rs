#![cfg(feature = "gbm-probe")]

use sophia_engine::{CompositorContentImage, CompositorNodeId, DirectScanoutVerdict};
use sophia_engine::{
    HeadBindingOutcome, HeadCompositionPlan, HeadCompositorCommand, HeadLayerBinding,
    HeadLogicalTransform, HeadSamplingClass, OutputSceneCursor, RenderHeadId,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, ContentGrant, ContentLimits, ContentResourceBegin,
    ContentResourceChunk, ContentResourceEnd, ContentResourceId, ContentResourceRetire,
    OutputHeadMapping, OutputId, OutputTransform, Rect, Region, ShellContentRecord, Size,
    SurfaceContentFidelity, SurfaceContentSet, SurfaceContentVariant, SurfaceId,
    SurfaceRasterTransform, TransactionId,
};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSource, LiveCpuBufferUpdate,
    LiveCpuPresentationLayer, LiveHeadCompositionLoweringError, LiveOwnedDmaBufPlane,
    LiveOwnedHeadCompositionSource, LiveOwnedHeadCompositionSourceKind,
    LiveOwnedMixedCompositionLayer, LiveOwnedMultiPlaneDmaBufFrame, LiveProductionCpuScene,
    LiveRendererImageId, lower_cpu_head_composition_plan, lower_head_composition_plan,
};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;

fn content_resource() -> (
    sophia_runtime::ContentResourceStore,
    sophia_runtime::ContentResourceLease,
) {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let resource = ContentResourceId {
        id: 11,
        generation: 1,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    store
        .begin(
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
        .chunk(
            TransactionId::from_raw(2),
            &ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0x7f; 32],
            },
            0,
        )
        .unwrap();
    store
        .end(
            TransactionId::from_raw(3),
            &ContentResourceEnd {
                grant,
                resource,
                total_bytes: 32,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    let lease = store.lease(grant, resource).unwrap();
    (store, lease)
}

fn plan() -> HeadCompositionPlan {
    let surface = SurfaceId::new(3, 1);
    HeadCompositionPlan {
        output: OutputId::from_raw(1),
        scene_generation: 4,
        head: RenderHeadId::from_raw(7),
        target_generation: 2,
        native_size: Size {
            width: 1_920,
            height: 1_080,
        },
        target_transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
        transform: HeadLogicalTransform {
            source: Size {
                width: 2_560,
                height: 1_440,
            },
            projected_scene: Rect {
                x: 0,
                y: 0,
                width: 1_920,
                height: 1_080,
            },
        },
        layers: vec![HeadLayerBinding {
            surface,
            committed_generation: 8,
            variant: 2,
            source: BufferSource::CpuBuffer { handle: 42 },
            source_pixel_size: Size {
                width: 600,
                height: 450,
            },
            density_millis: 750,
            opacity_millis: 1_000,
            // A head whose viewport starts at root (2560, 32): the same
            // surface, one rectangle in the root and another in this head's
            // framebuffer. Lowering must not confuse them.
            logical_geometry: Rect {
                x: 2_635,
                y: 122,
                width: 600,
                height: 450,
            },
            native_geometry: Rect {
                x: 75,
                y: 90,
                width: 600,
                height: 450,
            },
            native_clip: Rect {
                x: 75,
                y: 90,
                width: 600,
                height: 450,
            },
            requested_sampling: HeadSamplingClass::Exact,
            outcome: HeadBindingOutcome::Active,
        }],
        compositor: vec![HeadCompositorCommand::Surface { surface }],
        cursor: None::<OutputSceneCursor>,
        repaint: Region::single(Rect {
            x: 75,
            y: 90,
            width: 600,
            height: 450,
        }),
        logical_content_checksum: 0x55,
        // This fixture is a composed multi-layer plan; the renderer's own
        // tests do not exercise eligibility, which Engine decides.
        direct_scanout: DirectScanoutVerdict::default(),
    }
}

fn source(handle: u64) -> LiveCpuPresentationLayer {
    LiveCpuPresentationLayer {
        surface: SurfaceId::new(3, 1),
        geometry: Rect {
            x: 100,
            y: 120,
            width: 800,
            height: 600,
        },
        buffer: LiveCpuBufferSource {
            handle,
            size: Size {
                width: 600,
                height: 450,
            },
            stride: 2_400,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 9,
            bytes: Arc::new(vec![0; 2_400 * 450]),
        },
    }
}

#[test]
fn lowers_the_selected_variant_at_head_native_geometry() {
    let frame = lower_cpu_head_composition_plan(&plan(), &[source(42)]).unwrap();
    let LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } = &frame.layers[0] else {
        panic!("planned CPU surface did not remain a CPU layer")
    };
    assert_eq!(buffer.handle, 42);
    assert_eq!(placement.target.width, 600);
    assert_eq!(placement.target.height, 450);
    assert_eq!(placement.sampling, HeadSamplingClass::Exact);
    assert_eq!(frame.trace.unwrap().head, RenderHeadId::from_raw(7));
    let damage = frame.output_damage_snapshot.unwrap();
    assert_eq!(damage.output.size.width, 1_920);
    // Lowering measures damage in the head's own framebuffer and leaves the
    // root placement alone. Input reads the second one, so a mirror that
    // overwrote it would put this window back out of the pointer's reach.
    assert_eq!(
        damage.surfaces[0].geometry,
        Rect {
            x: 75,
            y: 90,
            width: 600,
            height: 450,
        }
    );
    assert_eq!(
        damage.surfaces[0].logical_geometry,
        Rect {
            x: 2_635,
            y: 122,
            width: 600,
            height: 450,
        }
    );
    assert_eq!(
        damage.surfaces[0].buffer,
        BufferSource::CpuBuffer { handle: 42 }
    );
}

#[test]
fn missing_selected_variant_never_falls_back_to_another_cpu_buffer() {
    assert_eq!(
        lower_cpu_head_composition_plan(&plan(), &[source(41)]).unwrap_err(),
        LiveHeadCompositionLoweringError::MissingCpuSource(42)
    );
}

#[test]
fn retained_renderer_image_uses_head_plan_geometry_instead_of_primary_geometry() {
    let mut plan = plan();
    plan.layers[0].source = BufferSource::DmaBuf { handle: 77 };
    let source = LiveOwnedHeadCompositionSource {
        surface: SurfaceId::new(3, 1),
        source: BufferSource::DmaBuf { handle: 77 },
        kind: LiveOwnedHeadCompositionSourceKind::RendererImage {
            image_id: LiveRendererImageId::from_raw(9),
            size: Size {
                width: 600,
                height: 450,
            },
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        },
    };

    let frame = lower_head_composition_plan(&plan, &[source]).unwrap();
    let LiveOwnedMixedCompositionLayer::RendererImage {
        image_id,
        placement,
        ..
    } = frame.layers[0]
    else {
        panic!("retained DMA-BUF did not lower to its per-head renderer image")
    };
    assert_eq!(image_id, LiveRendererImageId::from_raw(9));
    assert_eq!(placement.target, plan.layers[0].native_geometry);
    assert_eq!(placement.clip, Some(plan.layers[0].native_clip));
}

/// A retained image is a copy, so its size is its own fact.
///
/// The plan measures the surface's committed buffer; a renderer image holds
/// the compositor's copy of an earlier generation under that same identity.
/// The two agree only while the surface has not resized, and a live session
/// ended when they stopped agreeing -- a mirror surface whose committed buffer
/// had grown to 2560x1440 while the copy still held 1280x1440. Placement
/// carries the copy to the head at whatever size the head wants; a mirror
/// member already draws every retained image at a size of its own.
#[test]
fn retained_renderer_image_may_hold_a_generation_of_another_size() {
    let mut plan = plan();
    plan.layers[0].source = BufferSource::DmaBuf { handle: 77 };
    let source = LiveOwnedHeadCompositionSource {
        surface: SurfaceId::new(3, 1),
        source: BufferSource::DmaBuf { handle: 77 },
        kind: LiveOwnedHeadCompositionSourceKind::RendererImage {
            image_id: LiveRendererImageId::from_raw(9),
            size: Size {
                width: 300,
                height: 450,
            },
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        },
    };

    let frame = lower_head_composition_plan(&plan, &[source]).unwrap();
    let LiveOwnedMixedCompositionLayer::RendererImage { placement, .. } = frame.layers[0] else {
        panic!("retained copy of another generation did not lower")
    };
    assert_eq!(placement.target, plan.layers[0].native_geometry);
    assert_eq!(placement.sampling, HeadSamplingClass::Upsampled);
    assert_eq!(
        frame.output_damage_snapshot.unwrap().surfaces[0].source_size,
        Size {
            width: 300,
            height: 450,
        },
    );
}

/// A buffer the plan measured must be the buffer that arrives.
#[test]
fn cpu_source_of_the_wrong_size_is_still_refused() {
    let plan = plan();
    let source = LiveOwnedHeadCompositionSource {
        surface: SurfaceId::new(3, 1),
        source: BufferSource::CpuBuffer { handle: 42 },
        kind: LiveOwnedHeadCompositionSourceKind::Cpu(
            LiveCpuBufferSource {
                handle: 42,
                size: Size {
                    width: 300,
                    height: 450,
                },
                stride: 1_200,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 9,
                bytes: Arc::new(vec![0; 1_200 * 450]),
            }
            .into(),
        ),
    };

    assert_eq!(
        lower_head_composition_plan(&plan, &[source]).unwrap_err(),
        LiveHeadCompositionLoweringError::SourceSizeMismatch {
            surface: SurfaceId::new(3, 1),
            handle: 42,
            planned: Size {
                width: 600,
                height: 450,
            },
            held: Size {
                width: 300,
                height: 450,
            },
        }
    );
}

#[test]
fn dma_buf_source_duplicates_its_affine_plane_for_each_head_frame() {
    let mut plan = plan();
    plan.layers[0].source = BufferSource::DmaBuf { handle: 88 };
    let fd: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    let source_fd = fd.as_raw_fd();
    let source = LiveOwnedHeadCompositionSource {
        surface: SurfaceId::new(3, 1),
        source: BufferSource::DmaBuf { handle: 88 },
        kind: LiveOwnedHeadCompositionSourceKind::DmaBuf {
            image_id: LiveRendererImageId::from_raw(10),
            frame: LiveOwnedMultiPlaneDmaBufFrame {
                width: 600,
                height: 450,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                modifier: 0,
                plane_count: 1,
                planes: [
                    Some(LiveOwnedDmaBufPlane {
                        fd,
                        offset: 0,
                        stride: 2_400,
                    }),
                    None,
                    None,
                    None,
                ],
            },
        },
    };

    let first = lower_head_composition_plan(&plan, std::slice::from_ref(&source)).unwrap();
    let second = lower_head_composition_plan(&plan, std::slice::from_ref(&source)).unwrap();
    let plane_fd = |frame: &sophia_renderer_live::LiveOwnedMixedCompositionFrame| {
        let LiveOwnedMixedCompositionLayer::DmaBuf { frame, .. } = &frame.layers[0] else {
            panic!("DMA-BUF source changed kind")
        };
        frame.planes[0].as_ref().unwrap().fd.as_raw_fd()
    };
    assert_ne!(plane_fd(&first), source_fd);
    assert_ne!(plane_fd(&second), source_fd);
    assert_ne!(plane_fd(&first), plane_fd(&second));
}

#[test]
fn production_scene_resolves_all_resident_cpu_variants_by_handle() {
    let mut scene = LiveProductionCpuScene::new(Size {
        width: 800,
        height: 600,
    });
    let buffer = |handle, width, height, generation| LiveCpuBufferSource {
        handle,
        size: Size { width, height },
        stride: u32::try_from(width * 4).unwrap(),
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        generation,
        bytes: Arc::new(vec![0; usize::try_from(width * height * 4).unwrap()]),
    };
    scene
        .apply_updates([
            LiveCpuBufferUpdate::Replace(buffer(41, 800, 600, 1)),
            LiveCpuBufferUpdate::Replace(buffer(42, 600, 450, 1)),
        ])
        .unwrap();
    let content = SurfaceContentSet::new(
        Size {
            width: 800,
            height: 600,
        },
        vec![
            SurfaceContentVariant {
                variant: 1,
                source: BufferSource::CpuBuffer { handle: 41 },
                pixel_size: Size {
                    width: 800,
                    height: 600,
                },
                density_millis: 1_000,
                transform: SurfaceRasterTransform::Normal,
                fidelity: SurfaceContentFidelity::AuthorityRaster,
                damage: Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                }),
            },
            SurfaceContentVariant {
                variant: 2,
                source: BufferSource::CpuBuffer { handle: 42 },
                pixel_size: Size {
                    width: 600,
                    height: 450,
                },
                density_millis: 750,
                transform: SurfaceRasterTransform::Normal,
                fidelity: SurfaceContentFidelity::AuthorityRaster,
                damage: Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 600,
                    height: 450,
                }),
            },
        ],
    )
    .unwrap();
    let surface = SurfaceId::new(3, 1);
    let layers = scene.presentation_variant_layers(
        &[CommittedSurfaceState {
            surface,
            committed_generation: 1,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            content,
            damage: Region::empty(),
        }],
        &[surface],
    );
    assert_eq!(
        layers
            .iter()
            .map(|layer| layer.buffer.handle)
            .collect::<Vec<_>>(),
        vec![41, 42]
    );
}

/// The lowerer clips each band, and clips them one at a time.
///
/// This exercises the real lowering rather than a reimplementation of it, which
/// matters because the arithmetic is easy to mirror correctly in a test while
/// the production path does something else. The window here runs far off the
/// right of the scene: its left, top and bottom bands are partly visible and
/// must be trimmed to the scene, and its right band is entirely outside and must
/// disappear rather than reappear against the boundary.
#[test]
fn border_bands_are_clipped_individually_to_the_scene_they_belong_to() {
    let scene = Rect {
        x: 320,
        y: 180,
        width: 1_920,
        height: 1_080,
    };
    let inner = Rect {
        x: 400,
        y: 300,
        width: 4_000,
        height: 400,
    };
    let outer = Rect {
        x: 396,
        y: 296,
        width: 4_008,
        height: 408,
    };

    let mut plan = plan();
    plan.native_size = Size {
        width: 2_560,
        height: 1_440,
    };
    plan.transform.projected_scene = scene;
    plan.layers.clear();
    plan.compositor = vec![HeadCompositorCommand::Border(
        sophia_engine::HeadCompositorBorder {
            node: sophia_engine::CompositorNodeId::SurfaceChrome {
                surface: SurfaceId::new(3, 1),
                role: sophia_engine::SurfaceChromeRole::Frame,
            },
            generation: 3,
            outer,
            inner,
            color: sophia_engine::CompositorRgb8 {
                red: 0,
                green: 80,
                blue: 255,
            },
            clip: scene,
        },
    )];

    let frame = lower_cpu_head_composition_plan(&plan, &[]).unwrap();
    let bands = frame
        .layers
        .iter()
        .filter_map(|layer| match layer {
            LiveOwnedMixedCompositionLayer::Solid { geometry, .. } => Some(*geometry),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        !bands.is_empty(),
        "the visible bands of a straddling window must still be drawn"
    );
    let right_edge = scene.x + scene.width;
    for band in &bands {
        assert!(
            band.x >= scene.x
                && band.y >= scene.y
                && band.x + band.width <= right_edge
                && band.y + band.height <= scene.y + scene.height,
            "band {band:?} escapes the scene {scene:?}"
        );
        // Clipping outer and inner before subtracting them would produce a
        // vertical band flush against the boundary: a border down the side of a
        // window that is only running off the screen.
        assert!(
            !(band.width <= 8 && band.x + band.width == right_edge),
            "band {band:?} is the invented border at the scene's edge"
        );
    }
    // The real right band lies wholly outside and must simply be gone.
    assert!(
        bands.iter().all(|band| band.x < inner.x + inner.width),
        "a band survived from beyond the window's own right edge: {bands:?}"
    );

    // A window lying entirely outside this scene contributes nothing at all.
    //
    // This is the case that separates clipping the bands from clipping the rects
    // they come from. Clipping `outer` and `inner` here leaves both degenerate,
    // and their difference is still positive, so that approach emits a band at
    // the window's original off-screen coordinates while reporting itself as
    // clipped. Only clipping the result removes it.
    let elsewhere = Rect {
        x: 3_000,
        y: 300,
        width: 400,
        height: 400,
    };
    let HeadCompositorCommand::Border(mut border) = plan.compositor[0] else {
        unreachable!("the fixture holds exactly one border command");
    };
    border.inner = elsewhere;
    border.outer = Rect {
        x: elsewhere.x - 4,
        y: elsewhere.y - 4,
        width: elsewhere.width + 8,
        height: elsewhere.height + 8,
    };
    plan.compositor = vec![HeadCompositorCommand::Border(border)];

    let frame = lower_cpu_head_composition_plan(&plan, &[]).unwrap();
    let escaped = frame
        .layers
        .iter()
        .filter_map(|layer| match layer {
            LiveOwnedMixedCompositionLayer::Solid { geometry, .. } => Some(*geometry),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        escaped.is_empty(),
        "a window outside the scene still drew bands: {escaped:?}"
    );
}

/// A lowered frame that Engine proved needs no composition: one client
/// DMA-BUF covering the head exactly, opaque, unscaled, unclipped.
fn direct_frame() -> sophia_renderer_live::LiveOwnedMixedCompositionFrame {
    let fd: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        layers: vec![LiveOwnedMixedCompositionLayer::DmaBuf {
            image_id: LiveRendererImageId::from_raw(7),
            frame: LiveOwnedMultiPlaneDmaBufFrame {
                width: 640,
                height: 480,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                modifier: 0x0100_0000_0000_0001,
                plane_count: 1,
                planes: [
                    Some(LiveOwnedDmaBufPlane {
                        fd,
                        offset: 0,
                        stride: 2_560,
                    }),
                    None,
                    None,
                    None,
                ],
            },
            placement: sophia_renderer_live::LiveCompositionPlacement {
                target: Rect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                },
                clip: None,
                transform: sophia_protocol::Transform::IDENTITY,
                alpha: 1.0,
                sampling: HeadSamplingClass::Exact,
            },
        }],
        output_damage_snapshot: None,
        trace: None,
        direct_scanout: DirectScanoutVerdict::Eligible,
    }
}

const DIRECT_HEAD: Size = Size {
    width: 640,
    height: 480,
};

fn direct_layer_placement(
    frame: &mut sophia_renderer_live::LiveOwnedMixedCompositionFrame,
) -> &mut sophia_renderer_live::LiveCompositionPlacement {
    let LiveOwnedMixedCompositionLayer::DmaBuf { placement, .. } = &mut frame.layers[0] else {
        panic!("direct fixture changed layer kind")
    };
    placement
}

fn direct_layer_frame(
    frame: &mut sophia_renderer_live::LiveOwnedMixedCompositionFrame,
) -> &mut LiveOwnedMultiPlaneDmaBufFrame {
    let LiveOwnedMixedCompositionLayer::DmaBuf { frame, .. } = &mut frame.layers[0] else {
        panic!("direct fixture changed layer kind")
    };
    frame
}

#[test]
fn proven_full_head_client_buffer_becomes_a_plane_descriptor() {
    let frame = direct_frame();
    let buffer = frame.direct_scanout_buffer(DIRECT_HEAD).unwrap();

    // The descriptor describes the client's buffer, not a compositor one: its
    // format, its stride, and its modifier reach AddFB2 unchanged.
    assert_eq!(buffer.descriptor.size, DIRECT_HEAD);
    assert_eq!(
        buffer.descriptor.format,
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
    );
    assert_eq!(buffer.descriptor.pitch, 2_560);
    assert_eq!(buffer.descriptor.plane_count, 1);
    assert_eq!(buffer.descriptor.modifier, Some(0x0100_0000_0000_0001));
    assert!(buffer.descriptor.is_valid_scanout_buffer());
    assert_eq!(buffer.image_id, LiveRendererImageId::from_raw(7));
    assert!(buffer.planes[0].is_some());
    assert!(buffer.planes[1].is_none());

    // Duplicated, not taken: the frame it came from is still whole, which is
    // what lets it be composed instead if the driver refuses the buffer.
    let original = direct_layer_frame(&mut { frame }).planes[0]
        .as_ref()
        .map(|plane| plane.fd.as_raw_fd());
    assert!(original.is_some());
    assert_ne!(
        original.unwrap(),
        buffer.planes[0].as_ref().unwrap().fd.as_raw_fd()
    );
}

#[test]
fn an_unproven_frame_is_refused_however_it_is_shaped() {
    // Structurally identical to the eligible fixture. Only the proof differs,
    // so this is exactly the claim that the backend does not decide
    // eligibility for itself.
    let mut frame = direct_frame();
    frame.direct_scanout = DirectScanoutVerdict::default();
    assert_eq!(
        frame.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        sophia_renderer_live::LiveDirectScanoutRefusal::NotProven(DirectScanoutVerdict::default())
    );
}

#[test]
fn a_proof_that_disagrees_with_the_pixels_refuses_rather_than_flips() {
    // A verdict claiming eligibility over a frame that lowered to chrome as
    // well. Trusting the stamp here would put a client's buffer on the plane
    // and drop everything drawn over it.
    let mut frame = direct_frame();
    frame.layers.push(LiveOwnedMixedCompositionLayer::Solid {
        geometry: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 24,
        },
        color: sophia_engine::CompositorRgb8 {
            red: 0,
            green: 0,
            blue: 0,
        },
    });
    assert_eq!(
        frame.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        sophia_renderer_live::LiveDirectScanoutRefusal::LayerCount(2)
    );
}

#[test]
fn translucency_scaling_clipping_and_partial_cover_each_refuse() {
    use sophia_renderer_live::LiveDirectScanoutRefusal as Refusal;

    let mut translucent = direct_frame();
    direct_layer_placement(&mut translucent).alpha = 0.5;
    assert_eq!(
        translucent.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        Refusal::LayerTranslucent
    );

    let mut resampled = direct_frame();
    direct_layer_placement(&mut resampled).sampling = HeadSamplingClass::Upsampled;
    assert_eq!(
        resampled.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        Refusal::LayerResampled
    );

    let mut clipped = direct_frame();
    direct_layer_placement(&mut clipped).clip = Some(Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 240,
    });
    assert_eq!(
        clipped.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        Refusal::LayerClipped
    );

    let mut partial = direct_frame();
    direct_layer_placement(&mut partial).target = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 400,
    };
    assert_eq!(
        partial.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        Refusal::LayerNotHeadSized
    );

    // A clip that names the whole head clips nothing and is not a refusal.
    let mut whole = direct_frame();
    direct_layer_placement(&mut whole).clip = Some(Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    });
    assert!(whole.direct_scanout_buffer(DIRECT_HEAD).is_ok());
}

#[test]
fn argb_is_offered_at_full_head_and_the_driver_decides() {
    // Alpha is part of the image when something is behind it. Filling the head
    // exactly, nothing is, so it cannot change what is displayed -- and the
    // checks above have already established full head by the time format is
    // asked. The atomic test still decides whether the driver takes it.
    //
    // Refusing ARGB outright left this row with no client on this machine:
    // Kitty allocates ARGB whatever its background opacity says.
    let mut frame = direct_frame();
    direct_layer_frame(&mut frame).format =
        sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888;

    let buffer = frame
        .direct_scanout_buffer(DIRECT_HEAD)
        .expect("an opaque-by-coverage ARGB layer is offered");
    assert_eq!(
        buffer.descriptor.format,
        sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        "the client's own format reaches the descriptor unchanged"
    );
}

#[test]
fn a_format_that_is_neither_xrgb_nor_argb_is_refused() {
    // NV12: a plane may well scan it out, but nothing here has established
    // what its planes mean, and a format nobody classified must not ride along
    // on the strength of covering the head.
    const NV12: u32 = u32::from_le_bytes(*b"NV12");
    let mut frame = direct_frame();
    direct_layer_frame(&mut frame).format = NV12;
    assert_eq!(
        frame.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        sophia_renderer_live::LiveDirectScanoutRefusal::FormatNotOpaque(NV12)
    );
}

#[test]
fn a_buffer_smaller_than_the_head_refuses_even_when_its_placement_covers() {
    // The placement says the layer fills the head, but the buffer behind it
    // does not. A plane scans the buffer, not the placement.
    let mut frame = direct_frame();
    direct_layer_frame(&mut frame).height = 400;
    assert_eq!(
        frame.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        sophia_renderer_live::LiveDirectScanoutRefusal::BufferSizeMismatch
    );
}

#[test]
fn a_cpu_layer_has_no_buffer_to_hand_the_plane() {
    let mut frame = direct_frame();
    frame.layers[0] = LiveOwnedMixedCompositionLayer::Solid {
        geometry: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        color: sophia_engine::CompositorRgb8 {
            red: 1,
            green: 2,
            blue: 3,
        },
    };
    assert_eq!(
        frame.direct_scanout_buffer(DIRECT_HEAD).unwrap_err(),
        sophia_renderer_live::LiveDirectScanoutRefusal::LayerNotDmaBuf
    );
}

#[test]
fn lowering_keeps_the_verdict_only_while_it_stays_true_of_the_frame() {
    // The backend reads the verdict from the frame and never sees the plan --
    // so the verdict must describe the frame, not the plan it came from.
    // Lowering chooses a *source kind*, and a retained recompose substitutes
    // the compositor's promoted snapshot for the client's buffer: that frame
    // is structurally eligible and still cannot go to a plane, because the
    // snapshot is not the client's memory. Carrying Eligible over it made the
    // first overlay withdrawal on hardware read as a proof defect.
    let mut plan = plan();
    plan.direct_scanout = DirectScanoutVerdict::Eligible;
    let lowered = lower_cpu_head_composition_plan(&plan, &[source(42)]).unwrap();
    assert_eq!(
        lowered.direct_scanout,
        DirectScanoutVerdict::CompositionRequired("retained_source"),
        "a CPU-lowered layer is not the client's DMA-BUF, whatever the plan proved"
    );

    // A demotion, never a promotion: an ineligible plan stays ineligible no
    // matter what the lowering picked.
    let mut composed = plan.clone();
    composed.direct_scanout = DirectScanoutVerdict::CompositionRequired("border");
    let lowered = lower_cpu_head_composition_plan(&composed, &[source(42)]).unwrap();
    assert_eq!(
        lowered.direct_scanout,
        DirectScanoutVerdict::CompositionRequired("border")
    );

    // And the default that arrives with no plan behind it composes, so a
    // frame built anywhere else cannot be mistaken for a proven one.
    assert_eq!(
        sophia_renderer_live::LiveOwnedMixedCompositionFrame::default().direct_scanout,
        DirectScanoutVerdict::default()
    );
}

/// The eligible verdict survives lowering exactly when the frame's single
/// layer is the client's own buffer -- the case direct scanout exists for.
#[test]
fn lowering_keeps_eligible_when_the_client_buffer_is_the_frame() {
    let mut plan = plan();
    plan.direct_scanout = DirectScanoutVerdict::Eligible;
    plan.layers[0].source = BufferSource::DmaBuf { handle: 88 };
    let fd: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
    let source = LiveOwnedHeadCompositionSource {
        surface: SurfaceId::new(3, 1),
        source: BufferSource::DmaBuf { handle: 88 },
        kind: LiveOwnedHeadCompositionSourceKind::DmaBuf {
            image_id: LiveRendererImageId::from_raw(10),
            frame: LiveOwnedMultiPlaneDmaBufFrame {
                width: 600,
                height: 450,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                modifier: 0,
                plane_count: 1,
                planes: [
                    Some(LiveOwnedDmaBufPlane {
                        fd,
                        offset: 0,
                        stride: 2_400,
                    }),
                    None,
                    None,
                    None,
                ],
            },
        },
    };

    let lowered = lower_head_composition_plan(&plan, std::slice::from_ref(&source)).unwrap();
    assert_eq!(lowered.direct_scanout, DirectScanoutVerdict::Eligible);
}

/// Which refusals mean Engine's proof and the pixels disagree, and which are
/// the backend answering a question Engine never asked.
///
/// Counting the two together made the first run that produced a legitimate
/// refusal -- a client handing over ARGB -- read as a proof defect, and sent
/// the reader looking for a bug in the eligibility rules instead of at a
/// pixel format.
#[test]
fn a_format_refusal_is_not_a_proof_defect() {
    use sophia_renderer_live::LiveDirectScanoutRefusal as Refusal;

    // Engine proves structure, so these can only be its proof contradicting
    // the pixels that proof was computed from.
    for refusal in [
        Refusal::LayerCount(2),
        Refusal::LayerNotDmaBuf,
        Refusal::LayerResampled,
        Refusal::LayerTranslucent,
        Refusal::LayerTransformed,
        Refusal::LayerOffset,
        Refusal::LayerNotHeadSized,
        Refusal::LayerClipped,
        Refusal::BufferSizeMismatch,
    ] {
        assert!(
            refusal.is_structural_disagreement(),
            "{} should be a proof defect",
            refusal.reduced_name()
        );
    }

    // Engine never looks at a pixel format or a plane layout, so nothing it
    // proved is contradicted by the backend declining one.
    for refusal in [
        Refusal::NotProven(DirectScanoutVerdict::default()),
        Refusal::FormatNotOpaque(0),
        Refusal::PlaneLayoutUnusable,
        Refusal::PlaneFdCloneFailed,
    ] {
        assert!(
            !refusal.is_structural_disagreement(),
            "{} should not be a proof defect",
            refusal.reduced_name()
        );
    }
}

/// A compose refusal is reported as a plan fault, not a target fault.
///
/// `ComposeRefused` reduces to `Degraded` -- the honest answer for "this
/// frame could not be built" -- where `InvalidTarget` claims the render
/// target is wrong. That claim sent one diagnosis at the target while the
/// real fault was a layer naming a renderer image nobody had imported.
#[test]
fn a_compose_refusal_does_not_claim_the_target_is_invalid() {
    use sophia_renderer_live::{
        LiveRendererScanoutBufferExportDetail, LiveRendererScanoutBufferExportStatus,
    };

    assert_eq!(
        LiveRendererScanoutBufferExportDetail::ComposeRefused.status(),
        LiveRendererScanoutBufferExportStatus::Degraded
    );
    assert_ne!(
        LiveRendererScanoutBufferExportDetail::ComposeRefused.status(),
        LiveRendererScanoutBufferExportStatus::InvalidTarget
    );
    // The name reaches the evidence, where a reader can tell the two apart.
    assert_eq!(
        format!(
            "{:?}",
            LiveRendererScanoutBufferExportDetail::ComposeRefused
        ),
        "ComposeRefused"
    );
}

#[test]
fn shell_content_lowers_exact_pixels_and_keeps_the_resource_until_frame_retirement() {
    let (mut store, lease) = content_resource();
    let description = lease.description().clone();
    let mut plan = plan();
    plan.compositor.push(HeadCompositorCommand::ContentImage(
        sophia_engine::HeadCompositorContentImage {
            image: CompositorContentImage {
                node: CompositorNodeId::ShellContent {
                    grant: description.grant,
                    output: plan.output,
                    candidate: 17,
                    surface: 0,
                    placement: 0,
                },
                generation: description.resource.generation,
                output_size_px: plan.native_size,
                geometry_px: Rect {
                    x: 15,
                    y: 9,
                    width: 4,
                    height: 2,
                },
                size_px: Size {
                    width: 4,
                    height: 2,
                },
                stride: 16,
                format: u32::from_le_bytes(*b"AR24"),
                resource: lease,
            },
            geometry: Rect {
                x: 15,
                y: 9,
                width: 4,
                height: 2,
            },
            clip: Rect {
                x: 15,
                y: 9,
                width: 4,
                height: 2,
            },
        },
    ));
    let history = sophia_engine::head_output_damage_snapshot(&plan);
    let mut presented_history =
        sophia_engine::OutputFramePresentationState::new(history.output).unwrap();
    presented_history.queue(history.clone()).unwrap();
    presented_history.mark_submitted().unwrap();
    presented_history.mark_presented().unwrap();
    let lowered = lower_head_composition_plan(
        &plan,
        &[LiveOwnedHeadCompositionSource {
            surface: SurfaceId::new(3, 1),
            source: BufferSource::CpuBuffer { handle: 42 },
            kind: LiveOwnedHeadCompositionSourceKind::Cpu(source(42).buffer.into()),
        }],
    )
    .unwrap();
    let (buffer, placement) = lowered
        .layers
        .iter()
        .find_map(|layer| match layer {
            LiveOwnedMixedCompositionLayer::Cpu { buffer, placement }
                if buffer.handle & (1 << 63) != 0 =>
            {
                Some((buffer, placement))
            }
            _ => None,
        })
        .expect("shell content did not lower to a CPU composition layer");
    assert_eq!(&buffer.bytes[..], &[0x7f; 32]);
    assert_eq!(placement.target.x, 15);
    assert_eq!(placement.clip, Some(placement.target));

    while store.take_event().is_some() {}
    drop(plan);
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire {
                grant: description.grant,
                resource: description.resource,
            },
        )
        .unwrap();
    store.collect();
    assert!(store.take_event().is_none());

    // A queued/upload consumer may retain only the bytes, after dropping the
    // frame metadata. That consumer must still keep the resource accounted.
    let bytes_consumer = buffer.bytes.clone();
    drop(lowered);
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(store.usage().retiring, 32);
    assert_eq!(&bytes_consumer[..], &[0x7f; 32]);
    drop(bytes_consumer);
    store.collect();
    assert!(matches!(
        store.take_event().map(|event| event.record),
        Some(ShellContentRecord::ResourceReleased(released))
            if released.resource == description.resource
    ));
    // Presentation/input and damage facts survive resource retirement, with
    // exact identity intact and no lease hiding in the retained state.
    assert_eq!(
        presented_history
            .presented()
            .unwrap()
            .compositor_display_list,
        history.compositor_display_list
    );
    let identity = history
        .compositor_display_list
        .content_images()
        .next()
        .unwrap();
    assert_eq!(identity.resource, description);
    assert_eq!(store.usage().backing, 0);
    store.collect();
    assert!(store.take_event().is_none());
}
