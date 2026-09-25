#[test]
fn public_policy_snapshot_retains_an_admitted_surface_while_it_is_hidden() {
    let surface = SurfaceId::new(92, 4);
    let geometry = Rect {
        x: 24,
        y: 32,
        width: 640,
        height: 480,
    };
    let mut layout = PersistentLiveLayout::default();
    let mut observed = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(1));
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(1);
    observed.client = Some(client);
    add_test_surface_route(&mut observed, surface, client);
    observed.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 3,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
        },
    );
    observed
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Request,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 3,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
        });
    layout.observe_authority_batch(&observed);
    // Engine admission consumes planning ownership. The X frontend's
    // observation remains `mapped=false` because policy admission is not a
    // second client MapWindow request; neither fact ends policy ownership.
    layout.planning_surfaces.remove(&surface);

    let unrouted = SurfaceId::new(93, 1);
    let mut direct_observation =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(2));
    direct_observation.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface: unrouted,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 4,
            mapped: false,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    layout.observe_authority_batch(&direct_observation);

    assert!(layout.layers.is_empty());
    assert!(layout.planning_surfaces.is_empty());
    assert!(!layout.mapped_surfaces.contains(&surface));
    let surfaces = public_policy_surface_snapshots(
        &layout,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        sophia_engine::SurfaceChromeStyle::default(),
    )
    .unwrap();

    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0].surface, surface);
    assert_eq!(surfaces[0].generation, 7);
    assert_eq!(surfaces[0].current_output, None);
    assert_eq!(
        surfaces[0].geometry,
        sophia_engine::outer_surface_geometry(
            geometry,
            sophia_engine::SurfaceChromeStyle::default(),
        )
        .unwrap()
    );

    // Pixel identity is not policy state. Once admission has a retained
    // raster layer, its content generation may advance independently of the X
    // authority's window-lifecycle generation and must not stale a public WM
    // request that depends on the latter.
    let mut repainted = test_layer(surface, geometry);
    repainted.generation = 91;
    layout.layers.insert(surface, repainted);
    let repainted_surfaces = public_policy_surface_snapshots(
        &layout,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        sophia_engine::SurfaceChromeStyle::default(),
    )
    .unwrap();
    assert_eq!(repainted_surfaces[0].generation, 7);
    layout.layers.remove(&surface);

    let mut withdrawn =
        crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(3));
    withdrawn.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            mapped: false,
            ..observed.surface_presentations[0]
        },
    );
    withdrawn
        .presentation_intents
        .push(sophia_protocol::SurfacePresentationIntent {
            surface,
            kind: sophia_protocol::SurfacePresentationIntentKind::Withdraw,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            surface_kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            presentation_owner: None,
            stack_rank: 3,
            geometry,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 7,
        });
    layout.observe_authority_batch(&withdrawn);
    assert!(
        public_policy_surface_snapshots(
            &layout,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            sophia_engine::SurfaceChromeStyle::default(),
        )
        .unwrap()
        .is_empty()
    );
}
