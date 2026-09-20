// What a release answers for: its own hold after the surface is gone, and the
// proof recorded by service rather than by whatever input arrives next.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_final_release_carries_the_presss_own_custody_rather_than_replacing_it() {
    // A press and a release are two events owed to the same recipient, each
    // with its own admission and its own handle. Ending the physical hold
    // answers neither and transfers neither, so a release that built fresh
    // custody and dropped the record's left the press's delivery owed by
    // nobody -- its answer reachable only through owners outside this
    // instance.
    let client = XServerFrontendClientId(2551);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2551),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(None, &mut |_, _| Ok(()))
        .expect("the press delivers");
    let press_cell = private.terminal.holds[0]
        .custody
        .completion
        .clone()
        .expect("the press holds its own completion");

    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2552),
            272,
            false,
        ))
        .expect("the order to accept the release");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(None, &mut |_, _| Ok(()))
        .expect("the release delivers");

    assert!(
        private.terminal.holds.is_empty(),
        "the physical hold ended"
    );
    assert_eq!(private.terminal.settling.len(), 1);

    // BOTH CUSTODIES, DISTINCT. The release has its own, and the press's
    // travelled on rather than being replaced by it.
    let release_cell = private.terminal.settling[0]
        .custody
        .completion
        .clone()
        .expect("the release holds its own completion");
    let carried = private.terminal.settling[0]
        .press_custody
        .as_ref()
        .expect("the press's custody travelled with the release")
        .completion
        .clone()
        .expect("and still holds the press's own completion");
    assert!(
        Arc::ptr_eq(&carried, &press_cell),
        "the very completion the press was recorded with, not a replacement"
    );
    assert!(
        !Arc::ptr_eq(&release_cell, &press_cell),
        "and the release's is a different admission's, as it must be"
    );
}

#[test]
fn a_press_whose_answer_could_never_be_recognised_refuses_before_applying() {
    // Presses carry the same forward custody releases do, acquired on the
    // operation that creates the debt. A press that cannot get it refuses
    // before the effect rather than applying one whose answer nothing could
    // ever match to it.
    let client = XServerFrontendClientId(2541);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, .. } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    // Never admitted, so no completion was ever minted for it.
    let refused = private.run_ordered_input(
        keyboards,
        &button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2599),
            272,
            true,
        ),
        &pressed,
        watch,
    );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CompletionMissing)
        ),
        "a press with no completion is refused under its own cause"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "and nothing was applied: no hold, no obligation, nothing owed"
    );
}

#[test]
fn a_release_whose_answer_could_never_be_recognised_refuses_and_keeps_its_hold() {
    // Custody is acquired before the effect, and a release that cannot get it
    // is refused there -- before the ledger moves and before the record that
    // owns the native obligation leaves inventory. Enqueuing anyway would
    // hand over a delivery whose answer nothing could ever match to its debt.
    let client = XServerFrontendClientId(2531);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    // A real press through the ingress, so a hold exists with its obligation.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2531),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(None, &mut |_, _| Ok(()))
        .expect("the press delivers");
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(
        private.terminal.holds[0].native.is_some(),
        "the press left its source obligation on its record"
    );

    // A release whose delivery was never admitted, so no completion was ever
    // minted for it. This is the case the executor must refuse.
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let released = role.reserve(stamp, 9).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2599),
            272,
            false,
        ),
        &released,
        watch,
    );
    // NAMED, not merely refused. A known-absent completion says this delivery
    // will never be answerable; an unreadable ledger says nothing was
    // established either way. Reporting both as RecoveryUnavailable discarded
    // the difference at the boundary where it decides what to do next.
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CompletionMissing)
        ),
        "a release with no completion is refused under its own cause"
    );

    // AND THE OBLIGATION IS STILL HERE. Refusing before the effect is what
    // makes that true: nothing was released, so nothing was left unowned.
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "the hold stays exactly where it was"
    );
    assert!(
        private.terminal.holds[0].native.is_some(),
        "and it still owns the activation, query scope and selection it raised"
    );
    assert!(
        private.terminal.settling.is_empty(),
        "no release record was made for a release that did not happen"
    );
}

#[test]
fn a_release_holds_the_completion_of_the_admission_it_was_recorded_for() {
    // Acquiring the handle at dispatch meant looking the delivery up by id
    // again. By then the original ticket may have been pruned -- leaving no
    // handle at all -- or the id re-admitted, leaving the replacement's
    // handle on the older debt. Taken when the release is recorded, neither
    // is reachable.
    let client = XServerFrontendClientId(2521);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2521u64, true), (2522u64, false)] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);

    // BEFORE ANY TERMINAL VISIT. The handle is already held, because it was
    // taken on the operation that created this debt.
    let held = private.terminal.settling[0]
        .completion()
        .expect("the release holds its own completion from the moment it exists")
        .clone();
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");

    // The original ticket is answered and pruned, then the id is admitted
    // again -- the two situations that defeated a dispatch-time lookup.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the original admission is answered");
    recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::Flushed,
    });
    let reused = button_to(surface, delivery, 274, true);
    let readmitted = recovery.admit(&reused, 0, std::time::Instant::now());

    // Still its own, and still answering with its own outcome.
    assert!(
        Arc::ptr_eq(
            private.terminal.settling[0]
                .completion()
                .expect("still held"),
            &held
        ),
        "nothing refreshed the handle, so a re-admission did not replace it"
    );
    if readmitted {
        let fresh = recovery
            .completion_for(delivery).ok().flatten()
            .expect("the re-admission minted its own");
        assert!(
            !Arc::ptr_eq(&fresh, &held),
            "and the replacement is a different completion entirely"
        );
    }
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "the debt reads the answer of the admission it was recorded for"
    );
}

#[test]
fn custody_of_the_answer_is_taken_before_the_handover_not_after_it_succeeds() {
    // Stored after a successful send, custody would be absent exactly when it
    // matters most: an enqueue followed by an interruption leaves a record
    // that began a handover and cannot recognise its own receipt. A send that
    // fails is the reachable case with the same shape -- the handover was
    // attempted, so the answer must already be held.
    let client = XServerFrontendClientId(2511);
    let PreparedOrderedFixture {
        keeper,
        mut runner,
        ingress,
        registration,
        channels,
        durable,
        surface,
        window: _window,
        selections: _selections,
        client: _client,
        namespace: _namespace,
        deliveries: _deliveries,
        _acks,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2511u64, true), (2512u64, false)] {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    // Record the proof, so the release becomes eligible for an attempt.
    for _ in 0..4 {
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
        if private.terminal.settling[0].native_recorded() {
            break;
        }
    }
    assert!(private.terminal.settling[0].native_recorded());

    // The recipient's queue goes, while its routes stay. The sender is still
    // found, so the handover is attempted -- and refused.
    drop(channels);

    let step = private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    assert!(
        matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued: false,
                ..
            }
        ),
        "the handover was attempted and refused"
    );
    assert!(
        private.terminal.settling[0].completion().is_some(),
        "custody of the answer was taken before the send, so a handover that \
         failed still leaves the record able to recognise its own receipt"
    );
    drop(registration);
    drop(durable);
    drop(_acks);
}

#[test]
fn a_reused_delivery_id_does_not_settle_the_debt_that_had_it_before() {
    // A delivery id can be pruned and handed out again. The same client can
    // then publish an outcome under that number for a different incarnation
    // entirely, and matching on client and id alone would let it answer a
    // debt it has nothing to do with. Same client is the demonstrated case,
    // so a wrong-client check is not the protection.
    let client = XServerFrontendClientId(2501);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2501u64, true), (2502u64, false)] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    for _ in 0..12 {
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    }
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");
    assert!(
        private.terminal.settling[0].completion().is_some(),
        "custody of this admission's completion was taken before the handover"
    );

    // The delivery is answered and an ordinary observer consumes it, which
    // prunes the ticket and frees the id. Then the SAME id is admitted again
    // for different work by the same client.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::WriteFailed,
        )
        .expect("the first admission is answered");
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "the held cell carries its own admission's answer"
    );
    recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::WriteFailed,
    });

    // A fresh admission under the reused number. IT GETS ITS OWN CELL, which
    // is what makes the number harmless: the old holder's answer can never be
    // written by later work, whether or not that work is answered first.
    let reused = button_to(surface, delivery, 274, true);
    assert!(
        recovery.admit(&reused, 0, std::time::Instant::now()),
        "the pruned id is available again, which is the situation being guarded"
    );
    let fresh = recovery
        .completion_for(delivery).ok().flatten()
        .expect("the new admission minted its own completion");
    let held = private.terminal.settling[0]
        .completion()
        .expect("the release still holds its own");
    assert!(
        !Arc::ptr_eq(&fresh, held),
        "a reused delivery id mints a new completion rather than handing back \
         the one an earlier admission is still answered through"
    );
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the new admission is answered");
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "and the older debt still reads its own answer, not the newer one"
    );
}

#[test]
fn a_receipt_settles_only_what_it_establishes_and_never_authorises_a_replay() {
    // Two releases with the same shape and opposite answers. A flush is proof
    // the bytes went; a failed write is the absence of proof either way, and
    // the difference is the whole of what a receipt is for.
    let client = XServerFrontendClientId(2481);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, button, pressed) in [
        (2481u64, 272u32, true),
        (2482, 272, false),
        (2483, 273, true),
        (2484, 273, false),
    ] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 2);

    // Drive until both releases have their proof recorded and their delivery
    // handed over.
    for _ in 0..24 {
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    }
    for index in 0..2 {
        assert_eq!(
            private.terminal.settling[index].dispatch(),
            PrivateDispatchPhase::Enqueued,
            "release {index} reached its recipient's queue"
        );
        assert!(private.terminal.settling[index].attempt().is_some());
    }

    // The writer answers: one flushed, one failed.
    let recovery = &private.broker.registry.input_recovery;
    for (index, outcome) in [
        (0usize, XAuthorityInputDeliveryOutcome::Flushed),
        (1, XAuthorityInputDeliveryOutcome::WriteFailed),
    ] {
        let delivery = private.terminal.settling[index]
            .delivery()
            .expect("the release knows which delivery carries it");
        recovery
            .finish(client, Some(delivery), outcome)
            .expect("the writer's answer is published against the delivery it names");
    }

    // Answer both receipts.
    for _ in 0..4 {
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    }

    // THE FLUSH SETTLED THE RECIPIENT'S HALF.
    assert_eq!(
        private.terminal.settling[0].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Enqueued,
        "a flushed delivery is finished where it was, not marked unsendable"
    );

    // THE FAILED WRITE SETTLED NOTHING AND AUTHORISED NOTHING. It is not
    // proof that nobody received anything, so the debt stays owed; and it may
    // have put part of the event on the wire, so the event is never rebuilt.
    assert_eq!(
        private.terminal.settling[1].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "the cause is kept apart from what it settled"
    );
    assert_eq!(
        private.terminal.settling[1].dispatch(),
        PrivateDispatchPhase::Unrepeatable,
        "and the capsule is never offered again"
    );
    assert!(
        private.terminal.settling[1].attempt().is_none(),
        "the attempt was finished, so the ledger's slot is free for others"
    );
}

#[test]
fn an_attempt_that_cannot_be_placed_is_given_back_and_keeps_its_capsule() {
    // A claim this executor cannot place is an unused reservation. It goes
    // back with neither bit -- the debt is exactly as owed as before -- and
    // the capsule stays here, because the event was decided at a moment that
    // has passed and cannot be rebuilt.
    let client = XServerFrontendClientId(2471);
    // Taken by value: this control drops the recipient's routes on purpose,
    // so it has to own them rather than borrow them from a fixture that
    // outlives them.
    let PreparedOrderedFixture {
        keeper,
        mut runner,
        ingress,
        registration,
        channels,
        durable,
        surface,
        window: _window,
        selections: _selections,
        client: _client,
        namespace: _namespace,
        deliveries: _deliveries,
        _acks,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2471u64, true), (2472u64, false)] {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);
    // THE PRESS THIS RELEASE ENDS GOES FIRST. It is the earlier event on the
    // same connection, so the release cannot overtake it.
    assert!(matches!(
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Dispatched {
            enqueued: true,
            relinquished: false
        }
    ));

    assert!(matches!(
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Recorded { recorded: true }
    ));

    // The recipient's routes go. Nothing this executor holds changes: the
    // release is still owed and its event is still the one that was decided.
    drop(registration);
    drop(channels);

    let step = private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    assert!(
        matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued: false,
                ..
            }
        ),
        "the attempt could not be placed, so nothing was enqueued"
    );
    assert!(
        private.terminal.attempt_custody.is_none(),
        "and the ledger's slot was given back rather than held for a delivery \
         nobody will make"
    );
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "the record stops naming an attempt once the ledger confirmed it back"
    );
    assert!(
        !private.terminal.is_empty(),
        "the release itself is still owed"
    );
    drop(durable);
    drop(_acks);
}

#[test]
fn a_release_that_cannot_build_does_not_hide_another_recipients() {
    // Choosing the record before asking the ledger meant one release that can
    // never produce a capsule sat at the front and returned every visit
    // before the ledger's cursor moved. Everything behind it was hidden for
    // ever. The ledger selects now, so its cursor is what carries the visit
    // past a release this executor cannot serve.
    //
    // ACROSS RECIPIENTS, which is the only place the claim can be made. On one
    // connection a release that cannot be built DOES hide what is behind it,
    // and must: the events after it are owed to the same client in order, and
    // handing one over early would show it a button going down before the one
    // it was already owed came up.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(2461));
    attempt_release(&mut f, 2461, 272);

    let other = XServerFrontendClientId(2462);
    let other_window = XResourceId::new(0x302462, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: u16::MAX,
                    xi_event_mask: [0; 8],
                    xi_event_mask_words: 0,
                    route_lease: None,
                },
            )
            .unwrap();
        (registration, channels)
    };
    attempt_release(&mut f, 2463, 273);

    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.terminal.settling.len(), 2, "two releases are owed");
    assert_eq!(private.terminal.settling[0].reached().client(), f.client);
    assert_eq!(private.terminal.settling[1].reached().client(), other);

    // The first can never produce a capsule: its event is taken away, which
    // is what an unwrappable or already-consumed emission amounts to here.
    let stolen = private.terminal.settling[0]
        .native_mut()
        .and_then(PrivateNativeHold::take_release_emission);
    assert!(stolen.is_some(), "the first release did have an event to lose");
    drop(stolen);

    // Drive terminal visits. The first release can never be served; the
    // second, owed to a different connection, must still get there.
    for _ in 0..24 {
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("a terminal step");
    }

    assert_eq!(
        private.terminal.settling[1].dispatch(),
        PrivateDispatchPhase::Enqueued,
        "the other recipient's release still reached it"
    );
    assert!(
        private.terminal.settling[1].attempt().is_some(),
        "and it is named by the attempt it was delivered under"
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "while the one that cannot be built is still owed, not abandoned"
    );
    drop(other_registration);
}

#[test]
fn an_interrupted_handover_is_not_retried_just_because_its_slot_is_empty() {
    // An empty pending slot means one of two opposite things: nothing was
    // taken yet, or a handover began and never reported. Only the phase tells
    // them apart, and retrying the second would send an event the recipient
    // may already have. This fails closed.
    let client = XServerFrontendClientId(2451);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2451u64, true), (2452u64, false)] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);

    // THE PRESS THIS RELEASE ENDS GOES FIRST. It is the earlier event on the
    // same connection, so the release cannot overtake it.
    assert!(matches!(
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Dispatched {
            enqueued: true,
            relinquished: false
        }
    ));

    // Its proof goes in, which is what makes it eligible for an attempt.
    assert!(matches!(
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Recorded { recorded: true }
    ));

    // Stage exactly what an interrupted handover leaves behind: the capsule
    // gone from the slot and the phase saying the handover was begun. Nothing
    // else about the record is touched.
    stage_interrupted_handover(&mut private.terminal.settling[0]);
    assert!(
        settling_slot_is_empty(&private.terminal.settling[0]),
        "the slot is empty, which is the trap this control is about"
    );

    // FAILS CLOSED. No attempt is made, and the turn reports nothing to do
    // rather than inventing work from an absence.
    assert!(matches!(
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "no attempt was claimed for a delivery nobody can say was not received"
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Indeterminate,
        "and the phase still says so rather than being cleared by looking"
    );
}

#[test]
fn a_proof_recording_visit_is_charged_and_watched_like_any_other_step() {
    // A visit that returned before the charge hook would be work the service
    // never admitted and the supervisor never saw -- unbudgeted, unwatched,
    // and invisible to the accounting that bounds every other terminal step.
    let client = XServerFrontendClientId(2431);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
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
    let watch = watch.as_ref().expect("a sealed watch");

    // The press goes through the wrapper; the release is driven by hand so
    // the visit that follows it can be observed being charged.
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2431),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let lease = keeper.lease();
    ingress
        .submit(&lease, button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2432),
            272,
            false,
        ))
        .expect("the order to accept the release");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1, "the release is ready to deliver");
    // Installed by hand rather than through the wrapper, because the wrapper
    // charges nothing and would spend the visit this control is watching for.
    private.terminal.delivering.extend(turn);

    let charged: std::cell::RefCell<Vec<Option<crate::ReadySequence>>> =
        std::cell::RefCell::new(Vec::new());
    let mut charge = |sequence: Option<crate::ReadySequence>, _: std::time::Instant| {
        charged.borrow_mut().push(sequence);
        Ok(())
    };
    // One step to deliver the release itself, charged under its own entry.
    assert!(matches!(
        private.deliver_one(None, &mut charge).expect("a step"),
        PrivateDeliveryStep::Advanced { .. }
    ));
    assert!(
        charged.borrow()[0].is_some(),
        "an ordered entry charges under its own sequence"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "and its release is owed a recording"
    );
    charged.borrow_mut().clear();

    // The next step has nothing left to deliver, so it spends a recording
    // visit -- and asks to be charged for it first.
    let step = private.deliver_one(None, &mut charge).expect("a step");
    assert!(
        matches!(step, PrivateDeliveryStep::Recorded { recorded: true }),
        "the visit recorded the release's native bit"
    );
    assert_eq!(
        charged.borrow().as_slice(),
        &[None],
        "and it asked to be charged first, naming no ordered entry because it \
         is not one"
    );

    // With the proof in, the same release owes a delivery attempt. It is
    // native work too, and it charges under None for the same reason.
    charged.borrow_mut().clear();
    assert!(matches!(
        private.deliver_one(None, &mut charge).expect("a step"),
        PrivateDeliveryStep::Dispatched { .. }
    ));
    assert_eq!(
        charged.borrow().as_slice(),
        &[None],
        "a delivery attempt is charged before it is made, naming no ordered \
         entry because it is not one"
    );

    // With nothing left owed, an empty order is idle again and costs nothing.
    let before = charged.borrow().len();
    assert!(matches!(
        private.deliver_one(None, &mut charge).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert_eq!(
        charged.borrow().len(),
        before,
        "an idle turn with nothing owed charges nothing"
    );
}

#[test]
fn a_release_proof_is_recorded_by_service_rather_than_by_the_next_input() {
    // The blocker this control exists for: proof recording used to happen as
    // incidental work inside a successful input execution, so a release whose
    // proof was still owed only progressed when somebody submitted ANOTHER
    // input. An idle session never finished it. Recording is terminal work
    // now, and this proves it can be asked for with an empty order.
    let client = XServerFrontendClientId(2421);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;

    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2421),
            272,
            true,
        ))
        .expect("the order to accept the press");
    for _ in 0..4 {
        runner.service_turn(&keeper.lease()).expect("a serviceable turn");
    }
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2422),
            272,
            false,
        ))
        .expect("the order to accept the release");

    // NOTHING MORE IS SUBMITTED FROM HERE. Every recording counted below is
    // done by service turns alone.
    let mut recorded = 0;
    for _ in 0..12 {
        recorded += runner
            .service_turn(&keeper.lease())
            .expect("a serviceable turn")
            .recorded;
    }
    let private = runner.frontend.as_ref().expect("a live runner");
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "the release is owed and its record carries the obligation"
    );
    assert!(
        private.terminal.settling[0].native_recorded(),
        "service reached the obligation with no further input submitted"
    );
    // ONE visit, ONE entry. Not a sweep, and not repeated against an entry
    // already recorded.
    assert_eq!(
        recorded, 1,
        "exactly one visit put the bit in, and nothing revisited it after"
    );
    // WHAT THIS CONTROL DOES NOT PROVE, stated rather than implied: the
    // recording happens during the same run of turns that delivers the
    // release, so this does not by itself separate "service did it" from
    // "delivering it did it". What separates them is that removing the
    // terminal visit -- the only thing that calls the recording now -- fails
    // this control and the retained-debt control with it.
}
