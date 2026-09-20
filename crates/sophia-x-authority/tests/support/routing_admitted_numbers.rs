// Admitted numbers: the worker stopped by an unreadable boundary, the number a
// departure stays behind, and the attempt at an excluded number.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn an_unreadable_boundary_still_stops_the_worker_it_admitted() {
    // FAILING CLOSED TO A NEW START IS NOT A REASON TO LEAVE A RUNNING ONE
    // ALONE. The worker below was admitted through this boundary before it
    // became unreadable, so its stop is known: the association that published
    // it is immutable and does not live behind that lock.
    //
    // BOTH ENTRY POINTS ARE EXERCISED, because the bug was reachable through
    // each: the custody's own departure and a context's.
    for through_context in [false, true] {
        let client = XServerFrontendClientId(if through_context { 9010 } else { 9009 });
        let f = worker_fixture(client);
        f.permit();
        let custody = custody_for(&f, &f.fixture.keeper);
        let context = custody.prepare_control().expect("its own owner");
        let (release, held) = std::sync::mpsc::channel::<()>();
        assert_eq!(
            context.start(move || {
                std::thread::Builder::new().spawn(move || {
                    let _ = held.recv();
                })
            }),
            PrivateStartupOutcome::Started
        );
        assert!(custody.published_stop().is_some());

        let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = custody
                .source
                .departure
                .lock()
                .expect("a readable boundary");
            panic!("poisoning this connection's admission boundary, and nothing else");
        }));
        assert!(poisoning.is_err());

        let answer = if through_context {
            context.depart()
        } else {
            custody.depart_registered()
        };
        let stopped = f.stop.load(std::sync::atomic::Ordering::Acquire);

        // RELEASED AND COLLECTED BEFORE THE COMPARISONS THIS CONTROL IS
        // ABOUT. The startup, publication and poisoning above assert on their
        // own and would end this iteration before here; what follows the
        // collection is the departure's answer and the stop it sent.
        drop(release);
        let record = PrivateReapingRecord::bound_to(&custody);
        let reaped = record.reap().reaped;

        assert_eq!(
            answer,
            PrivateDeparted::Unreadable,
            "it established nothing about the slot, and says so"
        );
        assert!(
            stopped,
            "and the worker it had already admitted was told to stop \
             (through {})",
            if through_context { "its context" } else { "its custody" }
        );
        assert_eq!(reaped, PrivateReaped::Joined);
        drop(record);
        drop(custody);
        drop(f.fixture);
    }
}

#[test]
fn a_repeated_departure_ask_does_not_go_back_to_the_worker_slot() {
    // A RECORDED FACT IS REPORTED, NOT RE-ESTABLISHED. An ask that went to the
    // slot again would spend a second decision on a connection that has one --
    // and would wait for a slot somebody else may be holding, which is what
    // this control makes true while it asks.
    let f = worker_fixture(XServerFrontendClientId(9008));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let starting = custody.prepare_control().expect("its own owner");
    assert_eq!(
        starting.start(|| std::thread::Builder::new().spawn(|| {})),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        custody.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::WorkerRunning)
    );

    let (report, answered) = std::sync::mpsc::channel();
    let repeated = std::thread::scope(|scope| {
        // THE SLOT IS HELD BY THIS CONTROL for the whole of the second ask.
        let held = custody.worker_slot().lock().expect("a readable slot");
        let custody = &custody;
        let asking = scope.spawn(move || {
            report
                .send(custody.depart_registered())
                .expect("its caller waits");
        });
        let answer = answered.recv_timeout(Duration::from_secs(3));
        // RELEASED BEFORE ANYTHING COMPARES, so a second ask that did go to
        // the slot is collected rather than left waiting on this control.
        drop(held);
        asking.join().expect("the repeated ask returned");
        answer
    });

    assert_eq!(
        repeated.ok(),
        Some(PrivateDeparted::AlreadyDecided(PrivateDeparture::WorkerRunning)),
        "it reported the recorded fact without waiting for the slot"
    );
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    drop(record);
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_start_admitted_before_departure_still_cannot_spawn_after_it() {
    // THE OTHER ORDER. A start passes admission and is then delayed before it
    // reaches its transaction; a departure records that nothing was started;
    // and the delayed start must still not make a thread.
    //
    // A DIRECT SEAM CONTROL, AND LABELLED AS ONE. It does not pause a real
    // context.start between its two halves -- that needs a scheduling hook
    // this control does not have. What it does is drive the same two seams in
    // that order, through the connection's own admission boundary and the
    // approved startup transaction, so the worker-slot decision is what has to
    // refuse.
    let f = worker_fixture(XServerFrontendClientId(9011));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");

    // Admitted: this is what context.start does before it holds anything.
    assert_eq!(
        custody.admit_start(context.stop(), context.notice()),
        PrivateStartAdmission::Admitted
    );
    assert!(custody.published_stop().is_some());

    // The departure runs while that start has gone no further.
    assert_eq!(
        custody.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );

    // AND THE DELAYED START REACHES ITS TRANSACTION TOO LATE.
    let called = std::sync::atomic::AtomicBool::new(false);
    let outcome = start_connection_worker(
        custody.worker_slot(),
        context.stop(),
        context.notice(),
        || {
            called.store(true, std::sync::atomic::Ordering::Release);
            std::thread::Builder::new().spawn(|| {})
        },
    );
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    assert!(
        !called.load(std::sync::atomic::Ordering::Acquire),
        "the slot decision refused it, so no thread exists after a departure \
         that recorded none"
    );
    assert_eq!(
        custody.departure_observation(),
        Some(PrivateDeparture::NothingStarted),
        "and the recorded fact is still the one that was established"
    );
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_number_is_not_free_because_its_row_went() {
    // ABSENCE FROM THE CLIENT TABLE IS NOT AN ANSWER. A row is removed when a
    // send finds its endpoint disconnected and when a client stops draining,
    // so "no row" says nothing about whether the connection that had the
    // number has finished with it.
    //
    // THE ROW HERE GOES THROUGH A REAL ROUTING FAILURE -- its receiver is
    // dropped and an ordinary send finds the endpoint gone -- not by reaching
    // into the table.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9101);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    assert_eq!(private.broker.registry.occupancy.state_of(client), Some(PrivateNumberStanding::Held));

    // Its endpoint goes; the next send finds it disconnected and removes the
    // row, which is one of the paths this rule exists for.
    drop(channels);
    let sent = private.broker.registry.route_control(XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(91010),
            surface: SurfaceId::new(9101, 1),
        },
    });
    assert!(sent.is_err(), "the send found its endpoint gone: {sent:?}");
    assert!(
        !private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .contains_key(&client),
        "and its row went with it"
    );

    // THE NUMBER IS STILL THIS REGISTRATION'S. Its ending has not run, so
    // everything that ending does by number is still to come.
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("its number is not free");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "{refused:?}"
    );

    // AND A DIFFERENT NUMBER IS UNAFFECTED.
    let neighbour = XServerFrontendClientId(9102);
    let (other, _other_channels) = private
        .broker
        .registry
        .register_client_with_admission(neighbour, Some(admitted(neighbour)))
        .expect("another number is another connection's");
    drop(other);

    // ONLY ITS ENDING FREES IT, and then a real successor takes it.
    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        None,
        "its ending established that the number may be reused"
    );
    let (successor, _successor_channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free now");

    // THE SUCCESSOR'S OWN STATE IS ITS OWN. Nothing the predecessor's ending
    // did by number reached it: it has a row, a recovery registration of its
    // own and an expected writer of its own.
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .contains_key(&client)
    );
    // AND ITS OWN ENDPOINT WORKS. The predecessor's ending cancelled an
    // expected writer and disconnected a recovery ledger entry under this
    // number; a successor that had been reached by either would not accept
    // this.
    let accepted = private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(91011),
                surface: SurfaceId::new(9101, 2),
            },
        });
    assert!(
        accepted.is_ok(),
        "the successor's own endpoint accepts its own control: {accepted:?}"
    );
    drop(successor);
    drop((private, keeper, durable));
}

#[test]
fn a_number_stays_held_while_its_endings_effects_run() {
    // THE PROTECTED INTERVAL, OBSERVED FROM INSIDE IT. This control holds a
    // lock the ending needs, so the ending is demonstrably part-way through
    // its number-keyed effects while the question is asked. It establishes no
    // timing bound: the ending simply cannot get past this control.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9103);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let registry = private.broker.registry.clone();

    let (entered, row_gone, excluded, outcome, neighbour_ok) = std::thread::scope(|scope| {
        // A TABLE THE ENDING TAKES LATE, AND PUBLICATION NEVER TAKES. The
        // ending reaches this one after its writer cancellation, its recovery
        // disconnect and its row removal, so holding it stops the ending
        // inside the interval its number authorises -- and a registration
        // asked meanwhile needs none of the locks the ending is still to take.
        //
        // Holding an EARLIER one instead would deadlock this control against
        // the ending rather than test it: publication needs the recovery
        // ledger, and an ending stopped before releasing it would be waiting
        // for this table while this control waited for that ledger.
        let parents = registry
            .window_parents
            .lock()
            .expect("a readable parent table");
        let ending = scope.spawn(move || drop(registration));
        // OBSERVED INTO LOCALS, COMPARED AFTER THE LOCK GOES. A comparison
        // made here would, when it fails, unwind while this table is held and
        // leave the ending waiting on a poisoned lock forever -- so a control
        // that had something to report would hang instead of reporting it.
        let entered = waited_for(|| {
            matches!(
                registry.occupancy.state_of(client),
                Some(PrivateNumberStanding::Visiting)
            )
        });
        // AND ITS ROW IS GONE. Visiting is set before the row removal, so a
        // publication asked between the two would rightly be refused as a
        // duplicate -- the table's refusal, not the number's. The number's is
        // what this control is about, so the row's absence is established
        // first, while this table still holds the interval open.
        let row_gone = waited_for(|| {
            matches!(
                registry.client_senders(client),
                Err(XServerFrontendRouteError::UnknownClient { .. })
            )
        });
        let attempted = registry.register_client_with_admission(client, Some(admitted(client)));
        let excluded = matches!(
            attempted.as_ref().err(),
            Some(XServerFrontendRouteError::ClientNumberExcluded { client: same })
                if *same == client
        );
        let outcome = format!("{:?}", attempted.as_ref().err());

        // AND AN UNRELATED NUMBER IS NOT QUEUED BEHIND IT.
        //
        // KEPT, NOT DROPPED HERE. Its own ending would need the table this
        // control is holding, so dropping it inline would be this thread
        // waiting for a lock it holds itself -- which would look exactly like
        // the queueing this asserts does not happen.
        let neighbour = XServerFrontendClientId(9104);
        let admitted_neighbour =
            registry.register_client_with_admission(neighbour, Some(admitted(neighbour)));
        let neighbour_ok = admitted_neighbour.is_ok();
        drop(parents);
        // WHATEVER THAT ATTEMPT WAS, DISPOSED OF AFTER THE TABLE GOES. An
        // attempt that succeeded is a second live registration whose own
        // ending needs this table, so dropping it any earlier would be this
        // thread waiting for a lock it holds itself.
        drop(attempted);
        drop(admitted_neighbour);
        ending.join().expect("the ending finished");
        (entered, row_gone, excluded, outcome, neighbour_ok)
    });

    assert!(
        entered,
        "its ending is inside the interval its number authorises"
    );
    assert!(
        row_gone,
        "and past its row removal, so the table cannot answer the attempt"
    );
    assert!(
        excluded,
        "its number is not available while that interval is open: {outcome}"
    );
    assert!(
        neighbour_ok,
        "one connection's ending does not hold up another connection's number"
    );
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        None,
        "and the interval closed when the ending finished"
    );
    drop((private, keeper, durable));
}

#[test]
fn a_number_whose_ending_could_not_finish_stays_held() {
    // RETURNING FROM A BEST-EFFORT BODY IS NOT PROOF. An ending that could not
    // read a table it had to clear leaves work nobody did, so handing the
    // number on would hand it on over an effect that never happened.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9105);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");

    // A table this connection's ending must clear is poisoned by an ordinary
    // caught panic under its own lock. Nothing in it is rewritten.
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
        Some(PrivateNumberStanding::Unestablished),
        "its ending could not establish that the number may be reused"
    );
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("an unfinished ending keeps its number");
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
fn an_old_senders_failure_does_not_remove_the_connection_that_took_its_number() {
    // THE SCHEDULE THE NUMBER RESERVATION ALONE DOES NOT COVER. An operation
    // captured a sender while its connection was live; that connection ends,
    // its number is released, a successor takes it -- and only THEN does the
    // old send fail. The removal that follows is keyed by the number, so
    // without an identity check it revokes a connection nothing was wrong
    // with.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9106);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");

    // The old operation's sender AND the identity it came with, from the one
    // lookup an operation makes while its connection is live. The pair below
    // is that captured pair, not a sender beside an invented cell.
    let senders = private
        .broker
        .registry
        .client_senders(client)
        .expect("its own senders");
    let stale_incarnation = Arc::clone(&senders.connection_state);
    let stale = senders.control;

    // Its connection ends, which releases the number, and a real successor
    // takes it with channels of its own.
    drop(channels);
    drop(registration);
    assert_eq!(private.broker.registry.occupancy.state_of(client), None);
    let (successor, successor_channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");

    // NOW THE OLD SEND FAILS. Its endpoint is gone, which is exactly the path
    // that removes a row.
    let failed = private.broker.registry.route_to_client(
        client,
        &stale_incarnation,
        stale,
        X11RoutedControl::FocusOut {
            window: XResourceId::new(0x9106, 1),
            time_msec: 0,
            claim: None,
            origin: None,
        },
    );
    assert!(
        matches!(
            failed,
            Err(XServerFrontendRouteError::ClientQueueDisconnected { .. })
        ),
        "the old send really did find its endpoint gone: {failed:?}"
    );

    // AND THE SUCCESSOR IS STILL THERE. Its row was not removed by somebody
    // else's failure at the same number.
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .contains_key(&client),
        "a stale sender's failure removed the connection that took its number"
    );
    let accepted = private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(91060),
                surface: SurfaceId::new(9106, 1),
            },
        });
    assert!(accepted.is_ok(), "and it still routes: {accepted:?}");
    drop(successor_channels);
    drop(successor);
    drop((private, keeper, durable));
}

#[test]
fn a_stale_cleanup_request_does_not_run_against_the_successor() {
    // THE RECORD OUTLIVES ITS REGISTRATION -- its keeper holds it -- so asking
    // it to clean up again is reachable. A request that reacquired a
    // relinquished right would run every number-keyed effect against whoever
    // holds the number now.
    //
    // A DIRECT SEAM CONTROL. It calls the cleanup the registration's own Drop
    // calls, on the record that Drop already used, rather than arranging a
    // second Drop.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9107);
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
    let stale_record = Arc::clone(pin.cleanup_record());
    drop(pin);

    // Its ending runs, its number is released, and a real successor takes it.
    drop(registration);
    assert_eq!(private.broker.registry.occupancy.state_of(client), None);
    let (successor, successor_channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");
    let successor_state = private
        .broker
        .registry
        .client_senders(client)
        .expect("its own senders")
        .connection_state;

    // THE STALE RECORD IS ASKED AGAIN.
    stale_record.run_synchronous_cleanup();

    // AND THE SUCCESSOR IS UNTOUCHED: its row is there, it is the same
    // registration it was, and it still routes.
    let current = private
        .broker
        .registry
        .client_senders(client)
        .expect("the successor still has a row");
    assert!(
        Arc::ptr_eq(&current.connection_state, &successor_state),
        "the row under this number is still the successor's own"
    );
    let accepted = private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(91070),
                surface: SurfaceId::new(9107, 1),
            },
        });
    assert!(accepted.is_ok(), "and it still routes: {accepted:?}");
    assert!(
        matches!(
            private.broker.registry.occupancy.state_of(client),
            Some(PrivateNumberStanding::Held)
        ),
        "the successor's own claim is intact and not marked clearing"
    );
    drop(successor_channels);
    drop(successor);
    drop((stale_record, private, keeper, durable));
}

#[test]
fn a_refused_publication_changes_nothing_under_the_number_it_could_not_take() {
    // THE REFUSAL COMES BEFORE THE STATE, AND THAT ORDER IS THE RULE. The
    // recovery ledger's registration REPLACES a connection's entry -- its
    // lifecycle gate, its socket and its revocation all go with it -- and the
    // expected-writer record is keyed by the number too. An attempt that
    // touched either before finding the number excluded would have disarmed
    // the connection that actually holds it, and the refusal it then returned
    // would be a lie about having changed nothing.
    //
    // THE ROW IS GONE FIRST, so the client table cannot answer and the
    // occupancy record is what refuses.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9109);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the incumbent is admitted to the boundary");
    let (peer, owned) = UnixStream::pair().expect("socket pair");
    // A COPY THE TEST KEEPS, so that losing the ledger's own copy does not
    // close the connection by itself. What the peer observes below is a
    // shutdown somebody performed, not a file descriptor going out of scope.
    let retained = owned.try_clone().expect("a second handle on the same connection");
    private
        .broker
        .registry
        .input_recovery
        .attach(client, owned)
        .expect("the incumbent's own socket");
    private
        .broker
        .registry
        .control_completion()
        .expect("a private frontend installs one")
        .writer_started(client);

    drop(channels);
    let sent = private.broker.registry.route_control(XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(91090),
            surface: SurfaceId::new(9109, 1),
        },
    });
    assert!(sent.is_err(), "the send found its endpoint gone: {sent:?}");

    // THE ATTEMPT THAT CANNOT HAVE THE NUMBER.
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("its number is not free");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "{refused:?}"
    );
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "and the incumbent's own claim is not marked by somebody else's attempt"
    );

    // THE WRITER RECORD IS STILL THE INCUMBENT'S OWN. Its writer has started
    // and nothing expects a second one, so when that writer stops there is
    // nothing left to say this client may still execute.
    private
        .broker
        .registry
        .control_completion()
        .expect("a private frontend installs one")
        .writer_stopped(client);
    assert!(
        private
            .broker
            .registry
            .control_completion()
        .expect("a private frontend installs one")
            .enter_routing(client)
            .is_none(),
        "a refused attempt must not leave an expectation nobody will meet"
    );

    // AND THE RECOVERY ENTRY IS STILL THE INCUMBENT'S OWN, so its ending
    // reaches the socket it attached.
    peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("a bounded wait");
    drop(registration);
    let mut byte = [0u8; 1];
    let read = std::io::Read::read(&mut &peer, &mut byte);
    assert!(
        matches!(read, Ok(0)),
        "the incumbent's ending still reached its own socket: {read:?}"
    );
    drop((retained, peer, private, keeper, durable));
}

#[test]
fn a_successor_admitted_after_the_release_has_only_its_own_state() {
    // WHAT THE NUMBER AUTHORISED, AND WHAT IT DID NOT. A connection's ending
    // removes its recovery entry, its expected writer, its row, its surfaces,
    // its selection and presentation subscriptions and its pending presents --
    // all keyed by the number. The exclusion exists so that every one of those
    // reaches only the connection that held it.
    //
    // THIS IS THE AFTER PICTURE. The predecessor's ending established that the
    // number may be reused; the successor then builds each of those kinds for
    // itself, and each is observably its own rather than something inherited.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let registry = &private.broker.registry;
    let client = XServerFrontendClientId(9111);
    let namespace = admitted(client).namespace.id;
    let window = XResourceId::new(0x9111, 1);
    let selection = 0x9111;
    let event_id = XResourceId::new(0x9111, 7);

    let (registration, channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the predecessor is admitted to the boundary");
    registry
        .register_surface(client, namespace, SurfaceId::new(9111, 1), window)
        .expect("its own surface");
    registry
        .select_xfixes_selection_input(client, namespace, window, selection, 0xf)
        .expect("its own selection interest");
    registry
        .select_present_input(client, event_id, window, 0xf)
        .expect("its own presentation interest");
    registry
        .queue_present(
            TransactionId::from_raw(91110),
            client,
            window,
            XResourceId::new(0x9111, 2),
            1,
            None,
            false,
        )
        .expect("its own pending present");

    // ITS ENDING TAKES ALL OF IT, and only then is the number free.
    drop(channels);
    drop(registration);
    assert_eq!(
        registry.occupancy.state_of(client),
        None,
        "its ending established that the number may be reused"
    );
    assert!(
        registry
            .xfixes_selection_subscribers(namespace, selection, 0)
            .expect("a readable table")
            .is_empty(),
        "the predecessor's selection interest went with it"
    );
    assert!(
        registry
            .present_configure_subscribers(window)
            .expect("a readable table")
            .is_empty(),
        "and its presentation interest"
    );
    assert!(
        !registry
            .route_present_idle(TransactionId::from_raw(91110))
            .expect("a readable table"),
        "and its pending present"
    );

    // THE BOUNDARY'S OWN RETIREMENT, WHICH IS NOT THIS COMPONENT'S TO
    // PERFORM. A closed gate is a request; the lifecycle drive is what
    // completes it. Until it runs the boundary still holds the predecessor's
    // admission, and a successor at this number is refused there rather than
    // here -- which is the boundary's own exclusion, not this one.
    let lifecycle = registry
        .input_recovery
        .lifecycle
        .get()
        .expect("a private frontend installs one");
    let retired = lifecycle
        .drive(NonZeroUsize::new(4).expect("a budget"))
        .expect("a reachable boundary");
    assert!(retired >= 1, "the predecessor's slot is retired: {retired}");

    // THE SUCCESSOR, WITH THE SAME NUMBER AND NOTHING ELSE OF ITS
    // PREDECESSOR'S.
    let (successor, successor_channels) = registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("the number is free");
    registry
        .attach_private_lifecycle(&successor, admitted(client))
        .expect("the successor is admitted on its own account");
    let (peer, owned) = UnixStream::pair().expect("socket pair");
    let retained = owned.try_clone().expect("a second handle on the same connection");
    registry
        .input_recovery
        .attach(client, owned)
        .expect("a recovery entry of its own");
    registry
        .control_completion()
        .expect("a private frontend installs one")
        .writer_started(client);
    registry
        .register_surface(client, namespace, SurfaceId::new(9111, 3), window)
        .expect("a surface of its own");
    registry
        .select_xfixes_selection_input(client, namespace, window, selection, 0xf)
        .expect("a selection interest of its own");
    registry
        .select_present_input(client, event_id, window, 0xf)
        .expect("a presentation interest of its own");
    registry
        .queue_present(
            TransactionId::from_raw(91111),
            client,
            window,
            XResourceId::new(0x9111, 4),
            2,
            None,
            false,
        )
        .expect("a pending present of its own");

    // AND A LIVE QUEUE OF ITS OWN, holding the exact delivery it was given:
    // the same identity, the same encoded frames, and a completion nobody has
    // answered. The predecessor's ending settles unanswered deliveries under
    // its number, and this one is untouched.
    let (capsule, _endpoint, capsule_recovery, _receipts) = answerable_capsule(91112);
    let delivery = capsule.delivery();
    let cell = Arc::clone(
        &capsule
            .finalizer()
            .expect("an answerable capsule")
            .completion,
    );
    let frames = order_pass_frames(&capsule);
    let sender = registry
        .clients
        .lock()
        .expect("a readable registry")
        .get(&client)
        .expect("the successor's own row")
        .ordered
        .clone();
    gated_send(&sender, capsule).expect("the successor's own endpoint");

    assert_eq!(
        registry
            .xfixes_selection_subscribers(namespace, selection, 0)
            .expect("a readable table"),
        vec![(client, window)],
        "the successor's own selection interest"
    );
    assert_eq!(
        registry
            .present_configure_subscribers(window)
            .expect("a readable table"),
        vec![(client, event_id)],
        "the successor's own presentation interest"
    );
    assert!(
        registry
            .route_present_idle(TransactionId::from_raw(91111))
            .expect("a readable table"),
        "the successor's own pending present"
    );
    let queued = successor_channels
        .ordered
        .try_recv()
        .expect("the successor's own queue holds it");
    assert_eq!(queued.delivery(), delivery);
    assert!(
        Arc::ptr_eq(
            &cell,
            &queued.finalizer().expect("an answerable capsule").completion
        ),
        "the exact delivery it was given, not a rebuilt one"
    );
    assert_eq!(order_pass_frames(&queued), frames);
    assert!(
        queued
            .finalizer()
            .expect("an answerable capsule")
            .completion
            .answer()
            .is_none(),
        "and nothing has answered it"
    );

    // ITS OWN RECOVERY ENTRY, reached by its own ending and nobody else's.
    peer.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("a bounded wait");
    drop((queued, successor_channels));
    drop(successor);
    let mut byte = [0u8; 1];
    let read = std::io::Read::read(&mut &peer, &mut byte);
    assert!(
        matches!(read, Ok(0)),
        "the successor's own ending reached its own socket: {read:?}"
    );
    assert_eq!(registry.occupancy.state_of(client), None);
    drop((retained, peer, capsule_recovery, private, keeper, durable));
}

#[test]
fn two_attempts_at_one_number_cannot_both_have_it() {
    // TAKEN IN ONE ACQUISITION, so there is no interval in which two attempts
    // have both looked and neither has taken. Whichever refusal the loser gets
    // -- the table's or the number's -- exactly one attempt may proceed.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 8);
    let private = private_over(&keeper, 8);
    let registry = private.broker.registry.clone();
    let client = XServerFrontendClientId(9112);

    let (first, second) = std::thread::scope(|scope| {
        let contender = registry.clone();
        let racing = scope
            .spawn(move || contender.register_client_with_admission(client, Some(admitted(client))));
        let here = registry.register_client_with_admission(client, Some(admitted(client)));
        (here, racing.join().expect("the contender finished"))
    });

    let admitted_count = usize::from(first.is_ok()) + usize::from(second.is_ok());
    assert_eq!(
        admitted_count, 1,
        "exactly one attempt has the number: {:?} / {:?}",
        first.as_ref().err(),
        second.as_ref().err()
    );
    let refusal = first.as_ref().err().or_else(|| second.as_ref().err());
    assert!(
        matches!(
            refusal,
            Some(
                XServerFrontendRouteError::ClientNumberExcluded { client: same }
                    | XServerFrontendRouteError::DuplicateClient { client: same }
            ) if *same == client
        ),
        "the loser is refused for the number it could not have: {refusal:?}"
    );
    assert_eq!(
        registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "and the winner's claim is the one that is held"
    );

    // AND THE WINNER IS A WHOLE CONNECTION, not one that the loser's refusal
    // took anything from.
    let accepted = registry.route_control(XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(91120),
            surface: SurfaceId::new(9112, 1),
        },
    });
    assert!(accepted.is_ok(), "the winner routes: {accepted:?}");
    drop((first, second));
    assert_eq!(
        registry.occupancy.state_of(client),
        None,
        "and its own ending gives the number back"
    );
    drop((private, keeper, durable));
}

#[test]
fn an_operation_view_ending_does_not_give_the_number_back() {
    // THE RIGHT IS KEPT WITH THE RESPONSIBILITY. Views of a connection come
    // and go -- a custody pin, a maintenance name, a lease on the keeper --
    // and none of them is what ends the connection. A number given back when
    // one of those ended would be free while the effects keyed by it had not
    // run at all.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let client = XServerFrontendClientId(9113);
    let (registration, channels) = private
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
    drop(pin);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "a custody view ending is not this connection ending"
    );

    let name = registration.maintenance_identity().expect("a name");
    drop(name);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "nor a maintenance name going out of scope"
    );

    drop(channels);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "nor its endpoints going"
    );

    // ONLY THE RESPONSIBILITY ENDING DOES.
    drop(registration);
    assert_eq!(private.broker.registry.occupancy.state_of(client), None);
    drop((private, keeper, durable));
}

#[test]
fn a_refused_attempt_at_an_excluded_number_consumes_no_place() {
    // A REFUSAL IS NOT AN ADMISSION, AND IT IS NOT AN OCCUPANCY EITHER. The
    // attempt reserves a place before it can find out that the number is
    // excluded, so a place kept on that path would make each refusal shrink
    // the service by one connection.
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 3);
    let private = private_over(&keeper, 3);
    let registry = &private.broker.registry;
    let first = XServerFrontendClientId(9114);
    let (held, channels) = registry
        .register_client_with_admission(first, Some(admitted(first)))
        .expect("the first of three");
    let second = XServerFrontendClientId(9115);
    let (_second, _second_channels) = registry
        .register_client_with_admission(second, Some(admitted(second)))
        .expect("the second of three");

    // Its row goes through a real routing failure, so the attempt below has to
    // reach the number itself.
    drop(channels);
    let sent = registry.route_control(XAuthorityClientControlCommand {
        client: first,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(91140),
            surface: SurfaceId::new(9114, 1),
        },
    });
    assert!(sent.is_err(), "the send found its endpoint gone: {sent:?}");

    // FOUR REFUSALS. A place lost on each would leave none for the third
    // connection below.
    for attempt in 0..4 {
        let refused = registry
            .register_client_with_admission(first, Some(admitted(first)))
            .err()
            .unwrap_or_else(|| panic!("attempt {attempt} cannot have an excluded number"));
        assert!(
            matches!(
                refused,
                XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == first
            ),
            "{refused:?}"
        );
    }

    // THE THIRD PLACE IS STILL THERE.
    let third = XServerFrontendClientId(9116);
    let (_third, _third_channels) = registry
        .register_client_with_admission(third, Some(admitted(third)))
        .expect("the third of three");

    // AND THE ACCOUNTING IS EXACT, not merely non-empty: a fourth is refused
    // for the capacity it does not have, and says so in those terms. Which
    // capacity runs out first is this service's own arrangement -- here the
    // publication home's -- and the point is that it runs out at the fourth
    // and not at the third.
    let fourth = XServerFrontendClientId(9117);
    let saturated = registry
        .register_client_with_admission(fourth, Some(admitted(fourth)))
        .err()
        .expect("three is three");
    assert!(
        matches!(
            saturated,
            XServerFrontendRouteError::ContinuationUnavailable { client: same }
                if same == fourth
        ),
        "{saturated:?}"
    );
    assert_eq!(
        registry.occupancy.state_of(fourth),
        None,
        "a saturated attempt leaves no claim behind either"
    );
    drop(held);
    drop((private, keeper, durable));
}

/// An ending that could not perform one of its number-keyed effects.
///
/// `poison` takes the one lock that effect needs, under an ordinary caught
/// panic, and rewrites nothing. The registration then ends while its keeper
/// still holds its record. What is asserted is the number's standing
/// afterwards and what a successor is told -- not that the effect ran.
fn an_ending_that_could_not(
    client: XServerFrontendClientId,
    poison: impl FnOnce(&XServerFrontendRouteRegistry),
) {
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, 4);
    let private = private_over(&keeper, 4);
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a record and a row");
    let poisoning = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        poison(&private.broker.registry);
    }));
    assert!(poisoning.is_err(), "the poisoning panic is caught here");

    drop(registration);
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Unestablished),
        "its ending could not establish that the number may be reused"
    );
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("an unestablished ending keeps its number");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "{refused:?}"
    );
    drop((private, keeper, durable));
}
