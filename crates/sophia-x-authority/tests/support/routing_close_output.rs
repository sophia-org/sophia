// Closing the output: the socket ended without the lock it may be stalled
// under, and a stop seen before custody is taken.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_close_ends_the_socket_without_the_output_lock_it_may_be_stalled_under() {
    // (e) Ending a connection must not need the mutex a stalled write holds --
    // that is exactly when ending it is what is needed. The lock is held here
    // for the whole close.
    let client = XServerFrontendClientId(7741);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77410, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77410);
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, output) = serving_owner_for(&mut f, socket);
    let held = output.lock().expect("the connection's own output");
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (1, 0, 0),
        "and what it held was still offered an outcome: {steps:?}"
    );
    assert!(owner.retained_unanswered().is_empty());
    assert!(cell.answer().is_some());
    peer.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a deadline on the peer");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "the peer sees it ended while the output lock was never released"
    );
    drop(held);
}

#[test]
fn a_close_under_a_held_claim_transfers_a_deferral_rather_than_an_answer() {
    // (c) A deferral is the authority taking reporting responsibility under a
    // claim it holds. Counting it as an answer would tell a caller the
    // admission was settled when what actually happened is that someone else
    // now owes the report.
    let client = XServerFrontendClientId(7761);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77610, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77610);
    let recovery = private.broker.registry.input_recovery.clone();
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    // A real execution claim on this exact delivery, taken the ordinary way.
    let delivery = XAuthorityInputDeliveryId::from_raw(77610);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed,
        "the claim this control needs is the ledger's own"
    );

    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    let closing = owner.closing().expect("a close in progress");
    assert!(
        owner.retained_unanswered().is_empty(),
        "the offer was taken, in one form or another: {steps:?}"
    );
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 1),
        "and it was taken as a deferral, counted as itself"
    );
    assert!(
        cell.answer().is_none(),
        "a deferral is not a published answer: the claim still owes the report"
    );
}

#[test]
fn a_close_does_not_answer_a_capsule_belonging_to_another_endpoint() {
    // (d) This socket ending says nothing about another endpoint's recipient,
    // so a refused capsule is carried out still held rather than answered.
    let client = XServerFrontendClientId(7751);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77510, 272, true);

    let other = XServerFrontendClientId(7752);
    let other_window = XResourceId::new(0x307752, 1);
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
    attempt_run(&mut f, 77512, 273, true);

    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let foreign_cell = admitted_cell(private, 77512);
    let foreign = {
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| record.reached.client() == other)
            .expect("the other connection's own press");
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        capsule
    };
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();

    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    // STILL QUEUED, NEVER SERVED. The close path has to ask the same admission
    // question the serving path does: a capsule nobody classified is not this
    // connection's to answer for just because it is on its queue.
    gated_send(&sender, foreign).expect("onto this owner's queue");

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        steps.contains(&X11OrderedCloseStep::Foreign),
        "the close classified it rather than offering for it: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 0),
        "nothing of another endpoint's was answered by this close"
    );
    let [held] = owner.retained_foreign() else {
        panic!("the foreign capsule is retained by the close")
    };
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(77512)
    );
    assert_eq!(
        held.cause(),
        Some(X11OrderedAdmissionRefusal::ForeignEndpoint),
        "and still says why it was never this connection's to write"
    );
    assert!(
        foreign_cell.answer().is_none(),
        "closing this socket is not evidence about another endpoint's recipient"
    );
    drop(other_registration);
}

#[test]
fn a_close_retains_the_exact_capsule_when_the_authority_cannot_answer() {
    // The ordinary Refused branch: the authority takes nothing, so the close
    // consumes nothing. The capsule stays whole -- same completion, same
    // finalizer, same encoded bytes -- and its cell stays unanswered.
    let client = XServerFrontendClientId(7781);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77810);
    let recovery = private.broker.registry.input_recovery.clone();
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .unwrap()
        .ordered
        .clone();
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    assert!(cell.answer().is_none());

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let capsule = owner.queue.try_recv().expect("the actual queued source press");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(77810)
    );
    assert!(Arc::ptr_eq(&cell, &capsule.finalizer().unwrap().completion));
    assert_eq!(Arc::strong_count(capsule.finalizer().unwrap()), 1);
    let original_finalizer = Arc::downgrade(capsule.finalizer().unwrap());
    let original_frames: Vec<Vec<u8>> = (0..capsule.emission().frame_count())
        .map(|index| {
            capsule
                .emission()
                .encode_frame(index, XByteOrder::LittleEndian, 7)
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    assert!(!original_frames.is_empty());
    gated_send(&sender, capsule).unwrap();

    // The real mutex, poisoned without touching any ledger entry or outcome.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = recovery.state.lock().unwrap();
            panic!("intentional recovery-lock poison for returned-refusal fixture");
        }))
        .is_err()
    );
    assert!(recovery.state.is_poisoned());

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        steps.contains(&X11OrderedCloseStep::Adjudicated(
            PrivateAdjudication::Refused
        )),
        "the authority took nothing: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 0)
    );
    assert!(owner.retained_foreign().is_empty());
    let [held] = owner.retained_unanswered() else {
        panic!("the exact capsule is retained")
    };
    // The whole send state is kept, not the capsule alone: how far its bytes
    // got is part of what is still unknown about it.
    assert_eq!(held.frame_index(), 0);
    assert_eq!(held.blocked(), Duration::ZERO);
    let retained = held.delivery();
    assert_eq!(
        retained.delivery(),
        XAuthorityInputDeliveryId::from_raw(77810)
    );
    assert!(Arc::ptr_eq(&cell, &retained.finalizer().unwrap().completion));
    assert!(Arc::ptr_eq(
        &original_finalizer.upgrade().unwrap(),
        retained.finalizer().unwrap()
    ));
    let retained_frames: Vec<Vec<u8>> = (0..retained.emission().frame_count())
        .map(|index| {
            retained
                .emission()
                .encode_frame(index, XByteOrder::LittleEndian, 7)
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    assert_eq!(retained_frames, original_frames);
    assert!(cell.answer().is_none());
    let mut byte = [0u8; 1];
    assert_eq!((&peer).read(&mut byte).unwrap(), 0);
}

#[test]
fn a_close_offers_the_end_of_a_connection_whatever_caused_it() {
    // A caller used to name the terminal outcome, so a close could record a
    // flush for bytes that were never written through a real finalizer. It
    // passes a cause now, and the cause is diagnostic: whichever one it is,
    // what the recipient is told is that the connection ended.
    for cause in [
        X11OrderedCloseCause::ConnectionEnded,
        X11OrderedCloseCause::PreparationFailed,
        X11OrderedCloseCause::SupervisorStopped,
    ] {
        let client = XServerFrontendClientId(7791);
        let mut f = prepared_ordered_fixture(client);
        attempt_run(&mut f, 77910, 272, true);
        let private = f.runner.frontend.as_mut().unwrap();
        let cell = admitted_cell(private, 77910);
        for _ in 0..8 {
            private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
        }
        let (socket, _peer) = UnixStream::pair().unwrap();
        let (mut owner, _output) = serving_owner_for(&mut f, socket);
        let _ = close_to_quiet(&mut owner, cause);
        assert_eq!(
            cell.answer().map(|answer| answer.outcome),
            Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
            "no cause names a flush, a timeout or a write failure: {cause:?}"
        );
        assert_eq!(
            owner.closing().expect("closing").cause,
            cause,
            "the cause is kept for whoever reads it, and kept out of the answer"
        );
    }
}

#[test]
fn a_quiet_close_keeps_its_receiver_until_the_producers_are_actually_gone() {
    // An empty queue is not a stopped producer. While a sender for it is held
    // anywhere, a later capsule can still arrive, so a close that treated
    // Empty as the end would drop the receiver and lose whatever came next.
    let client = XServerFrontendClientId(7801);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78010, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78010);
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        steps.last(),
        Some(&X11OrderedCloseStep::Quiet),
        "a held sender means quiet, not finished: {steps:?}"
    );
    assert!(cell.answer().is_some(), "and what was queued was answered");

    // The receiver was kept, so something arriving afterwards is still this
    // close's to account for rather than something it threw away.
    let late = {
        let private = f.runner.frontend.as_mut().unwrap();
        let recovery = private.broker.registry.input_recovery.clone();
        attempt_run(&mut f, 78012, 273, true);
        let private = f.runner.frontend.as_mut().unwrap();
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| {
                record
                    .custody
                    .completion
                    .as_ref()
                    .is_some_and(|held| held.answer().is_none())
            })
            .expect("the later press");
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        capsule
    };
    let late_cell = late.finalizer().expect("carried").completion.clone();
    gated_send(&sender, late).expect("the retained receiver still has a queue");
    assert!(matches!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Adjudicated(_)
    ));
    assert!(
        late_cell.answer().is_some(),
        "a capsule that arrived after the close began is still accounted for"
    );

    // Only the producers actually going reports the end.
    drop(sender);
    f.runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .remove(&client);
    assert_eq!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Drained,
        "with every sender gone, nothing further can arrive"
    );
}

#[test]
fn an_unusable_output_is_not_reported_as_an_empty_queue() {
    // Idle says there was nothing to do. An output this connection cannot take
    // says the opposite: something is owed and cannot be done. A caller told
    // the first would stop asking.
    let client = XServerFrontendClientId(7811);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78110, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78110);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, output) = serving_owner_for(&mut f, socket);

    // The real output mutex, poisoned without touching the socket or anything
    // this connection owes.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = output.lock().unwrap();
            panic!("intentional output-lock poison for transport fixture");
        }))
        .is_err()
    );
    assert!(output.is_poisoned());

    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::TransportUnavailable
        ),
        "an unusable transport is its own answer"
    );
    assert!(
        owner.in_flight().is_none() && owner.refused().is_none(),
        "and nothing was received, written or disposed of"
    );
    assert!(
        cell.answer().is_none(),
        "a transport that could not be taken answers for nobody"
    );
}

#[test]
fn a_started_close_cannot_be_served_normally_again() {
    // Serving after a close has begun consumed the queued event and wrote at a
    // socket the close had already shut down, publishing WriteFailed through
    // the real finalizer -- a failure manufactured by a forbidden write,
    // recorded ahead of the ending the close was establishing. Two owners of
    // one admission is the whole problem.
    let client = XServerFrontendClientId(7821);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78210, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78210);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("a socket pair ends");

    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Closing
        ),
        "ordinary serving is over from the moment a close begins"
    );
    assert!(
        owner.in_flight().is_none(),
        "and it took nothing off the queue"
    );
    assert!(
        cell.answer().is_none(),
        "nothing was published by a write that must not have happened"
    );

    // The close itself is what answers it, and with the outcome a close knows.
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "{steps:?}"
    );
}

#[test]
fn a_close_reports_backpressure_rather_than_retaining_past_its_bound() {
    // Retention has to come from what is being retained. Growing the store
    // while holding custody is not reservation: it is finding out at the worst
    // moment that there was no room.
    let client = XServerFrontendClientId(7831);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78310, 272, true);

    let other = XServerFrontendClientId(7832);
    let other_window = XResourceId::new(0x307832, 1);
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

    // Real foreign capsules, built one at a time by the source, deliberately
    // put on this connection's queue. Same staged wrong-queue boundary as the
    // other foreign controls; not a claim that the producer misroutes.
    let sender = {
        let private = f.runner.frontend.as_ref().unwrap();
        private
            .broker
            .registry
            .clients
            .lock()
            .unwrap()
            .get(&client)
            .expect("this connection's row")
            .ordered
            .clone()
    };
    let mut built = Vec::new();
    for index in 0..9u64 {
        // A distinct button each time: the same one joins the hold that exists
        // rather than beginning another, and this needs separate admissions.
        let delivery = 78320 + index;
        let button = 273 + u32::try_from(index).expect("small");
        f.ingress
            .submit(&f.keeper.lease(), button_to(
                f.surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .unwrap();
        {
            let PrivatePreparedRunner {
                frontend,
                keyboards,
                watch,
                ..
            } = &mut f.runner;
            let private = frontend.as_mut().unwrap();
            private
                .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
                .unwrap();
            let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
                // The source will not accept another press here. However many
                // were built is what this control has to work with.
                break;
            };
            assert!(custody.observe().unwrap().is_some());
        }
        let private = f.runner.frontend.as_mut().unwrap();
        let recovery = private.broker.registry.input_recovery.clone();
        let cell = admitted_cell(private, delivery);
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| {
                record
                    .custody
                    .completion
                    .as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, &cell))
            })
            .expect("this admission's own hold");
        assert_eq!(record.reached.client(), other, "owed to the other endpoint");
        let emission = record
            .native
            .as_mut()
            .unwrap()
            .take_press_emission()
            .expect("its own press emission");
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        built.push(capsule);
    }
    let bound = {
        let private = f.runner.frontend.as_ref().unwrap();
        private.broker.registry.per_client_input_capacity.get()
    };
    assert_eq!(
        built.len(),
        bound,
        "enough real foreign capsules to reach the bound exactly"
    );

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    assert_eq!(owner.retention, bound, "retention is the queue's own capacity");
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("a socket pair ends");

    // One at a time, each classified and retained, right up to the bound.
    let mut retained = 0usize;
    for capsule in built {
        gated_send(&sender, capsule).expect("onto this connection's queue");
        assert_eq!(
            owner.advance_close(XByteOrder::LittleEndian, 7),
            X11OrderedCloseStep::Foreign,
            "another endpoint's capsule is classified and retained, never offered"
        );
        retained += 1;
    }
    assert_eq!(retained, bound);
    assert_eq!(owner.retained_foreign().len(), bound);

    // AT THE BOUND, NOT PAST IT. The next visit reports backpressure without
    // receiving anything, and the store is still exactly what was reserved.
    assert_eq!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Backpressure,
        "retention at its bound is backpressure, not a bigger store"
    );
    assert_eq!(owner.retained_foreign().len(), bound, "nothing more was taken");
    assert_eq!(
        owner.retention_capacity(),
        (bound, bound),
        "and neither store ever grew to make room"
    );
    drop(other_registration);
}

#[test]
fn an_unterminated_close_publishes_nothing_and_a_real_retry_publishes_once() {
    // A close that could not end its wire recorded the fact and then nothing
    // read it: the driver tested only that a close existed, so it published
    // ClientDisconnected for an admission whose connection was still carrying
    // bytes. And asking again returned success because a close existed,
    // acknowledging an ending nobody had attempted a second time.
    //
    // STAGED AT THE STATE, AND SAID SO. std on this host gives no way to make
    // shutdown fail other than NotConnected, which is an ending; I tried an
    // already-ended handle and it reports success, so a control built that way
    // would silently exercise the happy path while looking like it covered
    // both. The refused termination is written directly instead. The retry
    // below is real: it calls the actual shutdown on the actual socket.
    let client = XServerFrontendClientId(7841);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78410, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78410);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("the first attempt ends this socket");
    // Exactly what a refused shutdown leaves: the close is recorded, serving is
    // excluded, and termination is not a fact.
    {
        let closing = owner.closing.as_mut().expect("closing");
        closing.termination =
            X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied);
        closing.attempts = 1;
    }

    // NOTHING DERIVED FROM AN ENDING THAT DID NOT HAPPEN.
    assert!(
        matches!(
            owner.advance_close(XByteOrder::LittleEndian, 7),
            X11OrderedCloseStep::TerminationUnconfirmed(std::io::ErrorKind::PermissionDenied)
        ),
        "the driver refuses to offer anything"
    );
    assert!(
        matches!(
            owner.adjudicate_in_flight(),
            X11OrderedCloseStep::TerminationUnconfirmed(std::io::ErrorKind::PermissionDenied)
        ),
        "and so does the offer itself, so a later caller cannot go round the driver"
    );
    assert!(
        cell.answer().is_none(),
        "an ending that did not happen answers nobody"
    );
    assert!(
        owner.in_flight().is_none(),
        "and nothing was received under an unconfirmed close"
    );
    // Serving stays excluded throughout.
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::Closing
    ));

    // A REPEATED REQUEST IS A REAL RETRY. It calls the actual shutdown again,
    // keeps the original cause, and succeeds here.
    owner
        .begin_close(X11OrderedCloseCause::SupervisorStopped)
        .expect("the retry ends the wire");
    let closing = owner.closing().expect("closing");
    assert_eq!(
        closing.termination,
        X11OrderedTermination::Established,
        "and only that authorises anything"
    );
    assert_eq!(
        closing.cause,
        X11OrderedCloseCause::ConnectionEnded,
        "the original close's identity is kept, not replaced by the retry's"
    );
    assert_eq!(closing.attempts, 2, "the retry was an attempt, not a lookup");

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "{steps:?}"
    );
    let published = steps
        .iter()
        .filter(|step| {
            matches!(
                step,
                X11OrderedCloseStep::Adjudicated(PrivateAdjudication::Answered)
            )
        })
        .count();
    assert_eq!(published, 1, "exactly one terminal publication: {steps:?}");
}

#[test]
fn a_close_stops_retrying_a_termination_that_keeps_refusing() {
    // The retry is bounded. A close that kept asking forever is one that never
    // finishes, and whoever is waiting for this connection to end waits with
    // it. Staged at the state, as above.
    let client = XServerFrontendClientId(7851);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78510, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78510);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("the first attempt ends this socket");
    {
        let closing = owner.closing.as_mut().expect("closing");
        closing.termination =
            X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied);
        closing.attempts = X11_ORDERED_CLOSE_ATTEMPTS;
    }
    assert_eq!(
        owner.begin_close(X11OrderedCloseCause::ConnectionEnded),
        Err(std::io::ErrorKind::PermissionDenied),
        "past its bound it reports the unresolved termination rather than trying again"
    );
    assert_eq!(
        owner.closing().expect("closing").attempts,
        X11_ORDERED_CLOSE_ATTEMPTS,
        "and does not attempt past the bound"
    );
    assert!(cell.answer().is_none(), "still nothing offered");
}

#[test]
fn a_barred_wire_stops_every_writer_of_that_socket_including_control() {
    // A latch private to one writer fences only that writer. The permission is
    // the connection's, read under the same serialization every post-exposure
    // writer takes, so barring it closes the wire to all of them -- control
    // included, because control is the writer with PRIORITY, not the writer
    // allowed to follow the beginning of an event nobody can finish.
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = AtomicUsize::new(0);
    let sequence = AtomicU16::new(1);

    // Open: every path takes the wire.
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None)
            .expect("a readable output")
            .is_some(),
        "the non-control path writes while the wire is open"
    );
    write_x11_control_records(
        &output,
        &wire,
        XByteOrder::LittleEndian,
        &sequence,
        vec![vec![0u8; 32]],
    )
    .expect("control writes while the wire is open");

    wire.bar();

    // Barred: every path refuses, and refuses for what it is.
    let non_control = lock_x11_non_control_output(&output, &wire, &pending, None)
        .expect_err("the non-control path is barred");
    assert!(
        non_control.client_failure,
        "a wire holding an unfinished event is this client's failure, not the service's"
    );
    let control = write_x11_control_records(
        &output,
        &wire,
        XByteOrder::LittleEndian,
        &sequence,
        vec![vec![0u8; 32]],
    )
    .expect_err("control is barred too");
    assert!(control.client_failure);
    assert!(
        !control.service_shutdown,
        "one connection's unusable wire does not end the service"
    );

    // And control never waits on its own pending counter: with a control
    // registered as pending, the control path still reaches its refusal rather
    // than spinning.
    pending.store(1, Ordering::Release);
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &sequence,
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "control does not yield to control"
    );
}

#[test]
fn a_serving_owner_holds_its_connections_permission_not_one_of_its_own() {
    // What makes barring effective is that the owner holds the CONNECTION'S
    // permission. The trigger -- a shutdown that refuses after a part-written
    // frame -- is the branch this host gives me no honest way to reach, so
    // what this pins is the wiring: bar through the owner's own handle and
    // every writer of that socket is stopped.
    let client = XServerFrontendClientId(7861);
    let f = prepared_ordered_fixture(client);
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport =
        XAuthorityOrderedTransport::bind(&f.registration, f.channels.ordered, &output, &wire, &pending, None)
            .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    assert!(
        Arc::ptr_eq(&owner.wire, &wire),
        "the owner holds the connection's own permission, not a private latch"
    );
    let pending = AtomicUsize::new(0);
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None)
            .expect("a readable output")
            .is_some()
    );

    // Barred through the handle the owner holds.
    owner.wire.bar();
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None).is_err(),
        "barring through the owner stops the other writers of that socket"
    );
    // AND STOPS THE ORDERED STEP ITSELF. Holding the permission and never
    // asking it fenced nothing: the owner went on taking capsules and writing
    // them through the raw lock while every other writer was refused.
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::WireBarred
        ),
        "the ordered step reads the same permission as everyone else"
    );
    assert!(
        owner.in_flight().is_none(),
        "and took nothing while it was barred"
    );
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &AtomicU16::new(1),
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "control included"
    );
}

#[test]
fn an_ordered_step_yields_to_control_and_acts_on_being_stopped() {
    // Taking the raw lock skipped two things every other non-control writer
    // observes: the control-priority yield, and this writer's own stop. A stop
    // nothing acts on makes a join unbounded however carefully it was set.
    let client = XServerFrontendClientId(7871);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78710, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78710);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(f.client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(f.client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // TOLD TO STOP WHILE CONTROL IS PENDING. It yields, sees the stop, and
    // leaves -- taking nothing, writing nothing, answering nothing, and saying
    // so rather than reporting an empty queue.
    pending.store(1, Ordering::Release);
    stop.store(true, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Stopped
        ),
        "a stopped writer says it is leaving, not that there was nothing to do"
    );
    assert!(owner.in_flight().is_none(), "it took nothing");
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and wrote nothing"
    );
    assert!(cell.answer().is_none(), "and answered nobody");

    // WITH NO CONTROL PENDING AT ALL. The shared wait only observes stop while
    // control is pending, so this is the case that saw nothing and served on.
    pending.store(0, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Stopped
        ),
        "stop is this writer's own question, not one control has to raise"
    );
    assert!(owner.in_flight().is_none());
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and still nothing written"
    );
    assert!(cell.answer().is_none());

    // Cleared: ordinary resumption. This proves serving after control has
    // finished and the stop is lifted -- not live waiting, which this control
    // does not test.
    stop.store(false, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed
        ),
        "with no stop and no control pending, its own event is served"
    );
}

#[test]
fn a_stop_set_while_waiting_for_the_output_is_seen_before_taking_custody() {
    // An entry-only check leaves the window between passing it and acquiring
    // serialization. This closes it from the other side: the stop is set while
    // the owner is blocked on the mutex, so it can only be seen by asking
    // again under the guard.
    let client = XServerFrontendClientId(7881);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78810);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = X11ClientOutput::shared(socket, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(f.client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(f.client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // The parent takes the output, so the owner's serving step blocks on it.
    //
    // A COARSE RENDEZVOUS, AND NAMED AS ONE. The thread signals before calling
    // serve_one and the sleep only makes it likely the step is already blocked
    // on the mutex; a descheduled thread could still be at the entry check
    // when the stop is set, in which case this exercises that check instead.
    // Either way the required answer is the same, which is why it is asserted
    // here -- but this does not establish WHICH check saw it.
    let held = output.lock().expect("the connection's own output");
    let serving_stop = stop.clone();
    let (started, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        started.send(()).expect("started");
        let step = owner.serve_one(XByteOrder::LittleEndian, 7);
        (step, owner)
    });
    wait.recv().expect("the serving thread started");
    // It is now either about to take the lock or blocked on it. Setting the
    // stop here can only be observed by a check under the acquired guard.
    std::thread::sleep(Duration::from_millis(50));
    serving_stop.store(true, Ordering::Release);
    drop(held);

    let (step, owner) = server.join().expect("the serving thread finished");
    assert!(
        matches!(step, X11OrderedServeStep::Stopped),
        "a stop set while waiting for output is seen before custody is taken, got {step:?}"
    );
    assert!(owner.in_flight().is_none(), "it took nothing");
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and wrote nothing"
    );
    assert!(cell.answer().is_none(), "and answered nobody");
}
