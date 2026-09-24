use super::*;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/presented_input_eligibility.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/presented_input_geometry.rs"
));

fn surface(index: u32, generation: u32) -> SurfaceId {
    SurfaceId::new(index, generation)
}

#[test]
fn layer_templates_cover_every_committed_surface() {
    // The property scene_views rests on, and the one whose loss reopens the
    // invalid-surface tick: the engine pairs committed surfaces against
    // templates by SurfaceId and fails closed on a committed surface with no
    // template, so the template projection must be total over its committed
    // slice -- one template per entry, same id, metadata or not. A filter
    // added here, however reasonable it looks, is that bug again.
    let with_metadata = surface(21, 1);
    let without_metadata = surface(22, 3);
    let committed = vec![
        CommittedSurfaceState {
            surface: with_metadata,
            committed_generation: 4,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 7 },
                sophia_protocol::Size {
                    width: 640,
                    height: 480,
                },
            ),
            damage: Region::empty(),
        },
        CommittedSurfaceState {
            surface: without_metadata,
            committed_generation: 1,
            geometry: Rect {
                x: 640,
                y: 0,
                width: 640,
                height: 480,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 8 },
                sophia_protocol::Size {
                    width: 640,
                    height: 480,
                },
            ),
            damage: Region::empty(),
        },
    ];
    let metadata = BTreeMap::from([(
        with_metadata,
        LiveSurfaceProjectionMetadata {
            namespace: Some(NamespaceId::from_raw(3)),
            input_region: None,
        },
    )]);

    let templates = committed_layer_snapshots(&committed, &metadata);

    assert_eq!(templates.len(), committed.len());
    for (template, state) in templates.iter().zip(&committed) {
        assert_eq!(template.surface, state.surface);
    }
    // Missing metadata degrades the namespace, never the layer.
    assert_eq!(templates[1].namespace, None);
}

#[test]
fn presented_projection_keeps_retired_geometry_and_excludes_unpresented_surface() {
    let retired = surface(11, 2);
    let committed_only = surface(12, 1);
    let presented = OutputFrameDamageSnapshot {
        output: HeadlessOutput::deterministic(),
        surfaces: vec![OutputFrameSurfaceState {
            surface: retired,
            committed_generation: 7,
            logical_geometry: Rect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            },
            geometry: Rect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            },
            buffer: BufferSource::CpuBuffer { handle: 41 },
            source_size: sophia_protocol::Size {
                width: 300,
                height: 200,
            },
        }],
        compositor_display_list: CompositorDisplayList {
            output: OutputId::from_raw(1),
            commands: vec![CompositorDisplayCommand::Surface { surface: retired }],
        },
        software_cursor: None,
    };
    let metadata = BTreeMap::from([
        (
            retired,
            LiveSurfaceProjectionMetadata {
                namespace: Some(NamespaceId::from_raw(8)),
                input_region: None,
            },
        ),
        (
            committed_only,
            LiveSurfaceProjectionMetadata {
                namespace: Some(NamespaceId::from_raw(9)),
                input_region: None,
            },
        ),
    ]);

    let layers = presented_input_layer_snapshots(&presented, &metadata, &[retired]);

    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].surface, retired);
    assert_eq!(layers[0].generation, 7);
    assert_eq!(layers[0].geometry, presented.surfaces[0].geometry);
    assert_eq!(layers[0].namespace, Some(NamespaceId::from_raw(8)));
    assert!(!layers.iter().any(|layer| layer.surface == committed_only));
}

#[test]
fn presented_projection_preserves_retired_stacking_order() {
    let lower = surface(21, 1);
    let upper = surface(22, 1);
    let presented = OutputFrameDamageSnapshot {
        output: HeadlessOutput::deterministic(),
        surfaces: [lower, upper]
            .into_iter()
            .enumerate()
            .map(|(index, surface)| OutputFrameSurfaceState {
                surface,
                committed_generation: 1,
                logical_geometry: Rect {
                    x: i32::try_from(index).unwrap_or_default() * 10,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                geometry: Rect {
                    x: i32::try_from(index).unwrap_or_default() * 10,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                buffer: BufferSource::CpuBuffer {
                    handle: u64::try_from(index).unwrap_or_default() + 1,
                },
                source_size: sophia_protocol::Size {
                    width: 100,
                    height: 100,
                },
            })
            .collect(),
        compositor_display_list: CompositorDisplayList {
            output: OutputId::from_raw(1),
            commands: vec![
                CompositorDisplayCommand::Surface { surface: lower },
                CompositorDisplayCommand::Surface { surface: upper },
            ],
        },
        software_cursor: None,
    };

    let layers = presented_input_layer_snapshots(&presented, &BTreeMap::new(), &[lower, upper]);

    assert_eq!(layers[0].stack_rank, 0);
    assert_eq!(layers[1].stack_rank, 1);
}

#[test]
fn output_local_interaction_epochs_retire_independently() {
    let primary = HeadlessOutput::deterministic();
    let secondary = HeadlessOutput {
        id: OutputId::from_raw(2),
        ..primary
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[secondary, primary], None).unwrap();
    assert_eq!(runtime.input_projections()[0].output, primary.id);
    assert_eq!(runtime.input_projections()[1].output, secondary.id);
    let layer_for = |surface, handle| LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: Some(NamespaceId::from_raw(3)),
        stack_rank: 0,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
        source: BufferSource::CpuBuffer { handle },
        source_size: sophia_protocol::Size {
            width: 100,
            height: 100,
        },
        damage: Region::default(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    };

    runtime.replace_presented_input_projection(
        0,
        vec![layer_for(surface(40, 1), 1)],
        Vec::new(),
        None,
        Vec::new(),
        None,
        Vec::new(),
    );
    assert_eq!(runtime.input_projections()[0].epoch, 1);
    assert_eq!(runtime.input_projections()[1].epoch, 0);

    runtime.replace_presented_input_projection(
        1,
        vec![layer_for(surface(41, 1), 2)],
        Vec::new(),
        None,
        Vec::new(),
        None,
        Vec::new(),
    );
    assert_eq!(runtime.input_projections()[0].epoch, 1);
    assert_eq!(runtime.input_projections()[1].epoch, 1);

    // A buffer-only presentation is visual, not a lease-identity change.
    runtime.replace_presented_input_projection(
        0,
        vec![layer_for(surface(40, 1), 99)],
        Vec::new(),
        None,
        Vec::new(),
        None,
        Vec::new(),
    );
    assert_eq!(runtime.input_projections()[0].epoch, 1);
    assert_eq!(runtime.input_projections()[1].epoch, 1);

    runtime.replace_presented_input_projection(
        0,
        Vec::new(),
        Vec::new(),
        None,
        Vec::new(),
        None,
        Vec::new(),
    );
    assert_eq!(runtime.input_projections()[0].epoch, 2);
    assert_eq!(runtime.input_projections()[1].epoch, 1);
    assert_eq!(
        runtime.input_projections()[1].layers[0].surface,
        surface(41, 1)
    );
}

#[test]
fn descriptor_interaction_revokes_without_withdrawing_presented_occlusion() {
    let output = HeadlessOutput::deterministic();
    let geometry = Rect {
        x: 20,
        y: 20,
        width: 240,
        height: 80,
    };
    let target = sophia_engine::PresentedChromeTarget {
        id: sophia_engine::PresentedChromeTargetId {
            authority_session_epoch: 4,
            slot: 1,
            generation: 7,
        },
        output: output.id,
        geometry,
        action: sophia_protocol::ToplevelActionCapabilityRef {
            token: 9,
            issuer_epoch: 2,
            issuer_revocation_epoch: 3,
            recipient_epoch: 4,
            target_slot: 1,
            target_generation: 7,
        },
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    runtime.replace_presented_input_projection(
        0,
        Vec::new(),
        Vec::new(),
        None,
        vec![target],
        Some(geometry),
        Vec::new(),
    );
    assert_eq!(runtime.input_projections()[0].epoch, 1);

    assert_eq!(runtime.revoke_descriptor_overlay_interaction(), 1);
    assert!(runtime.input_projections()[0].descriptor_targets.is_empty());
    assert_eq!(
        runtime.input_projections()[0].descriptor_occlusion,
        Some(geometry)
    );
    assert_eq!(runtime.input_projections()[0].epoch, 2);
}

#[test]
fn retired_descriptor_frame_publishes_only_current_interaction() {
    let output = HeadlessOutput::deterministic();
    let geometry = Rect {
        x: 10,
        y: 10,
        width: 300,
        height: 64,
    };
    let command = CompositorDisplayCommand::Rect(sophia_engine::CompositorRect {
        opacity: 255,
        node: sophia_engine::CompositorNodeId::DescriptorOverlay {
            projection: 5,
            slot: u16::MAX,
            role: sophia_engine::DescriptorOverlayNodeRole::Panel,
        },
        generation: 5,
        geometry,
        color: sophia_engine::CompositorRgb8 {
            red: 1,
            green: 2,
            blue: 3,
        },
    });
    let target = sophia_engine::PresentedChromeTarget {
        id: sophia_engine::PresentedChromeTargetId {
            authority_session_epoch: 4,
            slot: 1,
            generation: 7,
        },
        output: output.id,
        geometry,
        action: sophia_protocol::ToplevelActionCapabilityRef {
            token: 9,
            issuer_epoch: 2,
            issuer_revocation_epoch: 3,
            recipient_epoch: 4,
            target_slot: 1,
            target_generation: 7,
        },
    };
    let overlay = sophia_engine::DescriptorOverlayProjection {
        output: output.id,
        generation: 8,
        geometry,
        commands: vec![command.clone()],
        targets: vec![target.clone()],
    };
    let presented = OutputFrameDamageSnapshot {
        output,
        surfaces: Vec::new(),
        compositor_display_list: CompositorDisplayList {
            output: output.id,
            commands: vec![command],
        }
        .into(),
        software_cursor: None,
    };

    assert_eq!(
        presented_descriptor_projection(&presented, Some(&overlay), output.id, true),
        (vec![target], Some(geometry))
    );
    assert_eq!(
        presented_descriptor_projection(&presented, Some(&overlay), output.id, false),
        (Vec::new(), Some(geometry))
    );
}

#[test]
fn committed_authority_state_does_not_publish_before_output_run() {
    let output = HeadlessOutput::deterministic();
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(1),
        authority: AuthorityKind::SophiaX,
        surface: surface(31, 1),
        namespace: Some(NamespaceId::from_raw(4)),
        target_geometry: Rect {
            x: 50,
            y: 60,
            width: 200,
            height: 100,
        },
        presentation_extent: sophia_protocol::Size {
            width: 200,
            height: 100,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 77 },
            sophia_protocol::Size {
                width: 200,
                height: 100,
            },
        ),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };

    runtime
        .prepare_authority_transactions(
            TransactionId::from_raw(1),
            std::slice::from_ref(&transaction),
            &[],
        )
        .unwrap();

    assert_eq!(runtime.committed_surfaces().len(), 1);
    assert!(runtime.input_layers().is_empty());
}

#[test]
fn a_shaped_input_region_reaches_both_input_projections_and_punches_through() {
    // t064: the SHAPE input region an authority commits with its surface
    // travels with the surface's metadata into both input projections, and
    // the hit test the session uses skips the layer outside it, so a shaped
    // panel is click-through exactly where it says it is.
    let panel = surface(41, 1);
    let below = surface(42, 1);
    let panel_geometry = Rect {
        x: 0,
        y: 0,
        width: 400,
        height: 40,
    };
    let below_geometry = Rect {
        x: 0,
        y: 0,
        width: 400,
        height: 300,
    };
    // The panel takes input on its left half only.
    let region = Region::single(Rect {
        x: 0,
        y: 0,
        width: 200,
        height: 40,
    });
    let metadata = BTreeMap::from([
        (
            panel,
            LiveSurfaceProjectionMetadata {
                namespace: Some(NamespaceId::from_raw(8)),
                input_region: Some(region.clone()),
            },
        ),
        (
            below,
            LiveSurfaceProjectionMetadata {
                namespace: Some(NamespaceId::from_raw(8)),
                input_region: None,
            },
        ),
    ]);
    let content = |width, height| {
        sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 5 },
            sophia_protocol::Size { width, height },
        )
    };

    // The committed path, bottom to top.
    let committed = [
        CommittedSurfaceState {
            surface: below,
            committed_generation: 1,
            geometry: below_geometry,
            content: content(400, 300),
            damage: Region::empty(),
        },
        CommittedSurfaceState {
            surface: panel,
            committed_generation: 1,
            geometry: panel_geometry,
            content: content(400, 40),
            damage: Region::empty(),
        },
    ];
    let committed_layers = committed
        .iter()
        .enumerate()
        .map(|(index, state)| layer_snapshot(index, state, metadata.get(&state.surface)))
        .collect::<Vec<_>>();
    assert_eq!(committed_layers[1].input_region, Some(region.clone()));
    assert_eq!(committed_layers[0].input_region, None);

    // The presented path.
    let presented_state = |surface, geometry: Rect| OutputFrameSurfaceState {
        surface,
        committed_generation: 1,
        logical_geometry: geometry,
        geometry,
        buffer: BufferSource::CpuBuffer { handle: 5 },
        source_size: sophia_protocol::Size {
            width: geometry.width,
            height: geometry.height,
        },
    };
    let presented = OutputFrameDamageSnapshot {
        output: HeadlessOutput::deterministic(),
        surfaces: vec![
            presented_state(below, below_geometry),
            presented_state(panel, panel_geometry),
        ],
        compositor_display_list: CompositorDisplayList {
            output: OutputId::from_raw(1),
            commands: vec![
                CompositorDisplayCommand::Surface { surface: below },
                CompositorDisplayCommand::Surface { surface: panel },
            ],
        },
        software_cursor: None,
    };
    let presented_layers = presented_input_layer_snapshots(&presented, &metadata, &[below, panel]);
    assert_eq!(presented_layers[1].input_region, Some(region));
    assert_eq!(presented_layers[0].input_region, None);

    // Through the hit test the session routes with: inside the region the
    // panel answers, outside it the window beneath does.
    let pointer = |x: f64, y: f64| sophia_protocol::InputEventPacket {
        serial: 1,
        seat: sophia_protocol::SeatId::from_raw(1),
        device: sophia_protocol::DeviceId::from_raw(1),
        time_msec: 1,
        kind: sophia_protocol::InputEventKind::PointerMotion,
        global_position: Some(sophia_protocol::Point { x, y }),
        target_surface: None,
        local_position: None,
    };
    for layers in [&committed_layers, &presented_layers] {
        assert_eq!(
            sophia_engine::hit_test_scene_surface_for_input(&pointer(100.0, 20.0), layers)
                .target_surface,
            Some(panel),
            "inside the region the panel answers"
        );
        assert_eq!(
            sophia_engine::hit_test_scene_surface_for_input(&pointer(300.0, 20.0), layers)
                .target_surface,
            Some(below),
            "outside the region the click falls through the panel"
        );
    }
}
