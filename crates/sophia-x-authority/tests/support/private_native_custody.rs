// Source/custody controls only. Key dispatch remains refused by the executor;
// these do not establish a service key path or surviving runner XKB history.

fn press_key_into_inventory(
    fixture: &mut KeyFixture,
    inventory: &mut PrivateTerminalInventory,
    id: u64,
) -> sophia_input_authority::Applied {
    let PrivateTerminalInventory {
        native_pending,
        pending_custody,
        next_event_order,
        ..
    } = inventory;
    let order = *next_event_order;
    *next_event_order += 1;
    let custody = std::cell::RefCell::new(pending_custody);
    fixture
        .press_with_transition(42, id, native_pending.key_slot().unwrap(), |recovery| {
            // The fixture calls this under the admitted recipient lookup,
            // before press_key installs its hold or enters the common effect.
            let mut pending = custody.borrow_mut();
            assert!(pending.is_none());
            **pending = Some(PrivateDeliveryCustody::new(
                order,
                recovery
                    .completion_for(XAuthorityInputDeliveryId::from_raw(id))
                    .unwrap(),
            ));
            assert!(pending.as_ref().unwrap().completion.is_some());
        })
        .unwrap()
        .0
}

#[test]
fn pending_key_custody_survives_interruption_without_a_local_hold_transfer() {
    let mut fixture = KeyFixture::new();
    let mut inventory = fixture.base.private.terminal.hand_over();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        press_key_into_inventory(&mut fixture, &mut inventory, 8801);
        panic!("after the native key effect, before a held record exists");
    }));
    assert!(interrupted.is_err());
    assert!(inventory.holds.is_empty());
    assert!(inventory.native_pending.is_some());
    assert_eq!(fixture.held(50), crate::XkbPhysicalKeyState::Held);
    let incarnation = inventory
        .native_pending
        .key_slot()
        .unwrap()
        .as_ref()
        .unwrap()
        .incarnation()
        .unwrap();
    let cell = inventory
        .pending_custody
        .as_ref()
        .unwrap()
        .completion
        .as_ref()
        .unwrap()
        .clone();
    assert!(cell.answer().is_none());
    assert!(matches!(
        inventory.native_pending.pointer_slot(),
        Err(Refusal::WrongPhase)
    ));
    assert!(!inventory.is_empty());
    let outstanding = inventory.outstanding();

    let mut carried = inventory.hand_over();
    assert!(inventory.native_pending.is_none() && inventory.pending_custody.is_none());
    assert_eq!(carried.outstanding(), outstanding);
    assert!(Arc::ptr_eq(
        carried
            .pending_custody
            .as_ref()
            .unwrap()
            .completion
            .as_ref()
            .unwrap(),
        &cell
    ));
    let held = carried.native_pending.key_slot().unwrap().as_mut().unwrap();
    assert_eq!(held.incarnation(), Some(incarnation));
    let emission = held.take_press_emission().unwrap();
    assert_eq!(emission.incarnation(), incarnation);
    assert_eq!(
        emission.delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(8801))
    );
    assert!(held.take_press_emission().is_none());
    assert_eq!(held.status(), Status::Held);
    assert!(held.proof().is_none());
}

#[test]
fn key_native_custody_carries_both_events_and_only_its_source_proof() {
    let mut fixture = KeyFixture::new();
    let mut inventory = fixture.base.private.terminal.hand_over();
    let applied = press_key_into_inventory(&mut fixture, &mut inventory, 8811);
    let reached = PrivateReachedResources {
        client: client(),
        window: window(),
        surface: surface(),
        namespace: namespace(),
        seat: seat(),
        grant: fixture.base.role.capability.grant(),
    };
    inventory.holds.push(PrivateHoldRecord {
        incarnation: applied.incarnation(),
        reached,
        custody: inventory.pending_custody.take().unwrap(),
        native: None,
    });
    inventory.holds[0].native = inventory.native_pending.take();
    let held = inventory.holds[0].native.as_ref().unwrap();
    assert!(held.pointer().is_none());
    assert!(held.key().is_some());
    assert_eq!(held.input(), applied.incarnation().input);
    assert_eq!(held.client(), client());
    assert_eq!(held.incarnation(), Some(applied.incarnation()));
    let endpoint = held.connection();
    let press_cell = inventory.holds[0]
        .custody
        .completion
        .as_ref()
        .unwrap()
        .clone();

    let release = fixture.route(42, false, 8812);
    fixture
        .base
        .private
        .broker
        .registry
        .input_recovery
        .admit_typed(&release, 1, Instant::now())
        .unwrap();
    let release_cell = fixture
        .base
        .private
        .broker
        .registry
        .input_recovery
        .completion_for(release.delivery.unwrap())
        .unwrap()
        .unwrap();
    inventory.pending_custody = Some(PrivateDeliveryCustody::new(1, Some(release_cell.clone())));
    let (outcome, built) = fixture.release(
        None,
        inventory.holds[0]
            .native
            .as_mut()
            .unwrap()
            .key_mut()
            .unwrap(),
        42,
        8812,
    );
    assert_eq!(outcome, ReleaseOutcome::DeliverTo(applied.incarnation()));
    assert_eq!(fixture.held(50), crate::XkbPhysicalKeyState::Released);
    let removed = inventory.holds.remove(0);
    inventory.settling.push(PrivateSettlingRelease {
        incarnation: removed.incarnation,
        reached: removed.reached,
        custody: inventory.pending_custody.take().unwrap(),
        press_custody: Some(removed.custody),
        native: removed.native,
        unbuilt: None,
        native_recorded: false,
        native_failure: None,
        native_attempts: 0,
        outcome,
        event: built.unwrap().map(XAuthorityInputEvent::Key),
        delivery: release.delivery,
        binding: PrivateReleaseBinding::Reached,
    });
    let mut carried = inventory.hand_over();
    let release = &mut carried.settling[0];
    assert!(Arc::ptr_eq(release.completion().unwrap(), &release_cell));
    assert!(Arc::ptr_eq(
        release
            .press_custody
            .as_ref()
            .unwrap()
            .completion
            .as_ref()
            .unwrap(),
        &press_cell
    ));
    assert!(release.record_native_once());
    assert!(!release.record_native_once());
    assert_eq!(release.native().unwrap().status(), Status::NativeReconciled);
    assert!(release.outcome_seen().is_none());
    assert!(release.attempt().is_none());
    fixture
        .base
        .private
        .controller
        .under_common_as_origin(|common, _| {
            let (identity, bits) = common.next_debt(&mut 0).unwrap();
            assert_eq!(identity, applied.incarnation());
            assert!(bits.native_reconciled && !bits.recipient_settled);
        })
        .unwrap();
    let native = release.native_mut().unwrap();
    let press = native.take_press_emission().unwrap();
    let released = native.take_release_emission().unwrap();
    assert!(press.endpoint().matches(endpoint.endpoint()));
    assert!(released.endpoint().matches(endpoint.endpoint()));
    assert_eq!(press.incarnation(), released.incarnation());
    assert_eq!(
        press.delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(8811))
    );
    assert_eq!(
        released.delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(8812))
    );
    assert!(native.take_press_emission().is_none());
    assert!(native.take_release_emission().is_none());
    assert!(press_cell.answer().is_none() && release_cell.answer().is_none());
}

#[test]
fn pending_pointer_cannot_be_replaced_by_a_key_installation_slot() {
    let mut fixture = Fixture::new();
    let mut inventory = fixture.private.terminal.hand_over();
    fixture.press(272, inventory.native_pending.pointer_slot().unwrap());
    let incarnation = inventory.native_pending.pointer().unwrap().incarnation();
    assert!(matches!(
        inventory.native_pending.key_slot(),
        Err(Refusal::WrongPhase)
    ));
    assert_eq!(
        inventory.native_pending.pointer().unwrap().incarnation(),
        incarnation
    );
    let retained = inventory.native_pending.take().unwrap();
    assert!(retained.pointer().is_some() && retained.key().is_none());
    assert_eq!(retained.incarnation(), incarnation);
    assert!(inventory.native_pending.key_slot().unwrap().is_none());
}

#[test]
fn native_record_storage_reserves_the_maximum_key_or_pointer_payload() {
    let fixture = KeyFixture::new();
    let inventory = &fixture.base.private.terminal;
    assert_eq!(inventory.holds.capacity(), PRIVATE_HOLD_RECORDS);
    assert_eq!(inventory.settling.capacity(), PRIVATE_HOLD_RECORDS);
    let bytes = PrivateTerminalInventory::native_storage_bytes(
        inventory.holds.capacity(),
        inventory.settling.capacity(),
    )
    .unwrap();
    let each_native =
        std::mem::size_of::<private_native::Hold>().max(std::mem::size_of::<KeyHold>());
    assert!(bytes >= (2 * PRIVATE_HOLD_RECORDS + 1) * each_native);
    assert!(bytes <= isize::MAX as usize);
    assert!(PrivateTerminalInventory::native_storage_bytes(usize::MAX, 1).is_none());
    assert!(PrivateTerminalInventory::native_storage_bytes(1, usize::MAX).is_none());
}
