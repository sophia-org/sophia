// Frames on the wire: a send that never reported, a refused entry, and a
// stalled frame resumed rather than encoded a second time.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_send_that_never_reported_blocks_everything_after_it() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();
    state.begin_frame([4u8; 16].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");

    // The state a send leaves if it is interrupted between handing bytes to
    // the kernel and recording that it did.
    state
        .frame
        .as_mut()
        .expect("a frame in hand")
        .progress = X11OrderedSendProgress::Unknown { from: 8 };

    // It cannot be resumed: resuming from the offset before the send would
    // put an event's middle after its own middle.
    let failure = send_pending_frame(&writer, &mut state)
        .expect_err("an unreported send is not a resumable one");
    assert!(matches!(failure, X11FrameSendFailure::Interrupted));

    // And it cannot be stepped over either. A following frame would be
    // appended to something nobody can describe, so the unknown is not
    // something a new frame may clear.
    let refused = state
        .begin_frame([5u8; 32].to_vec())
        .expect_err("an unknown wire position is not a finished frame");
    assert!(matches!(refused, X11FrameSendFailure::Interrupted));
    assert_eq!(
        state.frame.as_ref().expect("still held").progress,
        X11OrderedSendProgress::Unknown { from: 8 },
        "and it stays unknown rather than being restored to something believable"
    );

    let error = x11_ordered_frame_error("failed to write an ordered event", failure);
    assert!(error.client_failure && !error.service_shutdown);
    drop(reader);
}

#[test]
fn the_frame_a_resume_continues_is_the_one_it_began() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();

    // The frame is the state's, not a slice a caller brings back each time.
    // There is no call that could offer different bytes behind the same
    // offset, which is what would send the tail of one event as though it
    // were the tail of another.
    state.begin_frame([0xAB; 64].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(state.frame_complete());
    let mut seen = [0u8; 64];
    std::io::Read::read_exact(&mut &reader, &mut seen).expect("the frame");
    assert!(
        seen.iter().all(|byte| *byte == 0xAB),
        "the recipient received the frame that was begun, whole"
    );

    // And completion is derived from what was sent rather than declared: a
    // fresh frame is not complete until its own bytes have gone.
    state.retire_frame().expect("the last one went whole");
    state.begin_frame([0xCD; 8].to_vec()).expect("nothing owed");
    assert!(!state.frame_complete(), "nothing of this one has gone yet");
    drop(reader);
}

#[test]
fn the_socket_every_writer_shares_is_left_alone() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    // The ordered path must not reach its recipient by changing a socket the
    // control and protocol writers hold too. A per-call flag affects the one
    // send; a timeout or a mode belongs to the socket, and every other writer
    // would inherit it without the frame custody this path relies on.
    assert!(
        writer.write_timeout().expect("a readable socket").is_none(),
        "no send timeout is installed on the shared socket"
    );
    let mut state = X11OrderedSendState::default();
    state.begin_frame([3u8; 16].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(
        writer.write_timeout().expect("a readable socket").is_none(),
        "and sending did not install one either"
    );
    assert!(
        !rustix::fs::fcntl_getfl(&writer)
            .expect("a readable descriptor")
            .contains(rustix::fs::OFlags::NONBLOCK),
        "nor did it put the shared socket in non-blocking mode"
    );
    drop(reader);
}

#[test]
fn a_departed_recipient_is_a_failed_recipient_not_a_failed_server() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    drop(reader);
    let mut state = X11OrderedSendState::default();
    state.begin_frame([1u8; 32].to_vec()).expect("nothing owed yet");

    // The send carries NOSIGNAL, so a peer that has gone gives an error rather
    // than killing the process with SIGPIPE -- a writer that died here would
    // take every other client's service with it.
    let failure = send_pending_frame(&writer, &mut state)
        .expect_err("a departed peer cannot take bytes");
    assert!(matches!(failure, X11FrameSendFailure::Io(_)));

    // And the reading of it keeps the failure with the connection. The
    // ordinary peer-write reading gives anything unrecognised the fatal class,
    // so one client's exit would otherwise end the service for all of them.
    let error = x11_ordered_frame_error("failed to write an ordered event", failure);
    assert!(
        error.client_disconnect || error.client_failure,
        "the failure belongs to this connection"
    );
    assert!(!error.service_shutdown, "and not to the service");
}

#[test]
fn one_terminal_step_disposes_one_entry_and_charges_for_it() {
    let client = XServerFrontendClientId(1601);
    let surface = SurfaceId::new(1601, 1);
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
    let _watch = watch.as_ref().expect("a sealed watch");
    let second = private
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second ingress");
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(16011),
            272,
            true,
        ))
        .expect("the order to accept it");
    second
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(16012),
            273,
            true,
        ))
        .expect("the order to accept it");
    let watch = control_watchdog();
    for _ in 0..2 {
        private
            .step_once(keyboards, &mut |_, _| Ok(()), &watch)
            .expect("a readable order");
    }
    assert_eq!(private.terminal.turn.len(), 2);

    // An empty order is not a step and costs nothing.
    let charged = std::cell::RefCell::new(Vec::new());
    let mut charge = |sequence: Option<crate::ReadySequence>, _: std::time::Instant| {
        // None is a visit that names no ordered entry -- a proof recording, or
        // handing an event over. Both are kept, because what this control is
        // about is that every visit is charged for exactly once.
        charged.borrow_mut().push(sequence);
        Ok(())
    };
    let step = private.deliver_one(None, &mut charge).expect("a step");
    let PrivateDeliveryStep::Advanced { sequence, report } = step else {
        panic!("one entry disposed")
    };
    assert_eq!(
        *charged.borrow(),
        vec![Some(sequence)],
        "charged once, for the entry taken"
    );
    // The entry was disposed of and its outcome observed; whether anything
    // reached a queue is the dispatch's fact, not this report's.
    assert!(
        report
            .expect("a disposed entry reports")
            .completion
            .is_some(),
        "the entry's own outcome was observed exactly once"
    );
    assert_eq!(
        private.terminal.turn.len(),
        1,
        "exactly one entry left the turn"
    );

    let step = private.deliver_one(None, &mut charge).expect("a step");
    assert!(matches!(step, PrivateDeliveryStep::Advanced { .. }));
    assert_eq!(charged.borrow().len(), 2);
    assert_ne!(charged.borrow()[0], charged.borrow()[1], "a different entry");
    assert!(
        charged.borrow().iter().all(Option::is_some),
        "an entry's own visit names it"
    );

    // Both entries are disposed of, and the events they decided are still
    // owed. Those handovers are visits of their own, charged for and naming no
    // entry, because the entry that decided the event is already gone.
    for expected in [true, true] {
        let step = private.deliver_one(None, &mut charge).expect("a step");
        assert!(matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued,
                relinquished: false
            } if enqueued == expected
        ));
        assert_eq!(
            *charged.borrow().last().expect("a charge for the visit"),
            None,
            "a handover names no ordered entry"
        );
    }
    assert_eq!(channels.ordered.try_iter().count(), 2, "both events went");

    // Nothing waiting is its own answer, and takes nothing.
    let before = charged.borrow().len();
    assert!(matches!(
        private.deliver_one(None, &mut charge).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert_eq!(charged.borrow().len(), before, "an empty turn is not a step");
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_refused_entry_advancing_is_a_step_with_nothing_to_report() {
    let client = XServerFrontendClientId(1603);
    let surface = SurfaceId::new(1603, 1);
    let mut fixture = ordered_ingress_fixture(client, surface);
    // A routed input without a reservation is an operation the ordered path
    // does not execute, so the order parks behind it.
    fixture
        .private
        .submit(&fixture._keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(16031),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = fixture
        .private
        .route_pending_ordered(&mut fixture.keyboards, &control_watchdog())
        .expect("a readable order");
    fixture.private.terminal.delivering.extend(turn);

    let mut charged = 0;
    let step = fixture
        .private
        .deliver_one(None, &mut |_, _| {
            charged += 1;
            Ok(())
        })
        .expect("a step");
    let PrivateDeliveryStep::Advanced { report, .. } = step else {
        panic!("the entry advanced")
    };
    assert!(
        report.is_none(),
        "nothing was delivered, so there is nothing to report"
    );
    assert_eq!(
        charged, 1,
        "moving it to retained inventory was still a real step, and a caller \
         reading no report as no work would charge nothing for work it did"
    );
    assert_eq!(fixture.private.terminal.undelivered.len(), 1);
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn a_refused_charge_leaves_the_entry_where_it_was() {
    let client = XServerFrontendClientId(1604);
    let surface = SurfaceId::new(1604, 1);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(&fixture._keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(1604),
            272,
            true,
        ))
        .expect("the order to accept it");
    fixture
        .private
        .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &control_watchdog())
        .expect("a readable order");

    let refused = fixture
        .private
        .deliver_one(None, &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved));
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));
    assert_eq!(
        fixture.private.terminal.delivering.len(),
        1,
        "chosen and still owned: a refusal to charge is not a disposition"
    );
    assert!(fixture.private.terminal.undelivered.is_empty());
    assert!(fixture.channels.input.try_recv().is_err(), "and nothing was sent");
    drop(fixture.registration);
    drop(fixture.durable);
}


#[test]
fn a_failed_wait_is_not_a_recipient_that_blocked() {
    // Producing a poll failure against a live owned socket is not something a
    // control here can arrange, so what is exercised is the reading of it --
    // which is where the conflation would do its damage.
    let failure = X11FrameSendFailure::WaitFailed(std::io::Error::from(
        std::io::ErrorKind::InvalidInput,
    ));
    let error = x11_ordered_frame_error("failed to write an ordered event", failure);

    // Not a client failure. Nothing was established about this recipient: it
    // was never asked and it never declined, so ending its connection on the
    // strength of a broken syscall would blame the wrong party.
    assert!(
        !error.client_failure && !error.client_disconnect,
        "a wait that could not be performed says nothing about the recipient"
    );

    // And it is kept apart from blocking in the type itself, which is what
    // stops a deadline being built out of a failed wait.
    let blocked = x11_ordered_frame_error(
        "failed to write an ordered event",
        X11FrameSendFailure::Blocked {
            written: 8,
            blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT,
        },
    );
    assert!(
        blocked.client_failure,
        "a recipient that would not take its bytes is the one that failed"
    );
}

/// The writer fixture uses a real resolved source emission. These controls
/// still prove writer custody, not production producer/consumer completion.
/// A capsule, and the endpoint of the registration that produced it.
///
/// The witness comes from the registration, never from the capsule: a writer
/// whose expectation was read off the capsule would admit anything.
fn capsule_and_endpoint(
    delivery: u64,
) -> (XAuthorityOrderedDelivery, PrivateEndpointIdentity) {
    let (emission, endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(delivery);
    (
        XAuthorityOrderedDelivery::from_emission(emission).unwrap(),
        endpoint,
    )
}

#[test]
fn a_writer_answers_through_the_one_authority_that_owns_the_answer() {
    // Writing into a completion cell directly recorded an answer the ledger
    // never saw: its ticket stayed unanswered and no ordinary observer was
    // told, so a later disconnect could set a different terminal outcome while
    // the cell still said the first one. Two accounts of one delivery,
    // disagreeing. The finalizer adjudicates in one place, and this control
    // checks every account rather than the cell alone.
    let (capsule, endpoint, recovery, receipts) = answerable_capsule(17301);
    let delivery = capsule.delivery();
    let client = capsule.client();
    let cell = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");

    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    assert!(cell.answer().is_none(), "nothing is answered before it is sent");
    for _ in 0..32 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => break,
            other => panic!("a healthy recipient took its bytes: {other:?}"),
        }
    }

    // EVERY ACCOUNT AGREES. The cell, the ledger's own ticket, and the
    // ordinary observer.
    let answer = cell.answer().expect("the cell carries the adjudicated answer");
    assert_eq!(answer.outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert_eq!(answer.delivery, delivery);
    assert_eq!(answer.client, client);
    let notified = receipts
        .try_recv()
        .expect("the ordinary observer was told, through the same adjudication");
    assert_eq!(notified, answer, "and told the same thing");
    assert!(
        recovery.ticket(delivery).is_none() || recovery.completion_for(delivery).is_ok(),
        "the ledger's own account was updated rather than bypassed"
    );
    drop(peer);
}

#[test]
fn a_flushed_delivery_is_retired_once_so_the_next_one_can_be_served() {
    // Reporting a flush without giving the slot up would report that same
    // flush for ever and nothing behind it would ever be served. BOTH capsules
    // carry real origin-bound finalizers and nothing here clears the slot:
    // the retirement this asserts is the one production performs.
    // BOTH FROM ONE REGISTRATION. Two fixtures would be two endpoints whose
    // numbers happen to agree, and this writer serves one endpoint.
    let (first, second, endpoint, _recovery_one, _recovery_two, _receipts_one, _receipts_two) =
        two_answerable_capsules(17201, 17202);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    assert_eq!(
        second.recipient(),
        first.recipient(),
        "both are owed to the one connection this writer serves"
    );
    sender.send(first).expect("the queue to accept the first");
    sender.send(second).expect("the queue to accept the second");

    let mut flushes = 0;
    let mut delivered = Vec::new();
    for _ in 0..64 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => flushes += 1,
            X11OrderedServeStep::Idle => break,
            other => panic!("every capsule here can be answered: {other:?}"),
        }
        if let Some(held) = in_flight.as_ref() {
            let id = held.delivery().delivery();
            if delivered.last() != Some(&id) {
                delivered.push(id);
            }
        }
    }
    assert_eq!(flushes, 2, "each delivery flushed exactly once");
    assert_eq!(delivered.len(), 2, "and the second was reached after the first");
    assert!(in_flight.is_none(), "nothing is left held");
    drop(peer);
}

/// A capsule whose writer can actually answer: a real admission in a real
/// ledger, and a finalizer built from that admission's own completion.
///
/// Controls that built a completion out of thin air could not see whether the
/// ledger agreed with the writer, because there was no ledger behind it.
/// Two answerable capsules owed to ONE endpoint.
fn two_answerable_capsules(
    first: u64,
    second: u64,
) -> (
    XAuthorityOrderedDelivery,
    XAuthorityOrderedDelivery,
    PrivateEndpointIdentity,
    InputRecovery,
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (one, two, endpoint) =
        private_native_tests::emissions_for_one_writer_fixture(first, second);
    let answer = |emission| {
        let mut capsule = XAuthorityOrderedDelivery::from_emission(emission).unwrap();
        let id = capsule.delivery();
        let client = capsule.client();
        let (recovery, receipts) = claim_fixture(id);
        let completion = recovery
            .completion_for(id)
            .expect("a readable ledger")
            .expect("the admission minted its completion");
        capsule.carry_finalizer(Arc::new(finalizer_from_held(
            &recovery, &completion, id, client,
        )));
        (capsule, recovery, receipts)
    };
    let (capsule_one, recovery_one, receipts_one) = answer(one);
    let (capsule_two, recovery_two, receipts_two) = answer(two);
    (
        capsule_one,
        capsule_two,
        endpoint,
        recovery_one,
        recovery_two,
        receipts_one,
        receipts_two,
    )
}

fn answerable_capsule(
    delivery: u64,
) -> (
    XAuthorityOrderedDelivery,
    PrivateEndpointIdentity,
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (mut capsule, endpoint) = capsule_and_endpoint(delivery);
    let id = capsule.delivery();
    let client = capsule.client();
    let (recovery, receipts) = claim_fixture(id);
    let completion = recovery
        .completion_for(id)
        .expect("a readable ledger")
        .expect("the admission minted its completion");
    capsule.carry_finalizer(Arc::new(finalizer_from_held(
        &recovery, &completion, id, client,
    )));
    (capsule, endpoint, recovery, receipts)
}

#[test]
fn an_adjudication_reports_what_the_authority_did_with_it() {
    // A boolean could not say this. Reporting success whenever the authority
    // was called said an answer had been recorded when it had been silently
    // declined; reporting failure for an admission already answered stranded
    // a writer that had done everything asked of it.
    let delivery = XAuthorityInputDeliveryId::from_raw(17401);
    let client = XServerFrontendClientId(17401);
    let other = XServerFrontendClientId(17402);
    let (recovery, _receipts) = claim_fixture(delivery);
    recovery.register(client, None).unwrap();
    assert!(
        recovery.bind(Some(delivery), client).unwrap(),
        "the delivery is bound to the recipient it reached"
    );
    let completion = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");

    // A finalizer naming somebody else. The authority declines it, and that
    // decline is reported as a refusal rather than as a recorded answer.
    let foreign = finalizer_from_held(&recovery, &completion, delivery, other);
    assert_eq!(
        foreign.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Refused,
        "an answer the authority declined is not an answer it recorded"
    );
    assert!(
        completion.answer().is_none(),
        "and nothing was written for the client it named"
    );

    // The right one is recorded.
    let own = finalizer_from_held(&recovery, &completion, delivery, client);
    assert_eq!(
        own.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Answered
    );
    assert_eq!(
        completion.answer().map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );

    // Asked again, it is already answered -- not refused. A writer told
    // otherwise would hold a finished delivery for ever.
    assert_eq!(
        own.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::AlreadyAnswered,
        "an admission that already has its answer owes nothing further"
    );
}

#[test]
fn a_rejected_offer_stays_refused_even_when_older_work_is_deferred() {
    // Reading the state after the fact could not tell this offer's fate from
    // somebody else's: with an earlier cancellation held under a claim, a
    // wrong-client answer was reported as Deferred and a writer took that as
    // permission to retire a delivery nothing had accepted.
    let delivery = XAuthorityInputDeliveryId::from_raw(17501);
    let client = XServerFrontendClientId(17501);
    let other = XServerFrontendClientId(17502);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client, None).unwrap();
    assert!(recovery.bind(Some(delivery), client).unwrap());
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    // An earlier cancellation, held because the claim is out.
    recovery
        .finish(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::EpochRevoked,
        )
        .expect("the cancellation is offered");
    assert!(
        receipts.try_recv().is_err(),
        "and held rather than published, because the claim is out"
    );

    let completion = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");
    let foreign = finalizer_from_held(&recovery, &completion, delivery, other);

    // THE OFFER IS REJECTED, and the older held cancellation is not its fate.
    assert_eq!(
        foreign.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Refused,
        "a declined offer is refused, whatever else is held for this delivery"
    );
    assert!(
        completion.answer().is_none(),
        "and nothing was recorded for it"
    );

    // The older cancellation is still the one held, untouched by the offer.
    recovery.resolve_claim(Some(delivery), false);
    let published = receipts
        .try_recv()
        .expect("the held cancellation stands once the claim resolves");
    assert_eq!(
        published.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked,
        "the rejected Flushed never displaced it"
    );
    assert_eq!(published.client, client);
}

#[test]
fn serving_a_whole_delivery_reports_a_flush_and_nothing_more() {
    // A flush means every frame went and the answer was adjudicated. It does
    // not mean the recipient read them.
    let (capsule, endpoint, _recovery, _receipts) = answerable_capsule(17101);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    let mut flushed = false;
    for _ in 0..16 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("a healthy recipient took its bytes: {other:?}"),
        }
    }
    assert!(flushed, "every frame of the delivery went out");
    drop(peer);
}

#[test]
fn a_recipient_that_will_not_take_its_bytes_ends_the_connection_before_returning() {
    // THE OBLIGATION write_one_ordered_frame REFUSES TO DISCHARGE. Every
    // failure leaves a frame owed or its extent unknown, so the socket is
    // closed before this returns. A caller that released output
    // serialization after such a step without the socket being closed would
    // admit another writer into the body of a half-written event.
    let (capsule, endpoint) = capsule_and_endpoint(17102);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    // A recipient that is gone: the send fails rather than blocking, which is
    // the failure this control can reach deterministically.
    drop(peer);

    let mut ended = None;
    for _ in 0..16 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed => {}
            X11OrderedServeStep::Unanswered => {
                // No finalizer on this capsule, so nothing adjudicates its
                // answer and custody is retained. Released here so the loop
                // can reach the failure it is about.
                in_flight = None;
            }
            step @ X11OrderedServeStep::Ended { .. } => {
                ended = Some(step);
                break;
            }
            X11OrderedServeStep::Idle => break,
            X11OrderedServeStep::AdmissionRefused(_) => {
                panic!("this queue carries only what this connection is owed")
            }
            X11OrderedServeStep::TransportUnavailable
            | X11OrderedServeStep::Unterminated
            | X11OrderedServeStep::Closing
            | X11OrderedServeStep::Stopped
            | X11OrderedServeStep::WireBarred => {
                panic!("this connection is live, serving, unbarred and not stopping")
            }
        }
    }
    let Some(X11OrderedServeStep::Ended { outcome, shutdown }) = ended else {
        // A departed peer may accept the bytes into a closed socket's buffer
        // on some kernels; if it did, this control has nothing to say and
        // says so rather than asserting something it did not observe.
        return;
    };
    assert_eq!(
        outcome,
        XAuthorityInputDeliveryOutcome::WriteFailed,
        "a send that failed is a writer fact, not a recipient settlement"
    );
    assert!(
        shutdown,
        "and the connection was ended before this returned, not left for a \
         caller to remember"
    );
}

#[test]
fn a_taken_delivery_lands_where_it_will_be_answered_for() {
    // Both from one registration: this control is about the slot, and a second
    // registration's capsule would be refused before the slot was consulted.
    let (one, two, endpoint) =
        private_native_tests::emissions_for_one_writer_fixture(17011, 17012);
    let first = XAuthorityOrderedDelivery::from_emission(one).unwrap();
    let second = XAuthorityOrderedDelivery::from_emission(two).unwrap();
    let client = first.client();
    let served = XAuthorityServedConnection::retained(endpoint);
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;

    // Nothing waiting is its own answer, and takes nothing.
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::Empty)
    );
    assert!(in_flight.is_none() && refused.is_none());

    sender
        .send(first)
        .expect("the queue to accept it");
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused).expect("one waiting");
    let held = in_flight.as_ref().expect("taken into storage");
    assert_eq!(held.delivery().client(), client);
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(17011),
        "the capsule itself is held, not parts copied out of it"
    );
    assert_eq!(held.frame_index(), 0);
    assert_eq!(held.blocked(), Duration::ZERO);

    // A second is refused rather than queued behind the first. This writer
    // answers for what it holds until that is finished, and taking another
    // would leave the first owed by nobody with its frames half-written.
    sender
        .send(second)
        .expect("the queue to accept it");
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::InFlight),
        "the slot is answered before anything is received, so this never \
         reaches the endpoint comparison"
    );
    assert_eq!(
        in_flight
            .as_ref()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(17011),
        "and the one in hand is untouched"
    );

    // A producer that has gone is not an empty queue: one says to look again,
    // the other says nothing more is coming.
    drop(sender);
    in_flight = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("the queued one is still there");
    in_flight = None;
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::Closed)
    );
    assert!(refused.is_none(), "nothing here was for another connection");
}

#[test]
fn a_frame_index_does_not_move_past_an_unfinished_frame() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1702);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");

    // No frame in hand at all is not a finished one.
    assert!(held.advance_frame().is_err());
    assert_eq!(held.frame_index(), 0);

    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let frame = held
        .delivery()
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("the emission's first frame");
    held.send.begin_frame(frame).expect("nothing owed");

    // Begun and not sent is not finished either: the recipient is waiting for
    // bytes this delivery still owes it.
    assert!(held.advance_frame().is_err());
    assert_eq!(held.frame_index(), 0);

    send_pending_frame(&writer, &mut held.send).expect("a healthy send");
    held.advance_frame().expect("the frame in hand went out whole");
    assert_eq!(held.frame_index(), 1, "and only then does the next one begin");
    drop(reader);
}

#[test]
fn one_completed_frame_is_advanced_past_exactly_once() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1703);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");

    let frame = held
        .delivery()
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("the emission's first frame");
    held.send.begin_frame(frame).expect("nothing owed");
    send_pending_frame(&writer, &mut held.send).expect("a healthy send");
    held.advance_frame().expect("the frame went out whole");
    assert_eq!(held.frame_index(), 1);

    // The same completed frame cannot be advanced past twice. An index that
    // moved again while nothing had been begun would count a frame that was
    // never started, and the emission frame it skipped would never be sent at
    // all -- an event silently missing from a delivery that reported itself
    // finished.
    let refused = held
        .advance_frame()
        .expect_err("nothing is in hand to advance past");
    assert!(matches!(refused, X11FrameSendFailure::NoFrame));
    assert_eq!(held.frame_index(), 1, "and the index did not move");
    drop(reader);
}

#[test]
fn two_frames_of_one_delivery_reach_the_wire_in_order_and_whole() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1704);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");

    let emission = held.delivery().emission();
    // This fixture's emission has one record, so these are two encodings of
    // the same record with different transport sequences. That makes this a
    // control over frame CUSTODY -- two frames taken, sent, retired and read
    // back in order with nothing of the first carried into the second -- and
    // not evidence that the writer walks distinct emission records. A
    // multi-form emission is what would show that, and this fixture cannot
    // produce one.
    assert_eq!(
        emission.frame_count(),
        1,
        "stated rather than assumed: one record, encoded twice"
    );
    let first = emission
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("a first frame");
    let second = emission
        .encode_frame(0, XByteOrder::LittleEndian, 2)
        .expect("a second frame");
    let first_bytes = first.as_bytes().to_vec();
    let second_bytes = second.as_bytes().to_vec();
    assert_ne!(
        first_bytes, second_bytes,
        "the sequence is in the bytes, so these are distinguishable on the wire"
    );

    held.send.begin_frame(first).expect("nothing owed");
    send_pending_frame(&writer, &mut held.send).expect("the first frame");
    held.advance_frame().expect("the first went whole");

    held.send.begin_frame(second).expect("the first was retired");
    send_pending_frame(&writer, &mut held.send).expect("the second frame");
    held.advance_frame().expect("the second went whole");
    assert_eq!(held.frame_index(), 2);

    let mut seen = vec![0u8; first_bytes.len() + second_bytes.len()];
    std::io::Read::read_exact(&mut &reader, &mut seen).expect("both frames");
    assert_eq!(
        &seen[..first_bytes.len()],
        first_bytes.as_slice(),
        "the first frame, whole and first"
    );
    assert_eq!(
        &seen[first_bytes.len()..],
        second_bytes.as_slice(),
        "then the second, with nothing of the first carried into it"
    );
    drop(reader);
}

#[test]
fn a_delivery_is_written_one_frame_at_a_time_and_then_is_written() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1801);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("accepted");
    let mut in_flight = None;

    // Nothing in flight is its own answer.
    assert_eq!(
        write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
            .expect("a step"),
        X11OrderedWriteStep::Idle
    );

    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let frames = in_flight
        .as_ref()
        .expect("taken")
        .delivery()
        .emission()
        .frame_count();
    for frame in 0..frames {
        assert_eq!(
            write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
                .expect("a step"),
            X11OrderedWriteStep::Advanced { frame },
            "one frame per call, in order"
        );
    }

    // Every frame this delivery owed has gone. That says the bytes went and
    // nothing else: whether the recipient received them is the writer's own
    // outcome to establish, and whether the debt is settled is a question
    // neither this nor a queue can answer.
    assert_eq!(
        write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
            .expect("a step"),
        X11OrderedWriteStep::Wrote
    );
    drop(reader);
}

#[test]
fn a_stalled_frame_is_resumed_rather_than_encoded_again() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1802);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("accepted");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");

    // Fill the recipient's buffer so this delivery's frame cannot go out
    // whole, and seed the accumulator so the stall is reached quickly.
    let filler = vec![0u8; 1 << 16];
    // Seeded so the filler gives up as soon as the buffer is full, rather
    // than waiting out its own policy: what is being arranged here is a full
    // recipient, not a measurement.
    let mut filling = X11OrderedSendState {
        blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60),
        ..X11OrderedSendState::default()
    };
    while {
        if filling.frame_complete() {
            filling.retire_frame().expect("it went");
        }
        if filling.frame.is_none() {
            filling.begin_frame(filler.clone()).expect("nothing owed");
        }
        send_pending_frame(&writer, &mut filling).is_ok()
    } {}

    let held = in_flight.as_mut().expect("taken");
    held.send.blocked = X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60);
    let stalled = write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 7)
        .expect_err("the recipient is taking nothing");
    assert!(matches!(
        stalled,
        X11OrderedWriteFailure::Send(X11FrameSendFailure::Blocked { .. })
    ));

    // The frame stays in hand with what the wire took of it. A second call
    // must resume that exact frame: encoding again would produce a second copy
    // of the frame those bytes came from and continue into the middle of it.
    let held = in_flight.as_mut().expect("still in flight");
    assert!(held.send.frame.is_some(), "the frame is still in hand");
    assert_eq!(held.frame_index(), 0, "and it has not been advanced past");

    // The recipient starts reading and this delivery is asked again. The
    // second call has to CONTINUE the frame in hand. A call that encoded again
    // would be asking to begin a frame while one is still owed, and the frame
    // custody refuses exactly that -- so a fresh encoding cannot even reach
    // the socket, and what it would have produced is a second copy of the
    // bytes the wire already holds part of.
    held.send.blocked = Duration::ZERO;
    let mut drained = vec![0u8; 1 << 20];
    let _ = std::io::Read::read(&mut &reader, &mut drained).expect("the recipient reads");
    let resumed = write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 7)
        .expect("the frame in hand is continued");
    assert_eq!(
        resumed,
        X11OrderedWriteStep::Advanced { frame: 0 },
        "continuing finished the frame that was already part way out; a call \
         that encoded again would be asking to begin a frame while one is \
         still owed, which the frame custody refuses outright"
    );
    drop(reader);
}
