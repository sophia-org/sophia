// The order a release keeps: its source obligation carried rather than dropped,
// and a refusal retained by the delivery rather than discarded.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_final_release_carries_its_source_obligation_instead_of_dropping_it() {
    // The press raises real native state: an implicit activation, a query
    // scope and a selection, all owned by the hold and reachable only through
    // the exact connection it retained. A release that removed the record and
    // left the hold behind would retire none of them and leave nothing able
    // to -- the obligation would still be live with no owner.
    let client = XServerFrontendClientId(2411);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2411),
            272,
            true,
        ))
        .expect("the order to accept the press");
        let press_cell = admitted_cell(private, 2411);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2411)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        private.terminal.holds[0].native.is_some(),
        "the press left a source obligation on its record"
    );

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2412),
            272,
            false,
        ))
        .expect("the order to accept the release");
        let release_cell = admitted_cell(private, 2412);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("the release owes its recipient an event");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2412)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        private.terminal.holds.is_empty(),
        "the hold ended, so its record is gone"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "and its release is owed to whoever settles it"
    );
    assert!(
        private.terminal.settling[0].native().is_some(),
        "the obligation travelled with the release rather than dying with the \
         record: nothing else holds the connection its activation, query scope \
         and selection belong to"
    );
}

#[test]
fn a_final_release_clears_what_its_press_projected_and_reports_it() {
    let client = XServerFrontendClientId(761);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(761), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    assert!(run.first_press);
    let _ = pressed.observe();
    assert_eq!(
        projected_buttons(private, namespace, seat),
        256,
        "the press projects button one"
    );

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(762), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,

                watch,
            )
        .expect("the release to run");
    let Some(XAuthorityInputEvent::Pointer(event)) = run.event else {
        panic!("a final release owes an event");
    };
    assert!(
        matches!(
            event.kind,
            XAuthorityPointerEventKind::Button {
                button: 1,
                pressed: false
            }
        ),
        "it lifts button one"
    );
    assert_eq!(
        event.state, 256,
        "and reports the state before it, which still has that button down"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "the projection is cleared by the release, not left held"
    );
    let _ = released.observe();

    // A new press on the same input is refused until the release that just
    // happened is settled. The authority holds a barrier for it, and this path
    // has no settlement step yet -- so the projection being clean is what can
    // be shown here, and the barrier is named rather than worked around.
    let again = role.reserve(stamp, 3).expect("a reservation").accepted();
    let barred = private.run_ordered_input(
        keyboards,
        &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(763), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
        &again,

                watch,
            );
    assert!(
        matches!(
            barred,
            // Renamed, not reclassified: the source's press carries the
            // authority's own ReleaseBarrier out under Refusal::Authority,
            // so the barrier is still the named cause -- one layer in,
            // because the press is where the authority is now entered.
            Err(crate::PrivateExecutionRefusal::Native(
                private_native::Refusal::Authority(
                    sophia_input_authority::RegistrationError::ReleaseBarrier
                )
            ))
        ),
        "the release's debt bars the next press until it is settled, got {barred:?}"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "and the projection stayed clear, so no stale bit is hiding behind it"
    );
}

#[test]
fn a_release_with_a_survivor_leaves_the_projection_alone() {
    let client = XServerFrontendClientId(771);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Two presses on one input: the second joins, so two participants hold it.
    for (request, delivery) in [(1, 771), (2, 772)] {
        let custody = role.reserve(stamp, request).expect("a reservation").accepted();
        private
            .run_ordered_input(
                keyboards,
                &{
                let route = button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(delivery),
                    272,
                    true,
                );
                admit_for_direct_run(private, &route);
                route
            },
                &custody,

                watch,
            )
            .expect("the press to run");
        let _ = custody.observe();
    }
    assert_eq!(projected_buttons(private, namespace, seat), 256);

    // One release. Whether the aggregate is now clear is the ledger's to say,
    // and the projection must not be cleared while anyone still holds it.
    let released = role.reserve(stamp, 3).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(773), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,

                watch,
            )
        .expect("the release to run");
    match run.release.expect("a release outcome") {
        sophia_input_authority::ReleaseOutcome::SurvivorRemains => {
            assert!(run.event.is_none(), "a survivor owes nobody an event");
            assert_eq!(
                projected_buttons(private, namespace, seat),
                256,
                "and the button somebody still holds stays projected"
            );
        }
        sophia_input_authority::ReleaseOutcome::DeliverTo(_) => {
            assert_eq!(
                projected_buttons(private, namespace, seat),
                0,
                "a final release clears it"
            );
        }
        sophia_input_authority::ReleaseOutcome::NotHeld => {
            panic!("the input was held")
        }
    }
}

#[test]
fn a_ledger_owed_release_without_its_plan_refuses_rather_than_reporting_nothing() {
    let client = XServerFrontendClientId(781);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(781), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    let _ = pressed.observe();

    // The record of where the press went is lost. The ledger still ends the
    // hold, so somebody is owed the event that lifts the button -- and saying
    // "nothing to emit" would settle that debt by losing the evidence of it.
    private.terminal.holds.clear();

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(782), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
        &released,

                watch,
            );
    assert!(
        matches!(refused, Err(crate::PrivateExecutionRefusal::HoldPlanMissing)),
        "a hold that ended has a recipient; not knowing who is not the same as owing nobody, got {refused:?}"
    );
}

#[test]
fn a_release_whose_seat_projection_is_gone_retains_a_residual_rather_than_refusing() {
    let client = XServerFrontendClientId(791);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(791), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    let _ = pressed.observe();

    // The projection this press moved is gone. Building a fresh mapper here
    // would assert a clear history -- that the button was never down -- and
    // report a release against state that never held it.
    private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .remove(&(namespace, seat));

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(792), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
            watch,
        )
        // OLD CLAIM: the whole release is refused, because retained state that
        //   has become unavailable is not a fresh clear history.
        // NEW CLAIM: a missing mapper after DeliverTo is a POST-EFFECT
        //   RESIDUAL, not a refusal of the release. The aggregate release
        //   already happened in the ledger, and refusing here would discard a
        //   transition that had occurred rather than prevent one.
        // The original concern is unchanged and still asserted below: nothing
        // rebuilds the mapper, so no clear history is ever asserted.
        .expect("the aggregate release to occur despite the missing projection");

    // The aggregate release occurred, and the ledger says whose.
    assert!(
        matches!(
            run.release,
            Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
        ),
        "the hold ended and its delivery was decided"
    );
    assert_eq!(private.terminal.settling.len(), 1);

    // The required event remains UNBUILT, and says why. Owed and absent is
    // not the same as never owed: a reader finding an empty slot with no
    // cause could not tell this release from one that owed nothing.
    assert!(run.owes_event, "the recipient is still owed its release event");
    assert!(run.event.is_none(), "and it could not be built");
    assert_eq!(
        private.terminal.settling[0].unbuilt(),
        Some(PrivateAppliedRefusal::Interrupted),
        "the cause travels with the debt rather than being flattened away"
    );

    // The exact hold is retained against what actually went missing.
    assert!(
        matches!(
            private.terminal.settling[0]
                .native()
                .expect("the release carries its source obligation")
                .status(),
            private_native::Status::Retained(private_native::Residual::MissingMapper)
        ),
        "the obligation is retained naming the projection that is gone"
    );

    // AND THE MAPPER IS NOT RECREATED. This is the whole of the original
    // concern: a fresh mapper would assert the button was never down.
    assert!(
        !private
            .broker
            .registry
            .pointer_state
            .lock()
            .expect("the pointer state")
            .contains_key(&(namespace, seat)),
        "nothing rebuilt the projection, so no clear history is asserted"
    );

    // Neither half is inferred. There is no proof -- the release ended in a
    // residual -- so nothing recorded the native bit, and no receipt has
    // arrived to settle the recipient's.
    assert!(
        !private.terminal.settling[0].native_recorded(),
        "a residual produces no proof, so the native half stays owed"
    );
    let mut cursor = 0;
    let reported = private
        .authority()
        .under_common(|authority| authority.next_debt(&mut cursor))
        .expect("the authority to be readable")
        .expect("a debt for the release that just happened");
    assert!(
        !reported.1.native_reconciled,
        "the native half is not inferred from the release having happened"
    );
    assert!(
        !reported.1.recipient_settled,
        "and neither is the recipient's"
    );
}

#[test]
fn work_sent_through_the_ingress_runs_from_the_order_it_was_accepted_into() {
    let client = XServerFrontendClientId(801);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // The producer handle, with the reservation role bound to this client.
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // Sent through the producer. Nothing here builds custody by hand: the
    // reservation is made at submission, travels on the envelope, and is what
    // the consumer runs against.
    let lease = keeper.lease();
    let sequence = ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(801),
            272,
            true,
        ))
        .expect("the order to accept it");

    let mut turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1, "the order held exactly what was sent");
    let item = turn.remove(0);
    let PrivateOrderedItem::Ran {
        sequence: ran_sequence,
        run,
        custody,
        route,
    } = item
    else {
        panic!("the queued work ran");
    };
    assert_eq!(
        ran_sequence, sequence,
        "and it is the same place in the order the producer was given"
    );
    assert_eq!(
        route.delivery,
        Some(XAuthorityInputDeliveryId::from_raw(801)),
        "the accepted work comes back with it, delivery identity and all"
    );
    let reached = run.reached.expect("a press decides where it went");
    assert_eq!(reached.client(), client);
    assert_eq!(reached.window(), window);
    assert!(run.first_press);
    assert!(run.event.is_some(), "a first press owes an event");

    // The custody came back rather than being dropped inside the turn, so the
    // outcome is still there to take.
    assert!(matches!(
        custody.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // The order is empty now: the turn consumed it rather than copying it.
    assert!(
        private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order")
            .is_empty()
    );
}

#[test]
fn a_consumer_refusal_hands_back_the_custody_it_was_accepted_with() {
    let client = XServerFrontendClientId(811);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // A key: accepted by the order, refused by the consumer because no applied
    // focus can name where it would go.
    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(811));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    let lease = keeper.lease();
    ingress.submit(&lease, key).expect("the order to accept it");

    let mut turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1);
    let PrivateOrderedItem::Refused {
        sequence,
        refusal,
        custody,
        route,
    } = turn.remove(0)
    else {
        panic!("the consumer refused this one");
    };
    assert!(
        sequence.raw() > 0,
        "the refusal names the place in the order the work held"
    );
    assert!(matches!(
        refusal,
        crate::PrivateExecutionRefusal::Native(private_native::Refusal::Resolution(PrivateAppliedRefusal::FocusNotApplied))
    ));
    assert_eq!(
        route.request.target_surface, surface,
        "the work it was accepted for comes back whole"
    );

    // And so does the request the order took. A consumer declining to run
    // something is not the order never having accepted it, so the custody is
    // still here to be settled rather than erased by the decision.
    assert!(
        matches!(custody.observe(), Ok(Some(sophia_input_authority::RequestCompletion::Refused(
            sophia_input_authority::RegistrationError::StaleExecution
        )))),
        "the guarded source refused; its authority completion remains observable"
    );
}

#[test]
fn unreserved_work_in_the_order_is_handed_back_rather_than_run() {
    let client = XServerFrontendClientId(821);
    let surface = SurfaceId::new(821, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200821, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // The plain ingress reserves nothing, so this reaches the order without a
    // request behind it. The ordered path runs what was reserved before it was
    // published; something else accepted this, and running it would execute
    // against a request that does not exist.
    private
        .ingress()
        .submit(&service_keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(821),
            272,
            true,
        ))
        .expect("the order to accept it");

    let mut private = private;
    let mut turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert_eq!(turn.len(), 1);
    let PrivateOrderedItem::Parked { sequence } = turn.remove(0) else {
        panic!("work this path does not execute is parked, not run");
    };
    assert!(sequence.raw() > 0);
    assert_eq!(
        private.parked(),
        Some(sequence),
        "the report names it and the operation stays owned until something takes it"
    );

    // Still parked, so a later turn runs nothing rather than overtaking it.
    let again = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(
        matches!(again.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "a blocked turn says so rather than looking like an empty one"
    );

    // Taking it is what accepts responsibility, and it comes back whole.
    let (taken_sequence, operation) = private.take_parked().expect("the parked operation");
    assert_eq!(taken_sequence, sequence);
    assert!(matches!(operation, PrivateOperation::RoutedInput(_)));
    assert_eq!(private.parked(), None);

    // Nothing was pressed, so no hold was recorded against it.
    assert!(private.terminal.holds.is_empty());
}

#[test]
fn no_input_applies_past_an_earlier_operation_that_has_not_run() {
    let client = XServerFrontendClientId(831);
    let surface = SurfaceId::new(831, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200831, 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // An operation this path parks first, then input. A routed input that
    // carries no reservation is one the ordered path does not execute (a
    // control is routed from this order now, so it is no longer the
    // example): it stays parked, and the order is blocked behind it.
    private
        .submit(&service_keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(8310),
            272,
            true,
        ))
        .expect("the order to accept the unreserved input");
    ingress
        .submit(&service_keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(831),
            272,
            true,
        ))
        .expect("the order to accept the input");

    let mut private = private;
    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");

    // The report being in order is not enough. The effect order is what
    // matters: the button must not have applied while an earlier operation has
    // neither executed nor been cancelled.
    assert!(
        matches!(turn.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "the turn stops at the earlier operation rather than running past it"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "and no later hold was applied behind it"
    );

    // A second turn does not overtake it either. Stopping for one turn would
    // only move the problem to the next.
    let again = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(matches!(
        again.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert!(private.terminal.holds.is_empty());

    // Handing the operation to an owner does not lift the barrier. Taking it
    // moves it; it does not establish what becomes of it, and an owner that
    // took it and then dropped it has answered nothing. Until a path exists
    // that executes or cancels such an operation, the order stays blocked --
    // which is the honest state rather than a convenient one.
    let (_, parked) = private.take_parked().expect("the parked operation");
    assert!(matches!(parked, PrivateOperation::RoutedInput(_)));
    let after = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(
        matches!(after.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "holding the operation is not having answered for it"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "so no later hold applied behind it"
    );
    assert!(
        private.blocked().is_some(),
        "and the order says it is still blocked"
    );
}

#[test]
fn the_older_route_refuses_an_order_the_ordered_consumer_is_draining() {
    let client = XServerFrontendClientId(841);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // The ordered consumer takes a turn, which claims this order.
    assert!(
        private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order")
            .is_empty()
    );

    // Work is accepted with a reservation made for it.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(841),
            272,
            true,
        ))
        .expect("the order to accept it");

    // The older route discards the reservation and applies without the
    // execution it exists for, so it must not drain this order alongside.
    let lease = keeper.lease();
    let refused = private.route_pending(&lease);
    assert!(
        matches!(
            refused,
            Err(XServerFrontendRouteError::OrderedRunnerEngaged)
        ),
        "one permitted consumer per order, got {refused:?}"
    );

    // And the work is still there for the consumer that may run it.
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(
        matches!(turn.as_slice(), [PrivateOrderedItem::Ran { .. }]),
        "the refused drain took nothing away from the order"
    );
}

#[test]
fn a_turn_that_fails_part_way_keeps_what_it_already_took() {
    let client = XServerFrontendClientId(851);
    let surface = SurfaceId::new(851, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200851, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");
    let mut private = private;

    // A real sequence from a real submission, so nothing here invents an
    // identity the order never issued.
    private
        .ingress()
        .submit(&service_keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(851),
            272,
            true,
        ))
        .expect("the order to accept it");
    let parked = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    let [PrivateOrderedItem::Parked { sequence }] = parked.as_slice() else {
        panic!("unreserved work parks");
    };
    let sequence = *sequence;
    let _ = private.take_parked();
    // Cleared here only to reach the case under test. Nothing in production
    // lifts this yet, which is the point of the control above; this one is
    // about what a failed turn keeps, not about when the order resumes.
    private.parked_barrier = None;

    // Staged as an earlier iteration leaves it: work already taken out of the
    // order and recorded, with the turn still in progress. Staged rather than
    // raced, because making the queue fail between two real iterations is not
    // something a test can arrange deterministically.
    private.terminal.turn.push(PrivateOrderedItem::Parked { sequence });

    // The order becomes unreadable.
    let ready = std::sync::Arc::clone(&private.admission.ready);
    assert!(
        std::thread::spawn(move || {
            let _guard = ready.lock().unwrap();
            panic!("poisoning the order");
        })
        .join()
        .is_err()
    );

    let failed = private.route_pending_ordered(&mut keyboards, &control_watchdog());
    assert!(failed.is_err(), "the turn could not read the order");

    // What it had already taken is still owned. Returning results only on
    // success would drop every earlier iteration's work on a later one's
    // failure, and that work has left the order -- nothing else holds it.
    let recovered = private.take_interrupted_turn();
    assert_eq!(
        recovered.len(),
        1,
        "the interrupted turn's items survived the failure"
    );
    assert!(matches!(
        recovered.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert!(
        private.take_interrupted_turn().is_empty(),
        "and recovering them twice yields nothing the second time"
    );
}

#[test]
fn queuing_an_event_is_not_the_receipt_that_closes_a_release_debt() {
    let client = XServerFrontendClientId(861);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    // Registered with channels held, so a delivered event has somewhere to go.

    // Press, run, deliver.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(861),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let press_cell = admitted_cell(private, 861);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press was accepted onto the client's queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(861)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "and its outcome was taken exactly once, which is what freed the cell"
    );
    assert!(
        !delivered[0].debt_settled,
        "a press creates no release debt to close"
    );
    assert!(
        inbox.taken.is_empty() && channels.ordered.try_recv().is_err(),
        "and nothing else was put on that queue"
    );
    // The window is the source's own resolution, which the capsule carries in
    // its encoded frames rather than exposing as a field.
    assert_eq!(private.terminal.holds[0].reached.window(), window);

    // Release, run, deliver. Queuing establishes neither half: the recipient
    // half is the writer's outcome, and what the guarded code shows is that
    // the aggregate transition and the projection it moves happen in one
    // interval -- which is not the whole of native reconciliation.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(862),
            272,
            false,
        ))
        .expect("the order to accept the release");
        let release_cell = admitted_cell(private, 862);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("the release was queued too");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(862)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        delivered[0].sequence.raw() > 0,
        "each delivery names the place in the order it came from"
    );
    assert!(
        !delivered[0].debt_settled,
        "but no debt is closed by queuing: the recipient half is the writer's \
         outcome, and nothing here has observed one"
    );
    assert!(
        inbox.taken.is_empty() && channels.ordered.try_recv().is_err(),
        "and the release was the only thing behind it"
    );
    assert!(
        !private.terminal.settling.is_empty(),
        "so the continuation is retained, because the obligation is still open"
    );

    // And the same input still cannot press again. The release barrier stands
    // until the debt is genuinely closed, which needs a receipt this path
    // cannot yet obtain -- so the honest state is barred, not resumed.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(863),
            272,
            true,
        ))
        .expect("the order to accept the second press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let [PrivateOrderedItem::Refused { refusal, .. }] = turn.as_slice() else {
        panic!("the second press is barred while the debt is open");
    };
    assert!(
        matches!(
            refusal,
            // Renamed, not reclassified: the authority's own ReleaseBarrier
            // travels out of the source press under Refusal::Authority.
            crate::PrivateExecutionRefusal::Native(
                private_native::Refusal::Authority(
                    sophia_input_authority::RegistrationError::ReleaseBarrier
                )
            )
        ),
        "with the authority's own barrier, got {refusal:?}"
    );
}

#[test]
fn a_refusal_is_retained_by_delivery_rather_than_discarded() {
    let client = XServerFrontendClientId(871);
    let surface = SurfaceId::new(871, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = private_for_roles(&service_keeper);
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200871, 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let mut keyboards = private.keyboards().expect("this instance's state");

    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(871));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    ingress.submit(&service_keeper.lease(), key).expect("the order to accept it");

    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "a refusal is not a delivery, so it reports none"
    );

    // And it is not nothing either. Dropping it destroyed the custody the
    // order accepted, so the same request answered stale afterwards instead of
    // saying no outcome had been taken.
    assert_eq!(
        private.terminal.undelivered.len(),
        1,
        "the refusal is retained whole, with its custody"
    );
    let [PrivateUndelivered {
        item: PrivateOrderedItem::Refused { custody, .. },
    }] = private.terminal.undelivered.as_slice()
    else {
        panic!("retained as the refusal it was");
    };
    assert!(
        matches!(custody.observe(), Ok(None)),
        "no outcome was taken, which is a different answer from a stale request"
    );

    // A finite turn must still reach a publishable tail behind an unresolved
    // head. These are actual prepared/common refusals; removing their original
    // tickets is a staged publication fault, not a replacement completion.
    let durable = PrivateSettlementOwner::with_capacity(4);
    let ready_client = XServerFrontendClientId::from_raw(872);
    let mut fixture = prepared_ordered_fixture_with_store(ready_client, durable.clone());
    let recovery = fixture.runner.frontend().broker.registry.input_recovery.clone();
    let mut retained = Vec::new();
    for raw in [8721, 8722] {
        let id = XAuthorityInputDeliveryId::from_raw(raw);
        fixture.ingress.submit(&fixture.keeper.lease(), motion_to(fixture.surface, id)).unwrap();
        step_refused_request(&mut fixture);
        let entry = recovery.state.lock().unwrap().tickets.remove(&id).unwrap();
        retained.push((id, entry));
        assert!(fixture.runner.frontend.as_mut().unwrap().deliver_turn(Vec::new()).is_empty());
    }
    assert_eq!(durable.reserved(), Some(2));
    let (tail_id, tail) = retained.pop().unwrap();
    let tail_cell = tail.completion.clone();
    assert!(recovery.state.lock().unwrap().tickets.insert(tail_id, tail).is_none());
    let (head_id, head) = retained.pop().unwrap();
    let head_cell = head.completion.clone();
    let private = fixture.runner.frontend.as_mut().unwrap();
    let PrivateOrderedItem::Refused { custody, .. } = &private.terminal.undelivered[0].item else {
        panic!("the unresolved head is first before the finite retry pass");
    };
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &head_cell));
    assert!(private.deliver_turn(Vec::new()).is_empty());
    assert_eq!(private.terminal.undelivered.len(), 1);
    let PrivateOrderedItem::Refused { custody, .. } = &private.terminal.undelivered[0].item else {
        panic!("the unresolved original head remains owned");
    };
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &head_cell));
    assert_eq!(head_cell.answer(), None);
    assert_eq!(tail_cell.answer(), Some(XAuthorityClientInputDelivery {
        client: ready_client,
        delivery: tail_id,
        outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
    }));
    assert_eq!(durable.reserved(), Some(1), "only the tail returned its original credit");
    assert!(recovery.state.lock().unwrap().tickets.insert(head_id, head).is_none());
    assert!(private.deliver_turn(Vec::new()).is_empty());
    assert!(private.terminal.undelivered.is_empty());
    assert_eq!(head_cell.answer().unwrap().outcome, XAuthorityInputDeliveryOutcome::RouteRejected);
    assert_eq!(durable.reserved(), Some(0));
}
