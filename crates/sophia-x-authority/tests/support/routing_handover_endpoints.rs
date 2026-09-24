// Handover between endpoints: a recipient taking nothing beside one that is
// working, and the reservation a connection keeps to dispose into.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_recipient_taking_nothing_leaves_another_recipient_and_the_runner_working() {
    // SCOPE, stated because the obvious reading is wider than what this
    // establishes. It shows that a writer whose recipient takes nothing
    // returns Blocked with its delivery still owned, and that a second
    // recipient's writer and the service runner both complete their work on a
    // run where that is true. It does NOT establish that the first writer was
    // inside its readiness wait while the others ran -- that interval is not
    // observable from here without a fault probe -- and it does not establish
    // that a supervisor can interrupt a writer that is still waiting.
    let (writer_a, reader_a) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (writer_b, reader_b) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender_a, queue_a) = sync_channel(1);
    let (sender_b, queue_b) = sync_channel(1);
    let (capsule_a, endpoint_a) = capsule_and_endpoint(1901);
    let (capsule_b, endpoint_b) = capsule_and_endpoint(1902);
    let served_a = XAuthorityServedConnection::retained(endpoint_a);
    let served_b = XAuthorityServedConnection::retained(endpoint_b);
    sender_a.send(capsule_a).expect("accepted");
    sender_b.send(capsule_b).expect("accepted");

    let reached = Arc::new(AtomicBool::new(false));
    let signal = reached.clone();
    let stalled = std::thread::spawn(move || {
        let mut filling = X11OrderedSendState {
            blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(200),
            ..X11OrderedSendState::default()
        };
        let filler = vec![0u8; 1 << 16];
        while {
            if filling.frame_complete() {
                filling.retire_frame().expect("it went");
            }
            if filling.frame.is_none() {
                filling.begin_frame(filler.clone()).expect("nothing owed");
            }
            send_pending_frame(&writer_a, &mut filling).is_ok()
        } {}

        let mut in_flight = None;
        let mut refused = None;
        take_ordered_delivery(&queue_a, &served_a, &mut in_flight, &mut refused)
            .expect("one waiting");
        in_flight.as_mut().expect("taken").send.blocked =
            X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(200);
        // Set before the call, and it says only that: this writer is about to
        // attempt a send to a recipient that is taking nothing. It is not a
        // claim about where inside that call the thread is.
        signal.store(true, Ordering::Release);
        let outcome =
            write_one_ordered_frame(&writer_a, &mut in_flight, XByteOrder::LittleEndian, 1);
        (outcome, in_flight, writer_a)
    });

    let limit = std::time::Instant::now() + Duration::from_secs(5);
    while !reached.load(Ordering::Acquire) && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(reached.load(Ordering::Acquire), "A's writer began its attempt");

    // B's recipient reads, so B's delivery goes out whole -- and the bytes are
    // checked off the socket rather than inferred from the step's answer.
    let mut b_flight = None;
    let mut b_refused = None;
    take_ordered_delivery(&queue_b, &served_b, &mut b_flight, &mut b_refused)
        .expect("one waiting");
    let expected = {
        let held = b_flight.as_ref().expect("taken");
        let emission = held.delivery().emission();
        (0..emission.frame_count())
            .map(|index| {
                emission
                    .encode_frame(index, XByteOrder::LittleEndian, 1)
                    .expect("a frame")
                    .as_bytes()
                    .to_vec()
            })
            .collect::<Vec<_>>()
    };
    for _ in 0..expected.len() {
        assert!(matches!(
            write_one_ordered_frame(&writer_b, &mut b_flight, XByteOrder::LittleEndian, 1)
                .expect("B is reading"),
            X11OrderedWriteStep::Advanced { .. }
        ));
    }
    let mut seen = vec![0u8; expected.iter().map(Vec::len).sum()];
    std::io::Read::read_exact(&mut &reader_b, &mut seen).expect("B's bytes");
    assert_eq!(
        seen,
        expected.concat(),
        "B received exactly its delivery's frames, in order"
    );

    // The service runner shares nothing with either socket and completes.
    let client = XServerFrontendClientId(1903);
    let surface = SurfaceId::new(1903, 1);
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
    ingress
        .submit(&keeper.lease(), button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(1903),
            272,
            true,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 1903);
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
        XAuthorityInputDeliveryId::from_raw(1903)
    );
    assert_eq!(capsule.client(), client);

    // A's writer reports its own delivery blocked and still owns it. Whether
    // any of the frame reached the wire depends on how full the socket already
    // was, so what is asserted is ownership and position, not partiality.
    let (outcome, in_flight, writer_a) = stalled.join().expect("A's writer returns");
    assert!(matches!(
        outcome,
        Err(X11OrderedWriteFailure::Send(X11FrameSendFailure::Blocked { .. }))
    ));
    let held = in_flight.as_ref().expect("A still owns its delivery");
    assert!(held.send.frame.is_some(), "with its frame still in hand");
    assert_eq!(held.frame_index(), 0, "and not advanced past");

    // Taking the socket down afterwards does not disturb what is owned. This
    // is shutdown of a writer that has already returned, not interruption of
    // one still waiting.
    writer_a
        .shutdown(std::net::Shutdown::Both)
        .expect("the socket can be ended");
    assert!(in_flight.as_ref().expect("still owned").send.frame.is_some());
    drop(reader_a);
    drop(registration);
    drop(channels);
    drop(durable);
}


#[test]
fn a_request_carries_the_capability_it_was_reserved_under() {
    let client = XServerFrontendClientId(2001);
    let surface = SurfaceId::new(2001, 1);
    let window = XResourceId::new(0x202001, 1);
    let (private, _registration, role, _keyboards, _keeper) =
        ordered_fixture(client, surface, window);
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let reserved = role.reserve(stamp, 1).expect("a reservation");

    // The capability the reservation was issued with, and the one the custody
    // answers with, are the same. A native operation checks that the
    // capability and the permit name one source, and a capability read from
    // the producer at execution time would be whatever it holds then rather
    // than the one this request was accepted against -- which is the whole of
    // what makes that check mean anything.
    let expected = reserved.capability();
    let custody = reserved.accepted();
    assert_eq!(
        custody.capability().source(),
        expected.source(),
        "the custody answers with the capability the request was reserved under"
    );
    let _ = custody.observe();
}

/// One prepared runner with a recipient that has actually selected.
///
/// Built in the order the production path builds it: connection state
/// attached, the source's own window geometry and selections registered,
/// the runner prepared -- which installs the applied registry itself, so
/// nothing here reinstalls it -- and then a real initial focus clear applied
/// through the installed publication. A pointer press resolves against that
/// clear; it admits no key target and is not pretending to.
///
/// Every step unwraps. A setup that cannot reach the prepared state fails the
/// control rather than leaving its body to run against something else.
#[allow(dead_code)]
struct PreparedOrderedFixture {
    runner: PrivatePreparedRunner,
    /// Taken from the runner at construction so a control can borrow the
    /// frontend, the keyboards and the watch disjointly afterwards. The
    /// producer is owned, so holding it costs the runner no borrow.
    ingress: crate::PrivateIngress,
    durable: PrivateSettlementOwner,
    registration: XServerFrontendClientRouteRegistration,
    channels: XServerFrontendClientRouteChannels,
    _acks: Receiver<XAuthorityClientControlAck>,
    deliveries: Receiver<XAuthorityClientInputDelivery>,
    /// The source's own view of this connection's windows, so a control can
    /// change what a fresh resolution would reach.
    selections: Arc<Mutex<XCoreEventSelectionState>>,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: XResourceId,
    namespace: NamespaceId,
    /// The owner that keeps this store and this connection's evidence.
    ///
    /// DECLARED LAST SO IT IS DROPPED LAST. Fields go in declaration order, so
    /// the runner and its frontend end before this does -- which is the
    /// ordering the service has in life: a service exits, and its keeper is
    /// destroyed separately and afterwards.
    keeper: crate::PrivateServiceOwner,
}

fn prepared_ordered_fixture(client: XServerFrontendClientId) -> PreparedOrderedFixture {
    prepared_ordered_fixture_with_store(client, PrivateSettlementOwner::default())
}

fn prepared_ordered_fixture_with_store(
    client: XServerFrontendClientId,
    durable: PrivateSettlementOwner,
) -> PreparedOrderedFixture {
    let namespace = NamespaceId::from_raw(client.raw());
    let surface = SurfaceId::new(u32::try_from(client.raw()).unwrap(), 1);
    let window = XResourceId::new(0x200000 | client.raw(), 1);
    let (ack_sender, acks) = sync_channel(8);
    let (delivery_sender, deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let private = PrivateXServerFrontend::new(
        PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(cause, _)| panic!("construction refused: {cause:?}"));
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client registers");
    // Admitted through the lifecycle rather than beside it: attaching the
    // lifecycle admits too, so a fixture that did both would be refused as
    // already admitted, and one that admits without it leaves anything
    // closing this connection with no gate to close.
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary admits and the lifecycle attaches");
    // The selections the resolver actually reads, and the focus projection,
    // both retained here rather than left to defaults.
    let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(&registration, namespace, selections.clone(), focused.clone())
        .expect("the connection state attaches");
    {
        // The source's own view of this window: where it sits, that it is
        // mapped, and that this recipient selected button press and release.
        // Updating the coarse subscription map instead would leave the
        // resolver reading a selection state nobody had told anything.
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
        .expect("the surface registers");

    let mut runner = private
        .prepare_runner(namespace, &service_keeper)
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
    let ingress = runner
        .ingress_for(&service_keeper.lease(), client, DeviceId::from_raw(1))
        .expect("the runner exposes a producer");

    // A real initial clear through the publication the runner installed,
    // borrowed rather than installed again. Nothing here sets a published flag
    // or seeds the focus atomic as though that were the same thing.
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

    PreparedOrderedFixture {
        keeper: service_keeper,
        runner,
        ingress,
        durable,
        registration,
        channels,
        _acks: acks,
        deliveries,
        selections,
        client,
        surface,
        window,
        namespace,
    }
}

#[test]
fn a_prepared_runner_presses_through_its_real_producer() {
    let client = XServerFrontendClientId(2101);
    let mut fixture = prepared_ordered_fixture(client);
    let lease = fixture.keeper.lease();
    let ingress = fixture
        .runner
        .ingress_for(&lease, client, DeviceId::from_raw(1))
        .expect("the runner exposes a producer");
    ingress
        .submit(&fixture.keeper.lease(), button_to(
            fixture.surface,
            XAuthorityInputDeliveryId::from_raw(2101),
            272,
            true,
        ))
        .expect("the order accepts it");

    // Driven through the runner's own turn, which is what production drives.
    let progress = fixture
        .runner
        .service_turn(&lease)
        .expect("a readable order");
    assert_eq!(progress.taken, 1, "the runner took the submitted work");
    assert_eq!(progress.refused, 0, "and did not refuse it");
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

// Disposable signed-source review controls. No production source changes.
// Each operation uses the prepared source fixture, its actual reservation,
// native guarded execution and completion observation. Terminal dispatch is
// invoked separately to expose the scheduling state without unrelated work.
fn attempt_run(f: &mut PreparedOrderedFixture, id: u64, button: u32, pressed: bool) {
    f.ingress.submit(&f.keeper.lease(), button_to(f.surface, XAuthorityInputDeliveryId::from_raw(id), button, pressed)).unwrap();
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut f.runner;
    let p = frontend.as_mut().unwrap();
    assert!(matches!(p.step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap()).unwrap(), PrivateOrderedStep::Decided(_)));
    // Observe this decided request without invoking the terminal scheduler,
    // so the control can drive and inspect release dispatch separately.
    let Some(PrivateOrderedItem::Ran { run, custody, .. }) = p.terminal.turn.pop() else { panic!("real accepted input must run"); };
    if !pressed {
        assert!(matches!(run.release, Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))), "release outcome {:?}", run.release);
    }
    assert!(custody.observe().unwrap().is_some());
}

fn attempt_release(f: &mut PreparedOrderedFixture, id: u64, button: u32) {
    attempt_run(f, id, button, true);
    attempt_run(f, id + 1, button, false);
}


fn order_pass_frames(c: &XAuthorityOrderedDelivery) -> Vec<Vec<u8>> {
    let emission=c.emission();
    (0..emission.frame_count()).map(|index|emission.encode_frame(index,XByteOrder::LittleEndian,7).unwrap().as_bytes().to_vec()).collect()
}


/// The gated sender for this client, captured the way a producer captures it:
/// under the client table, cloned out of the row, table released.
fn capture_gated_sender(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
) -> PrivateGatedOrderedSender {
    let guard = private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry");
    let sender = guard
        .get(&client)
        .expect("this client has a row")
        .ordered
        .clone();
    drop(guard);
    sender
}

#[test]
fn a_sender_captured_before_a_close_is_refused_after_it() {
    // THE CAPTURE IS THE PROBLEM THE GATE EXISTS FOR. A producer clones the
    // sender under the client table and releases the table before it hands
    // anything over, so removing the row or reading a flag at lookup time
    // cannot stop a handover that already has its sender. This pins that exact
    // interval: captured before, used after.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8201));
    attempt_run(&mut f, 82010, 272, true);
    let captured = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    assert!(
        captured.admit().is_ok(),
        "before the close the captured sender admits"
    );

    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    assert_eq!(f.registration.ordered_handovers_fenced(), Some(true));

    // The same sender, captured before any of that.
    let refusal = captured
        .admit()
        .err()
        .expect("a captured sender is not permission");
    assert_eq!(refusal, PrivateHandoverRefusal::Fenced);

    // And nothing was lost to find that out: the press is still in custody,
    // its phase untouched, its cell unanswered, and no capsule reached the
    // queue.
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 82010);
    let private = f.runner.frontend.as_ref().unwrap();
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(private.terminal.holds[0].native.is_some());
    assert_eq!(
        handover_phase(private, &cell),
        Some(PrivateDispatchPhase::Untaken),
        "nothing has been offered yet, so nothing moved"
    );
    assert!(cell.answer().is_none());
    assert!(f.channels.ordered.try_recv().is_err());
}

#[test]
fn a_fence_waits_for_a_handover_already_admitted() {
    // THE OTHER SIDE OF THE SAME INTERVAL. A handover that got in before the
    // close finishes, and the close cannot report a fence until it has.
    //
    // WHAT THIS ESTABLISHES, exactly: this thread provably holds the admission
    // for the whole window, and the closer's own message -- sent only after
    // close returns -- does not arrive during it. What it does NOT establish
    // is that the closer reached its lock: the 150ms is a timeout, not a
    // rendezvous, and a closer that had not started yet would look the same
    // from here. The message witnesses completion, not arrival.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8211));
    attempt_run(&mut f, 82110, 272, true);
    let captured = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    // A fixture capsule, not one this recipient's own source built. It is here
    // to be a thing that crosses the gate, so what this control says is about
    // the gate and the channel -- not about which capsules a recipient admits,
    // which is the endpoint check's subject and is covered elsewhere.
    let (capsule, _endpoint) = capsule_and_endpoint(82119);
    let admitted = captured.admit().expect("an open endpoint");

    let gate = f.registration.ordered_gate.clone();
    let (done, fenced) = sync_channel(1);
    let closer = std::thread::spawn(move || {
        let outcome = gate.close();
        done.send(outcome).expect("the control is listening");
    });
    assert_eq!(
        fenced.recv_timeout(std::time::Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "a close cannot report a fence while a handover is admitted"
    );

    // The admitted handover completes, under the admission, as production
    // does it.
    admitted.try_send(capsule).expect("an admitted handover");
    drop(admitted);
    let outcome = fenced
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the close completes once the handover is done");
    assert_eq!(outcome, PrivateHandoverFence::Established);
    closer.join().expect("the closing thread");

    // The handover that was already in is on the queue, whole.
    let arrived = f
        .channels
        .ordered
        .try_recv()
        .expect("the admitted handover completed before the fence");
    assert_eq!(
        arrived.delivery(),
        XAuthorityInputDeliveryId::from_raw(82119)
    );
    assert!(f.channels.ordered.try_recv().is_err());
}

#[test]
fn closing_one_endpoint_leaves_a_replacement_registration_untouched() {
    // EXACT. The gate is minted with the queue and bound to the registration
    // that minted it, so a close reaches that registration's handovers and no
    // others. A close that found its gate by client id would fence whoever
    // holds the id now.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8221));
    attempt_run(&mut f, 82210, 272, true);
    let original = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    // ITS GATE IS CAPTURED BEFORE ITS REGISTRATION GOES. The gate is the
    // capability; keeping it is how this control closes the OLD endpoint after
    // that registration has given its number back.
    let original_gate = f.registration.handover_gate();
    let replacement_context = admission_with(f.client, 82219, ROLE_SESSION_GENERATION);
    let spare = spare_registration(&f);
    let original_registration = std::mem::replace(&mut f.registration, spare);
    let (replacement, replacement_channels, _endpoint) = replace_registration(
        f.runner.frontend.as_mut().unwrap(),
        f.client,
        replacement_context,
        namespaced(f.client, f.namespace).client_id,
        original_registration,
        None,
    );

    // The old endpoint closes. The replacement minted its own gate.
    // Closed through the OLD endpoint's own captured gate. Its registration
    // has gone -- which is what freed the number for the replacement -- and
    // the capability it published is what a later closer holds.
    assert_eq!(
        original_gate.close(),
        PrivateHandoverFence::AlreadyEstablished,
        "its own ending closed it; this is that same gate saying so"
    );
    assert_eq!(original_gate.fenced(), Some(true));
    assert_eq!(
        replacement.ordered_handovers_fenced(),
        Some(false),
        "closing an old endpoint must not reach a newer registration"
    );

    let (capsule, _) = capsule_and_endpoint(82219);
    assert_eq!(
        original.admit().err(),
        Some(PrivateHandoverRefusal::Fenced),
        "the closed endpoint's captured sender stays refused"
    );
    let live = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    gated_send(&live, capsule).expect("the replacement's endpoint is open");
    assert_eq!(
        replacement_channels
            .ordered
            .try_recv()
            .expect("onto the replacement's queue")
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(82219)
    );
    assert!(f.channels.ordered.try_recv().is_err());
    drop(replacement);
}

#[test]
fn a_fenced_endpoint_refuses_a_press_handover_and_keeps_everything() {
    // Through the production press path, not a hand-held sender.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8231));
    attempt_run(&mut f, 82310, 272, true);
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 82310);
    let private = f.runner.frontend.as_mut().unwrap();
    let native = private.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize;
    let incarnation = private.terminal.holds[0].incarnation;

    assert_eq!(
        private.dispatch_one_press(),
        Some(false),
        "a fenced endpoint takes no press"
    );

    // NOTHING TAKEN, NOTHING WRITTEN. The capsule was built before the
    // handover was attempted, so Pending is where it belongs -- and NOT
    // Indeterminate, which is the write-ahead mark saying a handover may
    // already have reached the recipient. An endpoint that refused admission
    // never began one, so this capsule is still offerable rather than
    // unrepeatable.
    let phase = handover_phase(private, &cell);
    assert_ne!(
        phase,
        Some(PrivateDispatchPhase::Indeterminate),
        "a refused admission must never mark a handover begun"
    );
    assert_eq!(phase, Some(PrivateDispatchPhase::Pending));
    let owned = matches!(
        private.terminal.holds[0].custody.pending.as_ref(),
        Some(PrivatePendingDelivery::Capsule(capsule))
            if capsule.delivery() == XAuthorityInputDeliveryId::from_raw(82310)
                && Arc::ptr_eq(&cell, &capsule.finalizer().unwrap().completion)
    );
    assert!(owned, "the exact capsule stays inventory-owned");
    assert_eq!(private.terminal.holds[0].incarnation, incarnation);
    assert_eq!(
        private.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize,
        native
    );
    assert!(cell.answer().is_none());
    assert!(f.channels.ordered.try_recv().is_err());
}

#[test]
fn a_fenced_endpoint_refuses_a_release_handover_and_gives_the_attempt_back() {
    // The release path carries an attempt reservation. A refusal before
    // anything is written down leaves it an unused reservation, so it goes
    // back through the confirmed give-back -- and the give-back runs with no
    // gate held, because admission was never obtained.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8241));
    attempt_release(&mut f, 82410, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));
    let press = f.channels.ordered.try_recv().expect("the press went first");
    assert_eq!(press.delivery(), XAuthorityInputDeliveryId::from_raw(82410));

    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 82411);
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(
        private.attempt_one_delivery(),
        Some(false),
        "a fenced endpoint takes no release"
    );
    // The attempt was claimed and given back before anything was written
    // down, so nothing is left holding it: not the record, and not the
    // executor's own attempt custody.
    assert!(
        private.terminal.attempt_custody.is_none(),
        "an attempt that was never begun leaves no custody behind"
    );
    let phase = handover_phase(private, &cell);
    assert_ne!(
        phase,
        Some(PrivateDispatchPhase::Indeterminate),
        "a refused admission must never mark a handover begun"
    );
    assert_eq!(phase, Some(PrivateDispatchPhase::Pending));
    assert!(
        private.terminal.settling[0].custody.attempt.is_none(),
        "the record never named an attempt it did not begin"
    );
    assert!(cell.answer().is_none());
    assert!(f.channels.ordered.try_recv().is_err());
}

#[test]
fn a_gate_that_cannot_be_read_is_retained_failure_and_never_a_fence() {
    // UNREADABLE IS NOT FENCED. A close that reported one as the other would
    // name a fence that was never established, and a caller would go on to end
    // a socket under handovers that may still be running. A producer refuses
    // for the same reason: an unestablished answer is not permission.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8251));
    attempt_run(&mut f, 82510, 272, true);
    let gate = f.registration.ordered_gate.clone();
    let poisoner = std::thread::spawn(move || {
        let _admitted = gate.fenced.lock().expect("an open gate");
        panic!("poison this gate while a handover holds it");
    });
    assert!(poisoner.join().is_err(), "the gate's holder panicked");

    assert_eq!(
        f.registration.ordered_handovers_fenced(),
        None,
        "an unreadable gate is neither open nor closed"
    );
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Unreadable,
        "nothing was established, so no fence may be reported"
    );
    let captured = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    assert_eq!(
        captured.admit().err(),
        Some(PrivateHandoverRefusal::Unreadable)
    );

    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 82510);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(false));
    let phase = handover_phase(private, &cell);
    assert_ne!(
        phase,
        Some(PrivateDispatchPhase::Indeterminate),
        "an unreadable gate is not permission, and refusing on it begins nothing"
    );
    assert_eq!(phase, Some(PrivateDispatchPhase::Pending));
    assert!(cell.answer().is_none());
    assert!(f.channels.ordered.try_recv().is_err());
}


#[test]
fn a_full_refusal_then_a_close_retains_once_and_gives_the_attempt_back_once() {
    // TWO REFUSALS, ONE ACCOUNTING. A queue that was Full gave the capsule
    // back and the attempt with it; a close afterwards refuses admission and
    // must not give the same attempt back a second time, nor take the capsule,
    // nor replay it. Full and fenced are different facts about the same
    // delivery and neither may be read as the other.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8261));
    // One complete pair of this recipient's own deliveries takes two of its
    // four slots, through the ordinary press-then-release flow.
    attempt_release(&mut f, 82610, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));
    assert_eq!(private.attempt_one_delivery(), Some(true));

    // A second pair: its press takes the third slot, and its release is the
    // one this control is about.
    attempt_release(&mut f, 82614, 274);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));
    assert_eq!(
        private.broker.registry.per_client_input_capacity.get(),
        4,
        "four slots"
    );

    // The fourth slot is filled directly, through the same gate production
    // uses, so the release below meets a genuinely full queue rather than a
    // staged refusal. The filler is a fixture capsule: it is here to occupy a
    // slot, and no claim is made that this recipient would admit it.
    let filler = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    let (fill, _) = capsule_and_endpoint(82699);
    gated_send(&filler, fill).expect("the fourth slot");

    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 82615);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(
        private.attempt_one_delivery(),
        Some(false),
        "the fifth delivery meets a genuinely Full queue"
    );
    let after_full = handover_phase(private, &cell);
    assert_eq!(after_full, Some(PrivateDispatchPhase::Pending));
    assert!(private.terminal.attempt_custody.is_none());
    let index = private
        .terminal
        .settling
        .iter()
        .position(|release| release.completion().is_some_and(|held| Arc::ptr_eq(held, &cell)))
        .expect("the refused release is still here");
    assert!(private.terminal.settling[index].custody.attempt.is_none());
    let frames = match private.terminal.settling[index].custody.pending.as_ref() {
        Some(PrivatePendingDelivery::Capsule(capsule)) => order_pass_frames(capsule),
        _ => panic!("the Full refusal gave the exact capsule back"),
    };

    // Now the endpoint closes, and the same delivery is offered again.
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.attempt_one_delivery(), Some(false));
    assert_eq!(
        handover_phase(private, &cell),
        after_full,
        "a fenced refusal leaves the phase the Full refusal left"
    );
    assert!(private.terminal.attempt_custody.is_none());
    assert!(private.terminal.settling[index].custody.attempt.is_none());
    match private.terminal.settling[index].custody.pending.as_ref() {
        Some(PrivatePendingDelivery::Capsule(capsule)) => {
            assert_eq!(
                capsule.delivery(),
                XAuthorityInputDeliveryId::from_raw(82615)
            );
            assert!(Arc::ptr_eq(&cell, &capsule.finalizer().unwrap().completion));
            assert_eq!(
                order_pass_frames(capsule),
                frames,
                "the bytes the release decided, unchanged by either refusal"
            );
        }
        _ => panic!("the fenced refusal took nothing"),
    }
    assert!(cell.answer().is_none());

    // The four that were admitted before the close are still theirs to read.
    let mut seen = Vec::new();
    while let Ok(capsule) = f.channels.ordered.try_recv() {
        seen.push(capsule.delivery());
    }
    assert_eq!(
        seen,
        [82610, 82611, 82614, 82699].map(XAuthorityInputDeliveryId::from_raw),
        "a close takes nothing back that was already handed over"
    );
}


#[test]
fn a_producer_waiting_on_the_ledger_is_not_holding_the_handover_gate() {
    // THE GATE IS NOT HELD ACROSS A LEDGER ACQUISITION. Every producer for
    // this endpoint passes through the gate, and the attempt path takes common
    // twice -- to claim, and to give back. A gate held across either would put
    // every producer for this connection behind the ledger, and would put the
    // gate underneath a lock other work takes for its own reasons.
    //
    // WHAT THIS ESTABLISHES, exactly: common is held for a window in which the
    // producer provably cannot complete, and throughout that window the gate
    // still answers a different thread. It does NOT establish where in the
    // attempt path the producer is waiting -- with common held it stops at the
    // claim, which is the first acquisition -- so it pins the claim side of
    // that rule and not the give-back side. The give-back runs after the
    // admission is dropped by construction; no control here witnesses it.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8271));
    attempt_release(&mut f, 82710, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    let captured = capture_gated_sender(f.runner.frontend.as_ref().unwrap(), f.client);
    let common = Arc::clone(&f.runner.frontend.as_ref().unwrap().authority().common);

    // Held for the whole window: nothing that needs the ledger can finish.
    let ledger = common.lock().expect("a readable authority");
    let mut frontend = f.runner.frontend.take().expect("this fixture's frontend");
    let (refused, produced) = sync_channel(1);
    let producer = std::thread::spawn(move || {
        let outcome = frontend.attempt_one_delivery();
        refused.send(outcome).expect("the control is listening");
        frontend
    });

    // Probed from a third thread, so a gate that is being held shows up as an
    // answer that does not arrive rather than as a control that never returns.
    let (probed, answers) = sync_channel(64);
    let prober = std::thread::spawn(move || {
        for _ in 0..40 {
            let answer = captured.admit().err();
            if probed.send(answer).is_err() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    for _ in 0..40 {
        assert_eq!(
            answers.recv_timeout(std::time::Duration::from_secs(2)),
            Ok(Some(PrivateHandoverRefusal::Fenced)),
            "the gate must answer while a producer waits on the ledger: a \
             producer holding it across that wait would block every other \
             producer for this endpoint"
        );
    }
    assert_eq!(
        produced.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty),
        "and the producer had not completed, so the window was real"
    );
    prober.join().expect("the probing thread");

    drop(ledger);
    assert_eq!(
        produced
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the producer completes once the ledger is free"),
        Some(false)
    );
    let frontend = producer.join().expect("the producing thread");
    assert!(frontend.terminal.attempt_custody.is_none());
    f.runner.frontend = Some(frontend);
}


/// A retained continuation with an INDEPENDENT HANDLE on its connection.
///
/// A receiver alone can never establish an ending -- it has nothing to end
/// with -- so a control whose subject is what happens after an ending must
/// bind a real transport rather than assert the ending into a record.
fn transport_continuation(
    registration: &XServerFrontendClientRouteRegistration,
    ordered: XAuthorityOrderedReceiver,
) -> PrivateOrderedContinuation {
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport = XAuthorityOrderedTransport::bind(
        registration,
        ordered,
        &output,
        &wire,
        &pending,
        None,
    )
    .unwrap_or_else(|_| panic!("this registration's own receiver"));
    PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Transport(Box::new(transport)),
        refusal: X11OrderedServingRefusal::Unserved,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    }
}

/// What a retained continuation is, read from the place a connection held.
fn retained_setup_kind(
    durable: &PrivateSettlementOwner,
    index: usize,
) -> Option<(&'static str, X11OrderedServingRefusal, usize, bool, bool)> {
    durable.with_ordered_continuation(index, |continuation| match continuation {
        PrivateOrderedContinuation::Setup {
            accepted,
            refusal,
            retained,
            drained,
            ended,
            ..
        } => (
            match accepted {
                PrivateOrderedSetupCustody::Receiver(_) => "receiver",
                PrivateOrderedSetupCustody::Transport(_) => "transport",
            },
            *refusal,
            retained.len(),
            *drained,
            *ended,
        ),
        PrivateOrderedContinuation::Serving { .. } => panic!("no owner is built yet"),
    })
}

#[test]
fn a_connections_ordered_output_is_retained_whole_when_its_registration_ends() {
    // UNTIL NOW THIS QUEUE WAS DROPPED. A registration publishes its ordered
    // sender before anything is bound, so a capsule can be accepted into that
    // queue immediately; ending the connection without moving the queue into
    // the place reserved for it discarded accepted work nobody had answered
    // for.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8301);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    assert_eq!(durable.continuations_reserved(), Some(1));

    // Bound where both halves are owned, exactly as connection setup does it.
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        None,
        "this receiver was minted by this registration, so it binds"
    );

    // A capsule is accepted for this connection before it ends.
    let sender = capture_gated_sender(&private, client);
    // A capsule that carries its own completion, so what survives retention
    // can be checked as a payload and not only as a count. It is a fixture
    // capsule: it is here to be work on this queue, and no claim is made that
    // this recipient would have admitted it.
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83010);
    let frames = order_pass_frames(&capsule);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    gated_send(&sender, capsule).expect("an open endpoint");

    drop(registration);
    let (kind, refusal, retained, drained, ended) =
        retained_setup_kind(&durable, 0).expect("the place this connection held");
    assert_eq!(kind, "transport", "the binding is retained, not just the queue");
    assert_eq!(
        refusal,
        X11OrderedServingRefusal::Unserved,
        "no owner was built, and that is the reason recorded"
    );
    assert_eq!(
        (retained, drained, ended),
        (0, false, false),
        "teardown receives nothing, learns nothing about producers, and ends \
         no wire: each of those is a separate act with its own outcome"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the place is still this connection's, because the work in it is not gone"
    );
    assert_eq!(durable.continuations_retained(), Some(1));

    // THE PAYLOAD, not the counters. A kind and a count would be satisfied by
    // a queue that had been emptied and a record that said the right words, so
    // the exact capsule is taken out of the retained receiver here and checked
    // against the one that went in: same delivery, same encoded frames, same
    // completion cell.
    let survived = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup { accepted, .. } = continuation else {
                panic!("no owner is built yet")
            };
            let PrivateOrderedSetupCustody::Transport(transport) = accepted else {
                panic!("this connection bound")
            };
            transport.ordered.receiver.try_recv().ok()
        })
        .expect("the place this connection held")
        .expect("the capsule accepted before the connection ended");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83010)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none(), "and nothing answered for it");
}

#[test]
fn a_connections_own_reservation_keeps_the_store_it_must_dispose_into() {
    // AN EXPOSED CONNECTION CARRIES ITS OWN GUARANTEE. Its registration holds
    // a place in the store and work can already be accepted for it, so the
    // store has to be there when that place is disposed of. Nothing else makes
    // it so: the constructor takes the store by reference, but a reference
    // parameter does not bind the instance -- or the registrations it hands
    // out -- to the caller's binding, and a caller may drop its own holder
    // while a connection is live. A reservation that did not hold it would
    // find, at teardown, that the place it was promised had gone.
    let capability;
    let client = XServerFrontendClientId(8341);
    let (registration, cell, frames, wire_weak, keeper) = {
        let durable = PrivateSettlementOwner::default();
        capability = durable.settlement_ref();
        let service_keeper = service_owner(&durable, 2);
        let private = private_over(&service_keeper, 2);
        let (registration, channels) = private
            .broker
            .registry
            .register_client_with_admission(client, Some(admitted(client)))
            .expect("a place and a row");
        let (stream, _peer) = UnixStream::pair().expect("a socket pair");
        let output = X11ClientOutput::shared(stream, 0);
        let wire = Arc::new(X11WirePermission::open());
        let pending = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            registration
                .bind_ordered_output(channels.ordered, &output, &wire, &pending)
                .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
            None
        );
        // A capsule is accepted for this connection before anything is
        // dropped. It is a fixture capsule carrying its own completion: it is
        // here to be work on this queue, and no claim is made that this
        // recipient would have admitted it.
        let sender = capture_gated_sender(&private, client);
        let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(83410);
        let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
        let frames = order_pass_frames(&capsule);
        gated_send(&sender, capsule).expect("an open endpoint");
        let wire_weak = Arc::downgrade(&wire);
        (registration, cell, frames, wire_weak, service_keeper)
    };
    // EVERY HOLDER OUTSIDE THE CONNECTION IS NOW GONE -- the caller's own
    // binding and the instance that cloned it -- and the connection is live.
    // The one exception is the service keeper, kept because the private
    // service guarantees it around every connection: a registration destroyed
    // after its keeper can establish nothing about its worker and defers, and
    // this control is about the teardown that runs when nothing was started.
    assert!(
        capability.owner().is_some(),
        "an exposed connection's reservation is a legitimate owner of its store"
    );

    // Read through an upgraded owner, so what teardown does is observed rather
    // than inferred from the connection having ended quietly.
    let kept = capability.owner().expect("held by the live reservation");
    drop(registration);
    assert_eq!(kept.continuations_retained(), Some(1));
    let survived = kept
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup { accepted, .. } = continuation else {
                panic!("no owner is built yet")
            };
            let PrivateOrderedSetupCustody::Transport(transport) = accepted else {
                panic!("this connection bound")
            };
            transport.ordered.receiver.try_recv().ok()
        })
        .expect("the place this connection held")
        .expect("the capsule accepted before anything was dropped");
    assert_eq!(
        survived.delivery(),
        XAuthorityInputDeliveryId::from_raw(83410)
    );
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none(), "and nothing answered for it");
    assert!(
        wire_weak.upgrade().is_some(),
        "the binding is retained with the queue, not merely the capsules"
    );

    // AND THIS IS ONE END OF A LIFETIME, NOT A RING. The connection has gone
    // and disposed of its place; when the last reader and the keeper let go,
    // so does the store, and the binding it retained goes with it.
    drop((survived, cell, kept, keeper));
    assert!(capability.owner().is_none());
    assert!(wire_weak.upgrade().is_none());
}

/// Put a staged continuation in a lease's home and retain its place, which is
/// what a connection's binding and its teardown do between them.
///
/// Both halves are the real ones: the home is the one the reservation made,
/// the binding is the home's own, and the place is accounted for by the
/// lease's own commitment. What is staged is the payload, as it was before.
fn retain_into(
    slot: PrivateOrderedContinuationSlot,
    source: &mut Option<PrivateOrderedContinuation>,
) -> PrivateContinuationCommit {
    let home = slot.home().expect("the place this lease names");
    if let Some(payload) = source.take() {
        assert!(
            matches!(home.bind(payload), PrivateHomeBinding::Bound),
            "a fresh home takes its connection's binding"
        );
    }
    let owed = home.retain();
    slot.commit(owed)
}
