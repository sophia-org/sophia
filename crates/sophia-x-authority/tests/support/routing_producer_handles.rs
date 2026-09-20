// Producers and consumers sharing one order: the handles they hold, what an
// accepted item owes, and what a class that cannot admit its whole pass keeps.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn two_producer_classes_share_one_order() {
    let namespace = NamespaceId::from_raw(51);
    let client = XServerFrontendClientId(68);
    let surface = SurfaceId::new(55, 1);
    let window = XResourceId::new(0x200190, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let input = private.ingress();
    let control = private.control_producer();

    // Alternating, from two genuinely different producer facades. Consumer
    // staging would have grouped these by source no matter how they arrived.
    let mut expected = Vec::new();
    for step in 0..4u64 {
        let at = input
            .submit(&service_keeper.lease(), motion_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(400 + step),
            ))
            .expect("an open coordinator to accept input");
        expected.push(at.raw());
        let at = control
            .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(step + 1),
                    surface,
                },
            })
            .expect("the shared admission to accept control");
        expected.push(at.raw());
    }

    // Positions rise in acceptance order across both producers, not grouped.
    assert!(
        expected.windows(2).all(|pair| pair[0] < pair[1]),
        "positions must rise in acceptance order across producers: {expected:?}"
    );
    let ran = private.route_pending(&service_keeper.lease()).expect("the shared order to run");
    assert_eq!(ran.len(), 8);

    // What the CONSUMER took, not what the producers were told. A consumer
    // that grouped entries someone else had already numbered would satisfy the
    // assertion above and fail this one.
    let taken: Vec<_> = ran.iter().map(|run| run.class).collect();
    assert_eq!(
        taken,
        vec![
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
        ],
        "the consumer must see the alternation the producers created"
    );
    let positions: Vec<_> = ran.iter().map(|run| run.sequence.raw()).collect();
    assert_eq!(positions, expected, "and at their own positions");

    // And carrying the identities that were actually submitted. A right-looking
    // class record can otherwise accompany the wrong payload entirely.
    let identities: Vec<_> = ran
        .iter()
        .map(|run| match run.identity {
            crate::PrivateIdentity::Delivery(delivery) => (delivery, None),
            crate::PrivateIdentity::Control {
                transaction,
                completion,
            } => {
                assert!(
                    completion.is_some(),
                    "an admitted control carries the registration that answers for it"
                );
                (None, Some(transaction))
            }
            crate::PrivateIdentity::Lease(_) => (None, None),
        })
        .collect();
    assert_eq!(
        identities,
        vec![
            (Some(XAuthorityInputDeliveryId::from_raw(400)), None),
            (None, Some(TransactionId::from_raw(1))),
            (Some(XAuthorityInputDeliveryId::from_raw(401)), None),
            (None, Some(TransactionId::from_raw(2))),
            (Some(XAuthorityInputDeliveryId::from_raw(402)), None),
            (None, Some(TransactionId::from_raw(3))),
            (Some(XAuthorityInputDeliveryId::from_raw(403)), None),
            (None, Some(TransactionId::from_raw(4))),
        ],
        "each run must name the operation its producer submitted"
    );
}

#[test]
fn a_send_that_returned_is_never_overtaken_by_one_that_started_later() {
    let namespace = NamespaceId::from_raw(52);
    let client = XServerFrontendClientId(69);
    let surface = SurfaceId::new(56, 1);
    let window = XResourceId::new(0x2001a0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let first = private.ingress();
    let second = private.control_producer();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    // A's send completes before B's begins, enforced rather than hoped for.
    let a_at = first
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(500)))
        .expect("an open coordinator to accept input");
    let gate_for_b = std::sync::Arc::clone(&barrier);
    // SCOPED, because accepting work now asks for a live owner and a lease is
    // a borrow of one. The thread's own act is what needs it, so the scope is
    // where it belongs.
    let b_at = std::thread::scope(|scope| {
        let keeper = &service_keeper;
        let b = scope.spawn(move || {
            gate_for_b.wait();
            second.submit(
                &keeper.lease(),
                XAuthorityClientControlCommand {
                    client,
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(9),
                        surface,
                    },
                },
            )
        });
        barrier.wait();
        b.join().expect("the second producer").expect("accepted")
    });

    assert!(
        a_at.raw() < b_at.raw(),
        "a completed send cannot be overtaken by one that started afterwards"
    );

    // And the consumer sees that precedence, not just the numbers.
    let mut private = private;
    let ran = private.route_pending(&service_keeper.lease()).expect("the shared order to run");
    let taken: Vec<_> = ran.iter().map(|run| run.sequence.raw()).collect();
    assert_eq!(taken, vec![a_at.raw(), b_at.raw()]);
}

#[test]
fn a_refused_control_comes_back_to_its_producer() {
    let client = XServerFrontendClientId(70);
    let surface = SurfaceId::new(57, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    // Ordinary share of two at the smallest production size.
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // A producer is refused for a client with no control writer, so a test
    // about admission capacity has to give it one.
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    let control = private.control_producer();
    let command = |transaction| XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
        },
    };

    control.submit(&service_keeper.lease(), command(1)).expect("room for the first");
    control.submit(&service_keeper.lease(), command(2)).expect("room for the second");

    // Nothing has drained, so the third has nowhere to go. Controls have no
    // recovery ticket capping them, so this is reachable by ordinary use.
    let (refusal, returned) = control
        .submit(&service_keeper.lease(), command(3))
        .expect_err("the ordinary share is full");
    assert_eq!(refusal, crate::AdmissionRefusal::Saturated);
    assert_eq!(
        returned, command(3),
        "a refused control is handed back, not destroyed"
    );
}

#[test]
fn producers_are_refused_once_their_consumer_is_gone() {
    let client = XServerFrontendClientId(71);
    let surface = SurfaceId::new(58, 1);
    let window = XResourceId::new(0x2001b0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(53), surface, window)
        .unwrap();

    // Producers outlive the frontend, which is ordinary: they are handles.
    let input = private.ingress();
    let control = private.control_producer();
    drop(private);

    // Accepting now would tell a producer its work is queued when nothing can
    // ever run it.
    match input.submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(600))) {
        Err(crate::PrivateSendError::Disconnected(_)) => {}
        other => panic!("a gone consumer is a disconnection, not {other:?}"),
    }
    let (refusal, _returned) = control
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1),
                surface,
            },
        })
        .expect_err("a gone consumer refuses control too");
    assert_eq!(refusal, crate::AdmissionRefusal::ConsumerGone);
}

#[test]
fn an_unreachable_queue_is_not_reported_as_a_finished_one() {
    let namespace = NamespaceId::from_raw(54);
    let client = XServerFrontendClientId(72);
    let surface = SurfaceId::new(59, 1);
    let window = XResourceId::new(0x2001c0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Accepted, and owed a run.
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(8201)))
        .expect("an open coordinator to accept work");

    // The queue becomes unreachable while that work is still in it.
    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    // Reporting an empty run here would say the pass finished while accepted
    // work sat in a queue nobody can open.
    assert!(
        private.route_pending(&service_keeper.lease()).is_err(),
        "an unreachable queue is not a drained one"
    );
    assert!(
        channels.input.try_recv().is_err(),
        "and nothing was delivered from it"
    );
}

#[test]
fn accepted_work_is_answered_when_its_consumer_goes_away() {
    let namespace = NamespaceId::from_raw(55);
    let client = XServerFrontendClientId(73);
    let surface = SurfaceId::new(60, 1);
    let window = XResourceId::new(0x2001d0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(8300);
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, delivery))
        .expect("an open coordinator to accept work");

    // The consumer goes away with that work still accepted and never run.
    drop(private);

    // It was promised a consumer, so it is owed an answer. Naming it in a
    // local and letting that local drop would be the same loss as dropping it
    // unnamed.
    // Bounded, so a failure to settle fails this test rather than hanging it.
    assert_eq!(
        delivery_receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("accepted work must be settled when its consumer disappears"),
        XAuthorityClientInputDelivery {
            client,
            delivery,
            // Not TargetGone: the client and its surface are still
            // registered, and it is the authority that stopped. A receipt that
            // is merely terminal is weaker than one that is true.
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        }
    );
}

#[test]
fn one_turn_of_service_is_bounded_while_a_producer_keeps_refilling() {
    const PRIVATE_CLEANUP_RESERVE_FOR_TESTS: usize = 4;
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4096);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // The constructor's own sizing at an ingress capacity of one.
    let budget = 2 + PRIVATE_CLEANUP_RESERVE_FOR_TESTS;

    // A producer that keeps putting work back as fast as the turn takes it.
    // Draining until empty would never end here, and the report would grow
    // without limit.
    let control = private.control_producer();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_for_producer = std::sync::Arc::clone(&stop);
    // SCOPED, because accepting work asks for a live owner and a lease is a
    // borrow of one. The producer thread's own acts are what need it.
    let outcome = std::thread::scope(|scope| {
        let keeper = &service_keeper;
        scope.spawn(move || {
            let mut transaction = 1u64;
            while !stop_for_producer.load(std::sync::atomic::Ordering::Acquire) {
                let _ = control.submit(
                    &keeper.lease(),
                    XAuthorityClientControlCommand {
                        client: XServerFrontendClientId(999),
                        command: XAuthorityControlCommand::FocusSurface {
                            transaction: TransactionId::from_raw(transaction),
                            surface: SurfaceId::new(61, 1),
                        },
                    },
                );
                transaction = transaction.wrapping_add(1);
            }
        });
        let outcome = private.route_pending(&service_keeper.lease());
        stop.store(true, std::sync::atomic::Ordering::Release);
        outcome
    });

    // Stopping on a routing failure is bounded too; the unbounded case is a
    // turn that runs for as long as a producer keeps feeding it.
    if let Ok(ran) = outcome {
        assert!(
            ran.len() <= budget,
            "a turn must not exceed its budget while work keeps arriving: ran {}",
            ran.len()
        );
    }
}

#[test]
fn accepted_control_is_acknowledged_when_its_consumer_goes_away() {
    let namespace = NamespaceId::from_raw(57);
    let client = XServerFrontendClientId(75);
    let surface = SurfaceId::new(62, 1);
    let window = XResourceId::new(0x2001f0, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(4242),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    drop(private);

    // Control has its own acknowledgement contract, so it gets that rather
    // than nothing and rather than a fabricated input receipt.
    let ack = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("accepted control must be acknowledged when its consumer goes");
    assert_eq!(ack.client, client);
    assert_eq!(ack.acknowledgement.transaction, TransactionId::from_raw(4242));
    assert_eq!(
        ack.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected,
        "the authority stopped; the client did not go anywhere"
    );
}

#[test]
fn every_control_run_names_its_own_transaction() {
    let client = XServerFrontendClientId(76);
    let surface = SurfaceId::new(63, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(58), surface, XResourceId::new(0x200200, 1))
        .unwrap();

    // Not FocusSurface. Every control command carries a transaction, so
    // recognising one variant and calling the rest untracked lost the identity
    // of all the others.
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ClearFocus {
                transaction: TransactionId::from_raw(5150),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    let ran = private.route_pending(&service_keeper.lease()).expect("a turn");
    assert!(
        matches!(
            ran.first().map(|run| run.identity),
            Some(crate::PrivateIdentity::Control {
                transaction,
                completion: Some(_),
            }) if transaction == TransactionId::from_raw(5150)
        ),
        "a control that is not FocusSurface still names its transaction"
    );
}

#[test]
fn a_full_acknowledgement_channel_retains_the_obligation() {
    let namespace = NamespaceId::from_raw(59);
    let client = XServerFrontendClientId(77);
    let surface = SurfaceId::new(64, 1);
    let window = XResourceId::new(0x200210, 1);
    // One slot, filled before shutdown, so the terminal ack cannot be sent.
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(777),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // Prefill the only slot.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    // The obligation cannot be discharged, so it is handed back rather than
    // counted and dropped. A caller that is still alive can do something with
    // it; a count in a log cannot.
    let report = private.shutdown();
    assert!(!report.is_settled());
    assert_eq!(report.owed(), 1, "the control is still owed");
    assert!(
        matches!(
            report.pending.first(),
            Some(PrivateOperation::Control(_, _))
        ),
        "and it is the control itself, not a note about it"
    );
    let _ = control_ack_receiver;
}

#[test]
fn an_unresolved_target_is_handed_back_rather_than_attributed() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    // No surface registered, so nothing resolves this target.
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(
            SurfaceId::new(65, 1),
            XAuthorityInputDeliveryId::from_raw(8400),
        ))
        .expect("an open coordinator to accept work");

    let report = private.shutdown();
    assert!(!report.is_settled());
    assert_eq!(
        report.owed(),
        1,
        "a receipt nobody can attribute is not a settlement"
    );
}

#[test]
fn a_retained_handle_settles_once_the_channel_drains() {
    let namespace = NamespaceId::from_raw(60);
    let client = XServerFrontendClientId(78);
    let surface = SurfaceId::new(66, 1);
    let window = XResourceId::new(0x200220, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(910),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // The only slot is taken by something else.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    let mut settlement = private.shutdown();
    assert_eq!(settlement.owed(), 1, "the channel was full");

    // Drain the original, then retry through the HANDLE. It keeps the registry
    // that accepted the work, so nothing else has to be supplied.
    let first = control_ack_receiver.recv().expect("the prefilled ack");
    assert_eq!(first.acknowledgement.transaction, TransactionId::from_raw(1));

    assert_eq!(settlement.retry(), 1, "the obligation is discharged now");
    assert!(settlement.is_settled());

    let owed = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the retained acknowledgement");
    assert_eq!(owed.acknowledgement.transaction, TransactionId::from_raw(910));
    assert_eq!(
        owed.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );

    // Retrying again answers nothing a second time.
    assert_eq!(settlement.retry(), 0);
    assert!(
        control_ack_receiver
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err(),
        "a settled obligation is not answered twice"
    );
}

#[test]
fn two_frontends_with_colliding_client_ids_never_cross_receivers() {
    let namespace = NamespaceId::from_raw(61);
    let client = XServerFrontendClientId(79);
    let surface = SurfaceId::new(67, 1);
    let window = XResourceId::new(0x200230, 1);
    let (first_ack, first_ack_receiver) = sync_channel(8);
    let (second_ack, second_ack_receiver) = sync_channel(8);
    let (first_delivery, _first_delivery_receiver) = channel();
    let (second_delivery, _second_delivery_receiver) = channel();
    // Two authorities, so the two instances are genuinely separate: each
    // derives its own coordinator from the one it owns.
    let first_parts = private_authority();
    let second_parts = private_authority();

    let build = |ack, delivery, parts: (
        sophia_input_authority::AuthorityInstance,
        sophia_input_authority::IssuerHandle,
        sophia_input_authority::SubmitHandle,
    )| {
        let (authority, issuer, submit) = parts;
        let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
        let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: ack,
            input_deliveries: delivery,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
        let (registration, channels) = private.broker.registry.register_client(client).unwrap();
        private
            .broker
            .registry
            .register_surface(client, namespace, surface, window)
            .unwrap();
        (private, registration, channels, service_keeper)
    };
    let (first, _r1, _c1, first_keeper) = build(first_ack, first_delivery, first_parts);
    let (second, _r2, _c2, second_keeper) = build(second_ack, second_delivery, second_parts);

    // The same client id in both, which is ordinary: ids are unique per
    // frontend, not across frontends.
    first
        .control_producer()
        .submit(&first_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1001),
                surface,
            },
        })
        .expect("the first frontend to accept");
    second
        .control_producer()
        .submit(&second_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(2002),
                surface,
            },
        })
        .expect("the second frontend to accept");

    let first_settlement = first.shutdown();
    let second_settlement = second.shutdown();
    assert!(first_settlement.is_settled());
    assert!(second_settlement.is_settled());

    // Each frontend's obligation went to its own receiver, and only its own.
    assert_eq!(
        first_ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1001)
    );
    assert!(first_ack_receiver.try_recv().is_err());
    assert_eq!(
        second_ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(2002)
    );
    assert!(second_ack_receiver.try_recv().is_err());
}

#[test]
fn an_abandoned_handle_leaves_its_work_with_a_durable_owner() {
    let namespace = NamespaceId::from_raw(62);
    let client = XServerFrontendClientId(80);
    let surface = SurfaceId::new(68, 1);
    let window = XResourceId::new(0x200240, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let durable = crate::PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1234),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // The only slot is taken, and the receiver is alive. Congestion, not
    // teardown.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    // The handle is abandoned while still full, which is the case that used to
    // destroy what it held.
    drop(private.shutdown());
    assert_eq!(
        durable.owed().expect("a readable owner"),
        1,
        "an abandoned obligation outlives the handle that held it"
    );

    // Capacity frees afterwards, and the durable owner discharges it against
    // the registry that accepted it.
    let first = control_ack_receiver.recv().expect("the prefilled ack");
    assert_eq!(first.acknowledgement.transaction, TransactionId::from_raw(1));

    assert_eq!(durable.drive().answered, 1, "the durable owner answers it");
    assert_eq!(durable.owed().expect("a readable owner"), 0);

    let owed = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the obligation the abandoned handle left behind");
    assert_eq!(
        owed.acknowledgement.transaction,
        TransactionId::from_raw(1234)
    );
    assert_eq!(
        owed.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );

    // Driving again answers nothing twice, and no other instance was involved.
    assert_eq!(durable.drive().answered, 0);
    assert!(
        control_ack_receiver
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err()
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "the answered credit is free again");
}

#[test]
fn an_unreadable_queue_is_owned_by_something_that_outlives_it() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let durable = crate::PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    drop(private.shutdown());

    // The fact outlives the handle rather than going away as a boolean on
    // something that has gone.
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
}

/// One private instance that accepts a control and immediately shuts down,
/// leaving the acknowledgement owed. Ported from the independent review.
fn review_settlement_queue(
    sender: SyncSender<XAuthorityClientControlAck>,
    keeper: &crate::PrivateServiceOwner,
    transaction: u64,
) -> (
    crate::PrivateSettlement,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
) {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();
    private
        .control_producer()
        .submit(&keeper.lease(), XAuthorityClientControlCommand {
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
        })
        .unwrap();
    (private.shutdown(), registration, channels)
}

fn review_settlement_expected(transaction: u64) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: XServerFrontendClientId(251),
        acknowledgement: XAuthorityControlAck {
            kind: XAuthorityControlKind::ConfigureSurface,
            transaction: TransactionId::from_raw(transaction),
            surface: SurfaceId::new(251, 1),
            outcome: XAuthorityControlOutcome::AuthorityRejected,
        },
    }
}

#[test]
fn review_settlement_two_pending_origins_reverse_retry_cannot_cross() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender_a, receiver_a) = sync_channel(1);
    let (sender_b, receiver_b) = sync_channel(1);
    let owner_of_durable = service_owner(&durable, 16);
    let (prefill_a, _r0a, _c0a) = review_settlement_queue(sender_a.clone(), &owner_of_durable, 9500);
    let (prefill_b, _r0b, _c0b) = review_settlement_queue(sender_b.clone(), &owner_of_durable, 9600);
    assert!(prefill_a.is_settled() && prefill_b.is_settled());
    let (mut a, _ra, _ca) = review_settlement_queue(sender_a.clone(), &owner_of_durable, 9501);
    let (mut b, _rb, _cb) = review_settlement_queue(sender_b.clone(), &owner_of_durable, 9601);
    assert_eq!((a.owed(), b.owed()), (1, 1));

    // Both retained, with identical numeric client and surface identities.
    // Free and retry B first; A's queue stays occupied throughout.
    assert_eq!(
        receiver_b
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9600)
    );
    assert_eq!(b.retry(), 1);
    assert_eq!(a.retry(), 0);
    assert_eq!(a.owed(), 1);
    assert_eq!(
        receiver_b
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9601)
    );
    assert_eq!(
        receiver_a
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9500)
    );
    assert_eq!(a.retry(), 1);
    assert_eq!(
        receiver_a
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9501)
    );
    assert!(a.is_settled() && b.is_settled());
    assert_eq!((a.retry(), b.retry()), (0, 0));
    assert!(receiver_a.try_recv().is_err());
    assert!(receiver_b.try_recv().is_err());
}

#[test]
fn review_settlement_dropped_pending_handle_preserves_accepted_outcome() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(1);
    let owner_of_durable = service_owner(&durable, 16);
    let (prefill, _r0, _c0) = review_settlement_queue(sender.clone(), &owner_of_durable, 9700);
    assert!(prefill.is_settled());
    let (pending, _r1, _c1) = review_settlement_queue(sender.clone(), &owner_of_durable, 9701);
    assert_eq!(pending.owed(), 1);
    assert!(!pending.is_settled());

    // The only unsettled handle is abandoned while the channel is still full.
    drop(pending);

    // The original is intact.
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9700)
    );

    // Adapted as instructed: responsibility for the accepted work did not end
    // with the handle, so the durable owner still holds it and can discharge
    // it now that there is room.
    assert_eq!(durable.owed().expect("a readable owner"), 1);
    assert_eq!(durable.drive().answered, 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("dropping the only unsettled handle must preserve responsibility for 9701"),
        review_settlement_expected(9701)
    );
    assert_eq!(durable.drive().answered, 0);
    assert!(receiver.try_recv().is_err());
}
