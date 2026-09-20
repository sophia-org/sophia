// Capsules and the admission they belong to: one refused before a byte is
// written, and a close that adjudicates a queued admission before its payload.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_capsule_for_another_connection_is_refused_before_any_byte_of_it_is_written() {
    // STAGED, AND SAID SO. Nothing in the routing path puts one connection's
    // capsule on another's queue today: dispatch looks the queue up by the
    // recipient the capsule names. This is the check that has to exist before
    // a per-connection loop is attached to that queue, because by the time a
    // frame for the wrong connection is on a wire the recipient has read it.
    //
    // Both capsules are real: two genuinely admitted connections, each
    // pressing through the source and building its own emission.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7641));
    attempt_run(&mut f, 76410, 272, true);

    let other = XServerFrontendClientId(7642);
    let other_window = XResourceId::new(0x307642, 1);
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
    attempt_run(&mut f, 76412, 273, true);

    // Each connection's own capsule, taken out of the record that owns it.
    let p = f.runner.frontend.as_mut().unwrap();
    assert_eq!(p.terminal.holds.len(), 2);
    assert_eq!(p.terminal.holds[0].reached.client(), f.client);
    assert_eq!(p.terminal.holds[1].reached.client(), other);
    let recovery = p.broker.registry.input_recovery.clone();
    let mine = {
        let record = &mut p.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, f.client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the press built its own capsule")
        };
        capsule
    };
    let theirs = {
        let record = &mut p.terminal.holds[1];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the other press built its own capsule")
        };
        capsule
    };
    assert_ne!(
        mine.recipient(),
        theirs.recipient(),
        "two connections, two identities"
    );
    let their_cell = theirs.finalizer().expect("carried").completion.clone();
    let my_delivery = mine.delivery();

    // A writer serving the first connection, and the other's capsule on its
    // queue ahead of its own.
    // The writer's expectation comes from the registration it serves, not from
    // anything it is about to be asked to write.
    let served = XAuthorityServedConnection::retained(
        p.endpoint_for(&f.registration)
            .expect("this connection's own endpoint"),
    );
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let mut in_flight = None;
    let mut refused = None;
    sender.send(theirs).expect("the queue to accept it");
    sender.send(mine).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(_)
    ));

    // NOTHING WENT ON THE WIRE. That is the whole point of checking at
    // admission: a frame is read by the time anyone could regret it.
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "no byte of another connection's event reached this one"
    );
    assert!(in_flight.is_none(), "and it was never taken as work");

    // It is retained rather than dropped -- the queue has already given it up
    // -- and it is not answered here.
    let held = refused.as_ref().expect("the capsule is owned by this writer");
    assert_eq!(
        held.cause(),
        Some(X11OrderedAdmissionRefusal::ForeignEndpoint),
        "and it says exactly what was wrong: not a flush, not a failure to write"
    );
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(76412)
    );
    assert_eq!(held.client(), other);
    assert!(Arc::ptr_eq(
        &their_cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert!(
        their_cell.answer().is_none(),
        "a writer that was never entitled to it does not answer for it"
    );

    // And nothing behind it is served while it is held: taking another would
    // overwrite the one thing that still owns this capsule.
    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(_)
    ));
    assert_eq!(
        refused
            .as_ref()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(76412),
        "the first is still the one held"
    );
    assert!(in_flight.is_none());

    // Disposed of, and this connection's own capsule is admitted normally.
    let _disposed = refused.take().expect("held");
    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed
    ));
    assert!(
        refused.is_none(),
        "its own capsule is not refused"
    );
    assert!(
        in_flight
            .as_ref()
            .is_none_or(|held| held.delivery().delivery() == my_delivery),
        "the connection's own event is what it serves"
    );
    drop(peer);
    drop(other_registration);
}

/// An admission for this client with a chosen admission id and generation.
fn admission_with(
    client: XServerFrontendClientId,
    admission: u64,
    generation: u64,
) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(admission),
        sophia_protocol::NamespaceContext::new(
            NamespaceId::from_raw(client.raw()),
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            generation,
        )
        .unwrap(),
    )
    .unwrap()
}

/// Replace this client's registration and admission, and say what the
/// replacement's exact endpoint is.
///
/// THE REGISTRATION/API SEAM, NOT AN ORDINARY RECONNECT. Nothing a client can
/// drive replaces a live registration: register_client_with_admission refuses a
/// duplicate, and admit refuses an already-bound client. This goes through the
/// registry's and the participant's own replacement path -- revoke, drop the
/// row, admit and register again -- because that is the seam an endpoint
/// identity has to survive. It is not evidence that a reconnect misdelivers.
fn replace_registration(
    private: &mut crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
    replacement: sophia_protocol::ClientAdmissionContext,
    previous: sophia_protocol::ClientAdmissionId,
    // TAKEN, NOT BORROWED. A client number belongs to one exact registration
    // until that registration's ending has finished with it, so a replacement
    // at the same number is only reachable once the original has gone. What a
    // control still wants afterwards -- its gate, its sender, its endpoint --
    // it captures before calling this.
    original: XServerFrontendClientRouteRegistration,
    surface: Option<(SurfaceId, XResourceId)>,
) -> (
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    PrivateEndpointIdentity,
) {
    private
        .participant
        .revoke_admission(client, previous)
        .expect("the admission this fixture made is the one it revokes");
    // The first registration's row goes before the second exists. Its
    // channels are the caller's to dispose of, because a caller may still be
    // holding one on purpose.
    //
    // THE REGISTRATION ITSELF GOES HERE, and it has to. A client number
    // belongs to one exact registration until that registration's ending has
    // finished with it, so a replacement at the same number is reachable only
    // afterwards. What a control still wants -- its gate, its senders, its
    // endpoint identity -- it captured before calling this.
    drop(original);
    private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .remove(&client);
    // The lifecycle owner still holds a record for the closed admission. It is
    // released by the owner's own drive, not by dropping anything, so the
    // replacement is admitted only after the first one has actually finished.
    let lifecycle = private.terminal.lifecycle.clone();
    for _ in 0..16 {
        lifecycle
            .drive(NonZeroUsize::new(1).unwrap())
            .expect("a readable lifecycle owner");
    }
    private
        .admission_participant()
        .admit(client, replacement)
        .expect("the boundary admits the replacement");
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(replacement))
        .expect("a replacement registration");
    // No attach_private_lifecycle here: with the owner already installed, the
    // admit above registered the replacement's gate itself, and attaching a
    // second one for the same client is refused as a duplicate.
    let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    if let Some((surface, window)) = surface {
        // The original registration's surface route went with it, so the
        // replacement registers its own.
        private
            .broker
            .registry
            .register_surface(client, replacement.namespace.id, surface, window)
            .expect("the replacement's surface");
        // The replacement selects the same window, so its own presses resolve.
        let mut state = selected.lock().expect("a readable selection state");
        state.register(
            window,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect { x: 0, y: 0, width: 200, height: 100 },
        );
        state.observe_mapped(window);
        state.update(window, Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .attach_connection_state(
            &registration,
            replacement.namespace.id,
            selected,
            Arc::new(AtomicU64::new(0)),
        )
        .expect("the replacement's connection state");
    let endpoint = private
        .endpoint_for(&registration)
        .expect("the replacement's own endpoint, from its own registration");
    (registration, channels, endpoint)
}

/// One real source-built capsule, and a replacement registration for the same
/// client that did not exist when it was built.
fn capsule_then_replacement(
    client: XServerFrontendClientId,
    delivery: u64,
    replacement: sophia_protocol::ClientAdmissionContext,
) -> (
    PrivatePreparedRunner,
    PrivateSettlementOwner,
    XAuthorityOrderedDelivery,
    Arc<PrivateDeliveryCompletion>,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    PrivateEndpointIdentity,
) {
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, delivery, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let cell = admitted_cell(private, delivery);
    let original = {
        let record = &mut private.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the press built its own capsule")
        };
        capsule
    };
    assert!(Arc::ptr_eq(
        &cell,
        &original.finalizer().expect("carried").completion
    ));
    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, endpoint) = replace_registration(
        private,
        client,
        replacement,
        admitted(client).client_id,
        original_registration,
        None,
    );
    (
        runner,
        durable,
        original,
        cell,
        registration,
        channels,
        endpoint,
    )
}

#[test]
fn a_capsule_from_a_replaced_admission_is_refused_though_every_number_agrees() {
    // Same client, same session generation, a different admission. The tuple
    // the ledger knows this connection by is identical on both sides, which is
    // exactly why it cannot be what admission is decided on: the boundary
    // itself treats a replacement admission inside one session as a different
    // admission, and a delayed revoke naming the old one must not close the
    // new one.
    let client = XServerFrontendClientId(7651);
    let replacement = admission_with(client, 76519, ROLE_SESSION_GENERATION);
    let (runner, durable, original, cell, registration, channels, endpoint) =
        capsule_then_replacement(client, 76510, replacement);
    assert_eq!(
        original.recipient(),
        sophia_input_authority::ConnectionIdentity {
            recipient: client.raw(),
            connection_generation: ROLE_SESSION_GENERATION,
        },
        "the capsule's ledger identity"
    );

    // WHAT THE TEARDOWN ALREADY DID is recorded before the writer is asked,
    // so what follows is attributed to the refusal and not to the revocation.
    // Replacing an admission ends its outstanding deliveries -- that is the
    // recovery answering for a connection that has gone -- and this control is
    // about the writer, which must add nothing to it either way.
    let answered_by_teardown = cell.answer();

    // The replacement writer's expectation comes from its own registration.
    let served = XAuthorityServedConnection::retained(endpoint);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;
    sender.send(original).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "not one byte of a replaced admission's event reached the replacement"
    );
    let held = refused.as_ref().expect("owned by this writer");
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(76510)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and the writer that never wrote for it changed nothing about its answer"
    );
    assert!(in_flight.is_none());
    drop(peer);
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_capsule_from_a_replaced_registration_is_refused_though_the_admission_agrees_too() {
    // The sharpest case: the replacement is admitted with THE SAME admission
    // context -- same client, same admission id, same namespace, same session
    // generation. Every number on both sides is equal. What differs is the
    // registration, and that is the whole of what distinguishes them.
    let client = XServerFrontendClientId(7661);
    let (runner, durable, original, cell, registration, channels, endpoint) =
        capsule_then_replacement(client, 76610, admitted(client));
    assert_eq!(
        original.recipient(),
        sophia_input_authority::ConnectionIdentity {
            recipient: client.raw(),
            connection_generation: ROLE_SESSION_GENERATION,
        }
    );

    let answered_by_teardown = cell.answer();
    let served = XAuthorityServedConnection::retained(endpoint);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;
    sender.send(original).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "identical numbers are not an entitlement to these bytes"
    );
    assert!(
        refused.is_some() && in_flight.is_none(),
        "it is owned as refused work, not taken as work to do"
    );
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and the refusal is not an answer"
    );
    drop(peer);
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_producer_does_not_send_an_old_capsule_through_a_replacement_entry() {
    // THE OTHER HALF OF THE SEAM. The writer's check catches a capsule that
    // reached the wrong queue; this catches one being put there. The row the
    // endpoint is compared against is the row the sender is cloned from, under
    // one guard, so there is no gap between deciding a row is right and taking
    // its channel.
    //
    // Registration/API seam, as above: nothing a client drives replaces a live
    // registration.
    let client = XServerFrontendClientId(7681);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 76810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 76810);

    // The press has an event owed and a capsule built for it, still owned by
    // the custody that made it.
    let recovery = private.broker.registry.input_recovery.clone();
    {
        let record = &mut private.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
    }
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Pending
    );

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, _endpoint) = replace_registration(
        private,
        client,
        admitted(client),
        admitted(client).client_id,
        original_registration,
        None,
    );
    let answered_by_teardown = cell.answer();

    // The replacement's row holds a different channel. Offering the old
    // capsule must not put it there.
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "a replacement registration is not handed the work of the one it replaced"
    );

    // And the capsule stays exactly where it was, unsent and unanswered by
    // this refusal.
    let custody = &private.terminal.holds[0].custody;
    assert_eq!(
        custody.dispatch,
        PrivateDispatchPhase::Pending,
        "no handover was begun for it"
    );
    let Some(PrivatePendingDelivery::Capsule(held)) = custody.pending.as_ref() else {
        panic!("the original capsule is still owned by the custody that built it")
    };
    assert_eq!(
        held.delivery(),
        XAuthorityInputDeliveryId::from_raw(76810)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &held.finalizer().expect("carried").completion
    ));
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and offering it to a row that is not its own answers nothing"
    );
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_producer_does_not_send_an_old_release_through_a_replacement_entry() {
    // The press control could not see this: the release reaches its queue by
    // the ledger-selected attempt path, which acquires its own sender. Both
    // paths have to check the row they send through.
    //
    // Registration/API seam, as elsewhere in this file.
    let client = XServerFrontendClientId(7691);
    let mut f = prepared_ordered_fixture(client);
    attempt_release(&mut f, 76910, 272);
    let private = f.runner.frontend.as_mut().unwrap();

    // The press goes first and normally, so what is left owed is the release.
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(
        f.channels.ordered.try_iter().count(),
        1,
        "the press this release ends"
    );
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.record_one_native(), Some(true));
    let release_cell = admitted_cell(private, 76911);
    assert_eq!(private.terminal.settling.len(), 1);
    assert!(private.terminal.settling[0].native_recorded());

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, _endpoint) = replace_registration(
        private,
        client,
        admitted(client),
        admitted(client).client_id,
        original_registration,
        None,
    );
    let answered_by_teardown = release_cell.answer();

    // The ledger may hand out an attempt; the row it would be served through
    // is not this release's, so nothing is written down and nothing is taken.
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "a replacement registration is not handed the release of the one it replaced"
    );
    let release = &private.terminal.settling[0];
    assert!(
        matches!(
            release.dispatch(),
            PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
        ),
        "no handover was begun for it: building its capsule may move Untaken to \
         Pending, but nothing past that, got {:?}",
        release.dispatch()
    );
    assert!(
        matches!(
            release.custody.pending.as_ref(),
            Some(PrivatePendingDelivery::Capsule(_))
        ),
        "and its own capsule is still there, unsent"
    );
    assert!(
        release.attempt().is_none(),
        "any attempt claimed for it was given back rather than held against an \
         unusable row"
    );
    assert_eq!(
        release_cell.answer(),
        answered_by_teardown,
        "and offering it to a row that is not its own answers nothing"
    );
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_serving_owner_keeps_its_own_endpoint_when_its_registration_is_replaced() {
    // The comparison controls prove two deliberately different identities
    // compare unequal. This proves what matters for attachment: an owner built
    // for one registration, holding that registration's receiver and socket,
    // goes on expecting THAT registration after a replacement exists -- and
    // refuses bytes for anything else through its own serving call rather than
    // through a free function a caller could hand three unrelated things.
    //
    // Registration/API seam, as elsewhere in this file.
    let client = XServerFrontendClientId(7701);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77010, 272, true);

    // A genuinely different connection, admitted and grabbed the ordinary way,
    // with its own source-built press. Nothing about this capsule is
    // fabricated: it is what its own endpoint is owed.
    let other = XServerFrontendClientId(7702);
    let other_window = XResourceId::new(0x307702, 1);
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
    attempt_run(&mut f, 77012, 273, true);

    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let foreign_cell = admitted_cell(private, 77012);
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

    // Kept before the replacement removes the row that holds it, so this
    // owner's queue can still be handed something afterwards. That is the
    // point: an incorrectly supplied capsule has to be caught at the serving
    // boundary, not only prevented at the producer.
    let original_sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("the original row")
        .ordered
        .clone();

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();

    // The owner is built from the registration and receiver that exist now,
    // and owns them from here on.
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport =
        XAuthorityOrderedTransport::bind(&original_registration, original_channels.ordered, &output, &wire, &pending, None)
            .unwrap_or_else(|(refusal, _)| {
                panic!("this connection's own receiver and output bind: {refusal:?}")
            });
    let mut owner =
        X11OrderedServingOwner::for_registration(private, &original_registration, transport)
            .unwrap_or_else(|(refusal, _)| {
                panic!("an owner for the registration that made this receiver: {refusal:?}")
            });

    // Its own connection's event is served normally, once.
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    let mut flushed = 0;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => flushed += 1,
            X11OrderedServeStep::Idle => break,
            other => panic!("its own endpoint's event is served: {other:?}"),
        }
    }
    assert_eq!(flushed, 1, "the endpoint it was built for writes once");
    assert!(owner.refused().is_none() && owner.in_flight().is_none());
    // Its own event's bytes are taken off the wire, so what is read later can
    // only be something written after this point.
    let mut drained = [0u8; 4096];
    assert!(
        (&peer).read(&mut drained).is_ok_and(|read| read > 0),
        "its own event really did reach the wire"
    );
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}

    // THE FOREIGN CAPSULE GOES ON THIS OWNER'S QUEUE FIRST, while this
    // connection's own gate is still open. Its ending closes that gate, and a
    // capsule offered afterwards would be refused by the gate rather than by
    // the owner -- which is a different refusal and not this control's
    // subject.
    gated_send(&original_sender, foreign).expect("this owner's queue accepts it");

    // FIRST, ITS ROW GOES AND ITS REGISTRATION DOES NOT. This is where the
    // stale capability is asked, because it is the only arrangement in which a
    // stale registration and a would-be replacement both exist: the number is
    // still this registration's, so the replacement is refused outright.
    private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .remove(&client);
    assert!(
        private.endpoint_for(&original_registration).is_err(),
        "a registration that is no longer the current row names no endpoint"
    );
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("its number is still this registration's");
    assert!(
        matches!(
            refused,
            XServerFrontendRouteError::ClientNumberExcluded { client: same } if same == client
        ),
        "a number whose registration still holds it is not available: {refused:?}"
    );

    // NOW the registration goes, which is what frees the number, and the
    // replacement is made. The owner is not rebuilt and is not told: it still
    // holds what it was given.
    let (replacement_registration, replacement_channels, replacement_endpoint) =
        replace_registration(
            private,
            client,
            admitted(client),
            admitted(client).client_id,
            original_registration,
            None,
        );

    // IT CANNOT ADOPT THE REPLACEMENT'S IDENTITY. The replacement can name its
    // own endpoint, and it is not the one this owner serves.
    assert!(
        !owner.served.endpoint().matches(&replacement_endpoint),
        "the owner serves the registration it was built for, not the current one"
    );
    assert!(
        private.endpoint_for(&replacement_registration).is_ok(),
        "and the replacement can name its own"
    );
    // THE STALE CAPABILITY WAS ALREADY ASKED, above, while it still existed.
    // It cannot be asked here: a registration holds its number until its own
    // ending has finished with it, so there is no moment at which a stale
    // registration and the replacement that took its number are both in a
    // caller's hands.

    // AND IT CANNOT BE HANDED ANOTHER ENDPOINT'S BYTES. The capsule queued
    // above is owed to a different connection, and the owner's own serving
    // call refuses it before any of it is written.
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "not one byte of another endpoint's event reached the socket this owner holds"
    );
    let held = owner.refused().expect("owned by this owner");
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(77012)
    );
    assert_eq!(held.cause(), Some(X11OrderedAdmissionRefusal::ForeignEndpoint));
    assert!(Arc::ptr_eq(
        &foreign_cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert!(owner.in_flight().is_none());
    assert!(
        foreign_cell.answer().is_none(),
        "and its admission is not answered by an owner that was never entitled to it"
    );

    // A second arrival while one is held is reported as itself and does not
    // overwrite the one thing that still owns the first.
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::AlreadyHolding)
    ));
    assert_eq!(
        owner
            .refused()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(77012)
    );
    drop(peer);
    drop(other_registration);
    drop(replacement_channels);
    drop(replacement_registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_serving_constructor_rejects_another_registrations_receiver() {
    // Two real prepared registrations and their actual registry-created
    // receivers. Independent origins make the mismatch unambiguous; no
    // capsule, outcome, socket traffic or production-loop scenario is staged.
    let a=prepared_ordered_fixture(XServerFrontendClientId(7711));
    let b=prepared_ordered_fixture(XServerFrontendClientId(7712));
    let private=a.runner.frontend.as_ref().unwrap();
    let endpoint_a=private.endpoint_for(&a.registration).unwrap();
    let endpoint_b=b.runner.frontend.as_ref().unwrap().endpoint_for(&b.registration).unwrap();
    assert!(!endpoint_a.matches(&endpoint_b));
    let (socket,_peer)=UnixStream::pair().unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    // The association is established where the transport is bound, so that is
    // where crossing two real connections is caught.
    let bound = XAuthorityOrderedTransport::bind(&a.registration, b.channels.ordered, &output, &wire, &pending, None);
    assert!(
        bound.is_err(),
        "binding must reject B's original receiver when given registration A"
    );
    let (refusal, returned) = bound.err().expect("refused");
    assert_eq!(refusal, X11OrderedServingRefusal::ForeignReceiver);
    assert!(
        returned.minted_by(&b.registration),
        "and the receiver handed back is B's own, whole"
    );
    // And the same refusal stands at the serving owner, for a transport that
    // was bound for a different registration.
    let transport = XAuthorityOrderedTransport::bind(&b.registration, returned, &output, &wire, &pending, None)
        .unwrap_or_else(|_| panic!("B's own registration and B's own receiver bind"));
    let owner = X11OrderedServingOwner::for_registration(private, &a.registration, transport);
    assert!(
        owner.is_err(),
        "a transport bound for another registration prepares no writer here"
    );
}

#[test]
fn a_failed_serving_constructor_preserves_its_original_queued_capsule() {
    let client=XServerFrontendClientId(7721);
    let mut f=prepared_ordered_fixture(client);
    // Real admitted native press and actual original ordered queue handover.
    attempt_run(&mut f,77210,272,true);
    let private=f.runner.frontend.as_mut().unwrap();
    let original=admitted_cell(private,77210);
    assert_eq!(private.dispatch_one_press(),Some(true));
    let sender=private.broker.registry.clients.lock().unwrap().get(&client).unwrap().ordered.clone();
    let capsule=f.channels.ordered.try_recv().expect("original queue accepted the actual press");
    assert_eq!(capsule.delivery(),XAuthorityInputDeliveryId::from_raw(77210));
    assert_eq!(capsule.client(),client);
    assert!(Arc::ptr_eq(&original,&capsule.finalizer().unwrap().completion));
    assert!(capsule.endpoint().matches(&private.endpoint_for(&f.registration).unwrap()));
    assert_eq!(Arc::strong_count(capsule.finalizer().unwrap()),1,"only this nonclone capsule owns its finalizer Arc");
    // Keep ONLY a Weak finalizer witness; a strong clone here would mask
    // destruction of the original queued capsule.
    let finalizer=Arc::downgrade(capsule.finalizer().unwrap());
    assert!(gated_send(&sender,capsule).is_ok(),"put the exact original capsule back into its original queue");
    assert!(finalizer.upgrade().is_some());
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].custody.pending.is_none());

    // Real participant revocation makes endpoint acquisition refuse while the
    // original registration, receiver, sender and queued capsule remain held.
    // No registry/authority field, outcome or phase is forced by this control.
    private.participant.revoke_admission(client,admitted(client).client_id).unwrap();
    assert!(private.endpoint_for(&f.registration).is_err());
    let answered_before=original.answer();
    assert!(Arc::ptr_eq(&original,private.terminal.holds[0].custody.completion.as_ref().unwrap()));
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].native.is_some());
    let (socket,_peer)=UnixStream::pair().unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert!(finalizer.upgrade().is_some(),"revocation did not destroy the actual queued capsule immediately before construction");
    let transport = XAuthorityOrderedTransport::bind(&f.registration, f.channels.ordered, &output, &wire, &pending, None)
        .unwrap_or_else(|_| panic!("this connection's own receiver and registration bind"));
    let returned=X11OrderedServingOwner::for_registration(private,&f.registration,transport);
    assert!(returned.is_err(),"the established endpoint refusal is returned");
    let payload_still_owned=finalizer.upgrade().is_some();
    assert_eq!(original.answer(),answered_before,"constructor failure does not change the completion answer");
    assert!(Arc::ptr_eq(&original,private.terminal.holds[0].custody.completion.as_ref().unwrap()));
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].native.is_some());
    assert!(payload_still_owned,"a refused constructor must return or durably retain its accepted receiver/capsule resources, not drop them through ?");
    drop(sender);
}

/// Drive a close to quiescence in bounded visits, reporting what each did.
fn close_to_quiet(
    owner: &mut X11OrderedServingOwner,
    cause: X11OrderedCloseCause,
) -> Vec<X11OrderedCloseStep> {
    owner
        .begin_close(cause)
        .unwrap_or_else(|kind| panic!("a socket pair ends: {kind:?}"));
    let mut steps = Vec::new();
    for _ in 0..32 {
        let step = owner.advance_close(XByteOrder::LittleEndian, 7);
        steps.push(step);
        if matches!(
            step,
            X11OrderedCloseStep::Quiet | X11OrderedCloseStep::Drained
        ) {
            break;
        }
    }
    steps
}

/// A serving owner for this fixture's own connection, with its output.
fn serving_owner_for(
    f: &mut PreparedOrderedFixture,
    socket: UnixStream,
) -> (X11OrderedServingOwner, Arc<Mutex<UnixStream>>) {
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
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
    let transport = XAuthorityOrderedTransport::bind(&f.registration, ordered, &output, &wire, &pending, None)
        .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));
    (owner, output)
}

#[test]
fn a_close_adjudicates_the_queued_admission_before_letting_its_payload_go() {
    // (a) The disposition is an answer this close established through the
    // capsule's own finalizer, not a client-wide sweep and not a drop whose
    // consequences someone else is assumed to clean up.
    let client = XServerFrontendClientId(7731);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77310, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77310);
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    assert!(cell.answer().is_none(), "nothing has answered it yet");

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        matches!(
            steps.first(),
            Some(X11OrderedCloseStep::Adjudicated(
                PrivateAdjudication::Answered
            ))
        ),
        "the queued admission was offered an outcome, not discarded: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (1, 0, 0)
    );
    assert!(owner.retained_unanswered().is_empty() && owner.retained_foreign().is_empty());
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "and the answer is on the admission's own completion"
    );
    // The socket really ended. Read with a deadline: a connection that was
    // never ended would leave this waiting for a peer that is still there,
    // and a control that hangs says nothing.
    peer.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a deadline on the peer");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "the peer sees the connection ended"
    );
}
