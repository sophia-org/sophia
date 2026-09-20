// Claims that have been applied: the recipient a joining press binds, the
// effect that stands, and the frame still owed bytes that cannot be abandoned.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_joining_press_binds_the_recipient_its_hold_reached_not_the_new_target() {
    let client = XServerFrontendClientId(1003);
    let surface = SurfaceId::new(1003, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let first = XAuthorityInputDeliveryId::from_raw(10031);
    let second = XAuthorityInputDeliveryId::from_raw(10032);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, registration, durable, selections: _, window: _,
        _acks,
        deliveries: _deliveries,
        client: _client,
        surface: _surface,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // A press that reaches this client and starts a hold.
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
        10031,
    );
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(first)
            .expect("tracked")
            .client,
        Some(client)
    );

    // A REAL REPLACEMENT, without forcing any state. B alone cannot displace
    // the implicit grab this press took, but A can release its own first --
    // so the owner ungrabs and B grabs, and a fresh resolution of this button
    // now genuinely reaches a different client than the press did.
    let owner = XServerFrontendClientId(1004);
    let (owner_registration, owner_channels) = private
        .broker
        .registry
        .register_client_with_admission(owner, Some(namespaced(owner, namespace)))
        .expect("a fresh client to register");
    private
        .broker
        .registry
        .attach_private_lifecycle(&owner_registration, namespaced(owner, namespace))
        .expect("the boundary to admit");
    {
        let mut grabs = private
            .broker
            .registry
            .input_authority
            .lock()
            .expect("the grab state");
        // The press's own client gives up the grab it holds. Nothing is
        // forced: this is the owner releasing its own.
        grabs.ungrab_pointer(namespace, client.raw());
        grabs
            .grab_pointer(
                namespace,
                crate::XActiveInputGrab {
                    owner: owner.raw(),
                    window: XResourceId::new(0x201004, 1),
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
    }

    // The same button again. The ledger joins the hold that exists: no new
    // hold, no new event, and the recipient is the one the hold already has.
    ingress
        .submit(&keeper.lease(), button_to(surface, second, 272, true))
        .expect("the order to accept it");
    let join_cell = admitted_cell(private, 10032);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
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
        "and it joined rather than starting a second hold"
    );

    // The binding follows what the press reached, not what the route would
    // resolve to now. Binding the grab owner would put this delivery's
    // outcome on a client it never reached: the owner's disconnect would
    // answer it, and the client that is actually holding the button would
    // not.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(second)
            .expect("tracked")
            .client,
        Some(client),
        "the join inherits the recipient its hold reached"
    );
    assert!(
        owner_channels.input.try_recv().is_err(),
        "and no other client of this instance received it either"
    );
    drop(owner_registration);
    drop(owner_channels);
    drop(registration);
    drop(channels);
    drop(durable);
}

/// What the ledger records about one delivery's claim lifetime.
fn claim_state(
    private: &crate::PrivateXServerFrontend,
    delivery: XAuthorityInputDeliveryId,
) -> (bool, bool) {
    let held = private
        .broker
        .registry
        .input_recovery
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = held.tickets.get(&delivery).expect("still tracked");
    (entry.claimed, entry.may_have_applied)
}

#[test]
fn a_refusal_before_the_effect_resolves_the_claim_as_having_applied_nothing() {
    let client = XServerFrontendClientId(1101);
    let surface = SurfaceId::new(1101, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1101);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(&fixture._keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // The admission goes, so the real boundary refuses this request before the
    // effect callback is ever invoked. That refusal leaves the execution by a
    // returned error, not by deciding anything -- and everything after a
    // fallible call is skipped when that call returns an error, which is
    // exactly where a copied progress marker would be wrong.
    fixture
        .private
        .admission_participant()
        .revoke_admission(
            client,
            sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        )
        .expect("the boundary to revoke");

    let turn = fixture
        .private
        .route_pending_ordered(&mut fixture.keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(fixture.private.deliver_turn(turn).is_empty());
    assert!(
        fixture.private.terminal.holds.is_empty(),
        "nothing was applied"
    );
    assert_eq!(
        claim_state(&fixture.private, delivery),
        (false, false),
        "the claim is given back, saying nothing was applied -- which the \
         guard has to know on a returned error, not only on an unwind"
    );
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn an_established_fact_is_reported_even_when_something_may_have_applied() {
    // A cancellation deferred under a claim is dropped when an effect may
    // have happened, and rightly: it says the delivery did not happen and the
    // effect contradicts it. An established fact is not that. A terminated
    // connection is not undone by an effect having occurred, and returning on
    // the same check left such a delivery deferred and then discarded --
    // answered to nobody, ever.
    let client = XServerFrontendClientId(1103);
    let delivery = XAuthorityInputDeliveryId::from_raw(1103);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client, None).unwrap();
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Claimed);
    assert!(recovery.bind(Some(delivery), client).unwrap());

    // The recipient's connection ends while the claim is held.
    recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .unwrap();
    assert!(
        receipts.try_recv().is_err(),
        "held while the claim is out, like any other outcome"
    );

    // The claim resolves having MAYBE APPLIED. A cancellation would be
    // dropped here; this is not a cancellation.
    recovery.resolve_claim(Some(delivery), true);
    let receipt = receipts
        .try_recv()
        .expect("an established termination is still reported");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::ClientDisconnected);
}

#[test]
fn a_cancellation_deferred_by_binding_under_a_claim_stands_when_nothing_applied() {
    // Recovery API composition, not the private consumer: that consumer now
    // rejects a closing recipient before binding. A race after recipient
    // validation still requires this claim/bind arbitration to remain correct.
    let client = XServerFrontendClientId(1102);
    let delivery = XAuthorityInputDeliveryId::from_raw(1102);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client, None).unwrap();
    recovery.disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected).unwrap();
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Claimed);
    assert!(!recovery.bind(Some(delivery), client).unwrap());
    assert!(receipts.try_recv().is_err(), "the claim defers this exact binding cancellation");
    assert_eq!(recovery.ticket(delivery).unwrap().client, Some(client));
    recovery.resolve_claim(Some(delivery), false);
    let receipt = receipts.try_recv().expect("no application means the deferred cancellation stands");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::ClientDisconnected);
    recovery.resolve_claim(Some(delivery), false);
    assert!(receipts.try_recv().is_err(), "resolution cannot publish twice");
}

#[test]
fn a_claim_is_given_back_even_when_the_ledger_cannot_be_read() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4004);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");

    let poisoner = recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    // Giving back a claim is the one thing that cannot decline. Only this
    // caller holds it, and a claim nobody gives back is a delivery nothing can
    // ever cancel again and no owner knows is owed.
    recovery.resolve_claim(Some(delivery), false);
    let held = recovery
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(
        !held
            .tickets
            .get(&delivery)
            .expect("still tracked")
            .claimed,
        "an unreadable ledger must not leave a permanent claim nobody owns"
    );
    drop(held);
    // And the cancellation it was holding is resolved rather than stranded
    // with it.
    let receipt = receipts
        .try_recv()
        .expect("the deferred cancellation to be resolved too");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
}

#[test]
fn a_cancellation_cannot_publish_once_the_delivery_may_have_applied() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4005);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");

    // This execution applied something, so the cancellation it lost to cannot
    // become the delivery's outcome.
    recovery.resolve_claim(Some(delivery), true);
    assert!(receipts.try_recv().is_err());

    // Nor can a later execution that happens to apply nothing publish it. What
    // one claim did is not what the delivery has been through: the effect
    // already happened, and a per-claim answer cannot erase that.
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery.resolve_claim(Some(delivery), false);
    assert!(
        receipts.try_recv().is_err(),
        "a second claim applying nothing does not put a stale cancellation \
         back in reach of a delivery whose effect already happened"
    );

    // Nor can a fresh one arriving through the ordinary entry.
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        receipts.try_recv().is_err(),
        "the same contradiction refused at the normal entry, not only the \
         deferred one"
    );

    // But what became of the delivery is still sayable. Refusing this too
    // would leave a delivery whose effect happened with no way to be answered
    // at all, which is the opposite failure.
    recovery
        .finish(
            XServerFrontendClientId(1),
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the ledger to be readable");
    assert_eq!(
        receipts
            .try_recv()
            .expect("an established outcome to publish")
            .outcome,
        XAuthorityInputDeliveryOutcome::Flushed,
        "a writer result is not a denial that the delivery happened, so it \
         publishes"
    );
}

#[test]
fn a_press_that_applied_cannot_be_revoked_afterwards() {
    let client = XServerFrontendClientId(1203);
    let surface = SurfaceId::new(1203, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1203);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, deliveries, registration, durable,
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
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1203);
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
        XAuthorityInputDeliveryId::from_raw(1203)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        claim_state(private, delivery),
        (false, true),
        "the claim is back, and the delivery is on record as having applied"
    );

    // A sweep that would have revoked it before is too late now: the effect
    // happened, the button is down, and saying the delivery was withdrawn
    // would tell everyone waiting to stop on account of something that did
    // occur.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        revoked.is_empty(),
        "nothing is reported revoked that was not"
    );
    assert!(deliveries.try_recv().is_err());

    // The client going is still sayable, because that is what became of the
    // delivery rather than a denial that it happened.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert_eq!(
        deliveries
            .try_recv()
            .expect("an established recipient fact")
            .outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_retained_release_debt_is_named_the_way_the_ledger_names_it() {
    let client = XServerFrontendClientId(1201);
    let surface = SurfaceId::new(1201, 1);
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
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        &keeper.lease(),
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        12011,
    );

    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(12012),
            272,
            false,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 12012);
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
        XAuthorityInputDeliveryId::from_raw(12012)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(projected_buttons(private, namespace, seat), 0);
    assert_eq!(private.terminal.settling.len(), 1);

    // What the ledger itself reports as owed. A debt is named by its whole
    // incarnation -- authority, recipient, connection generation and input --
    // and both settle and an attempt claim are matched against that name.
    let mut cursor = 0;
    let reported = private
        .authority()
        .under_common(|authority| authority.next_debt(&mut cursor))
        .expect("the authority to be readable")
        .expect("a retained debt for the release that just happened");

    // The retained record has to carry the same name. A record holding only
    // the number inside an incarnation can be compared with nothing the
    // ledger offers, so the debt it describes is one this executor could
    // never settle.
    assert_eq!(
        private.terminal.settling[0].incarnation(),
        reported.0,
        "the retained debt is named the way the ledger names it"
    );
    // And it records which delivery carries its event. A receipt arrives
    // naming a delivery and settles a debt named by an incarnation; nothing
    // else holds both, so without this a writer result could be observed and
    // still not be attributable to the debt it settles.
    assert_eq!(
        private.terminal.settling[0].delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(12012)),
        "the debt knows which delivery answers it"
    );
    assert_eq!(
        reported.0.input,
        private.terminal.settling[0].incarnation().input,
        "including the input it is for"
    );
    assert!(
        !reported.1.is_settled(),
        "and nothing has settled it whole: a release having happened \
         establishes neither half by itself"
    );
    // OLD CLAIM: neither bit is set, because a release having happened
    //   establishes neither half.
    // NEW CLAIM: the native half is set, and NOT because a release happened.
    //   The source produced a proof that its own projection was reconciled,
    //   and that proof recorded the bit once the adapter guards and the
    //   common transaction had both dropped. A release that ends with a
    //   residual produces no proof and leaves this false, which is what makes
    //   the bit evidence rather than a restatement of "a release occurred".
    assert!(
        reported.1.native_reconciled,
        "the source's own proof recorded the native half"
    );
    assert!(
        !reported.1.recipient_settled,
        "and nothing here establishes the recipient's half: an event being \
         owed, built, or queued is not a receipt"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn one_step_takes_one_item_and_marks_it_before_common() {
    let client = XServerFrontendClientId(1301);
    let surface = SurfaceId::new(1301, 1);
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
    // A second producer, so two items can wait at once: one grant holds one
    // completion cell, and the first request keeps it until it is observed.
    let second = private
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second ingress");
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(13011),
            272,
            true,
        ))
        .expect("the order to accept it");
    second
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(13012),
            273,
            true,
        ))
        .expect("the order to accept it");

    // Cloned rather than reached through the frontend, which the step borrows
    // mutably while the mark runs.
    let common = Arc::clone(&private.authority().common);
    let recovery = private.broker.registry.input_recovery.clone();
    let first_delivery = XAuthorityInputDeliveryId::from_raw(13011);
    let mut marked = Vec::new();
    let step = {
        let mut mark = |sequence: crate::ReadySequence,
                        _taken_at: std::time::Instant|
         -> Result<(), XServerFrontendRouteError> {
            // Common is not held: the mark sits above that guard in the rank
            // and reaching for it here would invert the order.
            assert!(
                common.try_lock().is_ok(),
                "the mark runs outside common"
            );
            // And the work is not merely un-guarded but un-attempted. This is
            // what distinguishes a mark placed before the effect from one
            // placed after the execution returned, where common is also free:
            // the ledger has not moved for this delivery yet.
            let held = recovery
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                !held
                    .tickets
                    .get(&first_delivery)
                    .expect("tracked")
                    .may_have_applied,
                "the mark names work that has not been attempted"
            );
            drop(held);
            marked.push(sequence);
            Ok(())
        };
        private
            .step_once(keyboards, &mut mark, watch)
            .expect("a readable order")
    };
    let PrivateOrderedStep::Decided(sequence) = step else {
        panic!("one item decided")
    };
    assert_eq!(marked.len(), 1, "one step marks exactly one item");
    // Stored by the step, not handed back for the caller to hold.
    assert_eq!(private.terminal.turn.len(), 1);
    let PrivateOrderedItem::Ran {
        sequence: stored, ..
    } = private.terminal.turn[0]
    else {
        panic!("the press ran")
    };
    assert_eq!(stored, sequence);
    assert_eq!(
        marked[0], sequence,
        "and marks the item it actually took, not one it was about to"
    );

    // The second is still waiting: a step does not drain what it was not
    // charged for.
    let mut second_marked = Vec::new();
    let step = private
        .step_once(keyboards, &mut |sequence, _| {
            second_marked.push(sequence);
            Ok(())
        }, watch)
        .expect("a readable order");
    assert!(matches!(step, PrivateOrderedStep::Decided(_)));
    assert_eq!(second_marked.len(), 1);
    assert_ne!(second_marked[0], marked[0], "a different item");

    // And now the order is empty, which is its own answer rather than a
    // failure to find work.
    assert!(matches!(
        private
            .step_once(
                keyboards,
                &mut |_, _| panic!("nothing to mark"),
                watch,
            )
            .expect("a readable order"),
        PrivateOrderedStep::Idle
    ));
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_blocked_order_takes_nothing_and_marks_nothing() {
    let client = XServerFrontendClientId(1302);
        let PreparedOrderedFixture {
        keeper,
        mut runner,
        ingress: _,
        channels,
        deliveries: _,
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
    let watch = watch.as_ref().expect("a sealed watch");
    // A control whose dequeue the budget hook refuses stays parked, and the
    // order parks behind it.
    private
        .control_producer()
        .submit(&keeper.lease(), configure(client, surface, 13021))
        .expect("the order to accept it");
    assert!(
        private
            .step_once(
                keyboards,
                &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
                watch,
            )
            .is_err(),
        "a refused start is the step's error, and the control stays parked"
    );
    assert!(private.parked().is_some());

    // Blocked is not idle. A runner told only "no item" would charge a start
    // and mark a watchdog for work it could never have run, and would keep
    // doing so for as long as the barrier stood.
    let step = private
        .step_once(
            keyboards,
            &mut |_, _| panic!("nothing may be taken while the order is blocked"),
            watch,
        )
        .expect("a readable order");
    assert!(
        matches!(step, PrivateOrderedStep::Blocked(_)),
        "the barrier is reported as itself, not as an empty order"
    );
    drop(channels);
}

#[test]
fn a_suppressed_revocation_still_cleans_up_the_connection_it_revoked() {
    let client = XServerFrontendClientId(1303);
    let surface = SurfaceId::new(1303, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let delivery = XAuthorityInputDeliveryId::from_raw(1303);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, deliveries, registration, durable, window,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    // A grab this client owns, over the window it actually has. The
    // source resolves a grab through the owner's own selection state, so
    // a grab naming a window this client never registered would leave the
    // press nothing to reach and prove nothing about cleanup.
    private
        .broker
        .registry
        .input_authority
        .lock()
        .expect("the grab state")
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
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

    // A press that applies, so its delivery can no longer be revoked.
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1303);
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
        XAuthorityInputDeliveryId::from_raw(1303)
    );
    assert_eq!(capsule.client(), client);

    // The sweep revokes the connection and publishes nothing for the delivery,
    // because saying it was withdrawn would contradict the effect.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(revoked.is_empty(), "nothing reported revoked that was not");
    assert!(deliveries.try_recv().is_err());

    // The connection was still taken down, so what it owned still has to go.
    // Reading cleanup off the published list would skip exactly this case and
    // leave a grab installed for a client whose socket is gone.
    assert!(
        private
            .broker
            .registry
            .input_authority
            .lock()
            .expect("the grab state")
            .pointer_grab(namespace)
            .is_none(),
        "the revoked connection's grab is gone even though its delivery \
         published nothing"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_mark_that_panics_does_not_take_the_work_with_it() {
    let client = XServerFrontendClientId(1304);
    let surface = SurfaceId::new(1304, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1304);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(&fixture._keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let reserved_before = fixture.durable.reserved().expect("a readable owner");
    assert_eq!(reserved_before, 1, "the order accepted and reserved for it");

    // No hook: a mark that panics is the ordinary way accounting fails. What
    // matters is where the work is standing when it does.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixture
            .private
            .step_once(
                &mut fixture.keyboards,
                &mut |_, _| panic!("accounting failed"),
                &control_watchdog(),
            );
    }));
    assert!(outcome.is_err(), "the mark panicked");

    // The item is this instance's, not the lost frame's. Had it still been a
    // local when the mark ran, the accepted custody and the payload would have
    // gone with the unwind, and nothing would be left to say the order had
    // ever given it out.
    let Some(PrivateOrderedItem::Refused {
        sequence: _,
        refusal: PrivateExecutionRefusal::NotAttempted,
        route,
        ..
    }) = &fixture.private.terminal.current
    else {
        panic!("the exact work is still held, un-attempted")
    };
    assert_eq!(
        route.delivery,
        Some(delivery),
        "and it is the work that was accepted, not a reconstruction of it"
    );
    assert_eq!(
        fixture.durable.reserved().expect("a readable owner"),
        reserved_before,
        "its reservation is still held, so nothing was silently freed"
    );

    // And the order is honestly blocked on it rather than quietly moving on.
    assert!(matches!(
        fixture
            .private
            .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &control_watchdog()),
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));

    // The obligation survives the instance, which is what retention is for.
    let settlement = fixture.private.shutdown();
    assert!(settlement.terminal_outstanding().expect("readable terminal inventory") >= 1);
    drop(settlement);
    assert_eq!(fixture.durable.terminal_inventories().expect("readable"), 1);
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn private_work_does_not_expire_because_it_waited() {
    let client = XServerFrontendClientId(1401);
    let surface = SurfaceId::new(1401, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1401);
        let PreparedOrderedFixture {
        keeper,
        mut runner, ingress, channels, deliveries, registration, durable,
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
    ingress
        .submit(&keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1401);

    // Long past the legacy deadline, and nothing has been attempted for it --
    // so this is the age of a queue entry, not of a send. No writer has
    // blocked, because no writer has been given anything.
    let expired = private
        .broker
        .registry
        .input_recovery
        .recover(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            false,
        )
        .expect("the ledger to be readable");
    assert!(
        expired.is_empty(),
        "waiting in a queue is not a transport failure, and manufacturing an \
         outcome from it would report a delivery finished that nothing tried"
    );
    assert!(deliveries.try_recv().is_err());
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "the obligation is retained rather than answered"
    );

    // It still runs when its turn comes: retaining it is not shelving it.
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
        XAuthorityInputDeliveryId::from_raw(1401)
    );
    assert_eq!(capsule.client(), client);

    // And a real cancellation still reaches it, because what was disabled is
    // the age producer and not the sweep.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert_eq!(
        deliveries
            .try_recv()
            .expect("an established recipient fact")
            .delivery,
        delivery
    );
    drop(registration);
    drop(channels);
    drop(durable);
}


#[test]
fn a_start_that_refuses_stops_before_the_effect() {
    let client = XServerFrontendClientId(1501);
    let surface = SurfaceId::new(1501, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1501);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(&fixture._keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // Charging a start can refuse -- a budget is spent, and a spent budget is
    // a real answer. It has to be able to say so rather than being told after
    // the work has already run.
    let refused = fixture.private.step_once(
        &mut fixture.keyboards,
        &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
        &control_watchdog(),
    );
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));

    // Nothing was applied and nothing was lost: the work is this instance's,
    // un-attempted, and the order is honestly blocked on it.
    assert!(fixture.private.terminal.holds.is_empty());
    assert!(matches!(
        &fixture.private.terminal.current,
        Some(PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::NotAttempted,
            ..
        })
    ));
    assert_eq!(claim_state(&fixture.private, delivery), (false, false));
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn nothing_runs_when_nothing_will_watch_it() {
    let client = XServerFrontendClientId(1502);
    let surface = SurfaceId::new(1502, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1502);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(&fixture._keeper.lease(), button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // A supervisor that has not been sealed will not take an execution. The
    // watch exists for the case where a call does not come back, so running
    // without one is starting exactly the case it was meant to catch with
    // nothing left to catch it.
    let unsealed = private_watchdog::PrivateWatchdogOwner::prepare(0).expect("a watchdog");
    let step = fixture
        .private
        .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &unsealed)
        .expect("a readable order");
    assert!(matches!(step, PrivateOrderedStep::Unwatched(_)));
    assert!(fixture.private.terminal.holds.is_empty());
    assert_eq!(
        claim_state(&fixture.private, delivery),
        (false, false),
        "the ledger never moved for it"
    );
    assert!(matches!(
        &fixture.private.terminal.current,
        Some(PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::NotAttempted,
            ..
        })
    ));
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn a_send_counts_only_what_it_waited_on_this_recipient() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();

    // A recipient that is taking its bytes costs no waiting.
    state.begin_frame([7u8; 32].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(state.frame_complete(), "the whole frame went out");
    assert_eq!(
        state.blocked(),
        Duration::ZERO,
        "nothing waited, so nothing is owed to a deadline"
    );

    // Nobody reads now. Seeded close to the limit rather than waiting out the
    // whole policy: what is under test is that real waiting accumulates onto
    // what this delivery already waited, and trips the bound.
    state.blocked = X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60);
    let failure = loop {
        if state.frame_complete() {
            state.retire_frame().expect("the last frame finished");
        }
        if state.frame.is_none() {
            state
                .begin_frame([9u8; 1 << 16].to_vec())
                .expect("nothing owed");
        }
        match send_pending_frame(&writer, &mut state) {
            Ok(()) => continue,
            Err(failure) => break failure,
        }
    };
    let X11FrameSendFailure::Blocked { written, blocked } = failure else {
        panic!("a recipient taking nothing is the blocked case, not an io error")
    };
    assert!(
        blocked >= X_AUTHORITY_ORDERED_BLOCKED_LIMIT,
        "the bound is reached by measured waiting: {blocked:?}"
    );
    assert_eq!(blocked, state.blocked(), "the owner's accumulator is the one added to");
    assert!(written > 0 && !state.frame_complete(), "it stopped part way");
    drop(reader);
}

#[test]
fn a_frame_still_owed_bytes_cannot_be_abandoned() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    // Seeded close to the limit so the stall is reached without waiting out
    // the whole policy.
    let mut state = X11OrderedSendState {
        blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60),
        ..X11OrderedSendState::default()
    };

    // Fill the buffer so a large frame stops part way through.
    loop {
        if state.frame_complete() {
            state.retire_frame().expect("it went whole");
        }
        if state.frame.is_none() {
            state.begin_frame([1u8; 1 << 16].to_vec()).expect("nothing owed");
        }
        if send_pending_frame(&writer, &mut state).is_err() {
            break;
        }
    }
    assert!(!state.frame_complete(), "a frame is still owed bytes");

    // Those bytes are an event's beginning and the recipient is waiting for
    // the rest of it. Writing a different frame now would put a second event's
    // opening bytes inside the first one's body, which an X11 client has no
    // way to notice.
    let refused = state
        .begin_frame([2u8; 32].to_vec())
        .expect_err("a partly sent frame cannot be walked away from");
    let X11FrameSendFailure::Incomplete { sent, len } = refused else {
        panic!("refused for being incomplete")
    };
    assert!(sent > 0 && sent < len);
    drop(reader);
}
