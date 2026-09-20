#[test]
fn native_key_normal_routing_stops_at_focus_and_honors_pointer_descendants() {
    for case in 0..7 {
        let mut f = KeyFixture::new();
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let middle = XResourceId::new(0x200002, 1);
        let leaf = XResourceId::new(0x200003, 1);
        {
            let mut selected = f.base.selections.lock().unwrap();
            selected.register(
                middle,
                window(),
                Rect {
                    x: 5,
                    y: 6,
                    width: 80,
                    height: 80,
                },
            );
            selected.observe_mapped(middle);
            selected.register(
                leaf,
                middle,
                Rect {
                    x: 3,
                    y: 4,
                    width: 40,
                    height: 40,
                },
            );
            selected.observe_mapped(leaf);
            selected.update(root, Some(if case == 1 { 3 } else { 0 }), None);
            selected.update(
                window(),
                Some(if matches!(case, 0 | 3 | 6) { 3 } else { 0 }),
                None,
            );
            selected.update(
                middle,
                Some(if matches!(case, 2 | 3 | 6) { 3 } else { 0 }),
                None,
            );
            selected.update(
                leaf,
                Some(if case == 0 { 3 } else { 0 }),
                Some(if case >= 2 { 1 } else { 0 }),
            );
        }
        let registry = f.base.private.broker.registry.clone();
        if matches!(case, 4 | 5) {
            registry.input_authority.lock().unwrap().select_xi_events(
                namespace(),
                client().raw(),
                if case == 4 { middle } else { leaf },
                &[(3, vec![1 << 2])],
            );
        }
        if case == 6 {
            registry
                .input_authority
                .lock()
                .unwrap()
                .grab_keyboard(
                    namespace(),
                    crate::XActiveInputGrab {
                        owner: client().raw(),
                        window: root,
                        owner_events: true,
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
        let mut pending = None;
        let result = f.press(30, 821, &mut pending);
        let expected = match case {
            0 | 5 => Some(leaf),
            3 => Some(window()),
            6 => Some(root),
            _ => None,
        };
        if let Some(target) = expected {
            result.unwrap();
            let hold = pending.as_mut().unwrap();
            assert_eq!(hold.delivered_window(), target, "routing case {case}");
            let frame = hold
                .take_press_emission()
                .unwrap()
                .encode_frame(0, XByteOrder::LittleEndian, 1)
                .unwrap();
            assert_eq!(frame.as_bytes()[0], if case == 5 { 35 } else { 2 });
            if case == 3 {
                assert_eq!(
                    &frame.as_bytes()[16..20],
                    &(middle.local.raw() as u32).to_le_bytes(),
                    "focus fallback still names the pointer's immediate child"
                );
            }
        } else {
            assert!(
                matches!(
                    result,
                    Err(Refusal::Resolution(PrivateAppliedRefusal::NotSelected))
                ),
                "routing case {case} must not propagate above focus or through DNP"
            );
            assert!(pending.is_none());
            assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Released);
        }
    }
}

fn assert_key_routing_uses_current_surface_geometry(passive: bool) {
    for (moved_x, adjusted) in [(10, false), (10, true), (30, false), (30, true)] {
        let mut f = KeyFixture::new();
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let child = XResourceId::new(0x200002, 1);
        {
            let mut selected = f.base.selections.lock().unwrap();
            selected.register(
                child,
                window(),
                Rect {
                    x: 15,
                    y: 20,
                    width: 10,
                    height: 20,
                },
            );
            selected.observe_mapped(child);
            selected.update(child, Some(3), None);
            // Stage the selection side of a geometry publication separately
            // from the runtime's real query-anchor adjustment below.
            selected.register(
                window(),
                root,
                Rect {
                    x: moved_x,
                    y: 0,
                    width: 200,
                    height: 100,
                },
            );
        }
        let registry = f.base.private.broker.registry.clone();
        if passive {
            registry
                .input_authority
                .lock()
                .unwrap()
                .grab_key(
                    namespace(),
                    crate::XPassiveInputGrab {
                        owner: client().raw(),
                        window: child,
                        detail: 38,
                        modifiers: 0,
                        owner_events: false,
                        pointer_mode: 1,
                        keyboard_mode: 1,
                        event_mask: 3,
                    },
                )
                .unwrap();
        }
        if adjusted {
            registry.input_authority.lock().unwrap().shift_query_anchor(
                namespace(),
                (0, 0),
                (moved_x, 0),
            );
        }
        let mut pending = None;
        let result = f.press(30, 831, &mut pending);
        if moved_x == 10 && adjusted {
            result.unwrap();
            let hold = pending.as_mut().unwrap();
            assert_eq!(
                hold.delivered_window(),
                window(),
                "stale local coordinates must not select the child"
            );
            assert!(
                registry
                    .input_authority
                    .lock()
                    .unwrap()
                    .keyboard_activation(namespace())
                    .unwrap()
                    .is_none(),
                "the old local hit must not activate a passive descendant"
            );
            let frame = hold
                .take_press_emission()
                .unwrap()
                .encode_frame(0, XByteOrder::LittleEndian, 1)
                .unwrap();
            assert_eq!(&frame.as_bytes()[24..26], &10_i16.to_le_bytes());
        } else {
            assert!(
                matches!(
                    result,
                    Err(Refusal::KeyboardPreparation(
                        crate::KeyboardPreparationRefusal::PointerNotApplied
                    ))
                ),
                "outside the observed surface requires a new authoritative pointer target"
            );
            assert!(pending.is_none());
            assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Released);
            assert!(
                registry
                    .input_authority
                    .lock()
                    .unwrap()
                    .keyboard_activation(namespace())
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn native_key_normal_routing_uses_current_geometry_after_surface_move() {
    assert_key_routing_uses_current_surface_geometry(false);
}

#[test]
fn native_key_passive_routing_uses_current_geometry_after_surface_move() {
    assert_key_routing_uses_current_surface_geometry(true);
}

#[test]
fn native_key_release_refreshes_child_without_reselecting_its_target() {
    for outside in [false, true] {
        let mut f = KeyFixture::new();
        let a = XResourceId::new(0x200002, 1);
        let b = XResourceId::new(0x200003, 1);
        {
            let mut selected = f.base.selections.lock().unwrap();
            for (child, x) in [(a, 0), (b, 40)] {
                selected.register(
                    child,
                    window(),
                    Rect {
                        x,
                        y: 0,
                        width: 40,
                        height: 80,
                    },
                );
                selected.observe_mapped(child);
            }
        }
        let mut pending = None;
        f.press(30, 841, &mut pending).unwrap();
        let mut hold = pending.unwrap();
        let press = hold.take_press_emission().unwrap();
        assert_eq!(hold.delivered_window(), window());
        assert_eq!(
            &press
                .encode_frame(0, XByteOrder::LittleEndian, 1)
                .unwrap()
                .as_bytes()[16..20],
            &(a.local.raw() as u32).to_le_bytes()
        );
        let x = if outside { 100 } else { 60 };
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .observe_query_input(
                namespace(),
                window(),
                XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Motion,
                    surface: surface(),
                    root_x: x,
                    root_y: 30,
                    event_x: x,
                    event_y: 30,
                    state: 0,
                    time_msec: 2,
                }),
            );
        // Different live selections must not change the retained release target.
        f.base.selections.lock().unwrap().update(b, Some(3), None);
        f.release(None, &mut hold, 30, 842).1.unwrap().unwrap();
        let frame = hold
            .take_release_emission()
            .unwrap()
            .encode_frame(0, XByteOrder::LittleEndian, 2)
            .unwrap();
        assert_eq!(
            &frame.as_bytes()[12..16],
            &(window().local.raw() as u32).to_le_bytes()
        );
        assert_eq!(
            &frame.as_bytes()[16..20],
            &(if outside { 0 } else { b.local.raw() as u32 }).to_le_bytes()
        );
    }
}

#[test]
fn native_key_noncongruent_visual_coordinates_remain_unavailable() {
    let mut f = KeyFixture::new();
    f.base
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .observe_query_input(
            namespace(),
            window(),
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind: XAuthorityPointerEventKind::Motion,
                surface: surface(),
                root_x: 120,
                root_y: 130,
                event_x: 20,
                event_y: 30,
                state: 0,
                time_msec: 2,
            }),
        );
    let mut pending = None;
    assert!(matches!(
        f.press(30, 851, &mut pending),
        Err(Refusal::KeyboardPreparation(
            crate::KeyboardPreparationRefusal::PointerNotApplied
        ))
    ));
    assert!(pending.is_none());
    assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Released);
}

#[test]
fn native_key_reaches_focus_while_the_pointer_is_over_another_clients_window() {
    // t140. A key press is reported to the pointer's window only when the
    // focus window is one of its ancestors; otherwise it is reported to the
    // focus window itself. A pointer over a THIRD client's window is exactly
    // that second case, so the key is delivered -- and before this it was
    // refused, because neither projection held that window and the path
    // resolution treated an unseeable branch as a missing hierarchy.
    //
    // It is the XTEST case that made it matter: an injected key targets the
    // focused surface with no pointer involvement at all, so it was correct
    // by construction and then refused for wherever the user had last left
    // the pointer.
    let mut f = KeyFixture::new();
    // Not registered in any projection, which is what makes it another
    // client's: this connection cannot see it, and cannot tell it from a
    // window that has gone.
    let foreign = XResourceId::new(0x7f0001, 1);
    {
        let mut selected = f.base.selections.lock().unwrap();
        assert!(
            !selected.geometries.contains_key(&foreign),
            "the control is only about a window this client cannot see"
        );
        selected.update(window(), Some(3), None);
    }
    // MOVED WHERE THE ROUTING ACTUALLY READS IT. The observation the key path
    // consults is the input authority's, not the selection state's snapshot;
    // setting only the latter leaves the pointer where the fixture put it and
    // the control proves nothing.
    f.base
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .observe_query_input(
            namespace(),
            foreign,
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind: XAuthorityPointerEventKind::Motion,
                surface: surface(),
                root_x: 900,
                root_y: 900,
                event_x: 4,
                event_y: 4,
                state: 0,
                time_msec: 2,
            }),
        );

    let mut pending = None;
    f.press(30, 821, &mut pending).expect(
        "a focused key must not be refused for where the pointer is",
    );
    let hold = pending.as_mut().expect("the press was held for delivery");
    assert_eq!(
        hold.delivered_window(),
        window(),
        "the key went to the focus window, which is what the pointer being \
         elsewhere means"
    );
    let frame = hold
        .take_press_emission()
        .unwrap()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .unwrap();
    assert_eq!(frame.as_bytes()[0], 2, "an ordinary KeyPress");
    // THE CHILD IS NONE, not the foreign window. The event names a child of
    // the event window containing the pointer, and a window in a branch this
    // client cannot see is not one of its children; naming it would hand a
    // client another client's resource id.
    assert_eq!(
        &frame.as_bytes()[16..20],
        &0u32.to_le_bytes(),
        "no child is named when the pointer is outside this client's tree"
    );
}
