// Endings over unreadable state: the number an ending keeps across an
// unreadable selection table, a poisoned ledger, and a healthy disconnect.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn an_ending_over_an_unreadable_selection_table_keeps_its_number() {
    // THE EFFECT IS STILL THERE TO BE DONE, and the ending said so. The
    // poisoned guard is read here as diagnostics only -- nothing is recovered
    // into service -- to show the exact subscription is still present under a
    // number the ending must therefore not have handed on.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let registry = &private.broker.registry;
    let client = XServerFrontendClientId(9120);
    let namespace = admitted(client).namespace.id;
    let window = XResourceId::new(0x9120, 1);
    let (registration, _channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    registry
        .select_xfixes_selection_input(client, namespace, window, 0x9120, 0xf)
        .expect("its own selection interest");
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = registry
            .xfixes_selection_subscriptions
            .lock()
            .expect("a readable table");
        panic!("poisoning this registry's selection table, and nothing else");
    }));
    assert!(poisoning.is_err());

    drop(registration);
    let remaining = match registry.xfixes_selection_subscriptions.lock() {
        Ok(_) => panic!("the table is poisoned"),
        Err(poisoned) => poisoned
            .into_inner()
            .contains_key(&(client, window, 0x9120)),
    };
    assert!(remaining, "the subscription the ending could not retire is still there");
    assert_eq!(
        registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished)
    );
    let refused = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("a number with work still under it is not free");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "{refused:?}"
    );
    drop((private, keeper, durable));
}

#[test]
fn an_ending_over_an_unreadable_focus_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9121), |registry| {
        let _held = registry.focused_surface.lock().expect("readable");
        panic!("poisoning the focus cell, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_core_event_subscriptions_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9122), |registry| {
        let _held = registry.core_event_subscriptions.lock().expect("readable");
        panic!("poisoning the core event subscriptions, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_randr_subscriptions_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9123), |registry| {
        let _held = registry.randr_subscriptions.lock().expect("readable");
        panic!("poisoning the RandR subscriptions, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_present_subscriptions_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9124), |registry| {
        let _held = registry.present_subscriptions.lock().expect("readable");
        panic!("poisoning the presentation subscriptions, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_pending_presentations_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9125), |registry| {
        let _held = registry
            .pending_presentations
            .entries
            .lock()
            .expect("readable");
        panic!("poisoning the pending presentations, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_frozen_input_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9126), |registry| {
        let _held = registry.frozen_input.lock().expect("readable");
        panic!("poisoning the frozen input, and nothing else");
    });
}

#[test]
fn an_ending_over_unreadable_parents_keeps_its_number() {
    an_ending_that_could_not(XServerFrontendClientId(9127), |registry| {
        let _held = registry.window_parents.lock().expect("readable");
        panic!("poisoning the parent table, and nothing else");
    });
}

#[test]
fn an_ending_over_an_unreadable_writer_registry_keeps_its_number() {
    // THE CANCELLATION THAT SILENTLY DID NOTHING. A registry it cannot read
    // is an expectation nobody cancelled, and a number freed over that is
    // freed over a client the registry still believes may execute.
    an_ending_that_could_not(XServerFrontendClientId(9128), |registry| {
        let completion = registry
            .control_completion()
            .expect("a private frontend installs one");
        let _held = completion.inner.lock().expect("readable");
        panic!("poisoning the writer registry, and nothing else");
    });
}

#[test]
fn an_ending_over_an_unreadable_recovery_ledger_keeps_its_number() {
    // THE DISCONNECT WHOSE RESULT WAS THROWN AWAY. A ledger that refused to
    // disconnect this connection has its gate open, its socket live and its
    // entry unrevoked, under a number the ending must therefore keep.
    an_ending_that_could_not(XServerFrontendClientId(9129), |registry| {
        let _held = registry.input_recovery.state.lock().expect("readable");
        panic!("poisoning the recovery ledger, and nothing else");
    });
}

#[test]
fn an_ending_over_an_unreadable_client_table_keeps_its_number() {
    // THE ONE TABLE WHOSE UNREADABILITY THE SUCCESSOR MEETS FIRST. Publication
    // reads it before it reaches the number, so the refusal here is the
    // table's own; the number's standing is what this control checks.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9130);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = private.broker.registry.clients.lock().expect("readable");
        panic!("poisoning the client table, and nothing else");
    }));
    assert!(poisoning.is_err());
    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished)
    );
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("nothing publishes through an unreadable table");
    assert!(
        matches!(refused, XServerFrontendRouteError::RegistryPoisoned),
        "{refused:?}"
    );
    drop((private, keeper, durable));
}

#[test]
fn an_ending_whose_recovery_entry_is_not_its_own_keeps_its_number() {
    // A DIRECT SEAM CONTROL. The exclusion makes this impossible from
    // publication -- the entry under a held number is its holder's -- so the
    // entry is replaced here directly, with one that names no occupant. The
    // ending's disconnect then finds an entry that is not this connection's
    // to act on, and must not certify over it.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9131);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    private
        .broker
        .registry
        .input_recovery
        .register(client, None)
        .expect("the entry is replaced with nobody's");
    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished),
        "a disconnect that was not this connection's to perform is not one performed"
    );
    drop((private, keeper, durable));
}

#[test]
fn an_unreadable_occupancy_record_is_reported_as_unreadable() {
    // NOT AS EXCLUDED. Excluded says an incumbent owns the number, which an
    // unreadable record has established nothing about. Startup is refused
    // either way; what differs is what the refusal claims to know.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9132);
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = private
            .broker
            .registry
            .occupancy
            .held
            .lock()
            .expect("readable");
        panic!("poisoning the occupancy record, and nothing else");
    }));
    assert!(poisoning.is_err());
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("nothing publishes through an unreadable record");
    assert!(
        matches!(refused, XServerFrontendRouteError::RegistryPoisoned),
        "{refused:?}"
    );
    drop((private, keeper, durable));
}

#[test]
fn a_second_visit_through_one_right_is_refused_while_the_first_is_inside() {
    // ONE RIGHT, TWO ASKINGS. The record outlives its registration, so the
    // same right can be asked to clean up twice. Two bodies admitted through
    // it would both run number-keyed effects; the first to return would free
    // the number for both, and the other would then act on the successor.
    //
    // THE FIRST VISIT IS HELD OPEN BY THIS TABLE; the second is asked from
    // another thread with a bounded wait, so a second visit that was admitted
    // -- and therefore blocked behind this table too -- is reported as a
    // failure rather than as a hang.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9133);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let record = Arc::clone(pin.cleanup_record());
    drop(pin);
    let registry = private.broker.registry.clone();

    let (entered, second_returned, standing_meanwhile, refused_meanwhile) =
        std::thread::scope(|scope| {
            let parents = registry
                .window_parents
                .lock()
                .expect("a readable parent table");
            let first = scope.spawn(move || drop(registration));
            let entered = waited_for(|| {
                matches!(
                    registry.occupancy.state_of(client),
                    Some(PrivateNumberStanding::Visiting)
                )
            });

            // THE SECOND ASKING OF THE SAME RECORD.
            let (done, returned) = channel();
            let asked = Arc::clone(&record);
            let second = scope.spawn(move || {
                asked.run_synchronous_cleanup();
                let _ = done.send(());
            });
            let second_returned = returned
                .recv_timeout(std::time::Duration::from_secs(3))
                .is_ok();
            let standing_meanwhile = registry.occupancy.state_of(client);
            // BOUND, NOT CONSUMED. An attempt that succeeded is a live
            // registration whose own ending needs the table this thread is
            // holding; disposing of it here would be waiting for a lock held
            // by this thread. It is disposed of after the table goes.
            let attempted = registry.register_client_with_admission(client, Some(admitted(client)));
            let refused_meanwhile = attempted.as_ref().err().map(|error| format!("{error:?}"));

            drop(parents);
            drop(attempted);
            first.join().expect("the first visit finished");
            second.join().expect("the second asking returned");
            (entered, second_returned, standing_meanwhile, refused_meanwhile)
        });

    assert!(entered, "the first visit is inside the interval");
    assert!(
        second_returned,
        "a second visit through the same right is refused at once, not admitted behind the first"
    );
    assert_eq!(
        standing_meanwhile,
        Some(PrivateNumberStanding::Visiting),
        "and the first visit's standing is untouched by the refusal"
    );
    assert!(
        refused_meanwhile
            .as_deref()
            .is_some_and(|text| text.contains("ClientNumberExcluded")),
        "the number is nobody else's while the first visit is inside: {refused_meanwhile:?}"
    );
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        None,
        "and only the first visit's return freed it"
    );
    drop((record, private, keeper, durable));
}

#[test]
fn an_unestablished_number_admits_no_further_visit() {
    // NO RETRY, AND NOTHING THAT LOOKS LIKE ONE. A visit that returned without
    // establishing its effects left the number this connection's. Asking the
    // record again is not a second chance: it is refused, the standing stays
    // what it was, and the successor is still told the number is not free.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9134);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let record = Arc::clone(pin.cleanup_record());
    drop(pin);
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = private
            .broker
            .registry
            .surfaces
            .lock()
            .expect("a readable surface table");
        panic!("poisoning this registry's surface table, and nothing else");
    }));
    assert!(poisoning.is_err());
    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished)
    );

    record.run_synchronous_cleanup();
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished),
        "a second asking neither reopens the interval nor frees the number"
    );
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("still not free");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "{refused:?}"
    );
    drop((record, private, keeper, durable));
}

#[test]
fn a_visited_number_is_not_given_back_as_unpublished() {
    // THE CUSTODY TYPE'S OWN RULE, AT ITS OWN SEAM. The unpublished release
    // exists for a publication that failed before anything ran under the
    // number. Once a visit has opened, that description is false, and the
    // release must not act on it.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9135);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let record = Arc::clone(pin.cleanup_record());
    drop(pin);
    let right = record.number.get().expect("a published record holds its right");

    assert!(right.begin_clearing(), "the first visit opens");
    assert!(!right.begin_clearing(), "and a second through the same right does not");
    right.relinquish_unpublished();
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Visiting),
        "a visited number is not an unpublished one"
    );
    right.finish(false);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished)
    );
    right.finish(true);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished),
        "a later report of success from no open visit establishes nothing"
    );
    assert!(!right.begin_clearing(), "and nothing reopens it");

    // The registration's own ending now finds its right already visited and
    // does none of its number-keyed effects; the number stays this
    // connection's, which is the rule.
    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished)
    );
    drop((record, private, keeper, durable));
}

#[test]
fn a_delayed_disconnect_reaches_only_the_connection_it_captured() {
    // THE LEDGER'S ENTRY IS KEYED BY NUMBER AND THE NUMBER IS REISSUED. A
    // disconnect decided while the old connection was live and performed
    // after its successor published must find the successor's entry and leave
    // it alone -- and the same call with the successor's own identity must
    // act. Both are asked here, in that order, against one live successor.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let registry = &private.broker.registry;
    let client = XServerFrontendClientId(9136);
    let (registration, channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let old_incarnation = Arc::clone(
        &registry
            .client_senders(client)
            .expect("its own senders")
            .connection_state,
    );
    drop(channels);
    drop(registration);
    assert_eq!(registry.occupancy.state_of(client), None);

    let (successor, successor_channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");
    let successor_incarnation = Arc::clone(
        &registry
            .client_senders(client)
            .expect("the successor's senders")
            .connection_state,
    );
    let (peer, owned) = UnixStream::pair().expect("socket pair");
    let retained = owned.try_clone().expect("a second handle on the same connection");
    registry
        .input_recovery
        .attach(client, owned)
        .expect("the successor's own socket");
    peer.set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .expect("a bounded wait");

    // THE OLD IDENTITY, ACTING LATE.
    let stale = registry.input_recovery.disconnect_exact(
        client,
        &old_incarnation,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        None,
    );
    assert!(matches!(stale, Ok(false)), "not this entry's to act on: {stale:?}");
    let mut byte = [0u8; 1];
    let untouched = std::io::Read::read(&mut &peer, &mut byte);
    assert!(
        untouched.is_err(),
        "the successor's socket is still open: {untouched:?}"
    );
    assert!(
        registry.client_senders(client).is_ok(),
        "and its row is still there"
    );

    // THE SUCCESSOR'S OWN IDENTITY, which is the entry's.
    let exact = registry.input_recovery.disconnect_exact(
        client,
        &successor_incarnation,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        None,
    );
    assert!(matches!(exact, Ok(true)), "{exact:?}");
    let ended = std::io::Read::read(&mut &peer, &mut byte);
    assert!(matches!(ended, Ok(0)), "its own disconnect reached its socket: {ended:?}");
    drop((successor_channels, successor, retained, peer, private, keeper, durable));
}

#[test]
fn a_stalled_watchers_ending_does_not_reach_its_numbers_successor() {
    // THE DISPATCHER'S REAL FAILURE ACTION, ON THE VALUE THE ROUTE HANDED
    // BACK. The old watcher's protocol queue is actually filled and the route
    // actually refuses it; that watcher then ends and is succeeded at the
    // same number; and only then does the dispatcher's ending run. It carries
    // the identity that stalled, so the successor -- its row, its recovery
    // entry, its socket -- is not the one ended.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let registry = &private.broker.registry;
    let client = XServerFrontendClientId(9137);
    let (registration, channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let change = || crate::XClientEvent::XfixesSelectionNotify {
        sequence: 0,
        subtype: 0,
        window: XResourceId::new(0x9137, 1),
        owner: crate::XResourceId::NONE,
        selection: 0x9137,
        time: 0,
        selection_time: 0,
    };
    let mut stalled = None;
    for _ in 0..4096 {
        match registry.route_protocol_to_watcher(client, change()) {
            Ok(()) => continue,
            Err(XServerFrontendWatcherRefusal::Stalled(recipient)) => {
                stalled = Some(recipient);
                break;
            }
            Err(XServerFrontendWatcherRefusal::Route(error)) => {
                panic!("the queue fills, it does not fail otherwise: {error:?}")
            }
        }
    }
    let stalled = stalled.expect("the old watcher's queue filled");
    assert_eq!(stalled.client(), client);

    // The old watcher ends and is succeeded, with a socket of its own.
    drop(channels);
    drop(registration);
    assert_eq!(registry.occupancy.state_of(client), None);
    let (successor, successor_channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");
    let (peer, owned) = UnixStream::pair().expect("socket pair");
    let retained = owned.try_clone().expect("a second handle on the same connection");
    registry
        .input_recovery
        .attach(client, owned)
        .expect("the successor's own socket");
    peer.set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .expect("a bounded wait");

    // NOW THE DISPATCHER ENDS THE WATCHER THAT STALLED.
    registry
        .disconnect_saturated_recipient(stalled)
        .expect("ending a stalled watcher is not an error");

    assert!(
        registry.client_senders(client).is_ok(),
        "the successor's row is not the stalled watcher's"
    );
    let mut byte = [0u8; 1];
    let untouched = std::io::Read::read(&mut &peer, &mut byte);
    assert!(
        untouched.is_err(),
        "and its socket was not shut down: {untouched:?}"
    );
    let accepted = registry.route_protocol(client, change());
    assert!(accepted.is_ok(), "and it still receives: {accepted:?}");
    assert!(
        successor_channels.protocol.try_recv().is_ok(),
        "on its own queue"
    );
    drop((successor_channels, successor, retained, peer, private, keeper, durable));
}

#[test]
fn a_stalled_watcher_that_is_still_live_is_the_one_ended() {
    // THE POSITIVE HALF OF THE SAME RULE. A value that named nobody would
    // leave every successor alone by leaving everyone alone. The watcher that
    // stalled, still live, is ended through it: its row goes and its socket
    // is shut down.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let registry = &private.broker.registry;
    let client = XServerFrontendClientId(9138);
    let (registration, channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let (peer, owned) = UnixStream::pair().expect("socket pair");
    let retained = owned.try_clone().expect("a second handle on the same connection");
    registry
        .input_recovery
        .attach(client, owned)
        .expect("the watcher's own socket");
    peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("a bounded wait");
    let change = || crate::XClientEvent::XfixesSelectionNotify {
        sequence: 0,
        subtype: 0,
        window: XResourceId::new(0x9138, 1),
        owner: crate::XResourceId::NONE,
        selection: 0x9138,
        time: 0,
        selection_time: 0,
    };
    let mut stalled = None;
    for _ in 0..4096 {
        match registry.route_protocol_to_watcher(client, change()) {
            Ok(()) => continue,
            Err(XServerFrontendWatcherRefusal::Stalled(recipient)) => {
                stalled = Some(recipient);
                break;
            }
            Err(XServerFrontendWatcherRefusal::Route(error)) => {
                panic!("the queue fills, it does not fail otherwise: {error:?}")
            }
        }
    }
    let stalled = stalled.expect("the watcher's queue filled");

    registry
        .disconnect_saturated_recipient(stalled)
        .expect("ending a stalled watcher is not an error");
    assert!(
        matches!(
            registry.client_senders(client),
            Err(XServerFrontendRouteError::UnknownClient { .. })
        ),
        "the stalled watcher's row is gone"
    );
    let mut byte = [0u8; 1];
    let ended = std::io::Read::read(&mut &peer, &mut byte);
    assert!(matches!(ended, Ok(0)), "and its socket was shut down: {ended:?}");
    drop((channels, registration, retained, peer, private, keeper, durable));
}

#[test]
fn a_stale_rights_late_report_does_not_free_the_successors_open_visit() {
    // THE OCCUPANT CHECK IN finish, AT ITS OWN SEAM. No production body
    // reaches finish with a foreign occupant now that begin_clearing admits
    // only from Held under the same identity: the only removal during a visit
    // is that visit's own return, so no second body can be inside when a
    // successor's claim goes in. That argument was made once before about a
    // premise that did not hold, so it is not relied on here. The old right is
    // retained directly and made to report late, while the successor's own
    // visit is open; the successor's standing must not move.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9139);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let PrivateCustodyReach::Reached(pin) = registration
        .registered_custody(&keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    let old_record = Arc::clone(pin.cleanup_record());
    drop(pin);
    drop(registration);
    assert_eq!(private.broker.registry.occupancy.state_of(client), None);
    let old_right = old_record
        .number
        .get()
        .expect("a published record keeps its right");

    let (successor, _successor_channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");
    let registry = private.broker.registry.clone();
    let (entered, after_stale_report) = std::thread::scope(|scope| {
        let parents = registry
            .window_parents
            .lock()
            .expect("a readable parent table");
        let ending = scope.spawn(move || drop(successor));
        let entered = waited_for(|| {
            matches!(
                registry.occupancy.state_of(client),
                Some(PrivateNumberStanding::Visiting)
            )
        });
        // THE OLD RIGHT REPORTS SUCCESS, LATE.
        old_right.finish(true);
        let after_stale_report = registry.occupancy.state_of(client);
        drop(parents);
        ending.join().expect("the successor's ending finished");
        (entered, after_stale_report)
    });
    assert!(entered, "the successor's own visit is open");
    assert_eq!(
        after_stale_report,
        Some(PrivateNumberStanding::Visiting),
        "a report from a right that is not the occupant frees nothing"
    );
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        None,
        "and the successor's own return does"
    );
    drop((old_record, private, keeper, durable));
}

/// A keyboard grab for a public-broker control, in the shape the dispatcher
/// installs one: synchronous keyboard mode, so the namespace's keyboard is
/// frozen and a routed key press is deferred rather than delivered.
fn public_keyboard_grab(client: XServerFrontendClientId) -> crate::XActiveInputGrab {
    crate::XActiveInputGrab {
        owner: client.raw(),
        window: XResourceId::new(0x9200, 1),
        owner_events: false,
        pointer_mode: 1,
        keyboard_mode: 0,
        event_mask: 0,
        xi_event_mask: [0; 8],
        xi_event_mask_words: 0,
        route_lease: None,
    }
}

#[test]
fn a_current_public_recipients_exact_disconnect_cleans_its_own_authority() {
    // THE POSITIVE HALF: on the public path -- no private lifecycle -- an
    // exact disconnect of the connection that holds the number really does
    // clean that connection's authority. A repair that protected a successor
    // by never cleaning anybody would pass the negative half and fail here.
    let namespace = NamespaceId::from_raw(9201);
    let client = XServerFrontendClientId(9201);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let registry = &broker.registry;
    assert!(registry.input_recovery.lifecycle.get().is_none(), "the public path");
    let (registration, channels) = registry.register_client(client).expect("a row");
    registry
        .input_authority
        .lock()
        .expect("a readable authority")
        .grab_keyboard(namespace, public_keyboard_grab(client))
        .expect("its own keyboard grab");
    let own = Arc::clone(
        &registry
            .client_senders(client)
            .expect("its own senders")
            .connection_state,
    );

    let exact = registry.input_recovery.disconnect_exact(
        client,
        &own,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        None,
    );
    assert!(matches!(exact, Ok(true)), "{exact:?}");
    assert!(
        registry
            .input_authority
            .lock()
            .expect("a readable authority")
            .keyboard_grab(namespace)
            .is_none(),
        "the current recipient's own grab is gone"
    );
    drop((channels, registration, broker));
}

#[test]
fn a_stale_exact_disconnect_leaves_a_public_successors_authority_alone() {
    // THE NEGATIVE HALF, AT THE ACT. The predecessor's identity, acting after
    // its successor published and installed a grab under the same number,
    // must find the entry is not its own and clean nothing -- and the
    // successor's own identity, acting afterwards, must clean exactly that.
    let namespace = NamespaceId::from_raw(9202);
    let client = XServerFrontendClientId(9202);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let registry = &broker.registry;
    let (registration, channels) = registry.register_client(client).expect("a row");
    let old = Arc::clone(
        &registry
            .client_senders(client)
            .expect("its own senders")
            .connection_state,
    );
    drop(channels);
    drop(registration);
    assert_eq!(registry.occupancy.state_of(client), None);

    let (successor, successor_channels) = registry.register_client(client).expect("free");
    registry
        .input_authority
        .lock()
        .expect("a readable authority")
        .grab_keyboard(namespace, public_keyboard_grab(client))
        .expect("the successor's own keyboard grab");
    let stale = registry.input_recovery.disconnect_exact(
        client,
        &old,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        None,
    );
    assert!(matches!(stale, Ok(false)), "{stale:?}");
    assert!(
        registry
            .input_authority
            .lock()
            .expect("a readable authority")
            .keyboard_grab(namespace)
            .is_some(),
        "the successor's grab is untouched by its predecessor's late disconnect"
    );
    let own = Arc::clone(
        &registry
            .client_senders(client)
            .expect("the successor's senders")
            .connection_state,
    );
    let exact = registry.input_recovery.disconnect_exact(
        client,
        &own,
        XAuthorityInputDeliveryOutcome::ClientDisconnected,
        None,
    );
    assert!(matches!(exact, Ok(true)), "{exact:?}");
    assert!(
        registry
            .input_authority
            .lock()
            .expect("a readable authority")
            .keyboard_grab(namespace)
            .is_none(),
        "and its own exact disconnect cleans its own grab"
    );
    drop((successor_channels, successor, broker));
}

#[test]
fn no_successor_can_publish_inside_a_public_exact_disconnect() {
    // THE INTERVAL ITSELF, WITHOUT A HOOK. The act's later effect is the
    // authority cleanup, which needs the authority lock; this control holds
    // that lock, so an exact disconnect is stopped exactly at that effect.
    // What is then observed is whether the ledger is still held there. If it
    // is, no publication can replace the entry inside the act -- shown by a
    // successor's registration not completing while the act is stopped. If it
    // is not (the repaired-away behaviour), the successor publishes and
    // installs its grab through this control's own guard, and the act's
    // resumed cleanup erases it.
    //
    // OBSERVED INTO LOCALS, COMPARED AFTER EVERY LOCK IS RELEASED AND EVERY
    // THREAD COLLECTED. Nothing here waits unboundedly on the act.
    let namespace = NamespaceId::from_raw(9203);
    let client = XServerFrontendClientId(9203);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let registry = broker.registry.clone();
    let (registration, channels) = registry.register_client(client).expect("a row");
    let own = Arc::clone(
        &registry
            .client_senders(client)
            .expect("its own senders")
            .connection_state,
    );
    // The row goes first through an ordinary disconnected send, so the
    // successor's publication below is refused by nothing but the ledger.
    drop(channels);
    let sent = registry.route_control(XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(92030),
            surface: SurfaceId::new(9203, 1),
        },
    });
    assert!(sent.is_err(), "{sent:?}");
    // And the predecessor's cleanup releases the number, but NOT its ledger
    // entry: the stale exact disconnect below is the delayed act that still
    // names the entry its identity was published for.
    let record_still_named = {
        let ledger = registry.input_recovery.state.lock().expect("a readable ledger");
        ledger
            .connections
            .get(&client)
            .is_some_and(|entry| entry.belongs_to(&own))
    };
    assert!(record_still_named, "the entry is still the predecessor's until somebody replaces it");
    // AND THE NUMBER IS ACTUALLY FREE. With the predecessor's registration
    // still alive, a successor would be refused by the number claim and the
    // question below -- can it publish inside the act -- would never be put
    // to the ledger. Its ending runs the same exact disconnect once already;
    // the act below is the delayed one, arriving after the number went back.
    drop(registration);
    assert_eq!(registry.occupancy.state_of(client), None);

    let (
        (ledger_held_at_the_effect, act_still_blocked),
        successor_published_inside,
        grab_after,
        act,
        successor_published,
        successor_refusal,
    ) = std::thread::scope(|scope| {
            let authority = registry
                .input_authority
                .lock()
                .expect("a readable authority");
            let acting = registry.clone();
            let act = scope.spawn(move || {
                acting.input_recovery.disconnect_exact(
                    client,
                    &own,
                    XAuthorityInputDeliveryOutcome::ClientDisconnected,
                    None,
                )
            });
            // The act reaches the authority effect and stops there. Whether it
            // still holds the ledger at that point is the fact under test.
            //
            // BLOCKED, NOT FINISHED. A ledger seen held while the act has not
            // returned is the act holding it somewhere past its identity
            // check; a ledger seen free with the act already returned would
            // mean the act never contended on the authority this control
            // holds, which is a different failure and is reported as one.
            // This does not by itself establish that the act has reached the
            // authority lock -- only that it holds the ledger and has not
            // finished while the authority is held here. Arrival at that lock
            // is what the pre-repair discriminator below shows: an act that
            // released the ledger before it reaches the authority lets the
            // successor publish inside, and its resumed cleanup erases the
            // successor's grab.
            let ledger_held_at_the_effect = waited_for(|| {
                registry.input_recovery.state.try_lock().is_err()
            });
            let act_still_blocked = !act.is_finished();
            // A successor tries to publish while the act is stopped.
            let publishing = registry.clone();
            let (published, publication) = channel();
            let successor = scope.spawn(move || {
                let attempt = publishing.register_client(client);
                let _ = published.send(attempt.is_ok());
                attempt
            });
            let successor_published_inside = publication
                .recv_timeout(std::time::Duration::from_millis(500))
                .unwrap_or(false);
            if successor_published_inside {
                // Through this control's own guard, as the dispatcher would
                // through its own.
                let mut authority = authority;
                authority
                    .grab_keyboard(namespace, public_keyboard_grab(client))
                    .expect("the successor's keyboard grab");
                drop(authority);
            } else {
                drop(authority);
            }
            let act = act.join().expect("the act returned");
            let successor = successor.join().expect("the publication returned");
            // THE SUCCESSOR REALLY PUBLISHES -- after the act. A refusal here
            // would mean the question was never put to the ledger at all.
            let successor_published = successor.is_ok();
            let successor_refusal = successor.as_ref().err().map(|error| format!("{error:?}"));
            if !successor_published_inside {
                // Published after the act, as it should be; give it its grab
                // now so the comparison below asks the same question.
                registry
                    .input_authority
                    .lock()
                    .expect("a readable authority")
                    .grab_keyboard(namespace, public_keyboard_grab(client))
                    .expect("the successor's keyboard grab");
            }
            let grab_after = registry
                .input_authority
                .lock()
                .expect("a readable authority")
                .keyboard_grab(namespace)
                .is_some();
            drop(successor);
            (
                (ledger_held_at_the_effect, act_still_blocked),
                successor_published_inside,
                grab_after,
                act,
                successor_published,
                successor_refusal,
            )
        });

    assert!(matches!(act, Ok(true)), "the predecessor's own disconnect: {act:?}");
    assert!(
        successor_published,
        "the successor really publishes once the act has returned: {successor_refusal:?}"
    );
    // THE FACTS TRAVEL IN THE MESSAGES. A failure anywhere says which of them
    // failed and what the others were, without printing from a passing run.
    assert!(
        ledger_held_at_the_effect && act_still_blocked,
        "the ledger is held by the act while it has not returned and the authority is held here \
         (ledger_held_at_the_effect={ledger_held_at_the_effect}, act_still_blocked={act_still_blocked}, \
         successor_published_inside={successor_published_inside}, successor_published={successor_published}, \
         grab_after={grab_after}, act={act:?})"
    );
    assert!(
        !successor_published_inside,
        "no successor can publish inside a public exact disconnect"
    );
    assert!(
        grab_after,
        "and the successor's grab, installed after the act, is its own"
    );
    drop(broker);
}

#[test]
fn a_ledger_poisoned_after_a_healthy_disconnect_still_keeps_the_number() {
    // THE ARM THAT DECIDES ON ITS OWN. The cleanup's recovery disconnect can
    // succeed while the ledger is readable, and the ledger can be poisoned by
    // another holder before the same cleanup settles its abandoned frozen
    // routes. That later failure is the only report of work nobody did, and
    // the number must not be freed over it.
    //
    // A REAL FROZEN ROUTE: a synchronous keyboard grab freezes the namespace,
    // and a key press routed through the broker is deferred into the frozen
    // queue rather than delivered. The cleanup is held open by the parent
    // table -- after its disconnect, before its frozen drain -- while the
    // ledger is poisoned from here by an ordinary caught panic.
    let namespace = NamespaceId::from_raw(9204);
    let client = XServerFrontendClientId(9204);
    let surface = SurfaceId::new(9204, 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let registry = broker.registry.clone();
    let (registration, _channels) = registry.register_client(client).expect("a row");
    registry
        .register_surface(client, namespace, surface, XResourceId::new(0x9204, 1))
        .expect("its own surface");
    registry
        .input_authority
        .lock()
        .expect("a readable authority")
        .grab_keyboard(namespace, public_keyboard_grab(client))
        .expect("a synchronous keyboard grab freezes the namespace");
    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 1,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 1,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::Key {
                    keycode: 30,
                    pressed: true,
                },
            },
            route_lease: None,
            delivery: Some(XAuthorityInputDeliveryId::from_raw(92040)),
            mode: XAuthorityRoutedInputMode::Deliver,
            origin: XAuthorityRoutedInputOrigin::Physical,
        })
        .expect("the broker accepts a routed input");
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        registry.frozen_input.lock().expect("readable").len(),
        1,
        "the key press is frozen, not delivered"
    );

    let (disconnect_completed_first, poisoning_caught) = std::thread::scope(|scope| {
        let parents = registry
            .window_parents
            .lock()
            .expect("a readable parent table");
        let ending = scope.spawn(move || drop(registration));
        // Past its row removal, which is after its disconnect.
        let row_gone = waited_for(|| {
            matches!(
                registry.client_senders(client),
                Err(XServerFrontendRouteError::UnknownClient { .. })
            )
        });
        let disconnect_completed_first = row_gone
            && registry
                .input_recovery
                .state
                .lock()
                .expect("still readable here")
                .connections
                .get(&client)
                .is_some_and(|entry| entry.revoked);
        // Now, and only now, the ledger is poisoned.
        let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = registry.input_recovery.state.lock().expect("readable");
            panic!("poisoning the ledger after the healthy disconnect, and nothing else");
        }));
        drop(parents);
        ending.join().expect("the ending finished");
        (disconnect_completed_first, poisoning.is_err())
    });
    assert!(disconnect_completed_first, "the disconnect succeeded while the ledger was readable");
    assert!(poisoning_caught);
    assert_eq!(
        registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished),
        "an abandoned route the ledger would not settle keeps the number"
    );
    drop(broker);
}
