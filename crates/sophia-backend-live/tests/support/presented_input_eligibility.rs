/// A retired frame does not keep routing input to a window that has left the
/// layout.
///
/// The presented projection exists so input follows pixels that actually
/// reached scanout. But a retired frame records the past: unmapping a menu
/// removes it from the session's presentation order immediately, while its
/// pixels stay on screen until the next flip retires. That older frame must
/// not restore pointer eligibility after the layout removes the menu.
#[test]
fn a_retired_frame_stops_routing_input_to_a_surface_that_left_the_layout() {
    let still_shown = SurfaceId::new(140, 1);
    let unmapped = SurfaceId::new(141, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 64,
        height: 64,
    };
    let state = |surface| sophia_engine::OutputFrameSurfaceState {
        surface,
        committed_generation: 1,
        // This output starts at the root origin, so the head's placement and
        // the root placement are the same rectangle.
        logical_geometry: geometry,
        geometry,
        buffer: BufferSource::CpuBuffer { handle: 9 },
        source_size: Size {
            width: 64,
            height: 64,
        },
    };
    let presented = sophia_engine::OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output: sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 64,
                height: 64,
            },
            scale: 1,
        },
        // Both windows' pixels are on screen: this frame was composed before
        // the menu was unmapped.
        surfaces: vec![state(still_shown), state(unmapped)],
        compositor_display_list: sophia_engine::CompositorDisplayList {
            output: OutputId::from_raw(1),
            commands: Vec::new(),
        },
        software_cursor: None,
    };
    let metadata = BTreeMap::new();

    // The session has already dropped the unmapped menu from its layout.
    let layers = presented_input_layer_snapshots(&presented, &metadata, &[still_shown]);
    assert_eq!(
        layers.iter().map(|layer| layer.surface).collect::<Vec<_>>(),
        vec![still_shown],
        "an unmapped window keeps its pixels for a frame but stops taking hits"
    );

    // While it is still in the layout its pixels do answer the pointer, so
    // this removes staleness rather than dropping live input.
    let both = presented_input_layer_snapshots(&presented, &metadata, &[still_shown, unmapped]);
    assert_eq!(both.len(), 2);

    // Nothing outside the retired frame is ever added, whatever the layout
    // claims -- input never reaches pixels that have not been flipped.
    let unpresented = SurfaceId::new(142, 1);
    let added = presented_input_layer_snapshots(
        &presented,
        &metadata,
        &[still_shown, unmapped, unpresented],
    );
    assert_eq!(added.len(), 2, "the layout cannot conjure unflipped pixels");
}

/// Input eligibility drops a surface when the layout drops it, without
/// waiting for a page flip, and a late retirement cannot bring it back.
///
/// On the native path nothing republishes the input projection until an
/// accepted page flip retires a frame, so pruning only at retirement leaves a
/// window that is unmapped between two flips answering the pointer for the
/// whole interval -- unbounded if the flip stalls. Both halves are required:
/// removal on layout publication, and the retired-frame filter so a flip still
/// in flight cannot resurrect the surface it was composed with.
#[test]
fn layout_removal_drops_input_eligibility_before_any_flip_retires() {
    let kept = SurfaceId::new(150, 1);
    let closed = SurfaceId::new(151, 1);
    let mut runtime = LiveProductionVisualRuntime::new(
        &[HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 256,
                height: 256,
            },
            scale: 1,
        }],
        None,
    )
    .unwrap();

    let template = |surface| LayerSnapshot {
        input_region: None,
        surface,
        authority_local_id: None,
        output: None,
        namespace: None,
        stack_rank: 0,
        geometry: Rect {
            x: 4,
            y: 8,
            width: 64,
            height: 64,
        },
        source_size: Size {
            width: 64,
            height: 64,
        },
        source: BufferSource::CpuBuffer { handle: 1 },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        translation: None,
    };

    // A retired frame put both windows on screen and both answer the pointer.
    runtime.input_projections[0].layers = vec![template(kept), template(closed)];
    let epoch_before = runtime.input_projections[0].epoch;

    // The menu closes. The layout is republished with only the survivor, and
    // no flip has retired since.
    runtime.apply_presentation_layout(&[template(kept)], &[]);

    let layers = &runtime.input_projections[0].layers;
    assert_eq!(
        layers.iter().map(|layer| layer.surface).collect::<Vec<_>>(),
        vec![kept],
        "the closed window stops answering the pointer without waiting for a flip"
    );
    assert!(
        runtime.input_projections[0].epoch > epoch_before,
        "a route held against the old projection must see it move"
    );
    assert_eq!(
        layers[0].geometry,
        template(kept).geometry,
        "the survivor keeps the geometry its retired pixels were drawn at"
    );

    // A flip composed before the close finally retires. It still carries the
    // closed window, and must not restore it.
    let state = |surface| sophia_engine::OutputFrameSurfaceState {
        surface,
        committed_generation: 1,
        logical_geometry: Rect { x: 4, y: 8, width: 64, height: 64, },
        geometry: Rect {
            x: 4,
            y: 8,
            width: 64,
            height: 64,
        },
        buffer: BufferSource::CpuBuffer { handle: 1 },
        source_size: Size {
            width: 64,
            height: 64,
        },
    };
    let stale = sophia_engine::OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output: sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(1),
            size: Size {
                width: 256,
                height: 256,
            },
            scale: 1,
        },
        surfaces: vec![state(kept), state(closed)],
        compositor_display_list: sophia_engine::CompositorDisplayList {
            output: OutputId::from_raw(1),
            commands: Vec::new(),
        },
        software_cursor: None,
    };
    let republished =
        presented_input_layer_snapshots(&stale, &BTreeMap::new(), &runtime.presentation_order);
    assert_eq!(
        republished
            .iter()
            .map(|layer| layer.surface)
            .collect::<Vec<_>>(),
        vec![kept],
        "a flip still in flight cannot resurrect the window the layout removed"
    );
}
