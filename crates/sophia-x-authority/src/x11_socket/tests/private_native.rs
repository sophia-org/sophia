/// Source composition controls: real common reservations/permits and the exact
/// registry/connection producers, without a socket writer or consumer turn.
mod private_native_tests {
    use super::super::private_native::{Hold, Owner, Refusal, Residual, Status};
    use super::*;
    use sophia_input_authority::{ExecutionPermit, ReleaseOutcome};
    use std::cell::Cell;

    fn namespace() -> NamespaceId {
        NamespaceId::from_raw(77)
    }
    fn client() -> XServerFrontendClientId {
        XServerFrontendClientId::from_raw(700)
    }
    fn seat() -> SeatId {
        SeatId::from_raw(1)
    }
    fn window() -> XResourceId {
        XResourceId::new(0x200001, 1)
    }
    fn surface() -> SurfaceId {
        SurfaceId::new(901, 1)
    }

    struct Fixture {
        private: PrivateXServerFrontend,
        _registration: XServerFrontendClientRouteRegistration,
        selections: Arc<Mutex<XCoreEventSelectionState>>,
        owner: Owner,
        role: PrivateReservationRole,
        sequence: Cell<u64>,
        /// Last field, so it is dropped after the instance it keeps for.
        _keeper: crate::PrivateServiceOwner,
    }

    impl Fixture {
        fn new() -> Self {
            let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
            let private = private_for_roles(&service_keeper);
            let admission = namespaced(client(), namespace());
            private.participant.admit(client(), admission).unwrap();
            let registration = private
                .broker
                .registry
                .register_client_with_admission(client(), Some(admission))
                .unwrap()
                .0;
            let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
            let focused = Arc::new(AtomicU64::new(0));
            private
                .broker
                .registry
                .attach_connection_state(
                    &registration,
                    namespace(),
                    selections.clone(),
                    focused.clone(),
                )
                .unwrap();
            let publication = private
                .broker
                .registry
                .install_private_applied(&private.participant, namespace())
                .unwrap();
            let mut runtime = XAuthorityRuntime::new();
            runtime.prepare_input_focus_namespace(namespace());
            publication
                .lock()
                .unwrap()
                .begin_focus_change()
                .unwrap()
                .apply(&mut runtime, &focused, None)
                .unwrap();
            {
                let mut selected = selections.lock().unwrap();
                selected.register(
                    window(),
                    XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                    Rect {
                        x: 0,
                        y: 0,
                        width: 200,
                        height: 100,
                    },
                );
                selected.observe_mapped(window());
                selected.update(window(), Some((1 << 2) | (1 << 3)), None);
            }
            private
                .broker
                .registry
                .pointer_state
                .lock()
                .unwrap()
                .insert((namespace(), seat()), crate::XCorePointerMapper::new());
            private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .prepare_ordered_namespace(namespace());
            let owner = Owner::prepare(
                &private.controller,
                &private.broker.registry,
                namespace(),
                seat(),
            )
            .unwrap();
            let role = private
                .reservation_role(client(), DeviceId::from_raw(1))
                .unwrap();
            Self {
                private,
                _registration: registration,
                selections,
                owner,
                role,
                sequence: Cell::new(0),
                _keeper: service_keeper,
            }
        }

        fn route(&self, button: u32, pressed: bool) -> XAuthorityRoutedInput {
            let mut route = button_to(
                surface(),
                XAuthorityInputDeliveryId::from_raw(10),
                button,
                pressed,
            );
            route.delivery = None;
            route.request.global_position = Point { x: 20.0, y: 30.0 };
            route.request.local_position = Point { x: 20.0, y: 30.0 };
            route
        }

        fn run<T>(
            &self,
            role: &PrivateReservationRole,
            action: impl FnOnce(
                &mut ExecutionPermit<'_>,
                &PrivateAdmissionBindings,
            ) -> Result<T, Refusal>,
        ) -> Result<T, Refusal> {
            let sequence = self.sequence.get() + 1;
            self.sequence.set(sequence);
            let request = role
                .reserve(self.private.control_gate().stamp().unwrap(), sequence)
                .unwrap()
                .accepted();
            let mut result = None;
            self.private
                .execute_ordered(
                    &request,
                    XServerFrontendClientId::from_raw(role.connection().recipient),
                    |permit, bindings| {
                        let value = action(permit, bindings);
                        let success = value.is_ok();
                        result = Some(value);
                        if success {
                            Ok(())
                        } else {
                            Err(sophia_input_authority::RegistrationError::RoutingUnavailable)
                        }
                    },
                )
                .unwrap();
            assert!(
                request.observe().unwrap().is_some(),
                "real terminal completion observed"
            );
            result.unwrap()
        }

        fn press(&self, button: u32, storage: &mut Option<Hold>) -> XAuthorityPointerEvent {
            self.press_route(&self.role, &self.route(button, true), storage)
                .unwrap()
                .1
                .unwrap()
        }

        fn press_route(
            &self,
            role: &PrivateReservationRole,
            route: &XAuthorityRoutedInput,
            storage: &mut Option<Hold>,
        ) -> Result<
            (
                sophia_input_authority::Applied,
                Option<XAuthorityPointerEvent>,
            ),
            Refusal,
        > {
            self.run(role, |permit, bindings| {
                let clients = self.private.broker.registry.clients.lock().unwrap();
                let mut guards = self.owner.lock_base()?;
                guards.press(
                    permit,
                    role.capability,
                    route,
                    window(),
                    implicit(),
                    storage,
                    &Cell::new(false),
                    |recipient| {
                        self.private.broker.registry.applied_client(
                            &clients,
                            recipient,
                            &bindings.bound[&recipient],
                        )
                    },
                    |witness, selected, prepared, event| {
                        witness
                            .lock_publication()
                            .unwrap()
                            .view(client(), selected, prepared.authority())?
                            .pointer(
                                window(),
                                *event,
                                None,
                                PrivatePointerSelection::Prepared(prepared),
                            )
                    },
                )
            })
        }

        fn release(
            &self,
            role: &PrivateReservationRole,
            button: u32,
            hold: &mut Hold,
        ) -> (ReleaseOutcome, Option<XAuthorityPointerEvent>) {
            let (outcome, event) = self
                .run(role, |permit, _| {
                    let connection = hold.connection();
                    let mut guards = self.owner.lock_for_release(&connection)?;
                    guards.release(permit, hold, &self.route(button, false), &Cell::new(false))
                })
                .unwrap();
            match event {
                Ok(event) => (outcome, event),
                Err(PrivateAppliedRefusal::Interrupted) => {
                    assert!(matches!(
                        hold.status(),
                        Status::Retained(Residual::MissingMapper | Residual::MissingQueryScope)
                    ));
                    (outcome, None)
                }
                Err(cause) => panic!("unexpected event geometry refusal: {cause:?}"),
            }
        }

        fn masks(&self) -> (u16, u16, u16) {
            let pointer = self.private.broker.registry.pointer_state.lock().unwrap();
            let authority = self.private.broker.registry.input_authority.lock().unwrap();
            let selections = self.selections.lock().unwrap();
            (
                pointer[&(namespace(), seat())].state(),
                authority.pointer_query_state(namespace()).mask,
                selections.pointer.unwrap().mask,
            )
        }
    }

    fn implicit() -> crate::XActiveInputGrab {
        crate::XActiveInputGrab {
            owner: client().raw(),
            window: window(),
            owner_events: true,
            pointer_mode: 1,
            keyboard_mode: 1,
            event_mask: (1 << 2) | (1 << 3),
            xi_event_mask: [0; 8],
            xi_event_mask_words: 0,
            route_lease: None,
        }
    }

    #[test]
    fn real_final_release_produces_native_only_evidence_for_the_full_incarnation() {
        let fixture = Fixture::new();
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .observe_query_modifiers(namespace(), 1);
        let mut storage = None;
        assert_eq!(fixture.press(272, &mut storage).state, 1);
        let mut hold = storage.unwrap();
        assert_eq!(hold.status(), Status::Held);
        assert_eq!(fixture.masks(), (0x100, 0x101, 0x101));
        let (outcome, event) = fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(
            outcome,
            ReleaseOutcome::DeliverTo(hold.incarnation().unwrap())
        );
        assert_eq!(event.unwrap().state, 0x101);
        assert_eq!(fixture.masks(), (0, 1, 1));
        assert!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace())
                .is_none()
        );
        assert_eq!(hold.status(), Status::NativeReconciled);
        let proof = hold.proof().unwrap();
        assert_eq!(proof.incarnation(), hold.incarnation().unwrap());
        assert!(
            !proof.record_native().unwrap(),
            "native proof cannot settle recipient debt"
        );
        fixture
            .private
            .controller
            .under_common_as_origin(|authority, _| {
                let (incarnation, bits) = authority.next_debt(&mut 0).unwrap();
                assert_eq!(incarnation, proof.incarnation());
                assert!(bits.native_reconciled);
                assert!(!bits.recipient_settled);
            })
            .unwrap();
    }

    #[test]
    fn missing_mapper_is_retained_after_the_real_aggregate_release() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        fixture
            .private
            .broker
            .registry
            .pointer_state
            .lock()
            .unwrap()
            .remove(&(namespace(), seat()));
        assert!(matches!(
            fixture.release(&fixture.role, 272, &mut hold).0,
            ReleaseOutcome::DeliverTo(_)
        ));
        assert_eq!(hold.status(), Status::Retained(Residual::MissingMapper));
        assert_other_phase_rejects_receipt(&mut hold);
        assert!(hold.proof().is_none());
        assert!(
            !fixture
                .private
                .broker
                .registry
                .pointer_state
                .lock()
                .unwrap()
                .contains_key(&(namespace(), seat()))
        );
    }

    #[test]
    fn external_route_lease_is_an_exact_retained_obligation_not_a_native_proof() {
        let fixture = Fixture::new();
        let mut route = fixture.route(272, true);
        route.route_lease = Some(sophia_protocol::ApplicationRouteLeaseIdentity {
            id: sophia_protocol::ApplicationRouteLeaseId::from_raw(3),
            seat: seat(),
            frontend_sequence: 4,
            control_epoch: 2,
        });
        let mut hold = None;
        fixture
            .press_route(&fixture.role, &route, &mut hold)
            .unwrap();
        let mut hold = hold.unwrap();
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(hold.status(), Status::Retained(Residual::ExternalLease));
        assert_other_phase_rejects_receipt(&mut hold);
        assert!(hold.proof().is_none());
        assert_eq!(fixture.masks(), (0, 0, 0));
    }

    #[test]
    fn colliding_names_and_foreign_permit_cannot_mutate_or_answer_another_origin() {
        let first = Fixture::new();
        let second = Fixture::new();
        let mut first_hold = None;
        first.press(272, &mut first_hold);
        let mut second_hold = None;
        second.press(273, &mut second_hold);
        assert!(matches!(
            first
                .owner
                .lock_for_release(&second_hold.as_ref().unwrap().connection()),
            Err(Refusal::ForeignOrigin)
        ));
        let mut foreign_storage = None;
        let result = second.run(&second.role, |permit, _bindings| {
            let mut guards = first.owner.lock_base()?;
            guards.press(
                permit,
                second.role.capability,
                &first.route(275, true),
                window(),
                implicit(),
                &mut foreign_storage,
                &Cell::new(false),
                |_| panic!("foreign before recipient witness"),
                |_, _, _, _| panic!("foreign before resolver"),
            )
        });
        assert_eq!(result.unwrap_err(), Refusal::ForeignOrigin);
        assert!(foreign_storage.is_none());
        assert_eq!(first.masks(), (0x100, 0x100, 0x100));
        assert_eq!(second.masks(), (0x400, 0x400, 0x400));
        let mut first_hold = first_hold.unwrap();
        first.release(&first.role, 272, &mut first_hold);
        first_hold.proof().unwrap().record_native().unwrap();
        second
            .private
            .controller
            .under_common_as_origin(|authority, _| {
                assert!(
                    authority.next_debt(&mut 0).is_none(),
                    "foreign live hold unchanged"
                );
            })
            .unwrap();
    }

    #[test]
    fn join_and_survivor_release_leave_every_native_projection_unchanged() {
        let fixture = Fixture::new();
        let other = fixture
            .private
            .reservation_role(client(), DeviceId::from_raw(2))
            .unwrap();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        let revision = fixture.selections.lock().unwrap().applied_revision;
        fixture
            .run(&other, |permit, _| {
                let connection = hold.connection();
                let mut guards = fixture.owner.lock_for_release(&connection)?;
                let joined = guards.join(permit, &hold, &Cell::new(false))?;
                assert!(!joined.first_press());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fixture.release(&fixture.role, 272, &mut hold),
            (ReleaseOutcome::SurvivorRemains, None)
        );
        assert_eq!(fixture.masks(), (0x100, 0x100, 0x100));
        assert_eq!(
            fixture.selections.lock().unwrap().applied_revision,
            revision
        );
        assert!(hold.proof().is_none());
        fixture.release(&other, 272, &mut hold);
        assert_eq!(hold.status(), Status::NativeReconciled);
    }

    #[test]
    fn release_preserves_motion_modifiers_and_selected_lineage_applied_after_press() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        let later_window = XResourceId::new(0x200002, 1);
        let later = XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Motion,
            surface: SurfaceId::new(902, 2),
            root_x: 90,
            root_y: 91,
            event_x: 40,
            event_y: 41,
            state: 0x104,
            time_msec: 200,
        };
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .observe_query_input(
                namespace(),
                later_window,
                XAuthorityInputEvent::Pointer(later),
            );
        fixture.selections.lock().unwrap().observe_pointer(
            later_window,
            later_window,
            later.root_x,
            later.root_y,
            later.event_x,
            later.event_y,
            later.state,
        );
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(hold.status(), Status::NativeReconciled);
        assert_eq!(fixture.masks(), (0, 4, 4));
        let query = fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_query_state(namespace())
            .position
            .unwrap();
        assert_eq!(
            (
                query.surface_window,
                query.surface,
                query.root_x,
                query.root_y,
                query.local_x,
                query.local_y
            ),
            (later_window, later.surface, 90, 91, 40, 41)
        );
        let selected = fixture.selections.lock().unwrap().pointer.unwrap();
        assert_eq!(
            (
                selected.surface_window,
                selected.pointer_window,
                selected.root_x,
                selected.root_y,
                selected.event_x,
                selected.event_y
            ),
            (later_window, later_window, 90, 91, 40, 41)
        );
    }

    #[test]
    fn owner_cleanup_removing_query_scope_is_retained_and_never_recreated() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .cleanup_owner(client().raw());
        assert!(
            !fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .has_ordered_namespace(namespace())
        );
        let (outcome, event) = fixture.release(&fixture.role, 272, &mut hold);
        assert!(matches!(outcome, ReleaseOutcome::DeliverTo(_)));
        assert!(
            event.is_none(),
            "unknown query modifiers are not a clear event state"
        );
        assert_eq!(hold.status(), Status::Retained(Residual::MissingQueryScope));
        assert_other_phase_rejects_receipt(&mut hold);
        assert!(hold.proof().is_none());
        let mut authority = fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap();
        assert!(!authority.has_ordered_namespace(namespace()));
        assert!(
            authority
                .observe_query_button_release(namespace(), 1)
                .is_err()
        );
        assert!(!authority.has_ordered_namespace(namespace()));
    }

    #[test]
    fn independent_explicit_grab_is_preserved_by_the_real_release() {
        let fixture = Fixture::new();
        let explicit = implicit();
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_pointer(namespace(), explicit)
            .unwrap();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(hold.status(), Status::NativeReconciled);
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace()),
            Some(explicit)
        );
    }

    #[test]
    fn replacing_automatic_capture_does_not_authorize_retiring_the_replacement() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        let explicit = implicit();
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_pointer(namespace(), explicit)
            .unwrap();
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(
            hold.status(),
            Status::Retained(Residual::Activation(
                crate::PointerActivationRetirement::Replaced
            ))
        );
        assert!(hold.proof().is_none());
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace()),
            Some(explicit)
        );
    }

    #[test]
    fn synchronous_grab_remains_an_explicit_thaw_obligation() {
        let fixture = Fixture::new();
        let mut grab = implicit();
        grab.keyboard_mode = 0;
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_pointer(namespace(), grab)
            .unwrap();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(hold.status(), Status::Retained(Residual::Synchronous));
        assert_other_phase_rejects_receipt(&mut hold);
        assert!(hold.proof().is_none());
        assert!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .keyboard_frozen(namespace())
        );
    }

    #[test]
    fn another_client_under_the_same_origin_cannot_supply_cleanup_guards() {
        let fixture = Fixture::new();
        let other = XServerFrontendClientId::from_raw(701);
        let admitted = namespaced(other, namespace());
        fixture.private.participant.admit(other, admitted).unwrap();
        let (registered, _) = fixture
            .private
            .broker
            .registry
            .register_client_with_admission(other, Some(admitted))
            .unwrap();
        let other_selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        fixture
            .private
            .broker
            .registry
            .attach_connection_state(
                &registered,
                namespace(),
                other_selections.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        for release in [false, true] {
            let progress = Cell::new(false);
            let result = fixture.run(&fixture.role, |permit, bindings| {
                let clients = fixture.private.broker.registry.clients.lock().unwrap();
                let witness = fixture
                    .private
                    .broker
                    .registry
                    .applied_client(&clients, other, &bindings.bound[&other])
                    .unwrap();
                let mut wrong = fixture.owner.lock_for_connection(&witness)?;
                if release {
                    wrong
                        .release(permit, &mut hold, &fixture.route(272, false), &progress)
                        .map(|_| ())
                } else {
                    wrong.join(permit, &hold, &progress).map(|_| ())
                }
            });
            assert_eq!(result, Err(Refusal::ForeignOrigin));
            assert!(
                !progress.get(),
                "wrong connection refused before common effect"
            );
            assert_eq!(hold.status(), Status::Held);
            assert!(other_selections.lock().unwrap().pointer.is_none());
        }
        assert_eq!(fixture.masks(), (0x100, 0x100, 0x100));
        assert!(
            matches!(
                fixture.release(&fixture.role, 272, &mut hold).0,
                ReleaseOutcome::DeliverTo(_)
            ),
            "wrong guards never ended the real hold"
        );
    }

    #[test]
    fn native_owner_cannot_substitute_a_mapper_seat_for_its_authority_binding() {
        let fixture = Fixture::new();
        assert!(matches!(
            Owner::prepare(
                &fixture.private.controller,
                &fixture.private.broker.registry,
                namespace(),
                SeatId::from_raw(2)
            ),
            Err(Refusal::ForeignOrigin)
        ));
    }

    #[test]
    fn release_coordinates_come_from_the_retained_surface_under_guard() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        let mut route = fixture.route(272, false);
        route.request.target_surface = SurfaceId::new(999, 9);
        route.request.local_position = Point { x: 150.0, y: 99.0 };
        let (outcome, event) = fixture
            .run(&fixture.role, |permit, _| {
                let connection = hold.connection();
                fixture.owner.lock_for_release(&connection)?.release(
                    permit,
                    &mut hold,
                    &route,
                    &Cell::new(false),
                )
            })
            .unwrap();
        assert!(matches!(outcome, ReleaseOutcome::DeliverTo(_)));
        let event = event.unwrap().unwrap();
        assert_eq!(event.surface, surface());
        assert_eq!((event.event_x, event.event_y), (20, 30));
    }

    #[test]
    fn actual_other_client_grab_selects_its_witness_under_the_same_native_guards() {
        let fixture = Fixture::new();
        let other = XServerFrontendClientId::from_raw(701);
        let basic = namespaced(other, namespace());
        let admitted = sophia_protocol::ClientAdmissionContext::new(
            basic.client_id,
            basic.namespace,
            sophia_protocol::ClientAuthProvenance::new(
                sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
                47,
            )
            .unwrap(),
        )
        .unwrap();
        fixture.private.participant.admit(other, admitted).unwrap();
        let (registered, _) = fixture
            .private
            .broker
            .registry
            .register_client_with_admission(other, Some(admitted))
            .unwrap();
        let other_selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        fixture
            .private
            .broker
            .registry
            .attach_connection_state(
                &registered,
                namespace(),
                other_selections.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        let target = XResourceId::new(0x300001, 1);
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        {
            let mut selected = other_selections.lock().unwrap();
            selected.register(
                target,
                root,
                Rect {
                    x: 5,
                    y: 6,
                    width: 100,
                    height: 100,
                },
            );
            selected.observe_mapped(target);
            selected.update(target, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grab = implicit();
        grab.owner = other.raw();
        grab.window = target;
        grab.owner_events = false;
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_pointer(namespace(), grab)
            .unwrap();
        let mut hold = None;
        // Supplying A for the actual selected B is refused before any effect.
        let refused = fixture.run(&fixture.role, |permit, bindings| {
            let clients = fixture.private.broker.registry.clients.lock().unwrap();
            fixture.owner.lock_base()?.press(
                permit,
                fixture.role.capability,
                &fixture.route(272, true),
                window(),
                implicit(),
                &mut hold,
                &Cell::new(false),
                |_| {
                    fixture.private.broker.registry.applied_client(
                        &clients,
                        client(),
                        &bindings.bound[&client()],
                    )
                },
                |_, _, _, _| panic!("wrong witness must not reach resolution"),
            )
        });
        assert_eq!(refused.unwrap_err(), Refusal::ForeignOrigin);
        assert!(hold.is_none());
        let bound_route = admitted_native_press(&fixture, 813);
        fixture
            .run(&fixture.role, |permit, bindings| {
                let clients = fixture.private.broker.registry.clients.lock().unwrap();
                let (applied, event) = fixture.owner.lock_base()?.press(
                    permit,
                    fixture.role.capability,
                    &bound_route,
                    window(),
                    implicit(),
                    &mut hold,
                    &Cell::new(false),
                    |selected| {
                        assert_eq!(selected, other);
                        fixture.private.broker.registry.applied_client(
                            &clients,
                            selected,
                            &bindings.bound[&selected],
                        )
                    },
                    |witness, selected, prepared, event| {
                        let mut rooted = *event;
                        rooted.event_x = rooted.root_x;
                        rooted.event_y = rooted.root_y;
                        witness
                            .lock_publication()
                            .unwrap()
                            .view(other, selected, prepared.authority())?
                            .pointer(
                                root,
                                rooted,
                                None,
                                PrivatePointerSelection::Prepared(prepared),
                            )
                    },
                )?;
                assert!(applied.first_press());
                assert_eq!(applied.incarnation().recipient, other.raw());
                assert_eq!(applied.incarnation().connection_generation, 47);
                assert_eq!((event.unwrap().event_x, event.unwrap().event_y), (20, 30));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            fixture.private.broker.registry.input_recovery
                .ticket(bound_route.delivery.unwrap()).unwrap().client,
            Some(other),
            "the native source binds the grab recipient, not the submitting client"
        );
        let mut hold = hold.unwrap();
        assert_eq!(hold.plan().surface_window, root);
        assert_eq!(hold.plan().primary_recipient_window().unwrap(), target);
        assert!(fixture.selections.lock().unwrap().pointer.is_none());
        let observed = other_selections.lock().unwrap().pointer.unwrap();
        assert_eq!(
            (
                observed.surface_window,
                observed.pointer_window,
                observed.event_x,
                observed.event_y
            ),
            (root, target, 20, 30)
        );
        let query = fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_query_state(namespace())
            .position
            .unwrap();
        assert_eq!(
            (query.surface_window, query.local_x, query.local_y),
            (window(), 20, 30)
        );
        fixture.release(&fixture.role, 272, &mut hold);
        assert_eq!(hold.status(), Status::NativeReconciled);
        assert_eq!(other_selections.lock().unwrap().pointer.unwrap().mask, 0);
    }

    include!("private_native_sibling.rs");
    include!("private_native_emission.rs");
    include!("private_native_binding.rs");
    include!("private_native_keyboard.rs");
}
