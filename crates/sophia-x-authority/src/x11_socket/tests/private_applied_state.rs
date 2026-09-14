mod private_applied_state {
    use super::*;

    fn root() -> XResourceId {
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1)
    }
    fn window(raw: u64) -> XResourceId {
        XResourceId::new(0x200000 + raw, 1)
    }
    fn rectangle(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn fixture() -> (
        PrivateAppliedRoutingState,
        XCoreEventSelectionState,
        crate::XInputAuthorityState,
    ) {
        let (authority, issuer, _) = private_authority();
        let identity = authority.authority_identity(&issuer).unwrap();
        let namespace = NamespaceId::from_raw(54);
        let client = XServerFrontendClientId::from_raw(7);
        let mut selections = XCoreEventSelectionState::default();
        selections
            .bind_private_origin(PrivateAppliedSelectionOrigin {
                authority: identity,
                namespace,
                client,
            })
            .unwrap();
        let mut state = PrivateAppliedRoutingState::new(identity, namespace);
        state
            .begin_focus_change()
            .unwrap()
            .apply(&mut XAuthorityRuntime::new(), &AtomicU64::new(0), None)
            .unwrap();
        (state, selections, crate::XInputAuthorityState::default())
    }

    fn pointer() -> XAuthorityPointerEvent {
        XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Button {
                button: 1,
                pressed: true,
            },
            surface: SurfaceId::new(1, 1),
            root_x: 125,
            root_y: 135,
            event_x: 25,
            event_y: 35,
            state: 0,
            time_msec: 100,
        }
    }

    fn tree(selections: &mut XCoreEventSelectionState) {
        selections.register(window(1), root(), rectangle(100, 100, 300, 300));
        selections.register(window(2), window(1), rectangle(20, 30, 100, 100));
        selections.observe_mapped(window(1));
        selections.observe_mapped(window(2));
    }

    #[test]
    fn queued_focus_and_projection_copy_never_publish_applied_authority() {
        let (mut state, selections, authority) = fixture();
        state.published = false;
        let queued_intent = AtomicU64::new(window(1).local.raw());
        assert_eq!(queued_intent.load(Ordering::Acquire), window(1).local.raw());
        assert!(matches!(
            state.view(
                XServerFrontendClientId::from_raw(7),
                &selections,
                &authority
            ),
            Err(PrivateAppliedRefusal::Unpublished)
        ));
    }

    #[test]
    fn actual_focus_effect_publishes_once_and_clear_publishes_no_key_target() {
        let (mut state, selections, authority) = fixture();
        let mut runtime = XAuthorityRuntime::new();
        let projection = AtomicU64::new(0);
        let route = XServerFrontendSurfaceRoute {
            client: XServerFrontendClientId::from_raw(7),
            namespace: state.namespace,
            admission: None,
            window: root(),
        };
        let before = state.revision;
        state
            .begin_focus_change()
            .unwrap()
            .apply(&mut runtime, &projection, Some(route))
            .unwrap();
        let view = state.view(route.client, &selections, &authority).unwrap();
        assert_eq!(view.authority(), state.authority);
        assert_eq!(view.revision(), before + 1);
        assert_eq!(view.focus(), Some(route));
        assert_eq!(runtime.input_focus(route.namespace).0, route.window);
        assert_eq!(projection.load(Ordering::Acquire), route.window.local.raw());
        state
            .begin_focus_change()
            .unwrap()
            .apply(&mut runtime, &projection, None)
            .unwrap();
        assert_eq!(
            state
                .view(route.client, &selections, &authority)
                .unwrap()
                .keyboard(true),
            Err(PrivateAppliedRefusal::FocusNotApplied)
        );
    }

    #[test]
    fn dropped_or_unwound_focus_transaction_leaves_no_readable_publication() {
        let (mut state, selections, authority) = fixture();
        let failed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _transaction = state.begin_focus_change().unwrap();
            panic!("producer interrupted before native effect");
        }));
        assert!(failed.is_err());
        assert!(matches!(
            state.view(
                XServerFrontendClientId::from_raw(7),
                &selections,
                &authority
            ),
            Err(PrivateAppliedRefusal::Unpublished)
        ));
        let mut runtime = XAuthorityRuntime::new();
        let projection = AtomicU64::new(0);
        let unknown_window = XServerFrontendSurfaceRoute {
            client: XServerFrontendClientId::from_raw(7),
            namespace: state.namespace,
            admission: None,
            window: window(999),
        };
        assert_eq!(
            state.begin_focus_change().unwrap().apply(
                &mut runtime,
                &projection,
                Some(unknown_window)
            ),
            Err(PrivateAppliedRefusal::NativeRefused)
        );
        assert_eq!(projection.load(Ordering::Acquire), 0);
        assert!(matches!(
            state.view(unknown_window.client, &selections, &authority),
            Err(PrivateAppliedRefusal::Unpublished)
        ));
    }

    #[test]
    fn private_selection_owner_cannot_be_substituted() {
        let (state, mut selections, authority) = fixture();
        let (foreign, issuer, _) = private_authority();
        let owner = selections.private_origin.unwrap();
        assert_eq!(
            selections.bind_private_origin(PrivateAppliedSelectionOrigin {
                authority: foreign.authority_identity(&issuer).unwrap(),
                ..owner
            }),
            Err(PrivateAppliedRefusal::ForeignOrigin)
        );
        assert!(matches!(
            state.view(
                XServerFrontendClientId::from_raw(8),
                &selections,
                &authority
            ),
            Err(PrivateAppliedRefusal::ForeignOrigin)
        ));
        assert!(matches!(
            state.view(
                owner.client,
                &XCoreEventSelectionState::default(),
                &authority
            ),
            Err(PrivateAppliedRefusal::UnboundSelections)
        ));
    }

    #[test]
    fn real_selection_updates_advance_revision_and_interrupted_update_stays_unknown() {
        let (state, mut selections, authority) = fixture();
        let owner = selections.private_origin.unwrap();
        let before = state
            .view(owner.client, &selections, &authority)
            .unwrap()
            .selection_revision();
        selections.update(root(), Some(1), None);
        assert_eq!(
            state
                .view(owner.client, &selections, &authority)
                .unwrap()
                .selection_revision(),
            before + 1
        );
        // Composition control for a mutation whose write-ahead began but
        // whose code did not reach its commit, not an injected allocator panic.
        selections.begin_applied_mutation();
        assert!(matches!(
            state.view(owner.client, &selections, &authority),
            Err(PrivateAppliedRefusal::Interrupted)
        ));
        selections.update(root(), Some(2), None);
        assert!(matches!(
            state.view(owner.client, &selections, &authority),
            Err(PrivateAppliedRefusal::Interrupted)
        ));
    }

    #[test]
    fn checked_hierarchy_accepts_sixty_four_links_and_refuses_sixty_five() {
        let mut selections = XCoreEventSelectionState::default();
        let mut parent = root();
        for index in 1..=64 {
            selections.register(window(index), parent, rectangle(0, 0, 100, 100));
            parent = window(index);
        }
        let ancestry = selections.ordered_ancestry(parent).unwrap();
        assert_eq!(ancestry.as_slice().len(), 65);
        assert_eq!(ancestry.as_slice().last(), Some(&root()));
        selections.register(window(65), parent, rectangle(0, 0, 100, 100));
        assert_eq!(
            selections.ordered_ancestry(window(65)),
            Err(PrivateAppliedRefusal::HierarchyOverflow)
        );
    }

    #[test]
    fn checked_hierarchy_refuses_cycle_missing_parent_and_coordinate_overflow() {
        let mut selections = XCoreEventSelectionState::default();
        selections.register(window(1), window(2), rectangle(0, 0, 100, 100));
        selections.register(window(2), window(1), rectangle(0, 0, 100, 100));
        assert_eq!(
            selections.ordered_ancestry(window(1)),
            Err(PrivateAppliedRefusal::HierarchyCycle)
        );
        selections.reparent(window(2), window(3), 0, 0);
        assert_eq!(
            selections.ordered_ancestry(window(1)),
            Err(PrivateAppliedRefusal::HierarchyMissing)
        );
        selections.register(window(2), root(), rectangle(i32::MAX, 0, 100, 100));
        selections.update_geometry(window(1), rectangle(1, 0, 100, 100));
        assert_eq!(
            selections.ordered_root_origin(window(1)),
            Err(PrivateAppliedRefusal::CoordinateOverflow)
        );
    }

    #[test]
    fn private_selection_has_no_wait_and_keeps_press_and_release_masks_distinct() {
        let (state, mut selections, authority) = fixture();
        tree(&mut selections);
        let client = selections.private_origin.unwrap().client;
        assert_eq!(
            state
                .view(client, &selections, &authority)
                .unwrap()
                .keyboard_for(window(2), true),
            Err(PrivateAppliedRefusal::NotSelected)
        );
        selections.update(window(2), Some(1 | (1 << 2)), None);
        let view = state.view(client, &selections, &authority).unwrap();
        assert!(view.keyboard_for(window(2), true).unwrap().core);
        assert_eq!(
            view.keyboard_for(window(2), false),
            Err(PrivateAppliedRefusal::NotSelected)
        );
        assert!(
            view.pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
                .is_ok()
        );
        let mut release = pointer();
        release.kind = XAuthorityPointerEventKind::Button {
            button: 1,
            pressed: false,
        };
        assert_eq!(
            view.pointer(window(1), release, None, PrivatePointerSelection::Current),
            Err(PrivateAppliedRefusal::NotSelected)
        );
    }

    #[test]
    fn frozen_pointer_coordinates_and_target_survive_later_selection_and_geometry_changes() {
        let (state, mut selections, authority) = fixture();
        tree(&mut selections);
        selections.update(window(2), Some(1 << 2), None);
        let client = selections.private_origin.unwrap().client;
        let frozen = state
            .view(client, &selections, &authority)
            .unwrap()
            .pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
            .unwrap();
        assert_eq!(frozen.event_window, window(2));
        let target = frozen.core.unwrap();
        assert_eq!((target.event_x, target.event_y), (5, 5));
        selections.configure_geometry(window(2), Some(90), Some(90), None, None);
        selections.update(window(2), Some(0), None);
        selections.update(window(1), Some(1 << 2), None);
        let current = state
            .view(client, &selections, &authority)
            .unwrap()
            .pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
            .unwrap();
        assert_eq!(current.delivered_window, window(1));
        assert_eq!(frozen.delivered_window, window(2));
        assert_eq!(
            (frozen.core.unwrap().event_x, frozen.core.unwrap().event_y),
            (5, 5)
        );
    }

    #[test]
    fn nearer_core_stops_xi_ancestor_but_same_window_xi_wins() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        selections.update(window(2), Some(1 << 2), None);
        let client = selections.private_origin.unwrap().client;
        authority.select_xi_events(
            state.namespace,
            client.raw(),
            window(1),
            &[(2, vec![1 << 4])],
        );
        let first = state
            .view(client, &selections, &authority)
            .unwrap()
            .pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
            .unwrap();
        assert!(first.core.is_some());
        assert!(first.master.iter().all(Option::is_none));
        authority.select_xi_events(
            state.namespace,
            client.raw(),
            window(2),
            &[(2, vec![1 << 4])],
        );
        let second = state
            .view(client, &selections, &authority)
            .unwrap()
            .pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
            .unwrap();
        assert!(second.core.is_none());
        let xi = second.master[0].unwrap();
        assert_eq!(xi.target.window, window(2));
        assert_eq!((xi.target.event_x, xi.target.event_y), (5, 5));
    }

    #[test]
    fn xi_ancestor_freezes_child_and_source_crossing_coordinate_records() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        let client = selections.private_origin.unwrap().client;
        authority.select_xi_events(
            state.namespace,
            client.raw(),
            window(1),
            &[
                (2, vec![(1 << 4) | (1 << 7)]),
                (crate::X_INPUT_POINTER_SOURCE_ID, vec![(1 << 4) | (1 << 7)]),
            ],
        );
        let frozen = state
            .view(client, &selections, &authority)
            .unwrap()
            .pointer(window(1), pointer(), None, PrivatePointerSelection::Current)
            .unwrap();
        let xi = frozen.master[0].unwrap();
        assert_eq!(xi.target.child, window(2));
        assert_eq!(xi.target.ancestry_depth, 1);
        assert_eq!((xi.target.event_x, xi.target.event_y), (25, 35));
        assert_eq!(
            frozen.source[0].unwrap().device,
            crate::X_INPUT_POINTER_SOURCE_ID
        );
        assert_eq!(frozen.crossings[3].unwrap().event_type, 7);
        assert_eq!(
            frozen.crossings[5].unwrap().device,
            Some(crate::X_INPUT_POINTER_SOURCE_ID)
        );
    }

    #[test]
    fn private_pointer_work_budget_is_aggregate_exact_and_counts_irrelevant_windows() {
        let (state, mut selections, authority) = fixture();
        selections.register(window(1), root(), rectangle(0, 0, 100, 100));
        selections.observe_mapped(window(1));
        selections.update(window(1), Some(1 << 2), None);
        let client = selections.private_origin.unwrap().client;
        let view = state.view(client, &selections, &authority).unwrap();
        view.pointer(
            window(1),
            pointer(),
            Some(window(1)),
            PrivatePointerSelection::Current,
        )
        .unwrap();
        let unused = view.budget.0.get();
        assert!(unused > 0 && unused < PRIVATE_ORDERED_WINDOW_WORK);
        // These windows cannot be pointer descendants and are unmapped, but
        // examining and rejecting them is still work charged to this request.
        for raw in 10..10 + unused as u64 {
            selections.register(window(raw), root(), rectangle(0, 0, 1, 1));
        }
        let view = state.view(client, &selections, &authority).unwrap();
        view.pointer(
            window(1),
            pointer(),
            Some(window(1)),
            PrivatePointerSelection::Current,
        )
        .unwrap();
        assert_eq!(view.budget.0.get(), 0);
        selections.register(window(10 + unused as u64), root(), rectangle(0, 0, 1, 1));
        assert_eq!(
            state
                .view(client, &selections, &authority)
                .unwrap()
                .pointer(
                    window(1),
                    pointer(),
                    Some(window(1)),
                    PrivatePointerSelection::Current
                ),
            Err(PrivateAppliedRefusal::TraversalBudget)
        );
    }

    #[test]
    fn nested_pointer_walks_share_the_window_allowance_with_geometry_and_ancestry() {
        let (state, mut selections, authority) = fixture();
        tree(&mut selections);
        selections.update(window(2), Some(1 << 2), None);
        for raw in 10..2050 {
            selections.register(window(raw), root(), rectangle(0, 0, 1, 1));
        }
        let client = selections.private_origin.unwrap().client;
        // Neither scan has 4096 windows. Their combined scan plus coordinate,
        // ancestry and selection work crosses the one shared allowance.
        assert_eq!(
            state
                .view(client, &selections, &authority)
                .unwrap()
                .pointer(window(1), pointer(), None, PrivatePointerSelection::Current),
            Err(PrivateAppliedRefusal::TraversalBudget)
        );
    }

    #[test]
    fn predicted_grab_can_freeze_delivery_before_it_is_committed() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        let client = selections.private_origin.unwrap().client;
        authority.prepare_ordered_namespace(state.namespace);
        let implicit = crate::XActiveInputGrab {
            owner: client.raw(),
            window: window(1),
            owner_events: false,
            pointer_mode: 1,
            keyboard_mode: 1,
            event_mask: 1 << 2,
            xi_event_mask: [0; 8],
            xi_event_mask_words: 0,
            route_lease: None,
        };
        // This actual passive registration supplies selection authority. The
        // implicit proposal alone must not turn an unselected window into a
        // recipient and is deliberately not the source of this control.
        authority
            .grab_button(
                state.namespace,
                crate::XPassiveInputGrab {
                    owner: client.raw(),
                    window: window(1),
                    detail: 1,
                    modifiers: 0,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: 1 << 2,
                },
            )
            .unwrap();
        let preview = authority
            .prepare_pointer_press(state.namespace, 1, 0, implicit)
            .unwrap();
        assert!(preview.authority().pointer_grab(state.namespace).is_none());
        let frozen = state
            .view(client, &selections, preview.authority())
            .unwrap()
            .pointer(
                window(1),
                pointer(),
                None,
                PrivatePointerSelection::Prepared(&preview),
            )
            .unwrap();
        assert_eq!(frozen.core.unwrap().window, window(1));
        assert_eq!(frozen.primary_recipient_window(), Ok(window(1)));
        assert!(preview.authority().pointer_grab(state.namespace).is_none());
        let committed = preview.commit();
        assert_eq!(committed.window, frozen.delivered_window);
        assert_eq!(authority.pointer_grab(state.namespace), Some(committed));
    }

    fn implicit_for(client: XServerFrontendClientId) -> crate::XActiveInputGrab {
        crate::XActiveInputGrab {
            owner: client.raw(),
            window: window(1),
            owner_events: true,
            pointer_mode: 1,
            keyboard_mode: 1,
            event_mask: u16::MAX,
            xi_event_mask: [0; 8],
            xi_event_mask_words: 0,
            route_lease: None,
        }
    }

    #[test]
    fn implicit_proposal_cannot_authorize_unselected_input_and_refines_to_reached_child() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        let client = selections.private_origin.unwrap().client;
        authority.prepare_ordered_namespace(state.namespace);
        {
            let preview = authority
                .prepare_pointer_press(state.namespace, 1, 0, implicit_for(client))
                .unwrap();
            assert!(preview.is_new_implicit());
            assert_eq!(
                state
                    .view(client, &selections, preview.authority())
                    .unwrap()
                    .pointer(
                        window(1),
                        pointer(),
                        None,
                        PrivatePointerSelection::Prepared(&preview)
                    ),
                Err(PrivateAppliedRefusal::NotSelected)
            );
        }
        selections.update(window(2), Some(1 << 2), None);
        let preview = authority
            .prepare_pointer_press(state.namespace, 1, 0, implicit_for(client))
            .unwrap();
        let frozen = state
            .view(client, &selections, preview.authority())
            .unwrap()
            .pointer(
                window(1),
                pointer(),
                None,
                PrivatePointerSelection::Prepared(&preview),
            )
            .unwrap();
        let preview = preview
            .with_implicit_window(frozen.primary_recipient_window().unwrap())
            .ok()
            .expect("new implicit");
        assert!(preview.authority().pointer_grab(state.namespace).is_none());
        assert_eq!(preview.commit().window, window(2));
    }

    #[test]
    fn prepared_grab_from_another_namespace_cannot_authorize_the_same_client_names() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        selections.update(window(2), Some(1 << 2), None);
        let client = selections.private_origin.unwrap().client;
        let foreign_namespace = NamespaceId::from_raw(55);
        authority.prepare_ordered_namespace(foreign_namespace);
        let preview = authority
            .prepare_pointer_press(foreign_namespace, 1, 0, implicit_for(client))
            .unwrap();
        assert_eq!(
            state
                .view(client, &selections, preview.authority())
                .unwrap()
                .pointer(
                    window(1),
                    pointer(),
                    None,
                    PrivatePointerSelection::Prepared(&preview)
                ),
            Err(PrivateAppliedRefusal::ForeignOrigin)
        );
    }

    #[test]
    fn incompatible_primary_xi_windows_refuse_instead_of_choosing_the_first() {
        let (state, mut selections, mut authority) = fixture();
        tree(&mut selections);
        let client = selections.private_origin.unwrap().client;
        authority.select_xi_events(
            state.namespace,
            client.raw(),
            window(2),
            &[(2, vec![1 << 6])],
        );
        authority.select_xi_events(
            state.namespace,
            client.raw(),
            window(1),
            &[(2, vec![1 << 4])],
        );
        let mut axis = pointer();
        axis.kind = XAuthorityPointerEventKind::Axis {
            button: 4,
            pressed: true,
            horizontal_position_v120: None,
            vertical_position_v120: Some(120),
        };
        assert_eq!(
            state
                .view(client, &selections, &authority)
                .unwrap()
                .pointer(window(1), axis, None, PrivatePointerSelection::Current),
            Err(PrivateAppliedRefusal::AmbiguousSelection)
        );
    }
}
