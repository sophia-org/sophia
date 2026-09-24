// The control writer's outcome: recorded once, answered once, and what a full
// channel retains of an effect a writer really applied.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[cfg(unix)]
fn configure(client: XServerFrontendClientId, surface: SurfaceId, transaction: u64) -> XAuthorityClientControlCommand {
    XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    }
}

#[test]
fn a_control_credit_is_released_exactly_once_when_its_outcome_is_recorded() {
    // The acknowledgement helper and the credit accounting, driven directly.
    // No command is applied here; a_writer_applies_a_control_and_its_credit_is
    // _released_once drives the production writer against the real runtime.
    let client = XServerFrontendClientId(252);
    let surface = SurfaceId::new(252, 1);
    let (acknowledgements, ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 12001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    assert_eq!(ran.len(), 1);

    // Routed is not answered: the writer still has it.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "handing a command to a writer is not an outcome"
    );

    // The writer's own path, with the registry the route registry installed.
    let routed = channels
        .control
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the routed control");
    let X11RoutedControl::Authority {
        command, completion, ..
    } = routed
    else {
        panic!("an authority control");
    };
    assert!(completion.is_some(), "the writer is given the registration");
    let writer = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: private.broker.registry.control_completion(),
    };
    assert!(
        writer.completion().is_some(),
        "a private instance installs its registry where a client writer reads it"
    );
    assert_eq!(
        writer.resume_execution(completion),
        ControlExecutionClaim::Resumed,
        "routing claimed execution already; a writer only continues it"
    );
    writer
        .send_ack_for(
            client,
            XAuthorityControlAck {
                kind: command.kind(),
                transaction: command.transaction(),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
            completion,
        )
        .expect("a free channel");
    assert!(ack_receiver.try_recv().is_ok());

    assert_eq!(
        private.reclaim_settled(),
        1,
        "an answered control releases its credit"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "and cannot release it a second time"
    );
}

#[test]
fn a_refused_control_leaves_no_registration_to_answer_for_it() {
    let client = XServerFrontendClientId(255);
    let surface = SurfaceId::new(255, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    // One credit for the whole owner, so the second submit is refused.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    let producer = private.control_producer();
    producer
        .submit(&owner_of_durable.lease(), configure(client, surface, 14001))
        .expect("the first to be accepted");
    let refused = producer.submit(&owner_of_durable.lease(), configure(client, surface, 14002));
    let Err((_, returned)) = refused else {
        panic!("the second to be refused once the owner is saturated");
    };
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(14002),
        "the caller keeps the command it was never told had been taken"
    );

    // The refused command has exactly one owner: the caller. A registration
    // left behind would answer for it at the next cancellation edge, and
    // answer twice if the caller retried.
    let report = private.shutdown();
    let answered: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        !answered.contains(&14002),
        "a refused command is not carried by the instance that refused it"
    );
}

#[test]
fn a_registration_only_answers_to_the_registry_that_issued_it() {
    let client = XServerFrontendClientId(256);
    let surface = SurfaceId::new(256, 1);
    let issuer = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let other = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&issuer, configure(client, surface, 15001));

    assert_eq!(
        issuer.state_of(token),
        crate::ControlRecordState::Outstanding
    );
    // Absent here too, and absence is not an outcome: answering for it would
    // release another instance's credit on the strength of a record this
    // registry never held.
    assert_eq!(
        other.state_of(token),
        crate::ControlRecordState::Unanswerable
    );

    assert!(issuer.discard(token));
    assert_eq!(issuer.state_of(token), crate::ControlRecordState::Retired);
    assert_eq!(
        other.state_of(token),
        crate::ControlRecordState::Unanswerable
    );
}

#[test]
fn an_owed_outcome_is_not_given_up_as_though_it_were_unexecuted() {
    let client = XServerFrontendClientId(257);
    let surface = SurfaceId::new(257, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 16001));
    assert_eq!(
        registry.publish_with(
            token,
            XAuthorityClientControlAck {
                client,
                acknowledgement: XAuthorityControlAck {
                    kind: XAuthorityControlKind::ConfigureSurface,
                    transaction: TransactionId::from_raw(16001),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
            },
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );

    // The effect already happened, so this record holds the only copy of an
    // outcome. Handing the operation to another owner cannot apply to it.
    assert!(
        !registry.discard(token),
        "an owed outcome is not something that can be handed on"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding
    );
}

#[test]
fn a_poisoned_registry_answers_for_nothing_and_frees_nothing() {
    let client = XServerFrontendClientId(258);
    let surface = SurfaceId::new(258, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 17001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);

    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let token = accepted(&registry, configure(client, surface, 17002));

    // An owed outcome, so the retry below has something to call back into.
    assert_eq!(
        registry.publish_with(
            token,
            XAuthorityClientControlAck {
                client,
                acknowledgement: XAuthorityControlAck {
                    kind: XAuthorityControlKind::ConfigureSurface,
                    transaction: TransactionId::from_raw(17002),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
            },
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );

    // Poison it from inside its own lock, which is the only way it happens.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            poisoner.publish_owed_with(|_| panic!("poisoning the registry"));
        })
        .join()
        .is_err(),
        "the panic happened while the lock was held"
    );

    // Silence is not an answer. Reading it as one would release a credit for
    // work still outstanding.
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Unanswerable
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "an unreadable registry releases no credit"
    );
}

#[test]
fn a_command_queued_to_a_writer_is_never_cancelled_as_unexecuted() {
    let client = XServerFrontendClientId(259);
    let surface = SurfaceId::new(259, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 18001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    // It left the queue, so draining the queue at shutdown cannot reach it.
    assert!(
        channels
            .control
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_ok(),
        "the command reached a client writer's queue"
    );

    // Routing was the first authoritative effect and claimed execution, so
    // this operation is mid-flight, not unexecuted. Handing it back here
    // would give it a second owner able to answer AuthorityRejected while the
    // copy still in the writer's queue went on to apply and answer Delivered.
    let report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        carried.is_empty(),
        "a command a writer could still run is not handed to a second owner"
    );
    assert_eq!(
        report.outstanding_control().expect("a readable registry"),
        1,
        "it is retained, because what the runtime did is not established"
    );
}

#[test]
fn a_rejected_control_leaves_no_record_behind_to_answer_again() {
    let client = XServerFrontendClientId(260);
    let surface = SurfaceId::new(260, 1);
    let (acknowledgements, ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    // Accepted and never routed, so shutdown answers it from the queue.
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 19001))
        .expect("the shared admission to accept control");

    let report = private.shutdown();
    assert!(report.pending.is_empty(), "the rejection was delivered");
    assert_eq!(
        ack_receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the rejection")
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );
    assert_eq!(
        report.outstanding_control().expect("a readable registry"),
        0,
        "a command handed to settlement gives up its record as it goes"
    );
}

#[test]
fn a_control_writer_records_that_its_client_has_none_when_it_stops() {
    let client = XServerFrontendClientId(261);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _client_channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
    let (routes, route_receiver) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: route_receiver,
        acknowledgements: sync_channel(4).0,
        completion: Some(registry.clone()),
    };

    let state = X11CoreSocketServerState::new();
    let (writer_stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let writer = spawn_x11_control_writer(
        X11ClientOutput::shared(writer_stream, 0),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(XCoreEventSelectionState::default())),
        Arc::new(AtomicU16::new(0)),
        state.atoms.clone(),
        state.properties.clone(),
        state.runtime.clone(),
        state.control_runtime_pending.clone(),
        crate::XWireClientResourceRange {
            base: 0x200000,
            mask: 0x1fffff,
        },
        NamespaceId::from_raw(261),
        client,
        Some(routing.clone()),
        channels,
    )
    .expect("a writer");

    assert!(
        routing.control_writer_present(client),
        "a serving writer is present"
    );

    // The route queue goes, which is one of the ways a writer leaves.
    drop(routes);
    writer.thread.join().expect("the writer thread").unwrap();

    assert!(
        !routing.control_writer_present(client),
        "a stopped writer stops work being accepted for the client it served"
    );
    drop(registration);
}

#[test]
fn a_writer_that_unwinds_still_records_that_its_client_has_none() {
    let client = XServerFrontendClientId(262);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
    assert!(routing.control_writer_present(client));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _seal = X11ControlWriterSeal {
            routing: Some(&routing),
            client,
        };
        panic!("a writer leaving the only way a call at the end cannot catch");
    }));
    assert!(unwound.is_err());
    assert!(
        !routing.control_writer_present(client),
        "however a writer leaves, its client stops having work accepted"
    );
    drop(registration);
}

#[test]
fn a_recorded_outcome_is_republished_once_the_channel_drains() {
    // Registry and retry contract, driven directly. The real-writer form is
    // a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied.
    let client = XServerFrontendClientId(263);
    let surface = SurfaceId::new(263, 1);
    // One slot, filled, so the writer's acknowledgement cannot be published.
    let (acknowledgements, ack_receiver) = sync_channel(1);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 20001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);

    let routed = channels
        .control
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the routed control");
    let X11RoutedControl::Authority {
        command, completion, ..
    } = routed
    else {
        panic!("an authority control");
    };

    // Prefill the only slot, then let the writer apply and answer.
    acknowledgements
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::ConfigureSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");
    let writer = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: private.broker.registry.control_completion(),
    };
    assert_eq!(
        writer.resume_execution(completion),
        ControlExecutionClaim::Resumed,
        "routing claimed execution already; a writer only continues it"
    );
    assert!(
        writer
            .send_ack_for(
                client,
                XAuthorityControlAck {
                    kind: command.kind(),
                    transaction: command.transaction(),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                completion,
            )
            .is_err(),
        "a full channel is reported to the writer"
    );

    // Unpublished is not answered, so the credit is still held.
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        private.republish_owed_acknowledgements(),
        0,
        "a channel that is still full publishes nothing"
    );

    // Drain the prefill and the outcome goes out, once.
    assert_eq!(
        ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    assert_eq!(
        ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(20001),
        "the outcome, not the command"
    );
    assert_eq!(
        private.republish_owed_acknowledgements(),
        0,
        "and not a second time"
    );
    assert_eq!(
        private.reclaim_settled(),
        1,
        "published at last, so the credit is released"
    );
    assert_eq!(private.reclaim_settled(), 0);
}


/// Register and take acceptance, as a producer does under the admitting hold.
///
/// A bare `register` leaves a reservation, which is still the producer's and
/// which no part of the instance may act on. Tests about an accepted operation
/// have to take that step too, or they are testing a different phase.
#[cfg(unix)]
fn accepted(
    registry: &crate::ControlCompletionRegistry,
    command: XAuthorityClientControlCommand,
) -> ControlCompletionToken {
    // A client with a running writer is what a test means by an accepted
    // operation. Registered-and-awaiting-a-spawn is a different state and the
    // tests that mean it say so.
    registry.writer_started(command.client);
    let token = registry
        .register(command)
        .expect("a fresh registry to have room");
    registry
        .begin_acceptance(token)
        .expect("a fresh reservation is acceptable")
        .commit();
    token
}

#[cfg(unix)]
fn completion_ack(
    command: XAuthorityClientControlCommand,
    outcome: XAuthorityControlOutcome,
) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: command.client,
        acknowledgement: XAuthorityControlAck {
            kind: command.command.kind(),
            transaction: command.command.transaction(),
            surface: command.command.surface(),
            outcome,
        },
    }
}

#[test]
fn an_acknowledgement_for_another_operation_cannot_settle_this_one() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let mine = configure(XServerFrontendClientId(301), SurfaceId::new(301, 1), 21001);
    let theirs = configure(XServerFrontendClientId(302), SurfaceId::new(302, 1), 21002);
    let token = accepted(&registry, mine);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // The token is the identity, but it is not the whole identity: what the
    // acknowledgement names must be what was registered, or one request's
    // outcome settles another's record.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(theirs, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::NotThisOperation),
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding,
        "and the operation it was registered for is still unanswered"
    );

    // Its own acknowledgement settles it.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(mine, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered),
    );
}

#[test]
fn an_established_outcome_is_not_replaced_by_a_later_contradiction() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(XServerFrontendClientId(303), SurfaceId::new(303, 1), 21003);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let established = completion_ack(command, XAuthorityControlOutcome::Delivered);
    assert_eq!(
        registry.publish_with(token, established, |_| ControlPublication::Retained),
        Ok(ControlPublication::Retained),
    );

    // What happened does not become something else later.
    let contradiction = completion_ack(command, XAuthorityControlOutcome::ClientGone);
    assert_eq!(
        registry.publish_with(token, contradiction, |_| ControlPublication::Retained),
        Err(crate::ControlPublicationRefusal::OutcomeAlreadyEstablished),
    );
    // Repeating the same one says nothing new and is not an error.
    assert_eq!(
        registry.publish_with(token, established, |_| ControlPublication::Retained),
        Ok(ControlPublication::Retained),
    );

    let mut republished = Vec::new();
    assert_eq!(
        registry.publish_owed_with(|acknowledgement| {
            republished.push(*acknowledgement);
            ControlPublication::Delivered
        }),
        1
    );
    assert_eq!(republished, vec![established], "the first outcome stands");
}

#[test]
fn a_retired_registration_cannot_answer_for_a_later_one() {
    let registry = crate::ControlCompletionRegistry::with_capacity(1).expect("an unused origin");
    let old = configure(XServerFrontendClientId(304), SurfaceId::new(304, 1), 21004);
    // One identity left. Exhaustion is reachable only by placing the counter
    // near its end, and that setter stays here in the test rather than in
    // production src.
    registry.inner.lock().unwrap().next_incarnation = u64::MAX - 1;
    let stale = accepted(&registry, old);
    assert!(registry.discard(stale));

    // The counter cannot advance, so registration refuses rather than issue
    // the identity it just gave away.
    let new = configure(XServerFrontendClientId(305), SurfaceId::new(305, 1), 21005);
    assert!(matches!(
        registry.register(new),
        Err((crate::ControlCompletionRefusal::Exhausted, returned))
            if returned.command.transaction() == TransactionId::from_raw(21005)
    ));
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "nothing was accepted, so nothing is owed"
    );

    // And the stale token answers for nothing.
    assert_eq!(
        registry.publish_with(
            stale,
            completion_ack(old, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::NoLongerHeld),
    );
}

#[test]
fn an_exhausted_registry_refuses_rather_than_issue_one_identity_twice() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    registry.inner.lock().unwrap().next_incarnation = u64::MAX - 1;
    let first = accepted(&registry, configure(
            XServerFrontendClientId(307),
            SurfaceId::new(307, 1),
            21008,
        ));
    let second = registry.register(configure(
        XServerFrontendClientId(308),
        SurfaceId::new(308, 1),
        21009,
    ));
    let Err((refusal, returned)) = second else {
        panic!("a second identity cannot exist, so it must be refused");
    };
    assert_eq!(refusal, crate::ControlCompletionRefusal::Exhausted);
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(21009),
        "and the command goes back to the caller that still owns it"
    );
    assert_eq!(
        registry.state_of(first),
        crate::ControlRecordState::Outstanding,
        "the one that was accepted keeps its own record"
    );
}

// The production control writer, on an owned socketpair, against the real
// runtime. Ported from an independent review's fixture: the acknowledgement
// helper can be exercised without any of this, and doing so proves nothing
// about whether an effect happened.

/// A runtime with one registered window, forty wide.
#[cfg(unix)]
fn writer_runtime(surface: SurfaceId) -> X11CoreSocketServerState {
    let state = X11CoreSocketServerState::new();
    state
        .runtime
        .lock()
        .unwrap()
        .apply(crate::XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(70000),
            namespace: NamespaceId::from_raw(252),
            kind: crate::XAuthorityRequestKind::CreateWindow {
                window: XResourceId::new(0x200252, 1),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 30,
                },
                constraints: sophia_protocol::SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
    assert_eq!(writer_window_width(&state), 40);
    state
}

#[cfg(unix)]
fn writer_window_width(state: &X11CoreSocketServerState) -> i32 {
    state
        .runtime
        .lock()
        .unwrap()
        .window_geometry(NamespaceId::from_raw(252), XResourceId::new(0x200252, 1))
        .unwrap()
        .width
}

#[cfg(unix)]
fn writer_windows(surface: SurfaceId) -> Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>> {
    Arc::new(Mutex::new(BTreeMap::from([(
        surface,
        XResourceId::new(0x200252, 1),
    )])))
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn writer_start(
    registry: Option<&XServerFrontendRouteRegistry>,
    state: &X11CoreSocketServerState,
    control: Receiver<X11RoutedControl>,
    completion: Option<crate::ControlCompletionRegistry>,
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    client: XServerFrontendClientId,
    windows: Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>>,
    priority: Arc<AtomicUsize>,
) -> (X11ControlWriter, std::os::unix::net::UnixStream) {
    if let Some(registry) = registry {
        registry
            .select_core_events(client, XResourceId::new(0x200252, 1), 1 << 17)
            .unwrap();
    }
    let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .unwrap();
    let writer = spawn_x11_control_writer(
        X11ClientOutput::shared(stream, 0),
        priority,
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        Arc::new(AtomicU64::new(0)),
        windows,
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(XCoreEventSelectionState::default())),
        Arc::new(AtomicU16::new(0)),
        state.atoms.clone(),
        state.properties.clone(),
        state.runtime.clone(),
        state.control_runtime_pending.clone(),
        crate::XWireClientResourceRange {
            base: 0x200000,
            mask: 0x1fffff,
        },
        NamespaceId::from_raw(252),
        client,
        registry.cloned(),
        X11ControlChannels::ClientBound {
            receiver: control,
            acknowledgements,
            completion,
        },
    )
    .expect("a writer");
    (writer, peer)
}

#[cfg(unix)]
fn writer_join(writer: X11ControlWriter) -> bool {
    writer.stop.store(true, Ordering::Release);
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !writer.thread.is_finished() && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(writer.thread.is_finished(), "an owned writer stops bounded");
    writer.thread.join().unwrap().is_ok()
}

/// Read one ConfigureNotify and check the width it announced.
#[cfg(unix)]
fn writer_configure_notify(peer: &mut std::os::unix::net::UnixStream, width: u16) {
    let mut frame = [0_u8; 32];
    std::io::Read::read_exact(peer, &mut frame).unwrap();
    assert_eq!(frame[0] & 127, 22, "ConfigureNotify");
    assert_eq!(
        u32::from_le_bytes(frame[8..12].try_into().unwrap()),
        0x200252
    );
    assert_eq!(u16::from_le_bytes(frame[20..22].try_into().unwrap()), width);
}

#[test]
fn a_writer_applies_a_control_and_its_credit_is_released_once() {
    let client = XServerFrontendClientId(290);
    let surface = SurfaceId::new(290, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(2);
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
    let state = writer_runtime(surface);

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 71001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(
        private.reclaim_settled(),
        0,
        "routed is not answered: the writer still has it"
    );

    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    let ack = acks
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the acknowledgement");
    assert_eq!(ack.acknowledgement.transaction, TransactionId::from_raw(71001));
    assert_eq!(ack.acknowledgement.outcome, XAuthorityControlOutcome::Delivered);
    // The effect, not a description of one.
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);

    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(writer_join(writer));
    assert!(acks.try_recv().is_err(), "and exactly one outcome");
}

#[test]
fn a_claimed_control_is_not_cancelled_out_from_under_its_writer() {
    let client = XServerFrontendClientId(293);
    let surface = SurfaceId::new(293, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(4);
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
    let state = writer_runtime(surface);
    let windows = writer_windows(surface);
    let priority = Arc::new(AtomicUsize::new(0));

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 73001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);

    // Park the writer inside the operation: it has taken the command and
    // claimed it, and cannot reach the runtime until this lock is released.
    let held = windows.lock().unwrap();
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements.clone(),
        client,
        windows.clone(),
        priority.clone(),
    );
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while priority.load(Ordering::Acquire) == 0 && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(
        priority.load(Ordering::Acquire) > 0,
        "the writer has the command and is inside the operation"
    );

    // Closing the instance cannot take an operation whose execution is
    // claimed. Handing it back would answer AuthorityRejected for work this
    // writer is about to apply, and the writer would then answer Delivered
    // for the same transaction.
    let mut report = private.shutdown();
    assert!(
        report.pending.is_empty(),
        "a claimed operation is not handed to a second owner"
    );
    assert_eq!(report.retry(), 0);
    assert_eq!(
        report.outstanding_control().expect("a readable registry"),
        1,
        "it is retained: what the runtime did is not established yet"
    );
    assert!(
        acks.recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "and no outcome is invented for it"
    );
    assert_eq!(writer_window_width(&state), 40);

    // Released, the same writer finishes the operation it claimed.
    drop(held);
    let answered = acks
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the writer's own outcome");
    assert_eq!(
        answered.acknowledgement.transaction,
        TransactionId::from_raw(73001)
    );
    assert_eq!(
        answered.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);
    assert!(writer_join(writer));
    assert!(
        acks.try_recv().is_err(),
        "exactly one terminal outcome, and it is the one that happened"
    );
}

#[test]
fn a_writer_refused_the_claim_produces_no_effect_and_no_outcome() {
    let client = XServerFrontendClientId(294);
    let surface = SurfaceId::new(294, 1);
    let state = writer_runtime(surface);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let (routes, control) = sync_channel(4);

    // Two operations a writer must refuse: one whose record was given up to
    // another owner, and one that never claimed execution, which means the
    // first authoritative effect was skipped.
    let command = configure(client, surface, 74001);
    let handed_on = accepted(&registry, command);
    assert!(registry.discard(handed_on));
    let unclaimed = accepted(&registry, configure(client, surface, 74002));

    let routed = |transaction, surface, completion| X11RoutedControl::Authority {
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
        focus: None,
        claim: None,
        completion,
    };
    routes
        .try_send(routed(74001, surface, Some(handed_on)))
        .expect("room");
    routes
        .try_send(routed(74002, surface, Some(unclaimed)))
        .expect("room");
    // A sentinel the writer does answer, so the two above are known to have
    // been reached rather than left in the queue by a writer that stopped
    // first. It names no surface this writer maps, so it changes nothing.
    routes
        .try_send(routed(74003, SurfaceId::new(999, 1), None))
        .expect("room");

    let (writer, mut peer) = writer_start(
        None,
        &state,
        control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    let sentinel = acks
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the sentinel, so the two before it were reached");
    assert_eq!(
        sentinel.acknowledgement.transaction,
        TransactionId::from_raw(74003)
    );
    assert_eq!(
        sentinel.acknowledgement.outcome,
        XAuthorityControlOutcome::UnknownSurface
    );
    assert!(writer_join(writer));

    assert_eq!(
        writer_window_width(&state),
        40,
        "a refused claim applies nothing"
    );
    assert!(
        acks.try_recv().is_err(),
        "and answers nothing: the outcome belongs to whoever refused the claim"
    );
    let mut byte = [0_u8; 1];
    assert_eq!(
        std::io::Read::read(&mut peer, &mut byte).unwrap(),
        0,
        "and writes nothing"
    );
    assert_eq!(
        registry.state_of(unclaimed),
        crate::ControlRecordState::Outstanding,
        "the record that still holds it keeps holding it"
    );
}

#[test]
fn a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied() {
    let client = XServerFrontendClientId(291);
    let surface = SurfaceId::new(291, 1);
    let durable = crate::PrivateSettlementOwner::default();
    // One slot, filled by a real earlier instance's rejection rather than a
    // forged acknowledgement.
    let (acknowledgements, acks) = sync_channel(1);
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
    let state = writer_runtime(surface);
    let earlier_client = XServerFrontendClientId(292);
    let earlier_surface = SurfaceId::new(292, 1);
    let (earlier, _channels, _registration, _deliveries) = private_with_client(
        acknowledgements.clone(),
        &owner_of_durable,
        earlier_client,
        earlier_surface,
    );
    earlier
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(earlier_client, earlier_surface, 72000))
        .expect("the shared admission to accept control");
    let _earlier = earlier.shutdown();

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 72001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    // The effect happens once, and then the writer cannot publish it.
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);
    assert!(!writer_join(writer), "a full channel fails this writer");
    assert_eq!(
        private.reclaim_settled(),
        0,
        "unpublished is not answered, so the credit stays"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // Drained, the retained outcome goes out once. The writer has already
    // gone, so nothing can apply the command a second time.
    assert_eq!(
        acks.recv_timeout(std::time::Duration::from_millis(500))
            .unwrap()
            .acknowledgement
            .transaction,
        TransactionId::from_raw(72000)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    let republished = acks
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the retained outcome");
    assert_eq!(
        republished.acknowledgement.transaction,
        TransactionId::from_raw(72001)
    );
    assert_eq!(
        republished.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(private.republish_owed_acknowledgements(), 0);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(acks.try_recv().is_err());
    let mut byte = [0_u8; 1];
    assert_eq!(
        std::io::Read::read(&mut peer, &mut byte).unwrap(),
        0,
        "republishing an outcome does not re-run the command"
    );
    assert_eq!(writer_window_width(&state), 80, "applied exactly once");

    // The registration outlived its writer: returning on a full channel is
    // exactly how that happens. Nothing is left to execute work for this
    // client, and the producer is told so, even though the client is still
    // registered and nothing about it has been revoked.
    assert!(
        !private.broker.registry.control_writer_present(client),
        "a writer that returned on a full channel is gone"
    );
    assert!(matches!(
        private
            .control_producer()
            .submit(&owner_of_durable.lease(), configure(client, surface, 72002)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(72002)
    ));
}
