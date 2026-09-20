// Ordered delivery of a press and its release: what a join leaves alone, and
// what a release applies when another execution holds its delivery.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


// Keep source admission alive when a test disconnects its recipient. A
// departed source is denied at the lifecycle gate before recipient binding.
fn separate_ordered_sender(
    private: &mut crate::PrivateXServerFrontend,
    ingress: &mut crate::PrivateIngress,
    recipient: XServerFrontendClientId,
) -> (XServerFrontendClientRouteRegistration, XServerFrontendClientRouteChannels) {
    let sender = XServerFrontendClientId(recipient.raw() + 50_000);
    let context = namespaced(sender, NamespaceId::from_raw(recipient.raw()));
    let registry = &private.broker.registry;
    let (registration, channels) = registry
        .register_client_with_admission(sender, Some(context))
        .unwrap();
    registry.attach_private_lifecycle(&registration, context).unwrap();
    *ingress = private.ingress_for(sender, DeviceId::from_raw(2)).unwrap();
    (registration, channels)
}

#[test]
fn a_join_leaves_the_source_obligation_alone_so_a_later_press_still_runs() {
    // The sequence that a join asked as a press poisons: press, join the same
    // button, then press a DIFFERENT button. Asking press for the join
    // installs a second source obligation and leaves it retained on the
    // disagreement the source reports, and the third press is then refused
    // WrongPhase for a phase the executor put there itself.
    let client = XServerFrontendClientId(2401);
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

    // The cell is taken where the admission mints it and travels out with the
    // reports, so every claim below names the admission it came from.
    let run = |private: &mut crate::PrivateXServerFrontend,
                   keyboards: &mut crate::PrivateKeyboards,
                   delivery: u64,
                   button: u32| {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .expect("the order to accept it");
        let cell = admitted_cell(private, delivery);
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        (private.deliver_turn(turn), cell)
    };

    let (first, first_cell) = run(private, keyboards, 2401, 272);
    assert_eq!(
        first.len(),
        1,
        "the press was disposed of and its own outcome reported"
    );
    let capsule = inbox
        .accepted(private, &channels.ordered, &first_cell, 8)
        .expect("a readable terminal step")
        .expect("the first press owes its recipient an event");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2401)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(private.terminal.holds.len(), 1);

    // The join. It owes nobody an event, and it must not leave a second
    // obligation behind it.
    let (joined, join_cell) = run(private, keyboards, 2402, 272);
    let _ = &joined;
    assert!(
        inbox
            .accepted(private, &channels.ordered, &join_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "a join owes nobody an event: the button is already down"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and it joined rather than beginning a second hold"
    );
    assert!(
        private.terminal.native_pending.is_none(),
        "the join asked the source for a join, so nothing was installed to retain"
    );

    // The press this blocker actually kills.
    let (third, third_cell) = run(private, keyboards, 2403, 273);
    let _ = &third;
    let capsule = inbox
        .accepted(private, &channels.ordered, &third_cell, 8)
        .expect("a readable terminal step")
        .expect("a different button still presses: the join left no phase behind it");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2403)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        private.terminal.holds.len(),
        2,
        "and it began its own hold rather than being refused"
    );
}

#[test]
fn an_ordered_press_binds_its_delivery_to_the_client_that_receives_it() {
    let client = XServerFrontendClientId(992);
    let delivery = XAuthorityInputDeliveryId::from_raw(992);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        channels,
        deliveries,
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

    // Before the turn the ledger tracks the delivery with no recipient: it
    // knows something was accepted, not who is waiting for it.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 992);
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("a tracked delivery")
            .client,
        None
    );

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
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press reached the client's queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(992)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("still tracked")
            .client,
        Some(client),
        "and the ledger now records who it went to"
    );

    // Which is what makes the client going answerable. An unbound delivery is
    // not one a disconnect can answer: it is answered to nobody, and only a
    // deadline would ever end it.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    let receipt = deliveries
        .try_recv()
        .expect("a terminal outcome for the bound delivery");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
}

#[test]
fn a_press_whose_recipient_is_already_gone_leaves_no_hold() {
    let client = XServerFrontendClientId(993);
    let surface = SurfaceId::new(993, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(993);
        let PreparedOrderedFixture {
        keeper,
        mut runner, mut ingress, channels, deliveries, registration, durable,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_sender_registration, _sender_channels) = separate_ordered_sender(private, &mut ingress, client);

    // The recipient closes first. The distinct submitter remains authorized,
    // but the recipient gate refuses before binding or pressing the ledger.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let cell = admitted_cell(private, delivery.raw());
    let reserved = durable.reserved().unwrap();
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "the delivery itself has not ended"
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(
        private.terminal.holds.is_empty(),
        "a press that cannot be delivered must not leave a hold: the release \
         answering it would be owed to a client that was already gone"
    );
    // Read the exact source refusal before terminal observation retires the
    // request. A closed recipient establishes no hidden common/native hold.
    let [PrivateOrderedItem::Refused { refusal, custody, .. }] = turn.as_slice() else {
        panic!("the closed recipient refused the actual original request");
    };
    assert_eq!(*refusal, PrivateExecutionRefusal::Native(private_native::Refusal::Connection(
        PrivateAppliedRegistryRefusal::AdmissionClosed
    )));
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &cell));
    let source_client = custody.client();
    assert!(channels.input.try_recv().is_err());
    assert_eq!(private.broker.registry.input_recovery.ticket(delivery).unwrap().client, None,
        "recipient closure refuses before recovery binding, not through its cancellation path");
    assert!(deliveries.try_recv().is_err());

    assert_eq!(cell.answer(), None, "the source refusal has not been published yet");
    assert!(private.deliver_turn(turn).is_empty());
    let rejected = XAuthorityClientInputDelivery {
        client: source_client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
    };
    assert_eq!(cell.answer(), Some(rejected));
    assert_eq!(deliveries.try_recv().unwrap(), rejected);
    assert!(deliveries.try_recv().is_err(), "the original refusal is published once");
    assert!(private.terminal.undelivered.is_empty());
    assert_eq!(durable.reserved(), Some(reserved - 1));

    // Now ask the ledger itself. An untouched one reports nothing was held and
    // the release finishes; one that was pressed would end a hold whose plan
    // this executor never recorded, and refuse.
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9931),
            272,
            false,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 9931);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let released = private.deliver_turn(turn);
    assert_eq!(
        released.len(),
        1,
        "the release finishes, because the ledger was never pressed"
    );
    let _ = &released;
    assert!(
        inbox
            .accepted(private, &channels.ordered, &press_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "and it owes nobody an event"
    );
    assert!(
        !private
            .terminal
            .undelivered
            .iter()
            .any(|entry| matches!(
                &entry.item,
                PrivateOrderedItem::Refused {
                    refusal: PrivateExecutionRefusal::HoldPlanMissing,
                    ..
                }
            )),
        "nothing ended a hold this executor never recorded"
    );
    drop(registration);
    drop(durable);
}

#[test]
fn a_release_to_a_gone_recipient_still_lifts_the_button() {
    let client = XServerFrontendClientId(994);
    let surface = SurfaceId::new(994, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        keeper,
        mut runner, mut ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_sender_registration, _sender_channels) = separate_ordered_sender(private, &mut ingress, client);

    // A press that lands while the client is there.
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9941),
            272,
            true,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 9941);
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
        XAuthorityInputDeliveryId::from_raw(9941)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(projected_buttons(private, namespace, seat), 0x100);
    assert_eq!(private.terminal.holds.len(), 1);

    // Then it goes, and the release arrives afterwards. The ledger is told,
    // and so is the routing side: output reaches a client through the queue
    // its connection owns, so a recipient that is gone is one with no entry
    // there. This is the state route_to_client itself leaves behind when a
    // queue reports its receiver dropped.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .remove(&client)
            .is_some(),
        "the connection was there to lose"
    );
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9942),
            272,
            false,
        ))
        .expect("the order to accept it");
        let release_cell = admitted_cell(private, 9942);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );

    // The hold ended and the button is up. Neither could be conditional on
    // anyone still being there to be told: a button left down because its
    // client vanished is held forever, by nobody.
    assert!(
        private.terminal.holds.is_empty(),
        "the hold ended"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "and the button this seat had down is up"
    );
    assert!(
        inbox
            .accepted(private, &channels.ordered, &release_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "but nothing was enqueued for a client that is gone"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "the debt is recorded even though nothing will be sent"
    );
    assert!(
        private.terminal.settling[0].binding() == PrivateReleaseBinding::Ended,
        "and it says which: established gone, not merely unlooked-up"
    );
    assert_eq!(
        private.terminal.settling[0].delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(9942)),
        "and still records which delivery would have answered it: what is \
         unknown is the receipt, not which delivery it belongs to"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_grabbed_press_binds_its_delivery_to_the_grab_owner_not_the_surface() {
    let client = XServerFrontendClientId(995);
    let owner = XServerFrontendClientId(996);
    let surface = SurfaceId::new(995, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let delivery = XAuthorityInputDeliveryId::from_raw(995);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // The grab owner is a real admitted client of this instance, in the same
    // namespace the grab is recorded in, with its own live queue: the press is
    // delivered to it, so the boundary has to know it and something has to be
    // there to receive it. Admitting it into its own namespace instead would
    // make the grab name a window this press's origin cannot reach.
    let (owner_registration, owner_channels) = private
        .broker
        .registry
        .register_client_with_admission(owner, Some(namespaced(owner, namespace)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(owner, namespaced(owner, namespace))
        .expect("the boundary to admit");
    // The grab owner's own view of its own window. A grab names a window to
    // deliver into, and the source resolves that window through the owner's
    // selection state -- so an owner that never registered one has nothing for
    // the press to reach, however well the grab is recorded.
    let owner_selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let owner_focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(
            &owner_registration,
            namespace,
            owner_selections.clone(),
            owner_focused.clone(),
        )
        .expect("the owner's connection state attaches");
    {
        let mut selected = owner_selections.lock().expect("the owner's selections");
        selected.register(
            XResourceId::new(0x200996, 1),
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(XResourceId::new(0x200996, 1));
        selected.update(XResourceId::new(0x200996, 1), Some((1 << 2) | (1 << 3)), None);
    }

    private
        .broker
        .registry
        .input_authority
        .lock()
        .expect("the grab state")
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: owner.raw(),
                window: XResourceId::new(0x200996, 1),
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .expect("the grab to take");

    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 995);
    let mut inbox = OrderedInbox::default();
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );
    let to_owner = inbox
        .accepted(private, &owner_channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the grab owner is the one that received it");
    assert_eq!(to_owner.delivery(), delivery);
    assert_eq!(to_owner.client(), owner);
    assert!(
        inbox.taken.is_empty(),
        "and nothing else was on the owner's queue"
    );
    assert!(
        channels.ordered.try_recv().is_err(),
        "and the surface's own client did not"
    );

    // The route named this surface, whose client is 995. The grab sent the
    // press to 996. What the ledger records is where the event went, because
    // that is who a disconnect has to answer for -- recording the route's
    // client would answer the wrong client's departure and leave this
    // delivery owed to nobody.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("still tracked")
            .client,
        Some(owner)
    );
    assert_eq!(
        private.terminal.holds[0].reached.client(),
        owner,
        "and the hold records the same recipient"
    );
    drop(owner_registration);
    drop(owner_channels);
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn an_unreadable_ledger_is_not_a_delivery_that_ended() {
    let client = XServerFrontendClientId(997);
    let surface = SurfaceId::new(997, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(997);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // The ledger becomes unreadable while that delivery is still waiting its
    // turn.
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        private.terminal.holds.is_empty(),
        "nothing is applied on the strength of what nobody could read"
    );
    // Refused for the absence of an answer, not for an answer. Reporting this
    // as ended would say the delivery was settled, which nothing established
    // -- and a caller acting on that would stop waiting for an outcome that
    // is still owed.
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::RecoveryUnavailable,
                ..
            }
        ),
        "an unreadable ledger is its own refusal"
    );
    assert!(channels.input.try_recv().is_err());
    drop(registration);
    drop(durable);
}

/// A press that ran, leaving a hold and a button down.
///
/// The cell is taken where the admission mints it and handed back, so a caller
/// can go on naming this exact admission after its ticket is answered.
fn held_button(
    private: &mut crate::PrivateXServerFrontend,
    ingress: &crate::PrivateIngress,
    service: &crate::PrivateServiceLease<'_>,
    keyboards: &mut crate::PrivateKeyboards,
    watch: &private_watchdog::PrivateWatchdogOwner,
    inbox: &mut OrderedInbox,
    queue: &Receiver<XAuthorityOrderedDelivery>,
    surface: SurfaceId,
    delivery: u64,
) -> (Arc<PrivateDeliveryCompletion>, XAuthorityOrderedDelivery) {
    ingress
        .submit(service, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(delivery),
            272,
            true,
        ))
        .expect("the order to accept it");
    let cell = admitted_cell(private, delivery);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, queue, &cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(delivery)
    );
    assert_eq!(private.terminal.holds.len(), 1);
    (cell, capsule)
}

#[test]
fn a_release_whose_delivery_ended_does_not_end_its_hold() {
    let client = XServerFrontendClientId(998);
    let surface = SurfaceId::new(998, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        &keeper.lease(),
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        9981,
    );

    // The release is accepted, and then its own delivery ends while it waits
    // its turn.
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9982),
            272,
            false,
        ))
        .expect("the order to accept it");
    let release_cell = admitted_cell(private, 9982);
    let reserved = durable.reserved().unwrap();
    private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    let cancelled = release_cell.answer().expect("the actual original cancellation");
    assert_eq!(cancelled.delivery, XAuthorityInputDeliveryId::from_raw(9982));
    assert_eq!(cancelled.outcome, XAuthorityInputDeliveryOutcome::EpochRevoked);

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");

    // Refused before the ledger moved. This is the case the gate exists for:
    // the press path finds out by binding, but a release binds only after its
    // transition, so without a check beforehand a withdrawn release would end
    // a hold and lift a button on the strength of a request whose outcome was
    // already reported.
    let [PrivateOrderedItem::Refused { refusal, custody, .. }] = turn.as_slice() else {
        panic!("the ended release was refused before terminal retirement");
    };
    assert_eq!(*refusal, PrivateExecutionRefusal::DeliveryEnded);
    assert!(Arc::ptr_eq(&custody.input_completion().unwrap().cell, &release_cell));
    assert!(private.deliver_turn(turn).is_empty());
    assert!(private.terminal.undelivered.is_empty());
    assert_eq!(release_cell.answer(), Some(cancelled), "the cancellation is immutable");
    assert_eq!(durable.reserved(), Some(reserved - 1), "only the no-effect release request retired");
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and the hold it would have ended is still here"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0x100,
        "with the button still down, because nothing lifted it"
    );
    // Not lost, either: the obligation is retained rather than discarded, and
    // whoever takes the inventory is the one that can still answer it.
    assert!(
        frontend
            .take()
            .expect("a live runner")
            .shutdown()
            .terminal_outstanding()
            .expect("readable terminal inventory")
            >= 1
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_release_does_not_move_the_ledger_when_nobody_can_read_the_deliveries() {
    let client = XServerFrontendClientId(999);
    let surface = SurfaceId::new(999, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        &keeper.lease(),
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        9991,
    );

    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9992),
            272,
            false,
        ))
        .expect("the order to accept it");
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::RecoveryUnavailable,
                ..
            }
        ),
        "nothing is known, and that is its own answer"
    );
    // The distinction is the whole point: this release may still be owed. Had
    // it run, the hold would be gone and the button up on the strength of
    // something nobody could read.
    assert_eq!(private.terminal.holds.len(), 1);
    assert_eq!(projected_buttons(private, namespace, seat), 0x100);
    assert!(
        private.terminal.settling.is_empty(),
        "and no debt was recorded, because no release happened"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

/// A ledger with one admitted, unbound delivery.
fn claim_fixture(
    delivery: XAuthorityInputDeliveryId,
) -> (
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (sender, receipts) = channel();
    let recovery = InputRecovery::new(
        8,
        Some(sender),
        Arc::new(Mutex::new(crate::XInputAuthorityState::default())),
    );
    recovery
        .admit_typed(
            &button_to(SurfaceId::new(1, 1), delivery, 272, true),
            1,
            std::time::Instant::now(),
        )
        .expect("a fresh delivery to be tracked");
    (recovery, receipts)
}

#[test]
fn a_cancellation_arriving_under_a_claim_does_not_publish_over_the_effect() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4001);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );

    // The revocation producer runs in the interval the execution holds. This
    // is the gap that a precheck leaves open: the ledger's own guard is not
    // held here, and the effect has not happened yet.
    let expired = recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        expired.is_empty(),
        "a delivery being applied right now is not an abandoned one"
    );
    assert!(
        receipts.try_recv().is_err(),
        "and nothing was published for it"
    );

    // The execution applied something, so the cancellation had an effect to
    // contradict and does not become this delivery's outcome. The delivery is
    // still owed one, which its writer result or its deadline answers -- not
    // the same as it having ended.
    recovery.resolve_claim(Some(delivery), true);
    assert!(receipts.try_recv().is_err());
    assert!(
        recovery.ticket(delivery).is_some(),
        "still tracked, still owed an outcome"
    );
}

#[test]
fn a_cancellation_that_lost_to_an_execution_applying_nothing_still_stands() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4002);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    assert!(
        recovery
            .recover(std::time::Instant::now(), true)
            .expect("readable")
            .is_empty()
    );

    // Nothing was applied under the claim, so the cancellation had nothing to
    // contradict. Dropping it here would lose a revocation on the strength of
    // an execution that did not happen.
    recovery.resolve_claim(Some(delivery), false);
    let receipt = receipts
        .try_recv()
        .expect("the revocation to be published once the claim gave way");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Ended,
        "and a later execution finds it ended"
    );
}

#[test]
fn one_delivery_cannot_be_claimed_by_two_executions() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4003);
    let (recovery, _receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Contended,
        "contended is not ended: nothing finished, and the delivery is still \
         owed an outcome by whoever holds it"
    );
    recovery.resolve_claim(Some(delivery), true);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed,
        "and the claim is available again once it is given back"
    );
}

#[test]
fn an_ordered_turn_gives_its_claim_back() {
    let client = XServerFrontendClientId(1001);
    let delivery = XAuthorityInputDeliveryId::from_raw(1001);
        let PreparedOrderedFixture {
        keeper,
        mut runner,
        ingress,
        channels,
        deliveries,
        surface,
        durable: _durable,
        registration: _registration,
        _acks,
        selections: _selections,
        client: _client,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1001);
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
        XAuthorityInputDeliveryId::from_raw(1001)
    );
    assert_eq!(capsule.client(), client);

    // Given back, so the delivery can still be cancelled. A claim nobody
    // resolves is not a delivery that is safe: it is one nothing can ever
    // answer again, because every cancellation after it defers forever.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    let receipt = deliveries
        .try_recv()
        .expect("the delivery to still be answerable");
    assert_eq!(receipt.delivery, delivery);
    drop(channels);
}

#[test]
fn a_release_whose_delivery_another_execution_holds_applies_nothing() {
    let client = XServerFrontendClientId(1002);
    let surface = SurfaceId::new(1002, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let release = XAuthorityInputDeliveryId::from_raw(10022);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        &keeper.lease(),
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        10021,
    );

    ingress
        .submit(&keeper.lease(), button_to(surface, release, 272, false))
        .expect("the order to accept it");
    // Something else holds this delivery. Its effect may be under way, and a
    // second one applied here would be a second effect for one request.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .claim_execution(Some(release)),
        ExecutionClaim::Claimed
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::DeliveryClaimedElsewhere,
                ..
            }
        ),
        "refused for contention, which is not the delivery having ended"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and nothing was applied: the hold is untouched"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0x100,
        "with the button still down"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}
