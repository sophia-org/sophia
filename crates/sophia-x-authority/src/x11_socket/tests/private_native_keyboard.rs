mod keys {
    use super::super::super::private_native::KeyHold;
    use super::*;

    struct KeyFixture {
        base: Fixture,
        keyboards: PrivateKeyboards,
    }

    impl KeyFixture {
        fn new() -> Self {
            let base = Fixture::new();
            let mut keyboards = base.private.keyboards().unwrap();
            assert!(keyboards.prepare(seat()));
            let registry = &base.private.broker.registry;
            let mut runtime = XAuthorityRuntime::new();
            runtime.prepare_input_focus_namespace(namespace());
            assert_eq!(
                runtime
                    .apply(crate::XAuthorityRequestPacket {
                        namespace: namespace(),
                        transaction: TransactionId::from_raw(1),
                        kind: crate::XAuthorityRequestKind::CreateWindow {
                            window: window(),
                            surface: surface(),
                            geometry: Rect {
                                x: 0,
                                y: 0,
                                width: 200,
                                height: 100
                            },
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
            map_test_window_for_focus(&mut runtime, namespace(), window());
            registry
                .private_applied
                .get()
                .unwrap()
                .publication
                .lock()
                .unwrap()
                .begin_focus_change()
                .unwrap()
                .apply(
                    &mut runtime,
                    &base
                        ._registration
                        .connection_state
                        .get()
                        .unwrap()
                        .focused_projection,
                    Some(XServerFrontendSurfaceRoute {
                        client: client(),
                        namespace: namespace(),
                        admission: None,
                        window: window(),
                    }),
                )
                .unwrap();
            {
                let mut selected = base.selections.lock().unwrap();
                selected.update(window(), Some(3), None);
                selected.xkb_state_details = 0xff;
                selected.observe_pointer(window(), window(), 20, 30, 20, 30, 0);
            }
            registry
                .input_authority
                .lock()
                .unwrap()
                .observe_query_input(
                    namespace(),
                    window(),
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Motion,
                        surface: surface(),
                        root_x: 20,
                        root_y: 30,
                        event_x: 20,
                        event_y: 30,
                        state: 0,
                        time_msec: 1,
                    }),
                );
            Self { base, keyboards }
        }

        fn route(&self, keycode: u32, pressed: bool, id: u64) -> XAuthorityRoutedInput {
            let mut route = self.base.route(272, pressed);
            route.request.kind = InputEventKind::Key { keycode, pressed };
            route.delivery = Some(XAuthorityInputDeliveryId::from_raw(id));
            route
        }

        fn press(
            &mut self,
            evdev: u32,
            id: u64,
            hold: &mut Option<KeyHold>,
        ) -> Result<(sophia_input_authority::Applied, Option<XAuthorityKeyEvent>), Refusal>
        {
            self.press_with_transition(evdev, id, hold, |_| {})
        }

        fn press_with_transition(
            &mut self,
            evdev: u32,
            id: u64,
            hold: &mut Option<KeyHold>,
            transition: impl Fn(&InputRecovery),
        ) -> Result<(sophia_input_authority::Applied, Option<XAuthorityKeyEvent>), Refusal>
        {
            let route = self.route(evdev, true, id);
            let Self { base, keyboards } = self;
            let recovery = &base.private.broker.registry.input_recovery;
            recovery.admit_typed(&route, 1, Instant::now()).unwrap();
            assert_eq!(
                recovery.claim_execution(route.delivery),
                ExecutionClaim::Claimed
            );
            let applied = Cell::new(false);
            let _claim = PrivateDeliveryClaim {
                completion: None,
                recovery,
                delivery: route.delivery,
                applied: &applied,
            };
            base.run(&base.role, |permit, bindings| {
                let registry = &base.private.broker.registry;
                let clients = registry.clients.lock().unwrap();
                let surfaces = registry.surfaces.lock().unwrap();
                base.owner.lock_base()?.press_key(
                    permit,
                    base.role.capability,
                    &route,
                    &surfaces,
                    keyboards,
                    hold,
                    &applied,
                    |recipient| {
                        let binding = bindings
                            .bound
                            .get(&recipient)
                            .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
                        let witness = registry.applied_client(&clients, recipient, binding)?;
                        transition(recovery);
                        Ok(witness)
                    },
                )
            })
        }

        fn join(
            &mut self,
            role: &PrivateReservationRole,
            hold: &KeyHold,
        ) -> sophia_input_authority::Applied {
            let Self { base, keyboards } = self;
            base.run(role, |permit, _| {
                let connection = hold.connection();
                base.owner.lock_for_release(&connection)?.join_key(
                    permit,
                    hold,
                    keyboards,
                    &Cell::new(false),
                )
            })
            .unwrap()
        }

        fn release(
            &mut self,
            role: Option<&PrivateReservationRole>,
            hold: &mut KeyHold,
            evdev: u32,
            id: u64,
        ) -> (
            ReleaseOutcome,
            Result<Option<XAuthorityKeyEvent>, PrivateAppliedRefusal>,
        ) {
            let route = self.route(evdev, false, id);
            let Self { base, keyboards } = self;
            base.run(role.unwrap_or(&base.role), |permit, _| {
                let connection = hold.connection();
                base.owner.lock_for_release(&connection)?.release_key(
                    permit,
                    hold,
                    &route,
                    keyboards,
                    &Cell::new(false),
                )
            })
            .unwrap()
        }

        fn role(&self, device: u64) -> PrivateReservationRole {
            self.base
                .private
                .reservation_role(client(), DeviceId::from_raw(device))
                .unwrap()
        }

        fn held(&self, key: u8) -> crate::XkbPhysicalKeyState {
            self.keyboards.seats[&seat()].physical_key_state(key)
        }
    }

    #[test]
    fn native_key_source_applies_xkb_once_and_records_only_native_proof() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        let (_, event) = f.press(42, 701, &mut pending).unwrap();
        let event = event.unwrap();
        assert_eq!(
            (event.keycode, event.state, event.modifiers_after),
            (50, 0, 1)
        );
        let mut hold = pending.unwrap();
        assert_eq!(hold.status(), Status::Held);
        assert_eq!(hold.client(), client());
        assert_eq!(hold.delivered_window(), window());
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Held);
        assert_eq!(
            f.base
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_query_state(namespace())
                .mask,
            1
        );
        let emission = hold.take_press_emission().unwrap();
        assert!(hold.take_press_emission().is_none());
        assert_eq!(
            emission.delivery(),
            Some(XAuthorityInputDeliveryId::from_raw(701))
        );
        assert_eq!(emission.incarnation(), hold.incarnation());
        assert_eq!(
            f.base
                .private
                .broker
                .registry
                .input_recovery
                .ticket(XAuthorityInputDeliveryId::from_raw(701))
                .unwrap()
                .client,
            Some(client())
        );
        assert_eq!(emission.frame_count(), 2);
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let key = emission.encode_frame(0, order, 0x1234).unwrap();
            assert_eq!(&key.as_bytes()[0..2], &[2, 50]);
            let notify = emission.encode_frame(1, order, 0x1234).unwrap();
            let wire = notify.as_bytes();
            assert_eq!(wire[0], crate::X_KEYBOARD_FIRST_EVENT);
            assert_eq!(&wire[9..13], &[1, 1, 0, 0]);
            let read = |offset| match order {
                XByteOrder::LittleEndian => u16::from_le_bytes([wire[offset], wire[offset + 1]]),
                XByteOrder::BigEndian => u16::from_be_bytes([wire[offset], wire[offset + 1]]),
            };
            assert_eq!(read(2), 0x1234);
            assert_eq!(
                read(24),
                0,
                "pointer buttons do not contain the change mask"
            );
            assert_eq!(
                read(26),
                0x1f03,
                "effective, depressed and five derived modifier states changed"
            );
            assert_eq!(&wire[28..30], &[50, 2]);
        }
        let (outcome, built) = f.release(None, &mut hold, 42, 702);
        assert!(matches!(outcome, ReleaseOutcome::DeliverTo(_)));
        assert_eq!(built.unwrap().unwrap().modifiers_after, 0);
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Released);
        assert_eq!(hold.status(), Status::NativeReconciled);
        f.base
            .private
            .controller
            .under_common_as_origin(|common, _| {
                let (incarnation, bits) = common.next_debt(&mut 0).unwrap();
                assert_eq!(incarnation, hold.incarnation().unwrap());
                assert!(!bits.native_reconciled && !bits.recipient_settled);
            })
            .unwrap();
        assert!(!hold.proof().unwrap().record_native().unwrap());
        f.base
            .private
            .controller
            .under_common_as_origin(|common, _| {
                let (_, bits) = common.next_debt(&mut 0).unwrap();
                assert!(bits.native_reconciled);
                assert!(!bits.recipient_settled);
            })
            .unwrap();
        assert_eq!(
            hold.take_release_emission().unwrap().delivery(),
            Some(XAuthorityInputDeliveryId::from_raw(702))
        );
    }

    #[test]
    fn native_key_join_and_survivor_keep_the_same_xkb_history_and_recipient() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        f.press(42, 711, &mut pending).unwrap();
        let mut hold = pending.unwrap();
        let identity = hold.incarnation().unwrap();
        hold.take_press_emission().unwrap();
        // A fresh resolution now refuses. A join must inherit the old decision.
        f.base
            .selections
            .lock()
            .unwrap()
            .update(window(), Some(0), None);
        let second = f.role(2);
        let joined = f.join(&second, &hold);
        assert!(!joined.first_press());
        assert_eq!(joined.incarnation(), identity);
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Held);
        assert!(hold.take_press_emission().is_none());
        let (outcome, built) = f.release(None, &mut hold, 42, 712);
        assert_eq!(outcome, ReleaseOutcome::SurvivorRemains);
        assert!(built.unwrap().is_none());
        assert_eq!(f.keyboards.modifiers(seat()), Some(1));
        assert!(hold.take_release_emission().is_none());
        assert!(hold.proof().is_none());
        let (outcome, built) = f.release(Some(&second), &mut hold, 42, 713);
        assert_eq!(outcome, ReleaseOutcome::DeliverTo(identity));
        assert!(built.unwrap().is_some());
        assert_eq!(f.keyboards.modifiers(seat()), Some(0));
    }

    #[test]
    fn native_key_emission_survives_selection_changes_and_release_uses_current_geometry() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        f.press(30, 721, &mut pending).unwrap();
        let mut hold = pending.unwrap();
        let press = hold.take_press_emission().unwrap();
        let before = press
            .encode_frame(0, XByteOrder::LittleEndian, 1)
            .unwrap()
            .as_bytes()
            .to_vec();
        {
            let mut selected = f.base.selections.lock().unwrap();
            selected.configure_geometry(window(), Some(10), Some(5), None, None);
            selected.update(window(), Some(0), None);
        }
        // Same source adjustment performed by runtime geometry changes. A
        // selected geometry alone is not an applied pointer-coordinate update.
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .shift_query_anchor(namespace(), (0, 0), (10, 5));
        f.release(None, &mut hold, 30, 722).1.unwrap().unwrap();
        let release = hold.take_release_emission().unwrap();
        let bytes = release
            .encode_frame(0, XByteOrder::LittleEndian, 2)
            .unwrap();
        assert_eq!(&bytes.as_bytes()[24..28], &[10, 0, 25, 0]);
        assert_eq!(
            &bytes.as_bytes()[12..16],
            &(window().local.raw() as u32).to_le_bytes()
        );
        assert_eq!(
            press
                .encode_frame(0, XByteOrder::LittleEndian, 1)
                .unwrap()
                .as_bytes(),
            before
        );
    }

    #[test]
    fn native_key_unpublished_focus_and_missing_selection_refuse_before_xkb() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        f.base
            .selections
            .lock()
            .unwrap()
            .update(window(), Some(0), None);
        assert!(matches!(
            f.press(42, 731, &mut pending),
            Err(Refusal::Resolution(PrivateAppliedRefusal::NotSelected))
        ));
        assert!(pending.is_none());
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Released);
        f.base
            .private
            .broker
            .registry
            .private_applied
            .get()
            .unwrap()
            .publication
            .lock()
            .unwrap()
            .begin_focus_change()
            .unwrap();
        assert!(matches!(
            f.press(42, 732, &mut pending),
            Err(Refusal::Resolution(PrivateAppliedRefusal::Unpublished))
        ));
        assert!(pending.is_none());
        assert_eq!(f.keyboards.modifiers(seat()), Some(0));
    }

    #[test]
    fn native_key_release_keeps_missing_history_as_a_post_effect_residual() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        f.press(42, 741, &mut pending).unwrap();
        let mut hold = pending.unwrap();
        // Source mutation is deliberately unbalanced, so physical history
        // becomes unavailable. Cleanup must not assert a known key release.
        f.keyboards
            .seats
            .get_mut(&seat())
            .unwrap()
            .map_evdev_key(42, true)
            .unwrap();
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Unavailable);
        let (outcome, built) = f.release(None, &mut hold, 42, 742);
        assert_eq!(
            outcome,
            ReleaseOutcome::DeliverTo(hold.incarnation().unwrap())
        );
        assert_eq!(built, Err(PrivateAppliedRefusal::Interrupted));
        assert_eq!(
            hold.status(),
            Status::Retained(Residual::KeyboardUnavailable)
        );
        assert!(hold.proof().is_none());
        assert!(hold.take_release_emission().is_none());
    }

    #[test]
    fn native_key_cross_client_grab_binds_and_encodes_its_actual_recipient() {
        for source_unavailable in [false, true] {
            let mut f = KeyFixture::new();
            let other = XServerFrontendClientId::from_raw(699);
            let target = XResourceId::new(0x300001, 1);
            let admission = namespaced(other, namespace());
            let registry = f.base.private.broker.registry.clone();
            f.base.private.participant.admit(other, admission).unwrap();
            let (registration, _channels) = registry
                .register_client_with_admission(other, Some(admission))
                .unwrap();
            let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
            registry
                .attach_connection_state(
                    &registration,
                    namespace(),
                    selections.clone(),
                    Arc::new(AtomicU64::new(0)),
                )
                .unwrap();
            {
                let mut selected = selections.lock().unwrap();
                selected.register(
                    target,
                    XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                    Rect {
                        x: 100,
                        y: 10,
                        width: 200,
                        height: 100,
                    },
                );
                selected.observe_mapped(target);
                // The active keyboard grab supplies event authority; this client
                // has not selected ordinary KeyPress on the focus's window.
            }
            registry
                .input_authority
                .lock()
                .unwrap()
                .grab_keyboard(
                    namespace(),
                    crate::XActiveInputGrab {
                        owner: other.raw(),
                        window: target,
                        owner_events: false,
                        pointer_mode: 1,
                        keyboard_mode: 1,
                        event_mask: 3,
                        xi_event_mask: [0; 8],
                        xi_event_mask_words: 0,
                        route_lease: None,
                    },
                )
                .unwrap();
            let mut pending = None;
            f.press(30, 751, &mut pending).unwrap();
            let mut hold = pending.unwrap();
            assert_eq!(hold.incarnation().unwrap().recipient, other.raw());
            assert_eq!(
                registry
                    .input_recovery
                    .ticket(XAuthorityInputDeliveryId::from_raw(751))
                    .unwrap()
                    .client,
                Some(other)
            );
            let emission = hold.take_press_emission().unwrap();
            assert_eq!(emission.connection().recipient, other.raw());
            let frame = emission
                .encode_frame(0, XByteOrder::LittleEndian, 1)
                .unwrap();
            assert_eq!(
                &frame.as_bytes()[12..16],
                &(target.local.raw() as u32).to_le_bytes()
            );
            assert_eq!(&frame.as_bytes()[24..26], &(-80_i16).to_le_bytes());
            assert_eq!(&frame.as_bytes()[26..28], &20_i16.to_le_bytes());
            drop(registration);
            if source_unavailable {
                f.base.selections.lock().unwrap().applied_revision = None;
            }
            let (outcome, built) = f.release(None, &mut hold, 30, 752);
            assert!(matches!(outcome, ReleaseOutcome::DeliverTo(_)));
            assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Released);
            if source_unavailable {
                assert_eq!(built, Err(PrivateAppliedRefusal::Interrupted));
                assert!(hold.take_release_emission().is_none());
            } else {
                assert!(built.unwrap().is_some());
                assert_eq!(
                    hold.take_release_emission().unwrap().connection().recipient,
                    other.raw()
                );
            }
        }
    }

    #[test]
    fn native_key_passive_trigger_retirement_answers_its_retained_sibling() {
        let mut f = KeyFixture::new();
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_key(
                namespace(),
                crate::XPassiveInputGrab {
                    owner: client().raw(),
                    window: window(),
                    detail: 50,
                    modifiers: 0,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: 3,
                },
            )
            .unwrap();
        let mut trigger = None;
        f.press(42, 761, &mut trigger).unwrap();
        let mut trigger = trigger.unwrap();
        let mut sibling = None;
        f.press(30, 762, &mut sibling).unwrap();
        let mut sibling = sibling.unwrap();
        f.release(None, &mut sibling, 30, 763).1.unwrap().unwrap();
        assert_eq!(
            sibling.status(),
            Status::Retained(Residual::KeyboardActivation(
                crate::KeyboardActivationRetirement::StillRequiredByTrigger
            ))
        );
        assert!(sibling.proof().is_none());
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Held);
        f.release(None, &mut trigger, 42, 764).1.unwrap().unwrap();
        assert_eq!(trigger.status(), Status::NativeReconciled);
        let receipt = trigger.activation_retirement().unwrap();
        let incarnation = sibling.incarnation().unwrap();
        assert_eq!(
            sibling
                .complete_shared_activation(receipt)
                .unwrap()
                .incarnation(),
            incarnation
        );
        assert_eq!(sibling.status(), Status::NativeReconciled);
        assert!(sibling.complete_shared_activation(receipt).is_err());
        assert!(
            f.base
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .keyboard_grab(namespace())
                .is_none()
        );
    }

    #[test]
    fn native_key_xi_selection_uses_exact_coordinates_and_does_not_emit_core() {
        let mut f = KeyFixture::new();
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .select_xi_events(
                namespace(),
                client().raw(),
                window(),
                &[(3, vec![(1 << 2) | (1 << 3)])],
            );
        let mut pending = None;
        f.press(30, 771, &mut pending).unwrap();
        let mut hold = pending.unwrap();
        let press = hold.take_press_emission().unwrap();
        assert_eq!(press.frame_count(), 1);
        let bytes = press.encode_frame(0, XByteOrder::LittleEndian, 1).unwrap();
        assert_eq!(bytes.as_bytes()[0], 35);
        assert_eq!(&bytes.as_bytes()[8..12], &[2, 0, 3, 0]);
        assert_eq!(&bytes.as_bytes()[32..36], &(20_u32 << 16).to_le_bytes());
        assert_eq!(&bytes.as_bytes()[36..40], &(30_u32 << 16).to_le_bytes());
        assert!(press.encode_frame(1, XByteOrder::LittleEndian, 1).is_none());
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .select_xi_events(namespace(), client().raw(), window(), &[(3, vec![0])]);
        f.release(None, &mut hold, 30, 772).1.unwrap().unwrap();
        let release = hold.take_release_emission().unwrap();
        assert_eq!(
            &release
                .encode_frame(0, XByteOrder::LittleEndian, 2)
                .unwrap()
                .as_bytes()[8..12],
            &[3, 0, 3, 0]
        );
    }

    #[test]
    fn native_key_lock_state_is_not_a_held_key_and_xi_keeps_all_pre_edge_components() {
        let mut f = KeyFixture::new();
        // Selecting only a derived component must still report a change.
        f.base.selections.lock().unwrap().xkb_state_details = 1 << 12;
        let mut caps = None;
        f.press(58, 781, &mut caps).unwrap();
        let mut caps = caps.unwrap();
        let emission = caps.take_press_emission().unwrap();
        assert_eq!(emission.frame_count(), 2);
        let notify = emission
            .encode_frame(1, XByteOrder::LittleEndian, 1)
            .unwrap();
        assert_eq!(notify.as_bytes()[12], 2, "CapsLock is locked");
        assert_eq!(
            notify.as_bytes()[23],
            2,
            "compat lookup state includes Lock"
        );
        assert_ne!(
            u16::from_le_bytes(notify.as_bytes()[26..28].try_into().unwrap()) & (1 << 12),
            0
        );
        f.release(None, &mut caps, 58, 782).1.unwrap().unwrap();
        assert_eq!(f.held(66), crate::XkbPhysicalKeyState::Released);
        assert_eq!(
            f.keyboards.modifiers(seat()),
            Some(2),
            "released lock key leaves the lock active"
        );
        assert_eq!(caps.status(), Status::NativeReconciled);

        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .select_xi_events(
                namespace(),
                client().raw(),
                window(),
                &[(3, vec![(1 << 2) | (1 << 3)])],
            );
        let mut letter = None;
        f.press(30, 783, &mut letter).unwrap();
        let emission = letter.as_mut().unwrap().take_press_emission().unwrap();
        assert_eq!(
            emission.frame_count(),
            1,
            "ordinary key changed no XKB state"
        );
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let frame = emission.encode_frame(0, order, 2).unwrap();
            let read = |offset| {
                let bytes = frame.as_bytes()[offset..offset + 4].try_into().unwrap();
                match order {
                    XByteOrder::LittleEndian => u32::from_le_bytes(bytes),
                    XByteOrder::BigEndian => u32::from_be_bytes(bytes),
                }
            };
            assert_eq!(read(60), 0, "no physically depressed modifier");
            assert_eq!(read(64), 0, "no latched modifier");
            assert_eq!(read(68), 2, "locked modifier is carried separately");
            assert_eq!(read(72), 2, "effective includes Lock");
        }
    }

    #[test]
    fn native_key_binding_refusal_precedes_the_common_hold_and_xkb() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        let once = Cell::new(false);
        let result = f.press_with_transition(42, 791, &mut pending, |recovery| {
            if !once.replace(true) {
                let mut state = recovery.state.lock().unwrap();
                recovery
                    .disconnect_locked(
                        &mut state,
                        client(),
                        XAuthorityInputDeliveryOutcome::ClientDisconnected,
                        None,
                    )
                    .unwrap();
            }
        });
        assert!(matches!(result, Err(Refusal::DeliveryEnded)));
        assert!(pending.is_none());
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Released);
        assert_eq!(f.keyboards.modifiers(seat()), Some(0));
        let next = f
            .base
            .run(&f.base.role, |permit, _| {
                permit
                    .press(
                        sophia_input_authority::Input::key(50).unwrap(),
                        sophia_input_authority::Recipient {
                            recipient: client().raw(),
                            connection_generation: 47,
                        },
                    )
                    .map_err(Refusal::Authority)
            })
            .unwrap();
        assert!(
            next.first_press(),
            "a refused delivery must not hide a common hold"
        );
    }

    #[test]
    fn native_key_unreadable_binding_keeps_its_cause_and_returns_the_claim() {
        let mut f = KeyFixture::new();
        let mut pending = None;
        let once = Cell::new(false);
        let result = f.press_with_transition(42, 801, &mut pending, |recovery| {
            if !once.replace(true) {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _state = recovery.state.lock().unwrap();
                        panic!("poison recovery inside source selection, before mandatory bind");
                    }))
                    .is_err()
                );
            }
        });
        assert!(matches!(result, Err(Refusal::RecoveryUnavailable)));
        assert!(pending.is_none());
        assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Released);
        let state = f
            .base
            .private
            .broker
            .registry
            .input_recovery
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ticket = &state.tickets[&XAuthorityInputDeliveryId::from_raw(801)];
        assert!(!ticket.claimed);
        assert!(ticket.terminal.is_none());
    }

    fn trigger_and_sibling(f: &mut KeyFixture) -> (KeyHold, KeyHold) {
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_key(
                namespace(),
                crate::XPassiveInputGrab {
                    owner: client().raw(),
                    window: window(),
                    detail: 50,
                    modifiers: 0,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: 3,
                },
            )
            .unwrap();
        let mut trigger = None;
        f.press(42, 811, &mut trigger).unwrap();
        let mut sibling = None;
        f.press(30, 812, &mut sibling).unwrap();
        (trigger.unwrap(), sibling.unwrap())
    }

    #[test]
    fn native_key_trigger_first_receipt_survives_absence_and_replacement_for_siblings() {
        for replace in [false, true] {
            let mut f = KeyFixture::new();
            let (mut trigger, mut sibling) = trigger_and_sibling(&mut f);
            f.release(None, &mut trigger, 42, 813).1.unwrap().unwrap();
            assert_eq!(trigger.status(), Status::NativeReconciled);
            let receipt = trigger.activation_retirement().unwrap();
            let registry = f.base.private.broker.registry.clone();
            if replace {
                registry
                    .input_authority
                    .lock()
                    .unwrap()
                    .grab_keyboard(
                        namespace(),
                        crate::XActiveInputGrab {
                            owner: client().raw(),
                            window: window(),
                            owner_events: false,
                            pointer_mode: 1,
                            keyboard_mode: 1,
                            event_mask: 3,
                            xi_event_mask: [0; 8],
                            xi_event_mask_words: 0,
                            route_lease: None,
                        },
                    )
                    .unwrap();
            }
            let current = registry
                .input_authority
                .lock()
                .unwrap()
                .keyboard_activation(namespace())
                .unwrap();
            f.release(None, &mut sibling, 30, 814).1.unwrap().unwrap();
            assert_eq!(
                sibling.status(),
                Status::Retained(Residual::KeyboardActivation(if replace {
                    crate::KeyboardActivationRetirement::Replaced
                } else {
                    crate::KeyboardActivationRetirement::AlreadyAbsent
                }))
            );
            assert!(
                sibling.proof().is_none(),
                "absence/replacement is not native evidence"
            );
            if replace {
                // Same numeric names on another origin cannot provide this proof.
                let mut foreign = KeyFixture::new();
                let (mut other_trigger, _other_sibling) = trigger_and_sibling(&mut foreign);
                foreign
                    .release(None, &mut other_trigger, 42, 813)
                    .1
                    .unwrap()
                    .unwrap();
                assert!(matches!(
                    sibling
                        .complete_shared_activation(other_trigger.activation_retirement().unwrap()),
                    Err(Refusal::ForeignOrigin)
                ));
            }
            let incarnation = sibling.incarnation().unwrap();
            assert_eq!(
                sibling
                    .complete_shared_activation(receipt)
                    .unwrap()
                    .incarnation(),
                incarnation
            );
            assert_eq!(sibling.status(), Status::NativeReconciled);
            assert!(
                sibling.complete_shared_activation(receipt).is_err(),
                "one-use completion"
            );
            assert_eq!(
                registry
                    .input_authority
                    .lock()
                    .unwrap()
                    .keyboard_activation(namespace())
                    .unwrap(),
                current,
                "completion does not read, replace, or retire the new grab"
            );
        }
    }
    include!("private_native_key_routing.rs");
    include!("private_native_key_direction.rs");
    include!("../../../tests/support/private_native_custody.rs");
    include!("../../../tests/support/private_key_metadata.rs");
    include!("../../../tests/support/private_native_key_reconciliation.rs");
}
