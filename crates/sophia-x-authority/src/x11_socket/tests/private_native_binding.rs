// These exercise the native source operation with a real recovery ticket and
// common permit. The callback changes recovery after actual target resolution,
// before the source's mandatory bind; this is not a producer/consumer turn test.
fn admitted_native_press(fixture: &Fixture, delivery: u64) -> XAuthorityRoutedInput {
    let mut route = fixture.route(272, true);
    route.delivery = Some(XAuthorityInputDeliveryId::from_raw(delivery));
    fixture
        .private
        .broker
        .registry
        .input_recovery
        .admit_typed(&route, 1, Instant::now())
        .unwrap();
    route
}

fn press_with_recovery_transition(
    fixture: &Fixture,
    route: &XAuthorityRoutedInput,
    storage: &mut Option<Hold>,
    applied: &Cell<bool>,
    after_resolution: impl FnOnce(&InputRecovery),
) -> Result<
    (
        sophia_input_authority::Applied,
        Option<XAuthorityPointerEvent>,
    ),
    Refusal,
> {
    let recovery = &fixture.private.broker.registry.input_recovery;
    assert_eq!(
        recovery.claim_execution(route.delivery),
        ExecutionClaim::Claimed
    );
    let _claim = PrivateDeliveryClaim {
        recovery,
        delivery: route.delivery,
        applied,
    };
    fixture.run(&fixture.role, |permit, bindings| {
        let clients = fixture.private.broker.registry.clients.lock().unwrap();
        fixture.owner.lock_base()?.press(
            permit,
            fixture.role.capability,
            route,
            window(),
            implicit(),
            storage,
            applied,
            |recipient| {
                fixture.private.broker.registry.applied_client(
                    &clients,
                    recipient,
                    &bindings.bound[&recipient],
                )
            },
            |witness, selected, prepared, event| {
                let plan = witness
                    .lock_publication()
                    .unwrap()
                    .view(client(), selected, prepared.authority())?
                    .pointer(
                        window(),
                        *event,
                        None,
                        PrivatePointerSelection::Prepared(prepared),
                    )?;
                after_resolution(recovery);
                Ok(plan)
            },
        )
    })
}

#[test]
fn native_press_binds_its_original_delivery_before_applying() {
    let fixture = Fixture::new();
    let route = admitted_native_press(&fixture, 810);
    let recovery = &fixture.private.broker.registry.input_recovery;
    assert_eq!(
        recovery.ticket(route.delivery.unwrap()).unwrap().client,
        None
    );
    let mut hold = None;
    let applied = Cell::new(false);
    let result =
        press_with_recovery_transition(&fixture, &route, &mut hold, &applied, |_| {}).unwrap();
    assert!(result.0.first_press());
    assert!(applied.get());
    assert_eq!(
        recovery.ticket(route.delivery.unwrap()).unwrap().client,
        Some(client())
    );
    assert!(recovery.active(route.delivery, client()));
    let emission = hold.as_mut().unwrap().take_press_emission().unwrap();
    assert_eq!(emission.delivery(), route.delivery);
    assert_eq!(emission.incarnation(), result.0.incarnation());
}

#[test]
fn native_press_binding_refusal_leaves_no_hidden_common_hold() {
    let fixture = Fixture::new();
    let route = admitted_native_press(&fixture, 811);
    let mut hold = None;
    let applied = Cell::new(false);
    let result =
        press_with_recovery_transition(&fixture, &route, &mut hold, &applied, |recovery| {
            let mut state = recovery.state.lock().unwrap();
            recovery
                .disconnect_locked(
                    &mut state,
                    client(),
                    XAuthorityInputDeliveryOutcome::ClientDisconnected,
                    None,
                )
                .unwrap();
        });
    assert_eq!(result.unwrap_err(), Refusal::DeliveryEnded);
    // run() observed the refused common completion. The same grant can now
    // interrogate the actual ledger through a new press: it must begin, not
    // join a hidden hold. Checking only the executor's Option cannot prove it.
    let next = fixture
        .run(&fixture.role, |permit, _| {
            permit
                .press(
                    sophia_input_authority::Input::button(
                        1,
                        sophia_input_authority::Capacity::PLANNED.button_domain(),
                    )
                    .unwrap(),
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
        "the refused native press left a hidden common hold"
    );
    assert!(!applied.get());
    assert!(hold.is_none());
    let pointer = fixture
        .private
        .broker
        .registry
        .pointer_state
        .lock()
        .unwrap();
    assert!(pointer[&(namespace(), seat())].all_buttons_released());
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
}

#[test]
fn native_press_unreadable_binding_is_not_an_ended_delivery() {
    let fixture = Fixture::new();
    let route = admitted_native_press(&fixture, 812);
    let mut hold = None;
    let applied = Cell::new(false);
    let result =
        press_with_recovery_transition(&fixture, &route, &mut hold, &applied, |recovery| {
            let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _state = recovery.state.lock().unwrap();
                panic!("poison the recovery ledger after exact target resolution");
            }));
            assert!(poisoned.is_err());
        });
    assert_eq!(result.unwrap_err(), Refusal::RecoveryUnavailable);
    assert!(!applied.get());
    assert!(hold.is_none());
    let state = fixture
        .private
        .broker
        .registry
        .input_recovery
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ticket = &state.tickets[&route.delivery.unwrap()];
    assert!(
        !ticket.claimed,
        "the claim guard still returns through poison"
    );
    assert!(ticket.terminal.is_none());
}
