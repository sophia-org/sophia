fn check_prepared_freeze(
    fixture: &PreparedOrderedFixture,
    previous: Option<&private_native::Freeze>,
) -> Result<private_native::FreezeCheck, private_native::Refusal> {
    let private = fixture.runner.frontend.as_ref().unwrap();
    private.participant.under_boundary(|_, _, bindings| {
        let clients = private.broker.registry.clients.lock().unwrap();
        let native = private.native_owner.as_ref().unwrap().lock_base().unwrap();
        native.freeze(bindings, &clients, previous, true)
    }).unwrap()
}

fn install_prepared_keyboard_freeze(fixture: &PreparedOrderedFixture) {
    let mut grab = public_keyboard_grab(fixture.client);
    grab.window = fixture.window;
    grab.keyboard_mode = 0;
    fixture.runner.frontend.as_ref().unwrap().broker.registry.input_authority
        .lock().unwrap().grab_keyboard(fixture.namespace, grab).unwrap();
}

fn captured_prepared_freeze(fixture: &PreparedOrderedFixture) -> private_native::Freeze {
    let private_native::FreezeCheck::Frozen(frozen) = check_prepared_freeze(fixture, None).unwrap() else {
        panic!("the actual source grab freezes this prepared history");
    };
    frozen
}

#[test]
fn private_freeze_requires_exact_original_source_and_open_admission_for_thaw() {
    let fixture = prepared_ordered_fixture(XServerFrontendClientId::from_raw(9766));
    install_prepared_keyboard_freeze(&fixture);
    let frozen = captured_prepared_freeze(&fixture);
    let authority = &fixture.runner.frontend.as_ref().unwrap().broker.registry.input_authority;
    authority.lock().unwrap().allow_events(fixture.namespace, fixture.client.raw() + 1, 3).unwrap();
    assert!(matches!(check_prepared_freeze(&fixture, Some(&frozen)), Ok(private_native::FreezeCheck::Frozen(_))));
    authority.lock().unwrap().allow_events(fixture.namespace, fixture.client.raw(), 3).unwrap();
    assert!(matches!(check_prepared_freeze(&fixture, Some(&frozen)), Ok(private_native::FreezeCheck::Ready)));
    fixture.runner.frontend.as_ref().unwrap().participant.under_boundary(|_, _, bindings| {
        bindings.bound[&fixture.client].lifecycle.as_ref().unwrap().close();
    }).unwrap();
    assert!(matches!(check_prepared_freeze(&fixture, Some(&frozen)), Err(private_native::Refusal::FreezeInvalidated)));
}

#[test]
fn private_freeze_rejects_identical_replacement_and_colliding_foreign_origin() {
    let fixture = prepared_ordered_fixture(XServerFrontendClientId::from_raw(9767));
    install_prepared_keyboard_freeze(&fixture);
    let frozen = captured_prepared_freeze(&fixture);
    let foreign = prepared_ordered_fixture(fixture.client);
    install_prepared_keyboard_freeze(&foreign);
    assert!(matches!(check_prepared_freeze(&foreign, Some(&frozen)), Err(private_native::Refusal::FreezeInvalidated)));
    let private = fixture.runner.frontend.as_ref().unwrap();
    private.participant.under_boundary(|_, _, bindings| {
        let clients = private.broker.registry.clients.lock().unwrap();
        let native = private.native_owner.as_ref().unwrap().lock_base().unwrap();
        assert!(matches!(native.freeze(bindings, &clients, Some(&frozen), false), Err(private_native::Refusal::FreezeInvalidated)));
    }).unwrap();
    let authority = &fixture.runner.frontend.as_ref().unwrap().broker.registry.input_authority;
    authority.lock().unwrap().ungrab_keyboard(fixture.namespace, fixture.client.raw());
    install_prepared_keyboard_freeze(&fixture);
    authority.lock().unwrap().allow_events(fixture.namespace, fixture.client.raw(), 3).unwrap();
    assert!(matches!(check_prepared_freeze(&fixture, Some(&frozen)), Err(private_native::Refusal::FreezeInvalidated)));
}
