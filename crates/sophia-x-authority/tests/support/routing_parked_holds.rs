// Holds parked across a turn: what an unresolved current item owes, what a
// durable owner keeps, and the ordered press whose delivery has ended.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_later_turn_does_not_overwrite_an_unresolved_current_item() {
    let client = XServerFrontendClientId(881);
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

    // Staged as an interruption before the effect leaves it: an item taken
    // from the order, owned, with execution not attempted.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(881),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let [PrivateOrderedItem::Ran { .. }] = turn.as_slice() else {
        panic!("the first item ran");
    };
    let taken = turn.into_iter().next().expect("the item");
    private.terminal.current = Some(taken);

    // More work arrives, from another producer: the first one's cell is still
    // busy, because the outcome of the item now held was never taken. A turn
    // that dequeued into the same slot would overwrite the only record of an
    // item already taken, whose application nobody can describe.
    let other = XServerFrontendClientId(882);
    let _other_registration = admit_role_client(private, other);
    private
        .broker
        .registry
        .register_surface(
            other,
            NamespaceId::from_raw(other.raw()),
            SurfaceId::new(882, 1),
            XResourceId::new(0x200882, 1),
        )
        .expect("the second surface to register");
    let lease = keeper.lease();
    private
        .ingress_for(other, DeviceId::from_raw(2))
        .expect("a second ingress")
        .submit(&lease, button_to(
            SurfaceId::new(882, 1),
            XAuthorityInputDeliveryId::from_raw(882),
            272,
            true,
        ))
        .expect("the order to accept it");
    let blocked = private.route_pending_ordered(keyboards, watch);
    assert!(
        matches!(
            blocked,
            Err(XServerFrontendRouteError::OrderedItemUnresolved)
        ),
        "the order refuses rather than overwriting it"
    );
    assert!(
        private.terminal.current.is_some(),
        "and the earlier item is still owned"
    );
}

#[test]
fn a_duplicate_that_owes_no_event_still_completes_so_its_hold_can_be_released() {
    let client = XServerFrontendClientId(891);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    // The cell is taken where the admission mints it, and travels out with the
    // reports, so every claim below names the admission it came from.
    let run_one_submission = |private: &mut crate::PrivateXServerFrontend,
                                  keyboards: &mut crate::PrivateKeyboards,
                                  delivery: u64,
                                  pressed: bool| {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let cell = admitted_cell(private, delivery);
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        (private.deliver_turn(turn), cell)
    };

    // A press that begins the hold: an event is owed and delivered.
    let (delivered, press_cell) = run_one_submission(private, keyboards, 891, true);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press reached its recipient");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(891)
    );
    assert_eq!(capsule.client(), client);

    // The same input pressed again joins the hold. It owes nobody an event,
    // which is an outcome rather than a failure to emit one -- so its
    // completion is taken and the grant's cell is freed.
    let (delivered, join_cell) = run_one_submission(private, keyboards, 892, true);
    assert_eq!(
        delivered.len(),
        1,
        "a join is reported as what happened, not retained for want of an event"
    );
    assert!(
        inbox
            .accepted(private, &channels.ordered, &join_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "nobody was owed one"
    );
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "and its outcome was taken, which is what frees the cell"
    );
    assert!(
        private.terminal.undelivered.is_empty(),
        "nothing is owed, so nothing is retained"
    );

    // Which means the hold this grant still owns can be released. Retaining
    // the join would have left the grant unable to reserve, holding a button
    // it could never let go of.
    let (delivered, release_cell) = run_one_submission(private, keyboards, 893, false);
    assert_eq!(delivered.len(), 1, "the release reserved and ran");
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("and it owed an event, which was queued");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(893)
    );
    assert_eq!(capsule.client(), client);
}

#[test]
fn a_parked_operation_is_handed_to_the_durable_owner_at_shutdown() {
    let client = XServerFrontendClientId(901);
    let surface = SurfaceId::new(901, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, _receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200901, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // A control, parked below by a refused start.
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9010),
                surface,
            },
        })
        .expect("the order to accept the control");
    // PARKED BY A REFUSED START. A control is routed from this order now,
    // so what parks it is the budget hook refusing its dequeue: the control
    // stays parked, un-attempted, behind its barrier.
    assert!(
        private
            .step_once(
                &mut keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                &control_watchdog(),
            )
            .is_err(),
        "a refused start is the step's error"
    );
    assert!(private.parked().is_some());

    let owed_before = durable.owed().expect("a readable owner");

    // Shutdown. The parked operation never ran and carries no custody, so an
    // instance that is going must hand it on rather than take it along: it is
    // work this instance accepted and could not answer, which is exactly what
    // the durable owner holds.
    drop(private.shutdown());
    assert!(
        durable.owed().expect("a readable owner") > owed_before,
        "the parked operation reached the owner rather than dying with the instance"
    );
}

#[test]
fn an_unreadable_observation_retains_its_entry_without_losing_the_event() {
    let client = XServerFrontendClientId(911);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(911),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");

    // The authority becomes unreadable after the execution and before the
    // observation. Nothing is sent on this path, so the window is exactly
    // between the request applying and its completion being read.
    let poisoner = private.authority().clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.common.lock().unwrap();
            panic!("poisoning the authority");
        })
        .join()
        .is_err()
    );

    let recovery = private.broker.registry.input_recovery.clone();
    let cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(911))
        .expect("a readable recovery")
        .expect("the admission minted a cell");

    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "nothing could be reported: the outcome was never established"
    );

    // Retained whole, because the handle able to take that observation later
    // is the item itself. Dropping it would leave the request answerable by
    // nobody.
    assert_eq!(
        private.terminal.undelivered.len(),
        1,
        "the unobservable entry is kept, not discarded"
    );

    // AND THE EVENT IS NOT LOST WITH IT. What could not be read was the
    // request's completion; the event is owed by the record that holds its
    // custody, and that record outlives this entry. So the handover still
    // happens, exactly once, carrying the cell the admission minted.
    let handed: Vec<_> = channels.ordered.try_iter().collect();
    assert_eq!(
        handed.len(),
        1,
        "an unreadable observation is not a reason to drop or repeat the event"
    );
    assert_eq!(
        handed[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(911)
    );
    assert!(
        Arc::ptr_eq(
            &cell,
            &handed[0].finalizer().expect("carried").completion
        ),
        "the exact admission's cell travels with it"
    );
    assert!(
        cell.answer().is_none(),
        "and no answer was invented for a request nobody could read"
    );
}

#[test]
fn a_parked_control_is_answered_exactly_once_after_shutdown() {
    let client = XServerFrontendClientId(921);
    let surface = SurfaceId::new(921, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, acks) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200921, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9210),
                surface,
            },
        })
        .expect("the order to accept the control");
    // PARKED BY A REFUSED START. A control is routed from this order now,
    // so what parks it is the budget hook refusing its dequeue: the control
    // stays parked, un-attempted, behind its barrier.
    assert!(
        private
            .step_once(
                &mut keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                &control_watchdog(),
            )
            .is_err(),
        "a refused start is the step's error"
    );

    // The credit this operation holds, read before shutdown. This does not
    // reserve a second one -- an earlier version of this comment said it did,
    // which was false -- so the assertion below is that exactly one credit is
    // returned from a known starting count, not that a duplicate release
    // could not saturate at zero.
    let reserved_before = durable.reserved().expect("a readable owner");
    assert_eq!(
        reserved_before, 1,
        "one accepted operation, one credit held"
    );

    drop(private.shutdown());

    // Exactly one acknowledgement for transaction 9210, whichever path
    // produced it.
    let mut answers = Vec::new();
    while let Ok(ack) = acks.try_recv() {
        answers.push(ack.acknowledgement.transaction);
    }
    let _drive = durable.drive();
    while let Ok(ack) = acks.try_recv() {
        answers.push(ack.acknowledgement.transaction);
    }
    let mine: Vec<_> = answers
        .iter()
        .filter(|transaction| **transaction == TransactionId::from_raw(9210))
        .collect();
    assert_eq!(
        mine.len(),
        1,
        "one operation, one answer -- handing the command on while its record \
         stayed available gave two owners able to publish for it, got {answers:?}"
    );

    // And the credit it held was released once, not twice.
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        reserved_before - 1,
        "exactly one credit returned"
    );
}

#[test]
fn an_accepted_handover_is_observed_rather_than_offered_again() {
    let client = XServerFrontendClientId(941);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(941),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");

    private.terminal.delivering.extend(turn);

    let recovery = private.broker.registry.input_recovery.clone();
    let cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(941))
        .expect("a readable recovery")
        .expect("the admission minted a cell");

    assert_eq!(
        channels.ordered.try_iter().count(),
        0,
        "nothing has been handed over by this test yet"
    );

    // A REAL HANDOVER, not a staged phase. The source takes its own emission,
    // builds its own wrapper and the queue actually accepts it -- which is the
    // only thing that can make a second offer a duplicate.
    let delivered = private.deliver_turn(Vec::new());
    let queued: Vec<_> = channels.ordered.try_iter().collect();
    assert_eq!(queued.len(), 1, "the event reached its recipient exactly once");
    assert_eq!(
        queued[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(941)
    );
    assert!(
        Arc::ptr_eq(
            &cell,
            &queued[0].finalizer().expect("carried").completion
        ),
        "carrying the cell this exact admission minted"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Enqueued,
        "and the custody records that it happened"
    );

    // Observed, never sent again. The entry's own step sends nothing at all,
    // and the custody that could send refuses to: an event already taken by a
    // queue owes only its outcome, and offering it again would deliver the
    // same transition twice with nothing downstream able to tell.
    for _ in 0..8 {
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("a readable terminal step");
    }
    assert_eq!(
        channels.ordered.try_iter().count(),
        0,
        "an accepted handover is not repeated"
    );
    assert_eq!(delivered.len(), 1, "and its outcome was taken");
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Enqueued,
        "the phase that says so is not reset by a later visit"
    );
    assert!(
        private.terminal.holds[0].custody.pending.is_none(),
        "and no wrapper is rebuilt for it"
    );
    assert!(cell.answer().is_none(), "the recipient has still not answered");
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "which is what frees the grant's cell"
    );
}

#[test]
fn a_parked_control_whose_registry_is_unreadable_is_kept_whole() {
    let client = XServerFrontendClientId(951);
    let surface = SurfaceId::new(951, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, _acks) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200951, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9510),
                surface,
            },
        })
        .expect("the order to accept the control");
    // PARKED BY A REFUSED START. A control is routed from this order now,
    // so what parks it is the budget hook refusing its dequeue: the control
    // stays parked, un-attempted, behind its barrier.
    assert!(
        private
            .step_once(
                &mut keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                &control_watchdog(),
            )
            .is_err(),
        "a refused start is the step's error"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The completion registry becomes unreadable before shutdown.
    let completion = private.completion.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = completion.inner.lock().unwrap();
            panic!("poisoning the completion registry");
        })
        .join()
        .is_err()
    );

    drop(private.shutdown());

    // An unreadable registry is not evidence that something else will answer
    // for this command. Asking by a boolean lost it: the same false covers
    // unreadable, foreign, absent and several live phases, and dropping on all
    // of them discarded an accepted command and the credit it held.
    assert_eq!(
        durable.owed().expect("a readable owner"),
        1,
        "the command is kept whole rather than dropped on an unreadable answer"
    );
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        1,
        "and it still holds the credit it was accepted with"
    );
}

#[test]
fn what_an_instance_still_owes_reaches_the_durable_owner() {
    let client = XServerFrontendClientId(961);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper, runner, ingress, durable, registration: _, channels: _, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // A press that begins a hold. Its plan is recorded, and the hold is an
    // obligation: a later release answers to what this press reached.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(961),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let _delivered = private.deliver_turn(turn);
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "the hold's plan is owed to whatever releases it"
    );
    let owed = private.terminal.outstanding().expect("readable terminal inventory");
    assert_eq!(owed, 2, "one held input and one live connection cleanup owner");
    let lifecycle = private.terminal.lifecycle.clone();
    assert_eq!(lifecycle.inventory().unwrap().open, 1);

    // Shutdown. The instance can no longer answer, so what it owes travels to
    // the handle rather than dying with it.
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        owed - 1,
        "only the completed connection cleanup leaves; the held input remains owed"
    );
    assert_eq!(lifecycle.inventory().unwrap(), PrivateLifecycleInventory::default());
    assert_eq!(
        durable.terminal_inventories().expect("a readable owner"),
        0,
        "and the durable owner has not been given it while a handle still holds it"
    );

    // The handle is abandoned too. Now it goes to the owner behind both,
    // rather than being dropped by the last thing able to pass it on.
    drop(settlement);
    assert_eq!(
        durable.terminal_inventories().expect("a readable owner"),
        1,
        "the obligations reached the owner that outlives both"
    );
}

#[test]
fn a_retained_hold_keeps_the_capabilities_needed_to_answer_it() {
    let client = XServerFrontendClientId(971);
    // Taken by value rather than borrowed: this control drops the producer,
    // the routes and the instance and then asks whether what they were
    // keeping alive is gone. References into a fixture that outlived them
    // would answer that question about the fixture instead.
    let PreparedOrderedFixture {
        keeper,
        mut runner,
        ingress,
        durable,
        registration,
        channels,
        surface,
        window: _,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // Weak handles to the two capabilities a retained hold needs: the seat
    // projection its release must move, and the authority its debt answers to.
    let projection = std::sync::Arc::downgrade(&private.broker.registry.pointer_state);
    let common = std::sync::Arc::downgrade(&private.authority().common);

    // A press that begins a hold, delivered and its completion observed -- so
    // its custody is gone and only the hold plan is left.
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(971),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(matches!(
        delivered[0].completion,
        Some(sophia_input_authority::RequestCompletion::Processed)
    ));

    // Everything that might have kept those capabilities alive incidentally
    // goes: the producer, the client's routes and channels, and the instance.
    drop(ingress);
    drop(channels);
    drop(registration);
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        1,
        "the hold is still owed"
    );
    assert!(
        !settlement.is_settled(),
        "and a retained obligation is not a settled report"
    );
    // The narrow counters stay narrow, which is why the settled question has
    // to be the aggregate one: a caller reading either of these alone sees an
    // instance with nothing left while it still owes a release.
    assert_eq!(settlement.owed(), 0, "no command is waiting");
    assert_eq!(settlement.outstanding(), 0, "and none is in flight");
    assert!(
        projection.upgrade().is_some(),
        "the seat projection its release must move is still reachable"
    );
    assert!(
        common.upgrade().is_some(),
        "and so is the authority its debt answers to"
    );

    // Handed on to the owner behind the handle, still with both. The
    // capability is asserted before the count: that an obligation was kept
    // somewhere is a weaker claim than that what answers it was kept with it.
    drop(settlement);
    assert!(
        projection.upgrade().is_some(),
        "an inventory that outlived its registry would describe obligations \
         nothing could act on"
    );
    assert!(common.upgrade().is_some());
    assert_eq!(durable.terminal_inventories().expect("readable"), 1);
    // And on the owner's side the same distinction holds: there is nothing to
    // drive, and driving is not what discharges this.
    assert_eq!(durable.owed(), Some(0));
    assert_eq!(durable.outstanding(), Some(0));
    let progress = durable.drive();
    assert!(progress.readable);
    assert!(
        !progress.made_progress(),
        "a drive loop ends here with the hold still owed, so 'no progress' \
         cannot be read as 'nothing owed'"
    );
    assert_eq!(durable.terminal_inventories().expect("readable"), 1);

    // And they go only when the obligations do -- and when the owner that
    // keeps the store and this connection's evidence goes with them.
    drop(durable);
    drop(keeper);
    assert!(projection.upgrade().is_none());
    assert!(common.upgrade().is_none());
}

/// One instance that ends owing a hold it can no longer answer for, handed to
/// `durable`. Returns the authority that hold answers to, so a caller can ask
/// which instance a retained inventory kept.
fn instance_handing_over_a_retained_hold(
    keeper: &crate::PrivateServiceOwner,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    namespace: NamespaceId,
    delivery: u64,
    button: u32,
) -> std::sync::Arc<Mutex<sophia_input_authority::AuthorityInstance>> {
    let (sender, _acks) = sync_channel(8);
    let (delivery_sender, _deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let common = std::sync::Arc::clone(&private.authority().common);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(namespaced(client, namespace)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");
    // The source resolves the recipient out of this connection's own
    // selection state, so the window has to exist there and to have selected
    // button events before anything is submitted.
    let window = XResourceId::new(0x200000 | client.raw(), 1);
    let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(&registration, namespace, selections.clone(), focused.clone())
        .expect("the connection state attaches");
    {
        let mut selected = selections.lock().expect("the selections");
        selected.register(
            window,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(window);
        selected.update(window, Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .expect("the surface to register");
    let mut runner = private
        .prepare_runner(namespace, keeper)
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
    {
        let publication = runner
            .frontend
            .as_ref()
            .expect("a live runner")
            .broker
            .registry
            .private_applied
            .get()
            .expect("prepare_runner installed it")
            .publication
            .clone();
        let mut runtime = XAuthorityRuntime::new();
        runtime.prepare_input_focus_namespace(namespace);
        publication
            .lock()
            .expect("the publication")
            .begin_focus_change()
            .expect("a focus change")
            .apply(&mut runtime, &focused, None)
            .expect("the clear applies");
    }
    let ingress = runner
        .ingress_for(&keeper.lease(), client, DeviceId::from_raw(1))
        .expect("an ingress");
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(delivery),
            button,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(matches!(
        delivered[0].completion,
        Some(sophia_input_authority::RequestCompletion::Processed)
    ));
    drop(ingress);
    drop(channels);
    drop(registration);
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        1,
        "the instance ends owing exactly the hold this control is about"
    );
    drop(settlement);
    common
}

#[test]
fn two_instances_owing_the_same_names_each_answer_through_their_own_origin() {
    // The same surface, in the same namespace, on the same seat, in two
    // unrelated instances. Nothing in the names distinguishes the two
    // obligations; only which registry and which authority accepted each does.
    let surface = SurfaceId::new(981, 1);
    let namespace = NamespaceId::from_raw(500);
    let seat = SeatId::from_raw(1);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    // Different buttons, so a projection read through the wrong registry is
    // visible rather than indistinguishable.
    let first = instance_handing_over_a_retained_hold(
        &owner_of_durable,
        XServerFrontendClientId(981),
        surface,
        namespace,
        981,
        272,
    );
    let second = instance_handing_over_a_retained_hold(
        &owner_of_durable,
        XServerFrontendClientId(982),
        surface,
        namespace,
        982,
        273,
    );
    assert_eq!(durable.terminal_inventories().expect("readable"), 2);
    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "two instances, two authorities"
    );

    // The clients' routes are gone -- both registrations dropped before
    // shutdown, which is the case retention exists for. What a release still
    // has to move is the seat projection its press raised, and that is the
    // registry's.
    // Reached here rather than through a query on the owner: what each
    // retained inventory kept is a question this control asks, not one the
    // owner needs to answer.
    let held = durable.inner.lock().expect("the owner to be readable");
    let answered = held
        .terminal
        .iter()
        .map(|inventory| {
            let projected = inventory
            .origin()
            .pointer_state
            .lock()
            .expect("its own seat projection")
                .get(&(namespace, seat))
                .map_or(0, |mapper| mapper.state());
            (
                projected,
                std::sync::Arc::ptr_eq(&inventory.controller().common, &first),
                std::sync::Arc::ptr_eq(&inventory.controller().common, &second),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        answered,
        vec![(0x100, true, false), (0x400, false, true)],
        "each retained hold reaches the seat projection its own press raised \
         and the authority its own credit answers to -- reaching the other \
         would release a button this instance never pressed, in an instance \
         that never accepted it"
    );
}

#[test]
fn an_ordered_press_whose_delivery_ended_does_not_execute() {
    let client = XServerFrontendClientId(991);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        durable: _,
        channels,
        surface,
        window,
        ..
    } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let delivery = XAuthorityInputDeliveryId::from_raw(991);

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let cell = admitted_cell(private, delivery.raw());
    let reserved = keeper.store().reserved().unwrap();
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "submission tracked a real delivery"
    );

    // It ends before the turn runs: the epoch is revoked while the work is
    // still sitting in the order.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert_eq!(revoked.len(), 1, "the admitted delivery is the one revoked");
    assert_eq!(
        revoked[0].delivery, delivery,
        "and it is the one this control submitted"
    );
    let cancelled = cell.answer().expect("the original cancellation was published");
    assert_eq!(cancelled, XAuthorityClientInputDelivery {
        client: XServerFrontendClientId::from_raw(0),
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::EpochRevoked,
    });

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let [PrivateOrderedItem::Refused { refusal, custody, .. }] = turn.as_slice() else {
        panic!("the ended delivery was refused before terminal retirement");
    };
    assert_eq!(*refusal, PrivateExecutionRefusal::DeliveryEnded);
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &cell));
    assert_eq!(cell.answer(), Some(cancelled));
    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "a delivery that ended is owed no event"
    );
    assert!(
        channels.input.try_recv().is_err(),
        "and nothing reached the client's queue"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "the ledger never moved, so there is no hold for a release to answer"
    );
    assert!(private.terminal.undelivered.is_empty(), "the exact common refusal was observed");
    assert_eq!(cell.answer(), Some(cancelled), "retirement never rewrites the original answer");
    assert_eq!(keeper.store().reserved(), Some(reserved - 1), "only this request's storage returned");
    // The no-effect request retired; the independent lifecycle still needs
    // its own cleanup before this inventory can become empty.
    let mut settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(settlement.terminal_outstanding(), Some(1), "one pending lifecycle cleanup");
    for _ in 0..16 { settlement.retry(); }
    assert_eq!(settlement.terminal_outstanding(), Some(0), "the lifecycle's own cleanup finished");
}

/// One admitted client with a real ingress, kept whole.
///
/// The receivers are held because a queue with no reader refuses everything,
/// and a control that could not tell that from a refusal on the merits would
/// pass for the wrong reason.
struct OrderedIngressFixture {
    durable: crate::PrivateSettlementOwner,
    private: crate::PrivateXServerFrontend,
    ingress: crate::PrivateIngress,
    keyboards: crate::PrivateKeyboards,
    registration: XServerFrontendClientRouteRegistration,
    channels: XServerFrontendClientRouteChannels,
    _acks: Receiver<XAuthorityClientControlAck>,
    deliveries: Receiver<XAuthorityClientInputDelivery>,
    /// Kept for the same reason, and dropped after the instance that used it.
    _keeper: crate::PrivateServiceOwner,
}

/// A sealed watchdog for a control that is not exercising the watch itself.
///
/// Real rather than absent: nothing runs unwatched, so a control that wants to
/// exercise something else still has to supply a supervisor that will take the
/// execution. The gate is dropped because these controls do not read it.
fn control_watchdog() -> private_watchdog::PrivateWatchdogOwner {
    let mut owner = private_watchdog::PrivateWatchdogOwner::prepare(0).expect("a watchdog");
    owner.seal().expect("a sealed watchdog");
    owner
}

fn ordered_ingress_fixture(
    client: XServerFrontendClientId,
    surface: SurfaceId,
) -> OrderedIngressFixture {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, acks) = sync_channel(8);
    let (delivery_sender, deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .broker.registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200000 | client.raw(), 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let keyboards = private.keyboards().expect("this instance's state");
    OrderedIngressFixture {
        _keeper: service_keeper,
        durable,
        private,
        ingress,
        keyboards,
        registration,
        channels,
        _acks: acks,
        deliveries,
    }
}
