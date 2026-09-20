use super::*;

fn runtime() -> LiveProductionVisualRuntime {
    LiveProductionVisualRuntime::new(
        &[
            HeadlessOutput {
                id: OutputId::from_raw(1),
                size: Size {
                    width: 2560,
                    height: 1440,
                },
                scale: 1,
            },
            HeadlessOutput {
                id: OutputId::from_raw(2),
                size: Size {
                    width: 1920,
                    height: 1080,
                },
                scale: 1,
            },
        ],
        None,
    )
    .unwrap()
}

fn layer(surface: SurfaceId) -> LayerSnapshot {
    LayerSnapshot {
        input_region: None,
        surface,
        authority_local_id: None,
        output: None,
        namespace: None,
        stack_rank: 0,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 2560,
            height: 32,
        },
        source_size: Size {
            width: 2560,
            height: 32,
        },
        source: BufferSource::CpuBuffer { handle: 1 },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        translation: None,
    }
}

#[test]
fn panel_route_reaches_display_list_and_is_withdrawn_with_session_visibility() {
    let panel = SurfaceId::new(1, 1);
    let unplaced = SurfaceId::new(2, 1);
    let mut runtime = runtime();
    let layers = [layer(panel), layer(unplaced)];
    let committed = layers
        .iter()
        .map(|l| {
            CommittedSurfaceState::with_source(
                l.surface,
                1,
                l.geometry,
                l.source,
                l.source_size,
                Region::empty(),
            )
        })
        .collect::<Vec<_>>();
    assert!(runtime.apply_presentation_layout(&layers, &[panel]));
    for output in [OutputId::from_raw(1), OutputId::from_raw(2)] {
        let list = runtime
            .display_list_for_output(
                output,
                runtime.outputs.logical_viewport(output).unwrap(),
                &committed,
                &[panel, unplaced],
            )
            .unwrap();
        assert!(
            list.commands
                .contains(&CompositorDisplayCommand::Surface { surface: panel })
        );
        assert!(
            !list
                .commands
                .contains(&CompositorDisplayCommand::Surface { surface: unplaced })
        );
    }
    assert!(
        runtime.apply_presentation_layout(&layers, &[]),
        "routing withdrawal invalidates the projection even with the same layer order"
    );
    assert!(runtime.geometry_routed_surfaces.is_empty());
    assert!(!runtime.apply_presentation_layout(&layers, &[]));
    runtime.apply_presentation_layout(&[], &[panel]);
    assert!(
        runtime.geometry_routed_surfaces.is_empty(),
        "stale route cannot resurrect an unmapped panel"
    );
}

#[test]
fn cpu_frame_orders_keep_offscreen_columns_on_their_assigned_output() {
    let column = SurfaceId::new(6291460, 1);
    let panel = SurfaceId::new(2, 1);
    let mut runtime = runtime();
    let mut scrolled = layer(column);
    scrolled.output = Some(OutputId::from_raw(1));
    scrolled.geometry = Rect {
        x: 3821,
        y: 41,
        width: 1258,
        height: 1390,
    };
    // This column is outside output 1 but geometrically intersects output 2.
    // A frontend-positioned panel, in contrast, may span both outputs.
    runtime.apply_presentation_layout(&[scrolled, layer(panel)], &[panel]);
    let orders = runtime.presentation_orders_by_output();
    assert_eq!(orders[&OutputId::from_raw(1)], vec![column, panel]);
    assert_eq!(orders[&OutputId::from_raw(2)], vec![panel]);
}

#[test]
fn reload_repaints_scrolling_columns_without_a_new_client_frame() {
    for motion in [true, false] {
        let mut runtime = runtime();
        runtime.set_transitions_enabled(motion);
        let output = OutputId::from_raw(1);
        let mut column = layer(SurfaceId::new(31, 1));
        column.output = Some(output);
        column.source = BufferSource::DmaBuf { handle: 31 };
        column.geometry = Rect {
            x: 1900,
            y: 40,
            width: 1260,
            height: 1392,
        };
        column.source_size = Size {
            width: column.geometry.width,
            height: column.geometry.height,
        };
        column.translation = Some(LayerTranslation {
            connection_epoch: 1,
            group: 1,
            x: 0,
            y: 0,
        });
        // Keep exactly the same committed client pixels through both reloads.
        let committed = [CommittedSurfaceState::from_layer_snapshot(&column)];
        runtime.apply_presentation_layout(std::slice::from_ref(&column), &[]);
        let before = retained_test_head_plan(&runtime, output, &committed);
        assert_eq!(before.layers.len(), 1);
        assert_eq!(before.layers[0].native_clip.width, 660);

        // A one-pixel gap increase changes the requested size. Until matching
        // pixels arrive, presentation must preserve the old committed extent.
        column.geometry = Rect {
            x: 3818,
            y: 41,
            width: 1259,
            height: 1390,
        };
        column.translation.as_mut().unwrap().connection_epoch = 2;
        let changed = runtime.apply_presentation_layout(std::slice::from_ref(&column), &[]);
        assert!(!runtime.translations.active(runtime.translation_time()));
        assert!(
            live_production_retained_projection_admitted(changed, false, true),
            "a stationary reload must request a retained repaint without client damage"
        );
        let awaiting_pixels = retained_test_head_plan(&runtime, output, &committed);
        assert_eq!(
            awaiting_pixels.layers[0].native_geometry,
            before.layers[0].native_geometry
        );

        // Restoring the original gaps makes the retained size usable again,
        // but the column's new position is outside its assigned output.
        column.geometry = Rect {
            x: 3820,
            ..committed[0].geometry
        };
        column.translation.as_mut().unwrap().connection_epoch = 3;
        let changed = runtime.apply_presentation_layout(std::slice::from_ref(&column), &[]);
        assert!(live_production_retained_projection_admitted(
            changed, false, true
        ));
        assert!(!reduce_live_production_frame_defer(true, changed, false));
        let after = retained_test_head_plan(&runtime, output, &committed);
        assert!(
            after.layers.is_empty(),
            "the offscreen column must leave output 1"
        );
        let damage = output_frame_damage(
            Some(&head_output_damage_snapshot(&before)),
            &head_output_damage_snapshot(&after),
        )
        .unwrap();
        let vacated = before.layers[0].native_clip;
        assert!(
            damage.rects.iter().any(|rect| {
                rect.x <= vacated.x
                    && rect.y <= vacated.y
                    && rect.x + rect.width >= vacated.x + vacated.width
                    && rect.y + rect.height >= vacated.y + vacated.height
            }),
            "repaint must cover the vacated region: {damage:?}"
        );
        let neighbor = retained_test_head_plan(&runtime, OutputId::from_raw(2), &committed);
        assert!(
            neighbor.layers.is_empty(),
            "the column must not leak onto output 2"
        );
        assert!(!runtime.apply_presentation_layout(std::slice::from_ref(&column), &[]));
        assert!(!runtime.frame_deadlines_pending());
    }
}

fn retained_test_head_plan(
    runtime: &LiveProductionVisualRuntime,
    output: OutputId,
    committed: &[CommittedSurfaceState],
) -> HeadCompositionPlan {
    let viewport = runtime.outputs.logical_viewport(output).unwrap();
    let list = runtime
        .display_list_for_output(output, viewport, committed, &runtime.presentation_order)
        .unwrap();
    let (presented, list) =
        runtime
            .translations
            .project(output, committed, list, runtime.translation_time());
    let snapshot =
        output_scene_snapshot_from_committed_in_view(output, 1, viewport, &presented, list, None)
            .unwrap();
    build_output_head_plans(
        &snapshot,
        &[HeadRenderTarget {
            head: RenderHeadId::from_raw(output.raw()),
            output,
            target_generation: 1,
            native_size: Size {
                width: viewport.width,
                height: viewport.height,
            },
            scale: 1,
            refresh_millihz: 60_000,
            transform: OutputTransform::Normal,
            mapping: OutputHeadMapping::Fit,
        }],
    )
    .unwrap()
    .remove(0)
}

#[test]
fn retained_repaints_wait_for_the_exact_first_present_to_retire() {
    let mut runtime = runtime();
    let surface = SurfaceId::new(3, 1);
    let transaction = TransactionId::from_raw(23649);
    let output = OutputId::from_raw(1);
    let geometry = Rect {
        x: 1919,
        y: 41,
        width: 1258,
        height: 1390,
    };
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let candidate = SurfaceTransaction {
        input_region: None,
        transaction,
        surface,
        authority: AuthorityKind::SophiaX,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 28 }, size),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let prepared = runtime.production.prepare_present_transaction(&candidate);
    assert!(prepared.is_ready());
    let present = LiveProductionSubmittedPresent::new(
        BTreeMap::from([(output, LiveProductionNativeFrameId::from_raw(3045))]),
        output,
        candidate.key(),
        transaction,
        surface,
        prepared,
        LiveRetainedRendererImageLayer {
            image_id: LiveRendererImageId::from_raw(28),
            size,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            placement: LiveCompositionPlacement {
                target: geometry,
                clip: None,
                transform: Transform::IDENTITY,
                alpha: 1.0,
                sampling: HeadSamplingClass::Exact,
            },
        },
    )
    .unwrap();
    assert!(!runtime.native_publication_blocked());
    runtime.present_scheduler.mark_rendering(present);
    assert!(
        runtime.native_publication_blocked(),
        "first-frame render owns its retirement proof before KMS submission"
    );
    runtime
        .present_scheduler
        .mark_output_submitted(output)
        .unwrap();
    assert!(runtime.native_publication_blocked());
    runtime
        .present_scheduler
        .mark_output_retired(LiveProductionPageFlipRetirement {
            output,
            ust: 1000,
            msc: 1,
        })
        .unwrap();
    assert!(
        runtime.native_publication_blocked(),
        "retirement must be settled before repainting"
    );
    runtime.present_scheduler.take_submitted().unwrap();
    assert!(!runtime.native_publication_blocked());
}

/// The other two clauses of the same guard, which the Present path above does
/// not reach. Both are states in which the native path already owns the head a
/// composed frame would be queued into.
#[test]
fn a_suspended_native_path_blocks_publication() {
    let mut runtime = runtime();
    assert!(!runtime.native_publication_blocked());

    runtime.native_suspended = true;
    assert!(
        runtime.native_publication_blocked(),
        "a suspended native path has no head to accept a composed frame"
    );

    runtime.native_suspended = false;
    assert!(!runtime.native_publication_blocked());
}

#[test]
fn a_bound_software_frame_blocks_publication() {
    let mut runtime = runtime();
    let output = OutputId::from_raw(1);
    let frame = LiveProductionNativeFrameId::from_raw(4096);
    assert!(!runtime.native_publication_blocked());

    runtime.software_present_frames_bound.insert(
        frame,
        software_present::LiveProductionSoftwarePresentBinding {
            frames: BTreeMap::from([(output, frame)]),
            clock_output: output,
            output_cohort: TransactionPresentationCohort::new(
                TransactionId::from_raw(23650),
                [output],
            )
            .unwrap(),
            retirements: BTreeMap::new(),
            submissions: Vec::new(),
            phase: LiveProductionSoftwarePresentFramePhase::Pending,
        },
    );
    assert!(
        runtime.native_publication_blocked(),
        "a bound software frame already owns the head it will present into"
    );

    runtime.software_present_frames_bound.clear();
    assert!(!runtime.native_publication_blocked());
}

#[test]
fn policy_status_does_not_create_a_builtin_bar_or_capture_panel_input() {
    let mut runtime = runtime();
    let publication = PolicyIndicatorPublication {
        generation: 1,
        connection_epoch: Some(1),
        indicators: vec![PolicyProjectionIndicator {
            output: OutputId::from_raw(1),
            slot: 0,
            indicator: 1,
            action: Some(WmActionId::from_raw(1)),
            state_bits: 1,
            label: "dev".into(),
        }],
        output_statuses: Vec::new(),
        tab_groups: Vec::new(),
    };
    runtime.set_indicator_publication(Some(publication.clone()));
    for output in [OutputId::from_raw(1), OutputId::from_raw(2)] {
        let list = runtime
            .display_list_for_output(
                output,
                runtime.outputs.logical_viewport(output).unwrap(),
                &[],
                &[],
            )
            .unwrap();
        assert!(
            !list
                .commands
                .iter()
                .any(|command| matches!(command, CompositorDisplayCommand::IndicatorStrip(_)))
        );
    }
    runtime.publish_committed_input_layers();
    assert!(
        runtime
            .input_projections
            .iter()
            .all(|projection| projection.chrome_targets.is_empty()
                && projection.chrome_occlusion.is_none())
    );
    assert_eq!(
        runtime.indicator_publication,
        Some(publication),
        "committed policy descriptors remain available to shell projections"
    );
}

fn window_layer(
    surface: SurfaceId,
    rank: u32,
    geometry: Rect,
    output: Option<OutputId>,
) -> LayerSnapshot {
    LayerSnapshot {
        stack_rank: rank,
        geometry,
        output,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        ..layer(surface)
    }
}

/// A repaint's raise orders the stack and never becomes the chrome focus.
#[test]
fn a_repaint_raise_never_becomes_the_chrome_focus() {
    let focused = SurfaceId::new(1, 1);
    let other = SurfaceId::new(2, 1);
    let popup = SurfaceId::new(3, 1);
    let mut runtime = runtime();
    let layers = [
        // Managed windows are placed on an output; the popup is client-positioned
        // and routes by geometry instead, exactly as the session distinguishes them.
        window_layer(
            focused,
            0,
            Rect {
                x: 100,
                y: 100,
                width: 800,
                height: 600,
            },
            Some(OutputId::from_raw(1)),
        ),
        window_layer(
            other,
            1,
            Rect {
                x: 300,
                y: 150,
                width: 640,
                height: 480,
            },
            Some(OutputId::from_raw(1)),
        ),
        window_layer(
            popup,
            2,
            Rect {
                x: 200,
                y: 200,
                width: 120,
                height: 80,
            },
            None,
        ),
    ];
    let committed = layers
        .iter()
        .map(|l| {
            CommittedSurfaceState::with_source(
                l.surface,
                1,
                l.geometry,
                l.source,
                l.source_size,
                Region::empty(),
            )
        })
        .collect::<Vec<_>>();
    runtime.set_surface_chrome_style(SurfaceChromeStyle {
        frame: SurfaceFrameStyle {
            width: 4,
            ..SurfaceFrameStyle::default()
        },
        ..SurfaceChromeStyle::default()
    });
    // The popup is client-positioned, so the session leaves it out of the chrome set.
    assert!(runtime.apply_presentation_layout(&layers, &[popup]));
    assert!(runtime.set_chrome_surfaces(&[focused, other]));

    let repaint = |runtime: &mut LiveProductionVisualRuntime, raised, focus| {
        let list = runtime.prepare_repaint(&committed, raised, focus).unwrap();
        let resolved = runtime.focused_surface();
        let summary = compositor_chrome_summary(&list, resolved);
        assert!(
            summary.frames > 0,
            "the windows must be framed for this regression to mean anything"
        );
        (resolved, summary.focused_frames)
    };

    assert_eq!(
        repaint(&mut runtime, None, Some(focused)),
        (Some(focused), 1)
    );
    // A frameless popup raised over the focused window.
    assert_eq!(
        repaint(&mut runtime, Some(popup), Some(focused)),
        (Some(focused), 1),
    );
    // A different FRAMED window raised: still not a focus change.
    assert_eq!(
        repaint(&mut runtime, Some(other), Some(focused)),
        (Some(focused), 1),
    );
    // No raise at all.
    assert_eq!(
        repaint(&mut runtime, None, Some(focused)),
        (Some(focused), 1)
    );

    // Separation, not suppression: the raise must still order the stack while
    // the focus stays put. Without it the top of the stack is the popup.
    let stack = |runtime: &mut LiveProductionVisualRuntime, raised| {
        let list = runtime
            .prepare_repaint(&committed, raised, Some(focused))
            .unwrap();
        list.commands
            .iter()
            .filter_map(|command| match command {
                CompositorDisplayCommand::Surface { surface } => Some(*surface),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(stack(&mut runtime, None).last(), Some(&popup));
    assert_eq!(
        stack(&mut runtime, Some(other)).last(),
        Some(&other),
        "a raised framed window must still reach the top of the stack"
    );
    assert_eq!(
        runtime.focused_surface(),
        Some(focused),
        "and raising it must not have moved the focus"
    );
    // An explicit focus that owns no chrome leaves the held focus standing.
    assert_eq!(
        repaint(&mut runtime, None, Some(popup)),
        (Some(focused), 1),
        "an explicit focus is still normalized against the chrome set"
    );
    // An explicit absence of focus clears it, even while something is raised.
    assert_eq!(repaint(&mut runtime, Some(focused), None), (None, 0));
}

/// Two production paths, one frame. While a Present owns the scanout, the CPU
/// production turn and the cadence repaint observe the in-flight candidate the
/// heads are showing, never the committed set it was prepared against.
///
/// This is the live defect of 2026-09-19: a surface the layout had moved to the
/// other output while its resize epoch never visually completed kept its old
/// committed rectangle, and every second turn framed it there, in the gaps
/// around the window that had taken its place.
#[test]
fn chrome_follows_the_displayed_view_while_a_present_is_in_flight() {
    let primary = OutputId::from_raw(1);
    let outputs = [
        HeadlessOutput {
            id: primary,
            size: Size {
                width: 2560,
                height: 1440,
            },
            scale: 1,
        },
        HeadlessOutput {
            id: OutputId::from_raw(2),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        },
    ];
    let window = SurfaceId::new(1, 1);
    let neighbour = SurfaceId::new(2, 1);
    // Where the window was committed, and where the layout moved it. The
    // session assigns managed windows to the primary output and lets the
    // second head show them by geometry, so both rectangles live in the
    // primary's list whichever head paints them.
    let old = Rect {
        x: 8,
        y: 40,
        width: 640,
        height: 480,
    };
    let new = Rect {
        x: 1300,
        y: 40,
        width: 320,
        height: 240,
    };
    let neighbour_geometry = Rect {
        x: 1300,
        y: 300,
        width: 320,
        height: 240,
    };
    let size_of = |geometry: Rect| Size {
        width: geometry.width,
        height: geometry.height,
    };

    let mut runtime = runtime();
    runtime.set_surface_chrome_style(SurfaceChromeStyle {
        frame: SurfaceFrameStyle {
            width: 4,
            ..SurfaceFrameStyle::default()
        },
        ..SurfaceChromeStyle::default()
    });
    let mut scene = LiveProductionCpuScene::new(outputs[0].size);
    let transaction = TransactionId::from_raw(500);
    let committed_transaction = |surface, geometry: Rect, handle| SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size_of(geometry),
        content: SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle },
            size_of(geometry),
        ),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let cpu_buffer = |surface, handle, geometry: Rect| {
        LiveProductionCpuBufferUpdate::new(
            transaction,
            surface,
            LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle,
                size: size_of(geometry),
                stride: u32::try_from(geometry.width * 4).unwrap(),
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![
                    0xff;
                    usize::try_from(geometry.width * geometry.height * 4)
                        .unwrap()
                ]),
            }),
        )
    };
    let batch = LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![
                committed_transaction(window, old, 11),
                committed_transaction(neighbour, neighbour_geometry, 12),
            ],
            cpu_buffer_updates: vec![
                cpu_buffer(window, 11, old),
                cpu_buffer(neighbour, 12, neighbour_geometry),
            ],
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let empty = LiveProductionAuthorityBatch {
        groups: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let placed = |surface, rank, geometry, output, handle| LayerSnapshot {
        source: BufferSource::CpuBuffer { handle },
        ..window_layer(surface, rank, geometry, Some(output))
    };
    let layout_before = [
        placed(window, 0, old, primary, 11),
        placed(neighbour, 1, neighbour_geometry, primary, 12),
    ];
    let layout_after = [
        placed(window, 0, new, primary, 11),
        placed(neighbour, 1, neighbour_geometry, primary, 12),
    ];
    let cycle = |runtime: &mut LiveProductionVisualRuntime,
                 scene: &mut LiveProductionCpuScene,
                 batch: &LiveProductionAuthorityBatch,
                 layout: &[LayerSnapshot]| {
        runtime
            .run_cpu_production_cycle(LiveProductionCycleRequest {
                batch,
                scene,
                raised_surface: None,
                focused_surface: Some(window),
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &outputs,
                native_scanout: None,
                wm_update: None,
                presentation_layout: layout,
                geometry_routed_surfaces: &[],
                chrome_surfaces: &[window, neighbour],
                indicator_publication: None,
                staged_cpu_buffer_handles: &[],
            })
            .unwrap();
        runtime.take_chrome_set_observation()
    };

    let first = cycle(&mut runtime, &mut scene, &batch, &layout_before)
        .expect("the first composition frames both windows");
    assert_eq!(first.frames, 2);
    assert_eq!(first.focused_frames, 1);
    // The first change's companions are not what this test is about.
    let _ = runtime.take_chrome_frame_observations();

    // The layout moves the window. Its next Present, the pixels at the new
    // size, is prepared against the committed set, which still holds the old
    // rectangle, and then owns the scanout. A move alone changes neither the
    // order nor the routing, so the layout reports nothing to repaint.
    assert!(!runtime.apply_presentation_layout(&layout_after, &[]));
    let present_transaction = TransactionId::from_raw(501);
    let candidate = SurfaceTransaction {
        input_region: None,
        transaction: present_transaction,
        authority: AuthorityKind::SophiaX,
        surface: window,
        namespace: None,
        target_geometry: new,
        presentation_extent: size_of(new),
        content: SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 28 }, size_of(new)),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let prepared = runtime.production.prepare_present_transaction(&candidate);
    assert!(prepared.is_ready());
    let present = LiveProductionSubmittedPresent::new(
        BTreeMap::from([(primary, LiveProductionNativeFrameId::from_raw(7))]),
        primary,
        candidate.key(),
        present_transaction,
        window,
        prepared,
        LiveRetainedRendererImageLayer {
            image_id: LiveRendererImageId::from_raw(28),
            size: size_of(new),
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            placement: LiveCompositionPlacement {
                target: new,
                clip: None,
                transform: Transform::IDENTITY,
                alpha: 1.0,
                sampling: HeadSamplingClass::Exact,
            },
        },
    )
    .unwrap();
    runtime.present_scheduler.mark_rendering(present);
    assert!(runtime.present_scheduler.has_in_flight());

    // The Present turn frames the candidate: the window at its new place.
    let view = runtime.displayed_surface_view().to_vec();
    runtime
        .record_focus_ring_observation(&view, LiveChromeObservationSource::Present, false)
        .unwrap();
    let presented = runtime
        .take_chrome_set_observation()
        .expect("the moved window changes the chrome");
    assert_eq!(presented.frames, 2);
    assert_eq!(presented.focused_frames, 1);
    let frames = runtime.take_chrome_frame_observations();
    assert_eq!(frames.len(), 2, "one companion per framed surface");
    let moved = frames
        .iter()
        .find(|frame| frame.surface == window)
        .expect("the window's frame is named");
    assert_eq!(moved.geometry, new);
    assert!(moved.focused);
    assert!(moved.in_flight);
    assert_eq!(moved.source, LiveChromeObservationSource::Present);
    assert_eq!(moved.generation, presented.generation);

    // The committed set still frames the window where it was. Observing it
    // would flip the generation back, which is the alternation this pins.
    let stale = runtime
        .display_list(
            runtime.production.committed_surfaces(),
            &runtime.presentation_order,
        )
        .unwrap();
    assert_ne!(
        compositor_chrome_summary(&stale, runtime.focused_surface).generation,
        presented.generation,
        "the committed rectangle must differ, or the control proves nothing"
    );

    // Every following turn, of either kind, frames what the heads show.
    for _ in 0..4 {
        assert_eq!(
            runtime.displayed_surface_view(),
            runtime.present_scheduler.in_flight_candidate().unwrap()
        );
        assert!(
            cycle(&mut runtime, &mut scene, &empty, &layout_after).is_none(),
            "a CPU production turn must not re-frame the committed rectangle"
        );
        let view = runtime.displayed_surface_view().to_vec();
        runtime
            .record_focus_ring_observation(&view, LiveChromeObservationSource::Repaint, true)
            .unwrap();
        assert!(
            runtime.take_chrome_set_observation().is_none(),
            "a cadence repaint must not re-frame the committed rectangle"
        );
        assert!(runtime.take_chrome_frame_observations().is_empty());
    }
}

/// The software frame frames the surfaces the session authorised, the same set
/// the head frames and the chrome observation frame. It used to frame every
/// surface in the presentation order, popups included.
#[test]
fn the_software_frame_frames_only_the_authorised_surfaces() {
    let primary = OutputId::from_raw(1);
    let outputs = [HeadlessOutput {
        id: primary,
        size: Size {
            width: 2560,
            height: 1440,
        },
        scale: 1,
    }];
    let framed = SurfaceId::new(1, 1);
    let popup = SurfaceId::new(2, 1);
    let framed_geometry = Rect {
        x: 100,
        y: 100,
        width: 400,
        height: 300,
    };
    let popup_geometry = Rect {
        x: 1000,
        y: 500,
        width: 200,
        height: 120,
    };
    let size_of = |geometry: Rect| Size {
        width: geometry.width,
        height: geometry.height,
    };
    let transaction = TransactionId::from_raw(600);
    let committed_transaction = |surface, geometry: Rect, handle| SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size_of(geometry),
        content: SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle },
            size_of(geometry),
        ),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let cpu_buffer = |surface, handle, geometry: Rect| {
        LiveProductionCpuBufferUpdate::new(
            transaction,
            surface,
            LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle,
                size: size_of(geometry),
                stride: u32::try_from(geometry.width * 4).unwrap(),
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![
                    0xff;
                    usize::try_from(geometry.width * geometry.height * 4)
                        .unwrap()
                ]),
            }),
        )
    };
    let batch = LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![
                committed_transaction(framed, framed_geometry, 21),
                committed_transaction(popup, popup_geometry, 22),
            ],
            cpu_buffer_updates: vec![
                cpu_buffer(framed, 21, framed_geometry),
                cpu_buffer(popup, 22, popup_geometry),
            ],
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let layout = [
        LayerSnapshot {
            source: BufferSource::CpuBuffer { handle: 21 },
            ..window_layer(framed, 0, framed_geometry, Some(primary))
        },
        LayerSnapshot {
            source: BufferSource::CpuBuffer { handle: 22 },
            ..window_layer(popup, 1, popup_geometry, None)
        },
    ];
    let compose = |chrome_surfaces: &[SurfaceId]| {
        let mut runtime = runtime();
        runtime.set_surface_chrome_style(SurfaceChromeStyle {
            frame: SurfaceFrameStyle {
                width: 1,
                ..SurfaceFrameStyle::default()
            },
            ..SurfaceChromeStyle::default()
        });
        let mut scene = LiveProductionCpuScene::new(outputs[0].size);
        let (submission, _, _) = runtime
            .run_cpu_production_cycle(LiveProductionCycleRequest {
                batch: &batch,
                scene: &mut scene,
                raised_surface: None,
                focused_surface: Some(framed),
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &outputs,
                native_scanout: None,
                wm_update: None,
                presentation_layout: &layout,
                geometry_routed_surfaces: &[popup],
                chrome_surfaces,
                indicator_publication: None,
                staged_cpu_buffer_handles: &[],
            })
            .unwrap();
        submission.composition
    };
    let pixel = |report: &LiveCpuCompositionReport, x: i32, y: i32| {
        let stride = usize::try_from(report.frame.stride).unwrap();
        let offset = usize::try_from(y).unwrap() * stride + usize::try_from(x).unwrap() * 4;
        report.frame.bytes[offset..offset + 4].to_vec()
    };
    // One pixel left of the popup, in the band a frame would occupy.
    let beside_popup = (
        popup_geometry.x - 1,
        popup_geometry.y + popup_geometry.height / 2,
    );
    let far_background = (2000, 1200);

    let only_framed = compose(&[framed]);
    assert_eq!(
        pixel(&only_framed, beside_popup.0, beside_popup.1),
        pixel(&only_framed, far_background.0, far_background.1),
        "an unauthorised surface gets no frame in the software composition"
    );

    let both = compose(&[framed, popup]);
    assert_ne!(
        pixel(&both, beside_popup.0, beside_popup.1),
        pixel(&both, far_background.0, far_background.1),
        "an authorised surface is framed"
    );
    assert_ne!(only_framed.checksum, both.checksum);
}
