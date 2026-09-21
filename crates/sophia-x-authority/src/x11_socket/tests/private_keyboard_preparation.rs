mod private_keyboard_preparation {
    use super::*;
    use crate::{KeyboardPreparationRefusal as R, XActiveInputGrab, XPassiveInputGrab};

    fn root() -> XResourceId {
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1)
    }
    fn window(raw: u64) -> XResourceId {
        XResourceId::new(0x200000 + raw, 1)
    }
    fn rectangle(x: i32, y: i32) -> Rect {
        Rect {
            x,
            y,
            width: 100,
            height: 100,
        }
    }

    struct Fixture {
        publication: PrivateAppliedRoutingState,
        selections: XCoreEventSelectionState,
        authority: crate::XInputAuthorityState,
        runtime: XAuthorityRuntime,
        client: XServerFrontendClientId,
    }

    impl Fixture {
        fn new(focus: XResourceId) -> Self {
            let (common, issuer, _) = private_authority();
            let namespace = NamespaceId::from_raw(64);
            let identity = common.authority_identity(&issuer).unwrap();
            let client = XServerFrontendClientId::from_raw(7);
            let mut selections = XCoreEventSelectionState::default();
            selections
                .bind_private_origin(PrivateAppliedSelectionOrigin {
                    authority: identity,
                    namespace,
                    client,
                })
                .unwrap();
            let mut runtime = XAuthorityRuntime::new();
            runtime.prepare_input_focus_namespace(namespace);
            for (raw, parent, geometry) in [
                (1, root(), rectangle(0, 0)),
                (2, window(1), rectangle(10, 10)),
                (3, root(), rectangle(300, 0)),
            ] {
                assert_eq!(
                    runtime
                        .apply(crate::XAuthorityRequestPacket {
                            namespace,
                            transaction: TransactionId::from_raw(raw),
                            kind: crate::XAuthorityRequestKind::CreateWindow {
                                window: window(raw),
                                surface: SurfaceId::new(raw as u32, 1),
                                geometry,
                                constraints: sophia_protocol::SurfaceConstraints {
                                    min_size: None,
                                    max_size: None
                                },
                                generation: 1,
                            },
                        })
                        .outcome,
                    crate::XAuthorityResponseOutcome::Accepted
                );
                selections.register(window(raw), parent, geometry);
                selections.observe_mapped(window(raw));
                map_test_window_for_focus(&mut runtime, namespace, window(raw));
            }
            let publication = PrivateAppliedRoutingState::new(identity, namespace);
            let mut authority = crate::XInputAuthorityState::default();
            authority.prepare_ordered_namespace(namespace);
            let mut this = Self {
                publication,
                selections,
                authority,
                runtime,
                client,
            };
            this.focus(Some(focus));
            this
        }

        fn namespace(&self) -> NamespaceId {
            self.publication.namespace
        }
        fn focus(&mut self, window: Option<XResourceId>) {
            let route = window.map(|window| XServerFrontendSurfaceRoute {
                client: self.client,
                namespace: self.namespace(),
                admission: None,
                window,
            });
            self.publication
                .begin_focus_change()
                .unwrap()
                .apply(&mut self.runtime, &AtomicU64::new(0), route)
                .unwrap();
        }
        fn pointer(&mut self, surface_window: XResourceId, x: i16, y: i16) {
            self.authority.observe_query_input(
                self.namespace(),
                surface_window,
                XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Motion,
                    surface: SurfaceId::new(1, 1),
                    root_x: x,
                    root_y: y,
                    event_x: x,
                    event_y: y,
                    state: 0,
                    time_msec: 1,
                }),
            );
        }
        fn prepare(
            &mut self,
            key: u8,
            modifiers: u16,
        ) -> Result<crate::PreparedKeyboardPress<'_>, R> {
            let topology = PrivateKeyboardTopology::from_applied(
                &self.publication,
                self.client,
                &self.selections,
            )?;
            self.authority
                .prepare_keyboard_press(key, modifiers, topology)
        }
        fn register(&mut self, grab: XPassiveInputGrab) {
            self.authority.grab_key(self.namespace(), grab).unwrap();
        }
    }

    fn passive(window: XResourceId) -> XPassiveInputGrab {
        XPassiveInputGrab {
            owner: 7,
            window,
            detail: 38,
            modifiers: 0,
            owner_events: false,
            pointer_mode: 1,
            keyboard_mode: 1,
            event_mask: 3,
        }
    }
    fn active(window: XResourceId) -> XActiveInputGrab {
        XActiveInputGrab {
            owner: 7,
            window,
            owner_events: false,
            pointer_mode: 1,
            keyboard_mode: 1,
            event_mask: 3,
            xi_event_mask: [0; 8],
            xi_event_mask_words: 0,
            route_lease: None,
        }
    }

    #[test]
    fn active_keyboard_ownership_precedes_new_passive_policy_and_clear_focus() {
        let mut f = Fixture::new(window(2));
        f.authority
            .grab_keyboard(f.namespace(), active(window(3)))
            .unwrap();
        let first = f
            .authority
            .keyboard_activation(f.namespace())
            .unwrap()
            .unwrap();
        f.register(passive(root()));
        f.focus(None);
        let prepared = f.prepare(38, 0).unwrap();
        assert_eq!(prepared.reserved_stamp(), Some(first.stamp()));
        assert_eq!(prepared.commit(), Some(first));
        assert_eq!(
            f.authority.keyboard_activation(f.namespace()).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn passive_preparation_is_pure_and_commit_names_the_actual_trigger() {
        let mut f = Fixture::new(window(2));
        let mut grab = passive(window(1));
        grab.keyboard_mode = 0;
        f.register(grab);
        let namespace = f.namespace();
        let reserved = {
            let prepared = f.prepare(38, 0).unwrap();
            assert!(prepared.authority().keyboard_grab(namespace).is_none());
            prepared.reserved_stamp().unwrap()
        };
        assert!(!f.authority.keyboard_frozen(f.namespace()));
        let second = f.prepare(38, 0).unwrap().commit().unwrap();
        assert_ne!(second.stamp(), reserved);
        assert_eq!(second.trigger(), Some(38));
        assert_eq!(second.recipient().window, window(1));
        assert!(f.authority.keyboard_frozen(f.namespace()));
        assert_eq!(
            f.authority.keyboard_activation(f.namespace()).unwrap(),
            Some(second)
        );
    }

    #[test]
    fn rootmost_focus_ancestor_wins_independent_of_registration_order() {
        for reverse in [false, true] {
            let mut f = Fixture::new(window(2));
            let mut windows = [window(2), window(3), window(1), root()];
            if reverse {
                windows.reverse();
            }
            for window in windows {
                f.register(passive(window));
            }
            // A focus ancestor decides this without inventing pointer evidence.
            assert_eq!(
                f.prepare(38, 0)
                    .unwrap()
                    .commit()
                    .unwrap()
                    .recipient()
                    .window,
                root()
            );
        }
    }

    #[test]
    fn descendant_passive_grab_requires_the_actual_pointer_path() {
        let mut f = Fixture::new(window(1));
        f.register(passive(window(2)));
        assert!(matches!(f.prepare(38, 0), Err(R::PointerNotApplied)));
        f.pointer(window(1), 15, 15);
        assert_eq!(
            f.prepare(38, 0).unwrap().recipient().unwrap().window,
            window(2)
        );
        f.pointer(window(1), 5, 5);
        assert!(f.prepare(38, 0).unwrap().commit().is_none());
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
        f.register(passive(window(3)));
        f.pointer(window(3), 5, 5);
        assert!(f.prepare(38, 0).unwrap().commit().is_none());
        f.pointer(window(1), -1, 5);
        assert!(matches!(f.prepare(38, 0), Err(R::PointerNotApplied)));
    }

    #[test]
    fn wildcard_and_modifier_matching_do_not_create_an_implicit_keyboard_grab() {
        let mut f = Fixture::new(window(1));
        let mut grab = passive(root());
        grab.modifiers = 1;
        f.register(grab);
        assert!(f.prepare(38, 0).unwrap().commit().is_none());
        assert!(f.prepare(39, 1).unwrap().commit().is_none());
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
        grab.detail = 0;
        grab.modifiers = crate::X_ANY_MODIFIER;
        f.register(grab);
        assert_eq!(
            f.prepare(39, 2).unwrap().commit().unwrap().trigger(),
            Some(39)
        );
    }

    #[test]
    fn ambiguous_same_window_policy_refuses_unless_an_ancestor_decides_first() {
        let mut f = Fixture::new(window(2));
        f.register(passive(window(2)));
        let mut wildcard = passive(window(2));
        wildcard.detail = 0;
        wildcard.owner_events = true;
        f.register(wildcard);
        assert!(matches!(f.prepare(38, 0), Err(R::AmbiguousPassive)));
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
        f.register(passive(root()));
        assert_eq!(
            f.prepare(38, 0).unwrap().recipient().unwrap().window,
            root()
        );
    }

    #[test]
    fn unavailable_focus_selection_and_pointer_hierarchy_refuse_before_effect() {
        let mut f = Fixture::new(window(1));
        f.register(passive(window(2)));
        f.pointer(window(1), 15, 15);
        f.publication.published = false;
        assert!(matches!(
            f.prepare(38, 0),
            Err(R::Applied(PrivateAppliedRefusal::Unpublished))
        ));
        f.focus(Some(window(1)));
        let revision = f.selections.applied_revision.take();
        assert!(matches!(
            f.prepare(38, 0),
            Err(R::Applied(PrivateAppliedRefusal::Interrupted))
        ));
        f.selections.applied_revision = revision;
        f.selections.observe_unmapped(window(1));
        assert!(matches!(f.prepare(38, 0), Err(R::FocusNotViewable)));
        f.selections.observe_mapped(window(1));
        f.selections.reparent(window(2), window(2), 0, 0);
        // The only matching descendant no longer has a valid topology.
        f.pointer(window(2), 5, 5);
        assert!(matches!(
            f.prepare(38, 0),
            Err(R::Applied(PrivateAppliedRefusal::HierarchyCycle))
        ));
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
    }

    #[test]
    fn passive_scan_is_bounded_before_any_activation() {
        let mut f = Fixture::new(window(1));
        for raw in 0..PRIVATE_ORDERED_WINDOW_WORK + 1 {
            f.register(passive(window(raw as u64 + 100)));
        }
        assert!(matches!(
            f.prepare(38, 0),
            Err(R::Applied(PrivateAppliedRefusal::TraversalBudget))
        ));
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
    }

    #[test]
    fn frozen_keyboard_and_invalid_inputs_cannot_start_a_new_activation() {
        let mut f = Fixture::new(window(1));
        f.register(passive(root()));
        assert!(matches!(f.prepare(7, 0), Err(R::InvalidKey)));
        assert!(matches!(f.prepare(38, 0x100), Err(R::InvalidModifiers)));
        let mut pointer = active(window(1));
        pointer.keyboard_mode = 0;
        f.authority.grab_pointer(f.namespace(), pointer).unwrap();
        assert!(matches!(f.prepare(38, 0), Err(R::KeyboardFrozen)));
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
        assert!(f.authority.keyboard_frozen(f.namespace()));
    }

    #[test]
    fn cleared_focus_and_a_foreign_selection_origin_are_not_ungrabbed_permission() {
        let mut f = Fixture::new(window(1));
        f.focus(None);
        assert!(matches!(f.prepare(38, 0), Err(R::FocusNotApplied)));
        f.focus(Some(window(1)));
        let mut origin = f.selections.private_origin.unwrap();
        origin.client = XServerFrontendClientId::from_raw(8);
        f.selections.private_origin = Some(origin);
        assert!(matches!(
            f.prepare(38, 0),
            Err(R::Applied(PrivateAppliedRefusal::ForeignOrigin))
        ));
        assert!(f.authority.keyboard_grab(f.namespace()).is_none());
    }
}
