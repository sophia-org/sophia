use sophia_engine::*;
use sophia_protocol::*;
use sophia_runtime::ContentResourceStore;

fn content_resource() -> sophia_runtime::ContentResourceLease {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let resource = ContentResourceId {
        id: 11,
        generation: 1,
    };
    let mut store = ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
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
    store.lease(grant, resource).unwrap()
}

fn variant(variant: u32, density: u32, source: u64) -> SurfaceContentVariant {
    let size = Size {
        width: i32::try_from((800_u64 * u64::from(density)).div_ceil(1_000)).unwrap(),
        height: i32::try_from((600_u64 * u64::from(density)).div_ceil(1_000)).unwrap(),
    };
    SurfaceContentVariant {
        variant,
        source: BufferSource::CpuBuffer { handle: source },
        pixel_size: size,
        density_millis: density,
        transform: SurfaceRasterTransform::Normal,
        fidelity: SurfaceContentFidelity::AuthorityRaster,
        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }),
    }
}

fn scene() -> OutputSceneSnapshot {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(4, 1);
    let geometry = Rect {
        x: 100,
        y: 120,
        width: 800,
        height: 600,
    };
    OutputSceneSnapshot {
        output,
        scene_generation: 12,
        logical_viewport: Rect {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        },
        surfaces: vec![OutputSceneSurface {
            surface,
            committed_generation: 8,
            geometry,
            clip: geometry,
            opacity_millis: 1_000,
            content: SurfaceContentSet::new(
                Size {
                    width: 800,
                    height: 600,
                },
                vec![variant(1, 1_000, 41), variant(2, 750, 42)],
            )
            .unwrap(),
            damage: Region::single(geometry),
        }],
        display_list: CompositorDisplayList {
            output,
            commands: vec![
                CompositorDisplayCommand::Surface { surface },
                CompositorDisplayCommand::Border(CompositorBorder {
                    node: CompositorNodeId::SurfaceChrome {
                        surface,
                        role: SurfaceChromeRole::Frame,
                    },
                    generation: 3,
                    outer: Rect {
                        x: 96,
                        y: 116,
                        width: 808,
                        height: 608,
                    },
                    inner: geometry,
                    color: CompositorRgb8 {
                        red: 0,
                        green: 80,
                        blue: 255,
                    },
                }),
            ],
        },
        cursor: Some(OutputSceneCursor {
            geometry: Rect {
                x: 640,
                y: 360,
                width: 24,
                height: 24,
            },
            source: BufferSource::CpuBuffer { handle: 99 },
            generation: 5,
        }),
        logical_damage: Region::single(geometry),
        logical_content_checksum: 0x1234,
    }
}

fn target(head: u64, width: i32, height: i32, mapping: OutputHeadMapping) -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(head),
        output: OutputId::from_raw(1),
        target_generation: 7,
        native_size: Size { width, height },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping,
    }
}

#[test]
fn unequal_mirror_heads_select_native_density_variants_from_one_scene() {
    let snapshot = scene();
    let plans = build_output_head_plans(
        &snapshot,
        &[
            target(1, 2560, 1440, OutputHeadMapping::Fit),
            target(2, 1920, 1080, OutputHeadMapping::Fit),
        ],
    )
    .unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].layers[0].variant, 1);
    assert_eq!(
        plans[0].layers[0].requested_sampling,
        HeadSamplingClass::Exact
    );
    assert_eq!(plans[0].layers[0].native_geometry.width, 800);
    assert_eq!(plans[1].layers[0].variant, 2);
    assert_eq!(plans[1].layers[0].density_millis, 750);
    assert_eq!(
        plans[1].layers[0].requested_sampling,
        HeadSamplingClass::Exact
    );
    assert_eq!(plans[1].layers[0].native_geometry.width, 600);
    assert_eq!(
        plans[1].native_size,
        Size {
            width: 1920,
            height: 1080
        }
    );
    assert_eq!(
        plans[0].logical_content_checksum,
        plans[1].logical_content_checksum
    );

    let HeadCompositorCommand::Border(smaller_border) = plans[1].compositor[1] else {
        panic!("border remained semantic until the smaller head plan")
    };
    assert_eq!(smaller_border.outer.width, 606);
    assert_eq!(plans[1].cursor.unwrap().geometry.width, 18);
    let damage = head_output_damage_snapshot(&plans[1]);
    assert_eq!(damage.output.size.width, 1_920);
    assert_eq!(damage.surfaces[0].geometry.width, 600);
    assert_eq!(
        damage.surfaces[0].buffer,
        BufferSource::CpuBuffer { handle: 42 }
    );
}

#[test]
fn sampling_classifies_each_axis_instead_of_collapsing_to_one_density() {
    assert_eq!(
        head_sampling_class(
            Size {
                width: 1_280,
                height: 1_440,
            },
            Size {
                width: 1_920,
                height: 1_080,
            },
        ),
        HeadSamplingClass::Mixed,
    );
    assert_eq!(
        head_sampling_class(
            Size {
                width: 1_920,
                height: 1_080,
            },
            Size {
                width: 1_920,
                height: 1_080,
            },
        ),
        HeadSamplingClass::Exact,
    );
}

#[test]
fn fit_adds_explicit_bars_and_projects_damage_with_filter_footprint() {
    let mut snapshot = scene();
    snapshot.surfaces[0].content = SurfaceContentSet::singleton(
        BufferSource::CpuBuffer { handle: 41 },
        Size {
            width: 800,
            height: 600,
        },
    );
    let plan =
        build_head_composition_plan(&snapshot, target(2, 1920, 1200, OutputHeadMapping::Fit))
            .unwrap();
    assert_eq!(
        plan.transform.projected_scene,
        Rect {
            x: 0,
            y: 60,
            width: 1920,
            height: 1080,
        }
    );
    assert_eq!(
        plan.compositor
            .iter()
            .filter(|command| matches!(command, HeadCompositorCommand::Background(_)))
            .count(),
        2
    );
    assert_eq!(
        plan.layers[0].requested_sampling,
        HeadSamplingClass::Downsampled
    );
    assert!(plan.repaint.rects[0].width > plan.layers[0].native_geometry.width);
}

#[test]
fn cover_crops_without_changing_the_scene_or_variant_identity() {
    let plan =
        build_head_composition_plan(&scene(), target(2, 1920, 1200, OutputHeadMapping::Cover))
            .unwrap();
    assert!(plan.transform.projected_scene.x < 0);
    assert!(
        plan.compositor
            .iter()
            .all(|command| !matches!(command, HeadCompositorCommand::Background(_)))
    );
    assert_eq!(plan.layers[0].variant, 1);
    assert_eq!(
        plan.layers[0].native_clip.x,
        0.max(plan.layers[0].native_geometry.x)
    );
}

#[test]
fn plan_rejects_a_display_list_surface_missing_from_the_snapshot() {
    let mut snapshot = scene();
    snapshot.display_list.commands[0] = CompositorDisplayCommand::Surface {
        surface: SurfaceId::new(99, 1),
    };
    assert_eq!(
        build_head_composition_plan(&snapshot, target(1, 2560, 1440, OutputHeadMapping::Fit),),
        Err(HeadCompositionPlanError::MissingDisplaySurface)
    );
}

#[test]
fn committed_scene_capture_is_one_immutable_fanout_source() {
    let source = scene();
    let committed = source
        .surfaces
        .iter()
        .map(|surface| CommittedSurfaceState {
            surface: surface.surface,
            committed_generation: surface.committed_generation,
            geometry: surface.geometry,
            content: surface.content.clone(),
            damage: surface.damage.clone(),
        })
        .collect::<Vec<_>>();
    let output = HeadlessOutput {
        id: source.output,
        size: Size {
            width: source.logical_viewport.width,
            height: source.logical_viewport.height,
        },
        scale: 1,
    };
    let captured = output_scene_snapshot_from_committed(
        output,
        44,
        &committed,
        source.display_list.clone(),
        source.cursor,
    )
    .unwrap();
    let plans = build_output_head_plans(
        &captured,
        &[
            target(1, 2560, 1440, OutputHeadMapping::Fit),
            target(2, 1920, 1080, OutputHeadMapping::Fit),
        ],
    )
    .unwrap();
    assert_eq!(plans[0].scene_generation, 44);
    assert_eq!(plans[1].scene_generation, 44);
    assert_eq!(
        plans[0].logical_content_checksum,
        plans[1].logical_content_checksum
    );

    let mut changed = committed;
    changed[0].committed_generation += 1;
    let changed = output_scene_snapshot_from_committed(
        output,
        45,
        &changed,
        source.display_list,
        source.cursor,
    )
    .unwrap();
    assert_ne!(
        captured.logical_content_checksum,
        changed.logical_content_checksum
    );
}

#[test]
fn committed_scene_capture_filters_off_view_surfaces_for_extended_outputs() {
    let source = scene();
    let visible = CommittedSurfaceState {
        surface: source.surfaces[0].surface,
        committed_generation: 8,
        geometry: Rect {
            x: 2_700,
            y: 100,
            width: 800,
            height: 600,
        },
        content: source.surfaces[0].content.clone(),
        damage: source.surfaces[0].damage.clone(),
    };
    let hidden = CommittedSurfaceState {
        surface: SurfaceId::new(9, 1),
        committed_generation: 2,
        geometry: Rect {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        },
        content: source.surfaces[0].content.clone(),
        damage: source.surfaces[0].damage.clone(),
    };
    let captured = output_scene_snapshot_from_committed_in_view(
        source.output,
        50,
        Rect {
            x: 2_560,
            y: 0,
            width: 1_920,
            height: 1_080,
        },
        &[visible, hidden],
        CompositorDisplayList {
            output: source.output,
            commands: vec![
                CompositorDisplayCommand::Surface {
                    surface: source.surfaces[0].surface,
                },
                CompositorDisplayCommand::Surface {
                    surface: SurfaceId::new(9, 1),
                },
            ],
        },
        None,
    )
    .unwrap();
    assert_eq!(captured.surfaces.len(), 1);
    assert_eq!(captured.surfaces[0].surface, source.surfaces[0].surface);
    assert_eq!(captured.display_list.commands.len(), 1);
}

#[test]
fn unsupported_target_and_surface_transforms_fail_closed() {
    let snapshot = scene();
    let mut rotated_target = target(1, 2_560, 1_440, OutputHeadMapping::Fit);
    rotated_target.transform = OutputTransform::Rotate90;
    assert_eq!(
        build_head_composition_plan(&snapshot, rotated_target),
        Err(HeadCompositionPlanError::UnsupportedTargetTransform)
    );

    let mut rotated_snapshot = scene();
    rotated_snapshot.surfaces[0].content = SurfaceContentSet::new(
        Size {
            width: 800,
            height: 600,
        },
        vec![SurfaceContentVariant {
            variant: 7,
            source: BufferSource::CpuBuffer { handle: 77 },
            pixel_size: Size {
                width: 600,
                height: 800,
            },
            density_millis: 1_000,
            transform: SurfaceRasterTransform::Rotate90,
            fidelity: SurfaceContentFidelity::AuthorityRaster,
            damage: Region::single(Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 800,
            }),
        }],
    )
    .unwrap();
    assert_eq!(
        build_head_composition_plan(
            &rotated_snapshot,
            target(1, 2_560, 1_440, OutputHeadMapping::Fit)
        ),
        Err(HeadCompositionPlanError::UnavailableSurfaceVariant)
    );
}

/// A scene placed inside a border may not paint into that border.
///
/// Every mirror policy until centre-unscaled projected the scene across the
/// whole head, so "the framebuffer" and "where the scene is" were one rect and
/// clipping to either gave the same answer. Placing a smaller scene inside a
/// larger head separates them, and content bounded by the framebuffer then
/// reaches into the margin that is supposed to hold background alone.
#[test]
fn a_scene_smaller_than_its_head_is_bounded_by_the_scene_not_the_framebuffer() {
    let mut snapshot = scene();
    snapshot.logical_viewport = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    // A window running off the right and bottom of the logical output. It is
    // retained because it partially intersects, which is what makes the clip
    // load-bearing rather than decorative.
    let surface = snapshot.surfaces[0].surface;
    let straddling = Rect {
        x: 1400,
        y: 700,
        width: 800,
        height: 600,
    };
    snapshot.surfaces[0].geometry = straddling;
    snapshot.surfaces[0].clip = straddling;
    snapshot.display_list.commands = vec![
        CompositorDisplayCommand::Surface { surface },
        CompositorDisplayCommand::Border(CompositorBorder {
            node: CompositorNodeId::SurfaceChrome {
                surface,
                role: SurfaceChromeRole::Frame,
            },
            generation: 3,
            outer: Rect {
                x: 1396,
                y: 696,
                width: 808,
                height: 608,
            },
            inner: straddling,
            color: CompositorRgb8 {
                red: 0,
                green: 80,
                blue: 255,
            },
        }),
    ];
    snapshot.cursor = Some(OutputSceneCursor {
        geometry: Rect {
            x: 1910,
            y: 1070,
            width: 24,
            height: 24,
        },
        source: BufferSource::CpuBuffer { handle: 99 },
        generation: 5,
    });

    // 1920x1080 placed unscaled and centred in 2560x1440: the scene occupies
    // x 320..2240 and y 180..1260, and the rest is border.
    let plans = build_output_head_plans(
        &snapshot,
        &[target(1, 2560, 1440, OutputHeadMapping::Exact)],
    )
    .unwrap();
    let plan = &plans[0];
    let painted = plan.transform.projected_scene;
    assert_eq!(painted.x, 320);
    assert_eq!(painted.y, 180);
    assert_eq!(painted.width, 1920);
    assert_eq!(painted.height, 1080);

    let contains = |rect: Rect| {
        rect.is_empty()
            || (rect.x >= painted.x
                && rect.y >= painted.y
                && rect.x + rect.width <= painted.x + painted.width
                && rect.y + rect.height <= painted.y + painted.height)
    };

    assert!(
        contains(plan.layers[0].native_clip),
        "surface clip {:?} escapes the scene {painted:?}",
        plan.layers[0].native_clip
    );
    assert!(
        contains(plan.cursor.unwrap().geometry),
        "cursor {:?} escapes the scene {painted:?}",
        plan.cursor.unwrap().geometry
    );

    let border = plan
        .compositor
        .iter()
        .find_map(|command| match command {
            HeadCompositorCommand::Border(border) => Some(*border),
            _ => None,
        })
        .expect("the straddling window keeps its border command");
    for band in compositor_border_bands(CompositorBorder {
        node: border.node,
        generation: border.generation,
        outer: border.outer,
        inner: border.inner,
        color: border.color,
    }) {
        let clipped = intersect_for_test(band.geometry, border.clip);
        assert!(
            contains(clipped),
            "border band {:?} clipped to {clipped:?} escapes the scene {painted:?}",
            band.geometry
        );
    }
}

/// Clipping the bands, never the rects they are derived from.
///
/// The four bands are the difference between `outer` and `inner`. Clip those two
/// first and the subtraction produces a band along the clip edge -- a bright line
/// down the side of a window that is merely running off the screen, which is
/// worse than the overflow it was meant to fix.
#[test]
fn a_window_cut_off_by_the_scene_edge_does_not_gain_a_border_there() {
    let painted = Rect {
        x: 320,
        y: 180,
        width: 1920,
        height: 1080,
    };
    // A window whose right edge is far past the scene: its real right band is
    // outside and must vanish, not be redrawn at the boundary.
    let inner = Rect {
        x: 400,
        y: 300,
        width: 4000,
        height: 400,
    };
    let outer = Rect {
        x: 396,
        y: 296,
        width: 4008,
        height: 408,
    };
    let color = CompositorRgb8 {
        red: 0,
        green: 80,
        blue: 255,
    };

    let bands = compositor_border_bands(CompositorBorder {
        node: CompositorNodeId::SurfaceChrome {
            surface: SurfaceId::new(4, 1),
            role: SurfaceChromeRole::Frame,
        },
        generation: 3,
        outer,
        inner,
        color,
    });
    let right_edge_of_scene = painted.x + painted.width;
    let survivors = bands
        .iter()
        .map(|band| intersect_for_test(band.geometry, painted))
        .filter(|geometry| !geometry.is_empty())
        .collect::<Vec<_>>();

    assert!(
        !survivors.is_empty(),
        "the top, bottom and left bands are visible and must survive"
    );
    for band in &survivors {
        assert!(
            band.x < right_edge_of_scene,
            "a band was placed at the scene's edge: {band:?}"
        );
        // A vertical band flush against the boundary would be the invented one.
        let vertical = band.width <= 8;
        assert!(
            !(vertical && band.x + band.width == right_edge_of_scene),
            "a vertical band ends exactly on the scene edge, which is the \
             spurious border clipping outer/inner first would create: {band:?}"
        );
    }
}

/// Mirrors the renderer's per-band clip so the engine test can assert on it.
fn intersect_for_test(first: Rect, second: Rect) -> Rect {
    let left = first.x.max(second.x);
    let top = first.y.max(second.y);
    let right = (first.x + first.width).min(second.x + second.width);
    let bottom = (first.y + first.height).min(second.y + second.height);
    Rect {
        x: left,
        y: top,
        width: (right - left).max(0),
        height: (bottom - top).max(0),
    }
}

// Direct-scanout eligibility. Every case is stated against a finished plan,
// because eligibility belongs to the exact frame that reaches the screen.

/// One opaque client DMA-BUF filling the head, drawn by nothing else. This is
/// the only shape the plane may scan out directly, and it is deliberately
/// narrow: everything below is a way of not being it.
fn direct_scanout_scene() -> OutputSceneSnapshot {
    let output = OutputId::from_raw(1);
    let surface = SurfaceId::new(4, 1);
    let full = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1440,
    };
    OutputSceneSnapshot {
        output,
        scene_generation: 12,
        logical_viewport: full,
        surfaces: vec![OutputSceneSurface {
            surface,
            committed_generation: 8,
            geometry: full,
            clip: full,
            opacity_millis: 1_000,
            content: SurfaceContentSet::new(
                Size {
                    width: 2560,
                    height: 1440,
                },
                vec![SurfaceContentVariant {
                    variant: 1,
                    source: BufferSource::DmaBuf { handle: 77 },
                    pixel_size: Size {
                        width: 2560,
                        height: 1440,
                    },
                    density_millis: 1_000,
                    transform: SurfaceRasterTransform::Normal,
                    fidelity: SurfaceContentFidelity::AuthorityRaster,
                    damage: Region::single(full),
                }],
            )
            .unwrap(),
            damage: Region::single(full),
        }],
        display_list: CompositorDisplayList {
            output,
            commands: vec![CompositorDisplayCommand::Surface { surface }],
        },
        cursor: None,
        logical_damage: Region::single(full),
        logical_content_checksum: 0x5678,
    }
}

fn direct_scanout_plan(scene: &OutputSceneSnapshot) -> HeadCompositionPlan {
    build_head_composition_plan(scene, target(1, 2560, 1440, OutputHeadMapping::Fit))
        .expect("the fullscreen scene plans")
}

#[test]
fn one_opaque_client_layer_filling_the_head_is_directly_scannable() {
    let plan = direct_scanout_plan(&direct_scanout_scene());

    assert_eq!(plan.direct_scanout, DirectScanoutVerdict::Eligible);
    assert!(plan.direct_scanout.is_eligible());
    // The letterbox fill is emitted unconditionally and is empty exactly when
    // the scene already covers the framebuffer, which is the only case that
    // could be eligible anyway.
    assert!(!plan.compositor.is_empty());
}

#[test]
fn chrome_over_a_fullscreen_client_requires_composition() {
    // The shape an ordinary Hagia desktop presents: the indicator strip is
    // drawn above the fullscreen window on purpose, and the guide asserts it
    // stays visible. Such a frame is composed, and this is why.
    let mut scene = direct_scanout_scene();
    let surface = SurfaceId::new(4, 1);
    // A focus border here; the production instance is the indicator strip,
    // which lowers to the same class of drawn primitive and is classified by
    // the same arm.
    scene
        .display_list
        .commands
        .push(CompositorDisplayCommand::Border(CompositorBorder {
            node: CompositorNodeId::SurfaceChrome {
                surface,
                role: SurfaceChromeRole::Frame,
            },
            generation: 3,
            outer: Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1440,
            },
            inner: Rect {
                x: 4,
                y: 4,
                width: 2552,
                height: 1432,
            },
            color: CompositorRgb8 {
                red: 0,
                green: 80,
                blue: 255,
            },
        }));

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        // Named, not merely "something painted". Writing this test the name
        // corrected the guess: the chrome here is a border, and a border and
        // an indicator strip are different problems to go and look at.
        DirectScanoutVerdict::CompositionRequired("border")
    );
}

#[test]
fn a_letterboxed_client_does_not_cover_its_head() {
    // The same client on a head whose aspect it does not fill. The plan does
    // carry letterbox rects the plane would not produce, but the layer is
    // centred inside them and so is not at the head's origin, which is the
    // first and most precise thing to say about it.
    let scene = direct_scanout_scene();
    let plan = build_head_composition_plan(&scene, target(1, 2560, 1600, OutputHeadMapping::Fit))
        .expect("the letterboxed scene plans");

    assert_eq!(plan.direct_scanout, DirectScanoutVerdict::LayerOffset);
    assert!(
        plan.compositor.iter().any(|command| matches!(
            command,
            HeadCompositorCommand::Background(rect)
                if rect.geometry.width != 0 && rect.geometry.height != 0
        )),
        "the letterbox rects this frame would need are present in the plan"
    );
}

#[test]
fn a_client_at_the_origin_but_short_of_the_head_is_not_head_sized() {
    // The other half of the split: this one starts where the head starts, so
    // it is not offset, and stops before the head ends. Without its own test
    // the size arm could be deleted and every other case here would still
    // pass, because the offset arm answers first for anything centred.
    let mut scene = direct_scanout_scene();
    scene.surfaces[0].geometry = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1400,
    };
    scene.surfaces[0].clip = scene.surfaces[0].geometry;
    // Its buffer shrinks with it, so the layer is not resampled -- resampling
    // answers before geometry does, and this test is about geometry.
    let shortened = Size {
        width: 2560,
        height: 1400,
    };
    scene.surfaces[0].content = SurfaceContentSet::new(
        shortened,
        vec![SurfaceContentVariant {
            variant: 1,
            source: BufferSource::DmaBuf { handle: 77 },
            pixel_size: shortened,
            density_millis: 1_000,
            transform: SurfaceRasterTransform::Normal,
            fidelity: SurfaceContentFidelity::AuthorityRaster,
            damage: Region::single(scene.surfaces[0].geometry),
        }],
    )
    .expect("the shortened content set is well formed");

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        DirectScanoutVerdict::LayerNotHeadSized
    );
}

#[test]
fn a_cpu_client_buffer_has_no_framebuffer_to_scan_out() {
    let mut scene = direct_scanout_scene();
    scene.surfaces[0].content = SurfaceContentSet::new(
        Size {
            width: 2560,
            height: 1440,
        },
        vec![SurfaceContentVariant {
            variant: 1,
            source: BufferSource::CpuBuffer { handle: 41 },
            pixel_size: Size {
                width: 2560,
                height: 1440,
            },
            density_millis: 1_000,
            transform: SurfaceRasterTransform::Normal,
            fidelity: SurfaceContentFidelity::AuthorityRaster,
            damage: Region::single(scene.logical_viewport),
        }],
    )
    .unwrap();

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        DirectScanoutVerdict::LayerNotDmaBuf
    );
}

#[test]
fn a_translucent_client_shows_what_is_behind_it() {
    let mut scene = direct_scanout_scene();
    scene.surfaces[0].opacity_millis = 800;

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        DirectScanoutVerdict::LayerTranslucent
    );
}

#[test]
fn a_composed_cursor_is_part_of_the_image() {
    // The hardware cursor rides its own plane and never appears in a plan;
    // a cursor that reaches the plan is one the compositor would draw.
    let mut scene = direct_scanout_scene();
    scene.cursor = Some(OutputSceneCursor {
        geometry: Rect {
            x: 640,
            y: 360,
            width: 24,
            height: 24,
        },
        source: BufferSource::CpuBuffer { handle: 99 },
        generation: 5,
    });

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        DirectScanoutVerdict::ComposedCursor
    );
}

#[test]
fn an_empty_head_scans_out_nothing() {
    let mut scene = direct_scanout_scene();
    scene.surfaces.clear();
    scene.display_list.commands.clear();

    assert_eq!(
        direct_scanout_plan(&scene).direct_scanout,
        DirectScanoutVerdict::LayerCount(0)
    );
}

#[test]
fn shell_content_keeps_physical_geometry_and_forces_composition() {
    let mut scene = direct_scanout_scene();
    scene
        .display_list
        .commands
        .push(CompositorDisplayCommand::ContentImage(
            CompositorContentImage {
                node: CompositorNodeId::ShellContent {
                    grant: ContentGrant {
                        connection_epoch: 7,
                        content_grant_epoch: 9,
                    },
                    output: scene.output,
                    candidate: 17,
                    surface: 0,
                    placement: 0,
                },
                generation: 1,
                output_size_px: Size {
                    width: 2_560,
                    height: 1_440,
                },
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
                resource: content_resource(),
            },
        ));

    let plan = direct_scanout_plan(&scene);
    assert_eq!(
        plan.direct_scanout,
        DirectScanoutVerdict::CompositionRequired("shell_content")
    );
    let content = plan
        .compositor
        .iter()
        .find_map(|command| match command {
            HeadCompositorCommand::ContentImage(content) => Some(content),
            _ => None,
        })
        .expect("shell content was projected into the head plan");
    assert_eq!(
        content.geometry,
        Rect {
            x: 15,
            y: 9,
            width: 4,
            height: 2,
        }
    );
    assert_eq!(content.clip, content.geometry);
}
