// Review controls over failed completion: terminal outcomes, the instance that
// owns them, and the storage reserved before any work is accepted.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn settlement_storage_is_reserved_before_work_is_accepted() {
    // One credit for the whole owner, shared across every instance below.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, receiver) = sync_channel(1);

    // The first settles straight away, filling the acknowledgement channel and
    // freeing its credit.
    let owner_of_durable = service_owner(&durable, 16);
    let (filled, _r0, _c0) = review_settlement_queue(sender.clone(), &owner_of_durable, 9800);
    assert!(filled.is_settled());
    assert_eq!(durable.reserved().expect("a readable owner"), 0);

    // The second cannot settle, because the channel is now full, so it keeps
    // the only credit.
    let (owed, _r1, _c1) = review_settlement_queue(sender.clone(), &owner_of_durable, 9801);
    assert_eq!(owed.owed(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // A third cannot even be accepted: the storage that would have to hold its
    // work if abandoned is spoken for. Refusing here costs a producer only
    // work it was never told had been taken.
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let service_keeper = service_owner(&durable, 16);
    let third = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    assert!(
        third
            .control_producer()
            .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
                client: XServerFrontendClientId(251),
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: TransactionId::from_raw(9802),
                    surface: SurfaceId::new(251, 1),
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 60,
                    },
                },
            })
            .is_err(),
        "work must not be accepted without storage to answer it"
    );

    // The second is still answerable: nothing was destroyed to make room.
    drop(owed);
    assert_eq!(durable.owed().expect("a readable owner"), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9800)
    );
    assert_eq!(durable.drive().answered, 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the accepted work survives"),
        review_settlement_expected(9801)
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "the answered credit is free again");
}

#[test]
fn a_failed_instance_hands_over_its_queue_not_a_tally() {
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

    // The queue and its registry are retained under the owner, so the failure
    // belongs to something that can be asked about it later rather than to a
    // number that cannot.
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
}

#[test]
fn review_owner_saturation_cannot_discard_two_already_accepted_controls() {
    // The independent negative, repaired as instructed: with reservation
    // before acceptance, either a submission is refused with its payload
    // before being accepted, or everything accepted settles exactly once.
    // Counting a loss is not an allowed outcome.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, receiver) = sync_channel(1);
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let service_keeper = service_owner(&durable, 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender.clone(),
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
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();

    let submit = |transaction: u64| {
        private
            .control_producer()
            .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
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
    };

    // Fill the acknowledgement channel with something real first.
    sender
        .try_send(review_settlement_expected(9900))
        .expect("the empty slot");

    let mut accepted = Vec::new();
    for transaction in [9901u64, 9902] {
        match submit(transaction) {
            Ok(_) => accepted.push(transaction),
            // Refused BEFORE acceptance, carrying its own command back. The
            // producer keeps work it was never told had been taken.
            Err((crate::AdmissionRefusal::Saturated, returned)) => {
                assert_eq!(returned.command.transaction(), TransactionId::from_raw(transaction));
            }
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
    }
    assert!(!accepted.is_empty(), "at least one was accepted");

    drop(private.shutdown());
    let owed_before = durable.owed().expect("a readable owner");

    // Drain the prefill, then drive.
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9900)
    );
    let mut settled = Vec::new();
    for _ in 0..4 {
        let _ = durable.drive();
        while let Ok(ack) = receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            settled.push(ack.acknowledgement.transaction.raw());
        }
    }

    // Everything accepted was answered, exactly once each. Nothing was
    // counted off as lost to make room.
    let mut expected: Vec<_> = accepted.clone();
    expected.sort_unstable();
    settled.sort_unstable();
    assert_eq!(
        settled, expected,
        "every accepted control is answered exactly once"
    );
    assert_eq!(durable.owed().expect("a readable owner"), 0);
    let _ = owed_before;
}

#[test]
fn a_failed_instances_queue_can_still_be_answered() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(4);
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let service_keeper = service_owner(&durable, 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
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
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(9950),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        })
        .expect("the shared admission to accept control");

    // The queue becomes unreadable with that work still in it.
    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    drop(private.shutdown());
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);

    // Retaining the queue was for this. A poisoned lock stays poisoned, but
    // the obligations behind it are intact and still owed, so they can be
    // answered against the registry that accepted them. A tally could have
    // been counted and never discharged.
    assert_eq!(durable.recover_failed().expect("a readable owner"), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the work the failed instance had accepted"),
        review_settlement_expected(9950)
    );
    assert_eq!(durable.failed_instances().expect("a readable owner"), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "its credit is free again");
}

#[test]
fn recovering_a_failed_queue_takes_the_completion_record_before_it_answers() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(4);
    let surface = SurfaceId::new(253, 1);
    let client = XServerFrontendClientId(253);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let service_keeper = service_owner(&durable, 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
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
        .register_surface(
            client,
            NamespaceId::from_raw(253),
            surface,
            XResourceId::new(0x200253, 1),
        )
        .unwrap();
    // Kept past the frontend, which shutdown consumes. The registry is what
    // knows whether an operation still has an owner able to publish for it.
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");
    private
        .control_producer()
        .submit(&service_keeper.lease(), configure(client, surface, 9970))
        .expect("the shared admission to accept control");
    assert_eq!(
        completion.outstanding(),
        Some(1),
        "accepted, so a record answers for it"
    );

    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();
    drop(private.shutdown());
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
    assert_eq!(
        completion.outstanding(),
        Some(1),
        "the poisoned early return hands nothing over, so the record is still live"
    );

    assert_eq!(durable.recover_failed().expect("a readable owner"), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the work the failed instance had accepted"),
        completion_ack(
            configure(client, surface, 9970),
            XAuthorityControlOutcome::AuthorityRejected
        )
    );
    // The point of the test. Emitting an outcome while a record that can also
    // publish one is still held gives the operation two owners, and the first
    // acknowledgement has already gone by the time anyone looks.
    assert_eq!(
        completion.outstanding(),
        Some(0),
        "recovery took the record before it published"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn a_failure_slot_is_reserved_before_an_instance_is_exposed() {
    // Room for one failed instance across the whole owner.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();

    let service_keeper = service_owner(&durable, 16);
    let first = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("the only failure slot: {refusal:?}"));

    // A second cannot be built: if it failed, there would be nowhere to hand
    // its queue. Refusing construction costs a caller an instance it never
    // had; refusing the transfer afterwards would drop responsibility for one
    // that existed and accepted work.
    let (second_delivery, _second_delivery_receiver) = channel();
    let (second_authority, second_issuer, second_submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: second_delivery,
            authority: second_authority,
            issuer: second_issuer,
            submit: second_submit,
        },
        &service_keeper,
    )
        .is_err(),
        "an instance without a failure slot must not be exposed"
    );

    // The first closes without failing, so its slot returns and another can
    // be built.
    drop(first);
    let (third_delivery, _third_delivery_receiver) = channel();
    let (third_authority, third_issuer, third_submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: third_delivery,
            authority: third_authority,
            issuer: third_issuer,
            submit: third_submit,
        },
        &service_keeper,
    )
        .is_ok(),
        "a slot returns when its instance closes without failing"
    );
}

#[test]
fn review_failed_empty_first_instance_cannot_evict_later_accepted_work() {
    // The independent negative, adapted as instructed: with reservation before
    // exposure, B is refused construction rather than being exposed, accepting
    // work, and then having nowhere to hand its queue.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();

    // A is built, accepts nothing, and fails. Its slot is spent on a failure
    // that carries no credit, which is why failure slots are counted apart
    // from credits.
    let service_keeper = service_owner(&durable, 16);
    let empty = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("the only failure slot: {refusal:?}"));
    let admission = std::sync::Arc::clone(&empty.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning A");
    })
    .join();
    drop(empty.shutdown());
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "A accepted nothing");

    // B is refused before exposure, so it never accepts work that would be
    // evicted. This is the whole difference: a refusal here costs a caller an
    // instance it never had.
    let (b_delivery, _b_delivery_receiver) = channel();
    let (b_authority, b_issuer, b_submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: b_delivery,
            authority: b_authority,
            issuer: b_issuer,
            submit: b_submit,
        },
        &service_keeper,
    )
        .is_err(),
        "B must not be exposed without room to hand over its queue"
    );

    // Resolving A returns the slot, and B can then be built.
    assert_eq!(durable.recover_failed().expect("a readable owner"), 0, "A had accepted nothing");
    assert_eq!(durable.failed_instances().expect("a readable owner"), 0);
    let (c_delivery, _c_delivery_receiver) = channel();
    let (c_authority, c_issuer, c_submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: c_delivery,
            authority: c_authority,
            issuer: c_issuer,
            submit: c_submit,
        },
        &service_keeper,
    )
        .is_ok(),
        "a resolved failure returns its slot"
    );
}

#[test]
fn review_credit_control_writer_pending_retains_credit_and_refuses_next() {
    // The independent negative: a control handed to a client writer is not an
    // acknowledgement, so its credit must not be freed when the consumer takes
    // it.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
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
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(10201),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        })
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The consumer routes it to the client's writer queue. Nothing has
    // acknowledged it.
    assert_eq!(private.route_pending(&service_keeper.lease()).expect("a turn").len(), 1);
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        1,
        "enqueueing to a writer is not an acknowledgement"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "nothing has reached a terminal outcome"
    );

    // And the capacity is genuinely still held: the next admission is refused.
    let refused = private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(10202),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        });
    assert!(
        refused.is_err(),
        "capacity held by unanswered work must not admit more"
    );

    // The command really did reach the client's queue.
    assert!(channels.control.try_recv().is_ok());
}

#[test]
fn review_terminal_recorded_then_observed_reclaims_exactly_once() {
    let namespace = NamespaceId::from_raw(63);
    let client = XServerFrontendClientId(81);
    let surface = SurfaceId::new(69, 1);
    let window = XResourceId::new(0x200250, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(10300);
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    assert_eq!(private.route_pending(&service_keeper.lease()).expect("a turn").len(), 1);
    // Routed to the client, not yet delivered: the ledger still holds a
    // ticket for it, so the credit stays with the work.
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The delivery reaches a terminal outcome and that outcome is observed,
    // which is when the ledger stops tracking it. Both halves matter: a
    // recorded terminal nobody has seen is not yet an answer.
    private
        .broker
        .registry
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert_eq!(
        private.reclaim_settled(),
        0,
        "recorded but unobserved is not yet answered"
    );
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .observe(XAuthorityClientInputDelivery {
                client,
                delivery,
                outcome: XAuthorityInputDeliveryOutcome::Flushed,
            }),
        "the outcome is observed"
    );

    assert_eq!(private.reclaim_settled(), 1, "answered, so reclaimed");
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn review_terminal_unreadable_recovery_cannot_prove_live_delivery_settled() {
    let namespace = NamespaceId::from_raw(64);
    let client = XServerFrontendClientId(82);
    let surface = SurfaceId::new(70, 1);
    let window = XResourceId::new(0x200260, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(10400)))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending(&service_keeper.lease()).expect("a turn").len(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The ledger becomes unreadable while that delivery is still live.
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    // Silence is not completion. Treating an unreadable ledger as every
    // delivery having ended is the most dangerous reading available, because
    // it frees whatever they were holding.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "an unreadable ledger must not free a live credit"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
}

#[test]
fn independent_terminal_kept_shutdown_handle_reclaims_late_completion_once() {
    let namespace = NamespaceId::from_raw(65);
    let client = XServerFrontendClientId(83);
    let surface = SurfaceId::new(71, 1);
    let window = XResourceId::new(0x200270, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let delivery = XAuthorityInputDeliveryId::from_raw(10500);
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending(&service_keeper.lease()).expect("a turn").len(), 1);

    // The instance goes away with that work routed and unanswered.
    let mut settlement = private.shutdown();
    assert_eq!(
        settlement.outstanding(),
        1,
        "routed work is carried, not destroyed with the instance"
    );
    assert!(!settlement.is_settled());
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // It can still finish afterwards, and the handle notices.
    settlement
        .origin
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert!(settlement.origin.input_recovery.observe(
        XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::Flushed,
        }
    ));
    assert_eq!(settlement.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(settlement.is_settled());
}

#[test]
fn independent_terminal_dropped_shutdown_handle_retains_late_completion_reclamation() {
    let namespace = NamespaceId::from_raw(66);
    let client = XServerFrontendClientId(84);
    let surface = SurfaceId::new(72, 1);
    let window = XResourceId::new(0x200280, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let delivery = XAuthorityInputDeliveryId::from_raw(10600);
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending(&service_keeper.lease()).expect("a turn").len(), 1);

    let origin = private.broker.registry.clone();
    // Nothing is owed -- the admission queue was empty -- so the handle's own
    // Drop used to return before it reached the routed work and destroy it.
    let settlement = private.shutdown();
    assert_eq!(settlement.owed(), 0);
    assert_eq!(settlement.outstanding(), 1);
    drop(settlement);

    assert_eq!(
        durable.outstanding().expect("a readable owner"),
        1,
        "routed work outlives an abandoned handle, however little else is owed"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "and keeps the credit it already had");

    // Driving before it finishes releases nothing: the work is still live, and
    // a drive is not a terminal outcome.
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.outstanding().expect("a readable owner"), 1, "still waiting on a real outcome");
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // It finishes for real, and driving the owner reclaims it once.
    origin
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert!(origin.input_recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::Flushed,
    }));

    // Reclaiming is progress even though nothing was acknowledged. A single
    // count would report this drive as having achieved nothing.
    let progress = durable.drive();
    assert_eq!(progress.answered, 0, "nothing was owed an acknowledgement");
    assert_eq!(progress.reclaimed, 1, "but a credit was released");
    assert!(progress.made_progress());
    assert_eq!(durable.outstanding().expect("a readable owner"), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "released once, on a real outcome");
    let again = durable.drive();
    assert!(!again.made_progress(), "and not a second time");
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn a_control_completion_closes_only_on_a_real_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11001),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = accepted(&registry, command);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);

    // A delivered acknowledgement retires the record.
    let (delivered, delivered_receiver) = sync_channel(4);
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: delivered,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11001),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    channels
        .send_ack_for(client, ack, Some(token))
        .expect("a free channel to publish");
    assert!(delivered_receiver.try_recv().is_ok());
    assert_eq!(registry.outstanding().expect("a readable registry"), 0, "a delivered ack retires it");
}

#[test]
fn a_full_channel_records_the_acknowledgement_rather_than_the_command() {
    // Registry contract only. Nothing here applies a command, so nothing here
    // shows an effect happening; what an applied effect looks like is
    // a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied.
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11002),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = accepted(&registry, command);

    // One slot, already taken.
    let (full, full_receiver) = sync_channel(1);
    full.try_send(XAuthorityClientControlAck {
        client,
        acknowledgement: XAuthorityControlAck {
            kind: XAuthorityControlKind::ConfigureSurface,
            transaction: TransactionId::from_raw(1),
            surface,
            outcome: XAuthorityControlOutcome::Delivered,
        },
    })
    .expect("the empty slot");
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: full,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11002),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    assert!(
        channels.send_ack_for(client, ack, Some(token)).is_err(),
        "a full channel reports failure to the writer"
    );

    // The outcome is retained, not the command. Where this is reached for
    // real the effect has already happened,
    // so this is republished later and never re-run.
    assert_eq!(registry.owed().expect("a readable registry"), 1);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);

    // Draining lets it publish, once.
    assert!(full_receiver.try_recv().is_ok());
    let mut republished = Vec::new();
    let delivered = registry.publish_owed_with(|acknowledgement| {
        republished.push(*acknowledgement);
        ControlPublication::Delivered
    });
    assert_eq!(delivered, 1);
    assert_eq!(republished.len(), 1);
    assert_eq!(
        republished[0].acknowledgement.transaction,
        TransactionId::from_raw(11002)
    );
    assert_eq!(registry.owed().expect("a readable registry"), 0);
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "and not a second time"
    );

    // A retry that cannot publish keeps the outcome rather than consuming it.
    let held = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&held, command);
    assert_eq!(
        held.publish_with(
            token,
            XAuthorityClientControlAck {
                client,
                acknowledgement: ack,
            },
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    assert_eq!(held.publish_owed_with(|_| ControlPublication::Retained), 0);
    assert_eq!(held.owed().expect("a readable registry"), 1, "a failed retry does not consume the outcome");
}

#[test]
fn a_gone_receiver_is_not_a_published_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11003),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = accepted(&registry, command);

    // The receiver is dropped, which send_ack has always reported as Ok.
    let (gone, gone_receiver) = sync_channel(4);
    drop(gone_receiver);
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: gone,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11003),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    // Ordinary behaviour is unchanged: the writer is not failed for this.
    channels
        .send_ack_for(client, ack, Some(token))
        .expect("a gone receiver is tolerated by the writer");

    // But nothing was published, so the record is not closed. Treating the Ok
    // as publication would mark work complete whose acknowledgement nobody
    // received.
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        1,
        "a gone receiver published nothing"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);
}

#[test]
fn a_cancellation_edge_does_not_call_a_partly_applied_command_unexecuted() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = |transaction| XAuthorityClientControlCommand {
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
    };
    let queued = accepted(&registry, command(11004));
    let started = accepted(&registry, command(11005));
    let _ = queued;
    assert_eq!(
        registry.claim_execution(started),
        crate::ControlExecutionClaim::Claimed
    );

    // Neither has an established outcome, so neither has an acknowledgement to
    // publish. A retry that produced one would be inventing the receipt these
    // records exist to avoid inventing.
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "a record with no outcome owes no acknowledgement"
    );
    assert_eq!(registry.outstanding().expect("a readable registry"), 2, "and both are still held");

    let cancellation = registry.cancel_unfinished();

    // The one that never started can be cancelled truthfully.
    assert_eq!(cancellation.cancellable.len(), 1);
    assert_eq!(
        cancellation.cancellable[0].1.command.transaction(),
        TransactionId::from_raw(11004)
    );
    // The one that had begun is not reported as unexecuted, because the
    // runtime may already have changed. It is retained until something
    // establishes what happened.
    assert_eq!(cancellation.indeterminate, 1);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);
}

/// Build a private frontend with a client and one surface registered.
#[cfg(unix)]
fn private_with_client(
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    keeper: &crate::PrivateServiceOwner,
    client: XServerFrontendClientId,
    surface: SurfaceId,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteChannels,
    XServerFrontendClientRouteRegistration,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (delivery_sender, delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: acknowledgements,
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
            NamespaceId::from_raw(252),
            surface,
            XResourceId::new(0x200252, 1),
        )
        .unwrap();
    // The registration and the delivery receiver are returned rather than
    // leaked: dropping either is a cancellation edge, and a test that hid one
    // would be proving a different thing from the one it names.
    (private, channels, registration, delivery_receiver)
}
