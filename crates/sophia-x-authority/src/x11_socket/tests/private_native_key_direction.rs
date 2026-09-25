// t220 key direction on the private native path: red controls for the design
// in docs/notes/investigations/z8jzl4oh-private-key-direction-physical-ownership-and-delivery.md.
//
// Each control separates the physical outcome (ledger hold, XKB, QueryKeymap,
// activation and lease settlement) from the protocol outcome (which window,
// which form, or no event). A control named `conflict_` asserts X11's rule
// where it disagrees with the retained-recipient contract; which of those
// the production change must satisfy is the director's decision.

fn query_keymap_has(f: &KeyFixture, key: u8) -> bool {
    let keys = f
        .base
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pressed_keys(namespace());
    keys[usize::from(key / 8)] & (1 << (key % 8)) != 0
}

fn select_core(f: &KeyFixture, window: XResourceId, mask: u32, dnp: u32) {
    f.base
        .selections
        .lock()
        .unwrap()
        .update(window, Some(mask), Some(dnp));
}

fn release_frame(hold: &mut KeyHold) -> Option<Vec<u8>> {
    hold.take_release_emission().map(|emission| {
        emission
            .encode_frame(0, XByteOrder::LittleEndian, 2)
            .unwrap()
            .as_bytes()
            .to_vec()
    })
}

fn event_window(frame: &[u8]) -> XResourceId {
    let local = if frame[0] == 35 {
        u32::from_le_bytes(frame[16..20].try_into().unwrap())
    } else {
        u32::from_le_bytes(frame[12..16].try_into().unwrap())
    };
    XResourceId::new(u64::from(local), 1)
}

fn child_under_pointer(f: &KeyFixture) -> XResourceId {
    // The fixture's pointer sits at (20, 30) in window(): this child holds it.
    let child = XResourceId::new(0x200002, 1);
    let mut selected = f.base.selections.lock().unwrap();
    selected.register(
        child,
        window(),
        Rect {
            x: 5,
            y: 6,
            width: 80,
            height: 80,
        },
    );
    selected.observe_mapped(child);
    child
}

fn leaf_under_pointer(f: &KeyFixture, middle: XResourceId) -> XResourceId {
    // Nested in the child at the pointer, so the focus is not the first
    // window past a do-not-propagate stop: the shared rule retries the focus
    // after a stop, and these controls keep that question out.
    let leaf = XResourceId::new(0x200003, 1);
    let mut selected = f.base.selections.lock().unwrap();
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
    leaf
}

/// The physical half of a settled release: the aggregate delivered, XKB and
/// QueryKeymap have the key up and the native obligation reconciled.
fn assert_physically_released(f: &KeyFixture, hold: &KeyHold, key: u8, outcome: ReleaseOutcome) {
    assert_eq!(outcome, ReleaseOutcome::DeliverTo(hold.incarnation().unwrap()));
    assert_eq!(f.held(key), crate::XkbPhysicalKeyState::Released);
    assert!(!query_keymap_has(f, key), "QueryKeymap still reports {key}");
    assert_eq!(hold.status(), Status::NativeReconciled);
}

// R1: a KeyPress-only recipient is told of the press; its release settles
// every physical obligation and writes nothing.
#[test]
fn t220_key_press_only_selection_settles_the_release_without_an_event() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 1, 0);
    let mut pending = None;
    f.press(30, 901, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert!(hold.take_press_emission().is_some());
    let (outcome, built) = f.release(None, &mut hold, 30, 902);
    assert_physically_released(&f, &hold, 38, outcome);
    assert_eq!(built, Ok(None), "no KeyRelease was selected");
    assert!(release_frame(&mut hold).is_none());
}

// R4: a KeyRelease-only recipient owns the key physically from the press,
// is told nothing of the press, and is told of the release.
#[test]
fn t220_key_release_only_selection_owns_the_press_and_receives_the_release() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 2, 0);
    let mut pending = None;
    let (_, event) = f
        .press(30, 911, &mut pending)
        .expect("an unselected press still enters physical state");
    assert!(event.is_none(), "no KeyPress was selected");
    let mut hold = pending.unwrap();
    assert!(hold.take_press_emission().is_none());
    assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Held);
    assert!(query_keymap_has(&f, 38));
    let (outcome, built) = f.release(None, &mut hold, 30, 912);
    assert_physically_released(&f, &hold, 38, outcome);
    assert!(built.unwrap().is_some());
    let frame = release_frame(&mut hold).unwrap();
    assert_eq!(frame[0], 3);
    assert_eq!(event_window(&frame), window());
}

// Nobody selected a modifier: its press still changes XKB and QueryKeymap,
// and the next selected key carries it. The final release clears both.
#[test]
fn t220_key_unselected_modifier_still_reaches_xkb_and_query_keymap() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 0, 0);
    let mut shift = None;
    let (_, event) = f
        .press(42, 921, &mut shift)
        .expect("an unselected Shift is still physically down");
    assert!(event.is_none());
    let mut shift = shift.unwrap();
    assert!(shift.take_press_emission().is_none());
    assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Held);
    assert_eq!(f.keyboards.modifiers(seat()), Some(1));
    assert!(query_keymap_has(&f, 50), "QueryKeymap while Shift is held");
    select_core(&f, window(), 3, 0);
    let mut key = None;
    let (_, event) = f.press(30, 922, &mut key).unwrap();
    assert_eq!(event.unwrap().state & 1, 1, "the selected key carries Shift");
    let mut key = key.unwrap();
    let (outcome, _) = f.release(None, &mut key, 30, 923);
    assert_physically_released(&f, &key, 38, outcome);
    // Press-only now, so X and the retained plan agree: no Shift release.
    select_core(&f, window(), 1, 0);
    let (outcome, built) = f.release(None, &mut shift, 42, 924);
    assert_physically_released(&f, &shift, 50, outcome);
    assert_eq!(built, Ok(None));
    assert_eq!(f.keyboards.modifiers(seat()), Some(0));
}

// Two sources hold an unselected key. Joining and the survivor release move
// no physical state; the final release clears it and writes nothing.
#[test]
fn t220_key_unselected_join_survivor_and_final_release() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 0, 0);
    let mut pending = None;
    f.press(42, 931, &mut pending)
        .expect("an unselected press still enters physical state");
    let mut hold = pending.unwrap();
    let identity = hold.incarnation().unwrap();
    let second = f.role(2);
    let joined = f.join(&second, &hold);
    assert!(!joined.first_press());
    assert_eq!(joined.incarnation(), identity);
    let (outcome, built) = f.release(None, &mut hold, 42, 932);
    assert_eq!(outcome, ReleaseOutcome::SurvivorRemains);
    assert_eq!(built, Ok(None));
    assert_eq!(f.held(50), crate::XkbPhysicalKeyState::Held);
    assert!(query_keymap_has(&f, 50), "the survivor still holds Shift");
    assert_eq!(f.keyboards.modifiers(seat()), Some(1));
    let (outcome, built) = f.release(Some(&second), &mut hold, 42, 933);
    assert_physically_released(&f, &hold, 50, outcome);
    assert_eq!(built, Ok(None));
    assert!(release_frame(&mut hold).is_none());
}

// R1 propagation: the press stops at the child that selected KeyPress; the
// release propagates past it to the toplevel that selected KeyRelease.
#[test]
fn t220_key_press_and_release_propagate_to_their_own_selectors() {
    let mut f = KeyFixture::new();
    let child = child_under_pointer(&f);
    select_core(&f, child, 1, 0);
    select_core(&f, window(), 2, 0);
    let mut pending = None;
    f.press(30, 941, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    let press = hold
        .take_press_emission()
        .unwrap()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(event_window(&press), child);
    let (outcome, _) = f.release(None, &mut hold, 30, 942);
    assert_physically_released(&f, &hold, 38, outcome);
    let frame = release_frame(&mut hold).expect("the toplevel selected KeyRelease");
    assert_eq!(event_window(&frame), window());
}

// R2: do-not-propagate is per direction. KeyRelease DNP on the leaf stops
// only the release; the press still propagates to the middle window.
#[test]
fn t220_key_release_do_not_propagate_stops_only_the_release() {
    let mut f = KeyFixture::new();
    let middle = child_under_pointer(&f);
    let leaf = leaf_under_pointer(&f, middle);
    select_core(&f, leaf, 0, 2);
    select_core(&f, middle, 3, 0);
    select_core(&f, window(), 0, 0);
    let mut pending = None;
    f.press(30, 951, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert_eq!(hold.delivered_window(), middle);
    assert!(hold.take_press_emission().is_some());
    let (outcome, built) = f.release(None, &mut hold, 30, 952);
    assert_physically_released(&f, &hold, 38, outcome);
    assert_eq!(built, Ok(None), "KeyRelease DNP on the leaf");
    assert!(release_frame(&mut hold).is_none());
}

// R2, the other half: KeyPress DNP on the leaf stops only the press.
#[test]
fn t220_key_press_do_not_propagate_leaves_the_release_selected() {
    let mut f = KeyFixture::new();
    let middle = child_under_pointer(&f);
    let leaf = leaf_under_pointer(&f, middle);
    select_core(&f, leaf, 0, 1);
    select_core(&f, middle, 3, 0);
    select_core(&f, window(), 0, 0);
    let mut pending = None;
    let (_, event) = f
        .press(30, 956, &mut pending)
        .expect("a press stopped by DNP is still physically down");
    assert!(event.is_none());
    let mut hold = pending.unwrap();
    assert!(hold.take_press_emission().is_none());
    let (outcome, _) = f.release(None, &mut hold, 30, 957);
    assert_physically_released(&f, &hold, 38, outcome);
    let frame = release_frame(&mut hold).expect("KeyRelease propagates");
    assert_eq!(event_window(&frame), middle);
}

// XI2 by direction: XI_KeyRelease only. No press in either form; the release
// is written as XI, never core.
#[test]
fn t220_key_xi_release_only_selection_receives_only_the_xi_release() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 0, 0);
    f.base
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(namespace(), client().raw(), window(), &[(3, vec![1 << 3])]);
    let mut pending = None;
    let (_, event) = f
        .press(30, 961, &mut pending)
        .expect("an XI release selector owns the press physically");
    assert!(event.is_none());
    let mut hold = pending.unwrap();
    assert!(hold.take_press_emission().is_none());
    let (outcome, _) = f.release(None, &mut hold, 30, 962);
    assert_physically_released(&f, &hold, 38, outcome);
    let frame = release_frame(&mut hold).expect("XI_KeyRelease was selected");
    assert_eq!(frame[0], 35);
    assert_eq!(&frame[8..10], &[3, 0]);
}

fn grab_keyboard(f: &KeyFixture, owner: XServerFrontendClientId, window: XResourceId, owner_events: bool, event_mask: u16) {
    f.base
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_keyboard(
            namespace(),
            crate::XActiveInputGrab {
                owner: owner.raw(),
                window,
                owner_events,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
}

// R3: an active grab whose mask holds only KeyPress. The press goes to the
// grab window; the release is written nowhere and the key still settles.
#[test]
fn t220_key_grab_press_only_mask_settles_the_release_without_an_event() {
    let mut f = KeyFixture::new();
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    grab_keyboard(&f, client(), root, false, 1);
    let mut pending = None;
    f.press(30, 971, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert_eq!(hold.delivered_window(), root);
    let (outcome, built) = f.release(None, &mut hold, 30, 972);
    assert_physically_released(&f, &hold, 38, outcome);
    assert_eq!(built, Ok(None), "the grab did not select KeyRelease");
    assert!(release_frame(&mut hold).is_none());
}

// R3 with owner_events: the owner's own selection is tried per direction
// before the grab. KeyRelease is selected on the focus window, KeyPress
// only by the grab: the press goes to the grab window, the release to focus.
#[test]
fn t220_key_owner_events_grab_resolves_each_direction_separately() {
    let mut f = KeyFixture::new();
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    select_core(&f, window(), 2, 0);
    grab_keyboard(&f, client(), root, true, 1);
    let mut pending = None;
    f.press(30, 976, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert_eq!(hold.delivered_window(), root);
    let (outcome, _) = f.release(None, &mut hold, 30, 977);
    assert_physically_released(&f, &hold, 38, outcome);
    let frame = release_frame(&mut hold).expect("the owner selected KeyRelease");
    assert_eq!(event_window(&frame), window());
}

// A passive grab activated by a press whose grab mask holds only KeyPress:
// the trigger's release retires the activation and writes nothing.
#[test]
fn t220_key_passive_press_only_grab_retires_without_a_release_event() {
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
                detail: 38,
                modifiers: 0,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: 1,
            },
        )
        .unwrap();
    let mut pending = None;
    f.press(30, 981, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    let (outcome, built) = f.release(None, &mut hold, 30, 982);
    assert_physically_released(&f, &hold, 38, outcome);
    assert!(hold.activation_retirement().is_some());
    assert!(
        f.base
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .keyboard_grab(namespace())
            .is_none(),
        "the passive activation ended with its trigger"
    );
    assert_eq!(built, Ok(None), "the passive grab did not select KeyRelease");
    assert!(release_frame(&mut hold).is_none());
}

// A route lease is an external obligation whether or not an event is owed:
// suppressing the release event must not report the lease settled.
#[test]
fn t220_key_route_lease_stays_owed_when_the_release_writes_nothing() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 1, 0);
    let mut route = f.route(30, true, 986);
    route.route_lease = Some(sophia_protocol::ApplicationRouteLeaseIdentity {
        id: sophia_protocol::ApplicationRouteLeaseId::from_raw(3),
        seat: seat(),
        frontend_sequence: 4,
        control_epoch: 2,
    });
    let mut pending = None;
    press_routed(&mut f, &route, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    let (outcome, built) = f.release(None, &mut hold, 30, 987);
    assert_eq!(outcome, ReleaseOutcome::DeliverTo(hold.incarnation().unwrap()));
    assert_eq!(f.held(38), crate::XkbPhysicalKeyState::Released);
    assert_eq!(hold.status(), Status::Retained(Residual::ExternalLease));
    assert!(hold.proof().is_none());
    assert_eq!(built, Ok(None));
    assert!(release_frame(&mut hold).is_none());
}

fn other_client(f: &KeyFixture, raw: u64) -> (XServerFrontendClientId, XServerFrontendClientRouteRegistration) {
    let other = XServerFrontendClientId::from_raw(raw);
    let admission = namespaced(other, namespace());
    let registry = f.base.private.broker.registry.clone();
    f.base.private.participant.admit(other, admission).unwrap();
    let (registration, _channels) = registry
        .register_client_with_admission(other, Some(admission))
        .unwrap();
    registry
        .attach_connection_state(
            &registration,
            namespace(),
            Arc::new(Mutex::new(XCoreEventSelectionState::default())),
            Arc::new(AtomicU64::new(0)),
        )
        .unwrap();
    (other, registration)
}

// Recipient departure: a grab owner that selected only KeyPress departs
// while the key is down. The key still settles physically; nothing is built
// for the departed recipient, and its obligation is not transferred.
#[test]
fn t220_key_departed_press_only_recipient_still_settles_physically() {
    let mut f = KeyFixture::new();
    let (other, registration) = other_client(&f, 699);
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    grab_keyboard(&f, other, root, false, 1);
    let mut pending = None;
    f.press(30, 991, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert_eq!(hold.incarnation().unwrap().recipient, other.raw());
    drop(registration);
    let (outcome, built) = f.release(None, &mut hold, 30, 992);
    assert_physically_released(&f, &hold, 38, outcome);
    assert_eq!(built, Ok(None));
    assert!(release_frame(&mut hold).is_none());
}

// Generation replacement (guard): once the recipient departs and its
// admission is revoked, the same client cannot be admitted again while the
// key's release is owed, so no replacement generation can be named by it.
// The release still settles physically and names only the original.
#[test]
fn t220_key_recipient_replacement_is_refused_while_the_release_is_owed() {
    let mut f = KeyFixture::new();
    let (other, registration) = other_client(&f, 698);
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    grab_keyboard(&f, other, root, false, 3);
    let mut pending = None;
    f.press(30, 996, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    let original = hold.incarnation().unwrap();
    drop(registration);
    f.base
        .private
        .participant
        .revoke_admission(other, sophia_protocol::ClientAdmissionId::from_raw(other.raw()))
        .unwrap();
    assert!(matches!(
        f.base
            .private
            .participant
            .admit(other, namespaced(other, namespace())),
        Err(PrivateAdmissionRefusal::AlreadyAdmitted)
    ));
    let (outcome, _) = f.release(None, &mut hold, 30, 997);
    assert_physically_released(&f, &hold, 38, outcome);
    if let Some(emission) = hold.take_release_emission() {
        assert_eq!(emission.connection().recipient, original.recipient);
        assert_eq!(
            emission.connection().connection_generation,
            original.connection_generation
        );
    }
}

// Selection change between press and release (conflict control): X decides
// a release by the selections in force when it happens. The retained plan
// decides it at the press. Here KeyRelease is deselected after the press.
#[test]
fn conflict_t220_key_release_deselected_after_the_press_is_not_written() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 3, 0);
    let mut pending = None;
    f.press(30, 1001, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    select_core(&f, window(), 1, 0);
    let (outcome, built) = f.release(None, &mut hold, 30, 1002);
    assert_physically_released(&f, &hold, 38, outcome);
    assert_eq!(built, Ok(None), "X: KeyRelease is no longer selected");
}

// The mirror (conflict control): KeyRelease selected only after the press.
#[test]
fn conflict_t220_key_release_selected_after_the_press_is_written() {
    let mut f = KeyFixture::new();
    select_core(&f, window(), 1, 0);
    let mut pending = None;
    f.press(30, 1006, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    select_core(&f, window(), 3, 0);
    let (outcome, _) = f.release(None, &mut hold, 30, 1007);
    assert_physically_released(&f, &hold, 38, outcome);
    assert!(
        release_frame(&mut hold).is_some(),
        "X: KeyRelease is selected when the release happens"
    );
}

// Pointer moves between press and release (conflict control): X resolves the
// release from the pointer and focus at release time. The press is taken with
// the pointer in the child that selects both directions; the pointer then
// leaves the child for window(), which also selects both.
#[test]
fn conflict_t220_key_release_follows_the_pointer_at_release_time() {
    let mut f = KeyFixture::new();
    let child = child_under_pointer(&f);
    select_core(&f, child, 3, 0);
    select_core(&f, window(), 3, 0);
    let mut pending = None;
    f.press(30, 1011, &mut pending).unwrap();
    let mut hold = pending.unwrap();
    assert_eq!(hold.delivered_window(), child);
    f.base
        .selections
        .lock()
        .unwrap()
        .observe_pointer(window(), window(), 150, 90, 150, 90, 0);
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
                root_x: 150,
                root_y: 90,
                event_x: 150,
                event_y: 90,
                state: 0,
                time_msec: 2,
            }),
        );
    let (outcome, _) = f.release(None, &mut hold, 30, 1012);
    assert_physically_released(&f, &hold, 38, outcome);
    let frame = release_frame(&mut hold).unwrap();
    assert_eq!(event_window(&frame), window(), "X: the pointer left the child");
}

fn press_routed(
    f: &mut KeyFixture,
    route: &XAuthorityRoutedInput,
    hold: &mut Option<KeyHold>,
) -> Result<(sophia_input_authority::Applied, Option<XAuthorityKeyEvent>), Refusal> {
    let KeyFixture { base, keyboards } = f;
    let recovery = &base.private.broker.registry.input_recovery;
    recovery.admit_typed(route, 1, Instant::now()).unwrap();
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
            route,
            &surfaces,
            keyboards,
            hold,
            &applied,
            |recipient| {
                let binding = bindings
                    .bound
                    .get(&recipient)
                    .ok_or(PrivateAppliedRegistryRefusal::MissingAdmission)?;
                registry.applied_client(&clients, recipient, binding)
            },
        )
    })
}
