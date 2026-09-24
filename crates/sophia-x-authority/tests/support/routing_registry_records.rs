// The registry's own records: what a producer's row owes, what a writer reads
// from it, and what an unreadable registry answers instead of an empty list.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn transferring_an_unexecuted_command_moves_its_credit_rather_than_freeing_it() {
    let client = XServerFrontendClientId(295);
    let surface = SurfaceId::new(295, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(8);
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    // One operation reaches a writer and stays there.
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 75001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // A second is accepted while the client is still there, and the client
    // goes before it can be routed. It keeps its record and never claims
    // execution, because routing refuses before its first effect.
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 75002))
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved().expect("a readable owner"), 2);
    drop(registration);
    assert!(matches!(
        private.route_pending(&owner_of_durable.lease()),
        Err(XServerFrontendRouteError::UnknownClient { .. })
    ));

    // Closing transfers the unexecuted one to a single new owner. Ownership
    // moving is not the operation terminating: the identity must leave
    // `outstanding` with the command, or one watcher reads the record's
    // absence as completion and frees the credit that settling the transfer
    // will free again.
    let mut report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert_eq!(carried, vec![75002], "handed on, with one owner");
    assert_eq!(durable.reserved().expect("a readable owner"), 2, "and nothing freed by the move");

    assert_eq!(report.retry(), 1, "the transfer settles, once");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(75002)
    );
    // And the one that reached a writer never began a step of its own, which
    // its own reports establish, so the settlement that outlived the instance
    // applies that proof and releases its credit too. Each is released once,
    // for a different reason: one was handed on and answered, the other is
    // known to have had no effect.
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "each released exactly once");
    assert!(acks.try_recv().is_err(), "and exactly one outcome");
    let _ = channels;
}

#[test]
fn a_claim_answers_for_every_state_a_record_can_be_in() {
    let client = XServerFrontendClientId(309);
    let surface = SurfaceId::new(309, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let other = crate::ControlCompletionRegistry::with_capacity(1).expect("an unused origin");
    let command = |transaction| configure(client, surface, transaction);

    // Reserved: still the producer's, so no part of the instance may act.
    let reservation = registry.register(command(22000)).expect("a fresh registry");
    for claim in [
        registry.claim_execution(reservation),
        registry.resume_execution(reservation),
    ] {
        assert_eq!(
            claim,
            crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotAccepted),
            "a reservation has not been handed over yet"
        );
    }
    assert!(registry.release_reservation(reservation));

    // Accepted: a beginning, and not a continuation of anything.
    let fresh = accepted(&registry, command(22001));
    assert_eq!(
        registry.resume_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotStarted),
        "a writer that finds it unclaimed means the first effect was skipped"
    );
    assert_eq!(
        registry.claim_execution(fresh),
        crate::ControlExecutionClaim::Claimed
    );

    // Applying: a continuation, and not a second beginning.
    assert_eq!(
        registry.resume_execution(fresh),
        crate::ControlExecutionClaim::Resumed
    );
    assert_eq!(
        registry.claim_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyApplying),
        "claiming twice would apply it twice"
    );

    // Answered: acting now would act after the answer.
    assert_eq!(
        registry.publish_with(
            fresh,
            completion_ack(command(22001), XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    for claim in [
        registry.claim_execution(fresh),
        registry.resume_execution(fresh),
    ] {
        assert_eq!(
            claim,
            crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyAnswered)
        );
    }

    // Gone: another owner holds it.
    let handed_on = accepted(&registry, command(22002));
    assert!(registry.discard(handed_on));
    assert_eq!(
        registry.claim_execution(handed_on),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoLongerHeld)
    );

    // Foreign: this registry never accepted it.
    assert_eq!(
        other.claim_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Foreign)
    );

    // None of those permit an effect; the two that are permission do.
    for refused in [
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotStarted),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyApplying),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyAnswered),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoLongerHeld),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Foreign),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotAccepted),
    ] {
        assert!(!refused.permits_effects(), "{refused:?}");
    }
    assert!(crate::ControlExecutionClaim::Claimed.permits_effects());
    assert!(crate::ControlExecutionClaim::Resumed.permits_effects());
    assert!(crate::ControlExecutionClaim::Ungoverned.permits_effects());
}

#[test]
fn a_poisoned_registry_permits_no_effect() {
    let client = XServerFrontendClientId(310);
    let surface = SurfaceId::new(310, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(client, surface, 23001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            poisoner.publish_owed_with(|_| panic!("poisoning the registry"));
        })
        .join()
        .is_err()
    );

    // A registry that cannot be read cannot establish who owns this, and
    // proceeding on silence is how an effect happens beside an answer.
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable)
    );
    assert_eq!(
        registry.resume_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable)
    );
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::Unavailable)
    );
}

#[test]
fn a_registration_with_no_registry_to_answer_to_permits_no_effect() {
    let client = XServerFrontendClientId(311);
    let surface = SurfaceId::new(311, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 24001));

    // A writer holding a registration whose registry it cannot reach cannot
    // establish who owns the outcome, so it may not produce one.
    let orphaned = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(1).0,
        completion: None,
    };
    assert_eq!(
        orphaned.resume_execution(Some(token)),
        ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable)
    );
    // An operation with no registration at all is ungoverned, exactly as the
    // ordinary path has always been.
    assert_eq!(
        orphaned.resume_execution(None),
        ControlExecutionClaim::Ungoverned
    );
}

#[test]
fn a_focus_control_that_cannot_claim_disturbs_no_one_elses_focus() {
    let focused = XServerFrontendClientId(312);
    let claimant = XServerFrontendClientId(313);
    let focused_surface = SurfaceId::new(312, 1);
    let claimant_surface = SurfaceId::new(313, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, focused, focused_surface);
    let (claimant_registration, claimant_channels) = private
        .broker
        .registry
        .register_client(claimant)
        .expect("a second client");
    private
        .broker
        .registry
        .register_surface(
            claimant,
            NamespaceId::from_raw(252),
            claimant_surface,
            XResourceId::new(0x200253, 1),
        )
        .expect("a second surface");

    // The first client holds focus.
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(25001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());

    // A second client's focus command whose execution cannot be claimed.
    // Routing is the first authoritative effect for focus: it sends FocusOut
    // to whoever held focus and moves the focused surface before any writer
    // runs, so a claim taken later would leave those behind a record still
    // calling the operation unexecuted.
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let command = XAuthorityClientControlCommand {
        client: claimant,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(25002),
            surface: claimant_surface,
        },
    };
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed,
        "already claimed, so routing cannot claim it again"
    );

    assert!(matches!(
        private
            .broker
            .registry
            .route_control_with_completion(command, Some(token)),
        Err(XServerFrontendRouteError::ControlNotClaimable { .. })
    ));
    assert!(
        held_channels.control.try_recv().is_err(),
        "the client that holds focus is not told it lost it"
    );
    assert!(
        claimant_channels.control.try_recv().is_err(),
        "and the command that could not be claimed was not enqueued"
    );
    drop(claimant_registration);
}

#[test]
fn a_producer_is_told_which_refusal_it_met() {
    let client = XServerFrontendClientId(314);
    let surface = SurfaceId::new(314, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // A client whose control writer has stopped: nothing is left to execute
    // this, so retrying is not the advice. Saturation would have said try
    // again.
    private.broker.registry.mark_control_writer_gone(client);
    assert!(matches!(
        private
            .control_producer()
            .submit(&owner_of_durable.lease(), configure(client, surface, 26001)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(26001)
    ));

    // Exhausted identities: terminal, and not the same answer as a full
    // queue. The counter is placed at its end from the test, not from a
    // setter in production src.
    let elsewhere = XServerFrontendClientId(315);
    let (_elsewhere_registration, _elsewhere_channels) =
        private.broker.registry.register_client(elsewhere).unwrap();
    registry.inner.lock().unwrap().next_incarnation = u64::MAX;
    assert!(matches!(
        private
            .control_producer()
            .submit(&owner_of_durable.lease(), configure(elsewhere, surface, 26002)),
        Err((crate::AdmissionRefusal::Exhausted, _))
    ));

    // An unreadable registry established nothing at all.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );
    assert!(matches!(
        private
            .control_producer()
            .submit(&owner_of_durable.lease(), configure(elsewhere, surface, 26003)),
        Err((crate::AdmissionRefusal::Unavailable, _))
    ));
}

#[test]
fn a_reservation_is_its_producers_until_the_instance_accepts_it() {
    let client = XServerFrontendClientId(316);
    let surface = SurfaceId::new(316, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // A producer paused between reserving and being accepted. This is what a
    // real submit looks like at that instant.
    let command = configure(client, surface, 27001);
    let reservation = registry.register(command).expect("a fresh registry");

    // Closing must not answer for it. The producer still holds the command
    // and is about to be told it was never taken; an outcome published here
    // would be a second owner answering for work its owner keeps.
    let mut report = private.shutdown();
    let cancelled: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        !cancelled.contains(&27001),
        "a reservation is not accepted work and is not the instance's to hand on"
    );
    assert_eq!(report.retry(), 0);
    assert!(
        acks.try_recv().is_err(),
        "and nothing answers for a command its producer still owns"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "nor is a credit released that was never taken");

    // The producer resumes, is refused, and takes its command back. The
    // reservation goes with it, leaving nothing behind to answer later.
    assert!(registry.release_reservation(reservation));
    assert_eq!(registry.outstanding().expect("a readable registry"), 0);
}

#[test]
fn an_acknowledgement_the_record_refuses_never_reaches_the_receiver() {
    let client = XServerFrontendClientId(317);
    let surface = SurfaceId::new(317, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 28001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // An acknowledgement naming a different operation. Validating after the
    // send would have refused nothing: it would already be at the receiver.
    assert!(
        channels
            .send_ack_for(
                client,
                XAuthorityControlAck {
                    kind: XAuthorityControlKind::ConfigureSurface,
                    transaction: TransactionId::from_raw(28999),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                Some(token),
            )
            .is_err(),
        "the record refuses it"
    );
    assert!(
        acks.try_recv().is_err(),
        "and nothing was sent, which is what refusing has to mean"
    );

    // Its own outcome is established, and a contradicting one cannot follow
    // it out to the receiver either.
    channels
        .send_ack_for(
            client,
            completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
            Some(token),
        )
        .expect("its own outcome publishes");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );

    // Retired now, so a duplicate is refused rather than sent again.
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(token),
            )
            .is_err(),
        "a retired registration publishes nothing"
    );
    assert!(acks.try_recv().is_err(), "exactly one terminal publication");
}

#[test]
fn a_contradicting_outcome_is_refused_before_it_is_sent() {
    let client = XServerFrontendClientId(318);
    let surface = SurfaceId::new(318, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    // One slot, filled, so the first outcome is established and unpublished.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 29001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(token),
            )
            .is_err(),
        "a full channel is reported to the writer"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);

    // Drain, then contradict what was established. The effect that happened
    // does not become a different effect, and the receiver never sees a claim
    // that it did.
    assert!(acks.try_recv().is_ok());
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::AuthorityRejected).acknowledgement,
                Some(token),
            )
            .is_err(),
        "the established outcome stands"
    );
    assert!(
        acks.try_recv().is_err(),
        "and the contradiction was never sent"
    );

    // The retry publishes the outcome that actually happened, once.
    let mut published = Vec::new();
    assert_eq!(
        registry.publish_owed_with(|owed| {
            published.push(*owed);
            ControlPublication::Delivered
        }),
        1
    );
    assert_eq!(
        published[0].acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
}

#[test]
fn a_real_submit_paused_before_acceptance_is_not_answered_by_a_close() {
    let client = XServerFrontendClientId(319);
    let surface = SurfaceId::new(319, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Stall a real submit between reserving its record and taking a credit,
    // which is where it stops being nothing and starts being something an
    // instance could mistake for its own.
    let held = durable.inner.lock().unwrap();
    let (finished, refusals) = sync_channel(1);
    let producer = private.control_producer();
    let submitting = std::thread::spawn(move || {
        let _ = finished.send(producer.submit(&owner_of_durable.lease(), configure(client, surface, 83001)));
    });
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while registry.outstanding().expect("a readable registry") == 0 && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    let reserved_while_stalled = registry.outstanding().expect("a readable registry");

    // The instance closes while that reservation exists. It is not accepted
    // work, so it is not the instance's to answer or hand on.
    let mut report = private.settle_accepted();
    // Observed under the barrier and asserted after it. An assertion here
    // would unwind with the barrier still held, and the report's own Drop
    // reacquires the same lock.
    let cancelled: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    let answered_while_stalled = acks.try_recv().is_ok();
    drop(held);

    // The producer resumes, is refused, and takes its own command back.
    let refused = refusals
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the producer to be answered");
    submitting.join().expect("the producer thread");

    assert_eq!(
        reserved_while_stalled, 1,
        "the submit had reserved its record and had not been accepted"
    );
    assert!(
        cancelled.is_empty(),
        "a close does not take a command its producer still owns"
    );
    assert!(
        !answered_while_stalled,
        "and nothing answers for a transaction that was never accepted"
    );
    let Err((refusal, returned)) = refused else {
        panic!("a closed instance accepts nothing");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::ConsumerGone);
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(83001),
        "the caller keeps the command it was never told had been taken"
    );
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "and the reservation went back with it, leaving nothing to answer later"
    );
    assert_eq!(report.retry(), 0);
    assert!(acks.try_recv().is_err(), "exactly no outcomes");
}

#[test]
fn a_producer_reserves_nothing_for_a_client_that_has_gone() {
    let client = XServerFrontendClientId(320);
    let surface = SurfaceId::new(320, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // The client goes. Nothing is left to execute work for it, so a producer
    // is refused before anything is reserved: the seal ledger is bounded and
    // can forget a client with nothing left to protect, and this is the
    // answer that does not depend on it.
    drop(registration);
    assert!(matches!(
        private
            .control_producer()
            .submit(&owner_of_durable.lease(), configure(client, surface, 84001)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(84001)
    ));
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "nothing was reserved, so nothing is owed and no credit was taken"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn no_outcome_is_published_for_a_record_its_producer_still_owns() {
    let client = XServerFrontendClientId(321);
    let surface = SurfaceId::new(321, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 30001);
    let reservation = registry.register(command).expect("a fresh registry");

    // Nothing has been handed over, so there is no outcome of it to publish.
    // Checked before the emission: a verdict reached afterwards would leave
    // the acknowledgement at the receiver whatever it then decided.
    assert_eq!(
        registry.publish_with(
            reservation,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| panic!("a reservation must not reach the emission"),
        ),
        Err(crate::ControlPublicationRefusal::NotAccepted)
    );
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(reservation),
            )
            .is_err(),
        "and the writer path refuses it too"
    );
    assert!(acks.try_recv().is_err(), "nothing was sent");
    assert_eq!(registry.outstanding().expect("a readable registry"), 1, "and the reservation is untouched");
}

#[test]
fn a_publication_that_fails_leaves_the_reservation_with_its_producer() {
    let client = XServerFrontendClientId(322);
    let surface = SurfaceId::new(322, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let producer = private.control_producer();

    // Fill the shared order until it refuses.
    let mut accepted_count = 0;
    let mut refused = None;
    for transaction in 31000..31100 {
        match producer.submit(&owner_of_durable.lease(), configure(client, surface, transaction)) {
            Ok(_) => accepted_count += 1,
            Err(refusal) => {
                refused = Some(refusal);
                break;
            }
        }
    }
    let Some((refusal, returned)) = refused else {
        panic!("a bounded order refuses eventually");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::Saturated);

    // The queue entry could not be published, so the handover was rolled back
    // and the reservation released with the command. A promotion recorded
    // beside the publication would have left the record accepted, and
    // releasing a reservation does not remove an accepted record: it would
    // have stayed here answering for work this caller was handed back.
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        accepted_count,
        "only what was accepted has a record"
    );
    assert_eq!(
        returned.command.transaction().raw(),
        31000 + accepted_count as u64,
        "and the caller keeps the one that was refused"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), accepted_count);
}

#[test]
fn nothing_is_admitted_without_the_handover_it_was_accepted_for() {
    let client = XServerFrontendClientId(323);
    let surface = SurfaceId::new(323, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);

    // A handover that cannot be prepared -- an unreadable registry, a record
    // already gone -- means the queue entry must not be published either.
    // Admitting anyway would leave the instance owning work whose record says
    // its producer still owns it, which is the whole reason these are one
    // transaction.
    let refused = private.admission.accept_with(
        crate::ReadyClass::Control,
        PrivateOperation::Control(configure(client, surface, 32001), None),
        || None,
    );
    let Err((refusal, returned)) = refused else {
        panic!("no handover, no acceptance");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::Unavailable);
    assert!(
        matches!(
            returned,
            PrivateOperation::Control(control, _)
                if control.command.transaction() == TransactionId::from_raw(32001)
        ),
        "the payload goes back to its caller"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "and so does the credit it reserved");
    assert!(
        private.route_pending(&owner_of_durable.lease()).expect("a turn").is_empty(),
        "and nothing was queued"
    );
}

#[test]
fn stopping_one_writer_does_not_leave_the_others_running() {
    let client = XServerFrontendClientId(324);
    let surface = SurfaceId::new(324, 1);
    let state = writer_runtime(surface);
    let (routes, control) = sync_channel(4);
    let (acknowledgements, _acks) = sync_channel(4);
    let (control_writer, _peer) = writer_start(
        None,
        &state,
        control,
        None,
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    let control_stop = control_writer.stop.clone();

    // An input writer that fails the moment it is joined, ahead of the others.
    let failing_stop = Arc::new(AtomicBool::new(false));
    let failing = X11InputEventWriter {
        stop: failing_stop.clone(),
        thread: std::thread::spawn(|| {
            Err(X11SetupSocketError::new("an input writer that failed"))
        }),
    };
    let mut writers = X11ClientWriters {
        input: Some(failing),
        control: Some(control_writer),
        protocol: None,
        drain: None,
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    };

    let shutdown = writers.shut_down();
    assert!(shutdown.outcome.is_err(), "the first failure is reported");
    assert_eq!(
        shutdown.joined, 2,
        "and every writer is waited for, not left detached behind the failure"
    );
    // And the ones behind it were stopped and joined anyway. Returning on the
    // first failure left them running, never told to stop, holding a stream
    // and a route queue.
    assert!(
        control_stop.load(Ordering::Acquire),
        "every writer is told to stop, whatever an earlier one did"
    );
    assert!(
        writers.control.is_none() && writers.input.is_none(),
        "and every writer is joined, not abandoned"
    );
    let again = writers.shut_down();
    assert!(again.outcome.is_ok() && again.joined == 0,
        "shutting down twice finds nothing left to do");
    let _ = routes;
}

#[test]
fn a_registration_lost_while_its_writer_is_there_abandons_nothing() {
    let client = XServerFrontendClientId(325);
    let surface = SurfaceId::new(325, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // One reaches a writer's queue and claims execution; one never leaves the
    // shared order.
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 33001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 33002))
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved().expect("a readable owner"), 2);

    // A writer is running for this client. The registration going is not
    // proof that it stopped: it may have claimed this operation and still be
    // inside it, about to establish an outcome that abandoning it here would
    // then refuse.
    registry.writer_started(client);
    drop(registration);
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty(),
        "nothing is owed a cleanup while something could still answer"
    );
    registry.writer_started(client);
    let reconciled = registry.reconcile_client(client);
    assert!(reconciled.readable);
    assert_eq!(reconciled.abandoned, 0);
    assert_eq!(
        reconciled.applying, 1,
        "it is still being applied, and that is all anyone here knows"
    );
    assert_eq!(
        reconciled.unexecuted, 1,
        "and the one that never started is still truthfully unexecuted"
    );
    assert_eq!(reconciled.owed, 0, "nothing was answered");
    assert_eq!(reconciled.reserved, 0);

    // The writer going is the last-owner edge, and it makes the transition
    // itself rather than leaving it for whoever asks next.
    registry.writer_stopped(client);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "the last owner leaving is what owes the cleanup"
    );
    let reconciled = registry.reconcile_client(client);
    assert_eq!(reconciled.abandoned, 1);
    assert_eq!(reconciled.applying, 0);
    assert!(
        acks.try_recv().is_err(),
        "nothing is published for an operation nobody can describe"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "and its credit stays with it"
    );

    // It cannot be resumed, and no outcome may be published for it.
    let cleanups = registry.cleanups_owed().expect("a readable registry");
    assert_eq!(cleanups.len(), 1);
    let owed = cleanups[0];
    assert_eq!(
        owed.command.command.transaction(),
        TransactionId::from_raw(33001)
    );
    assert_eq!(
        registry.resume_execution(owed.token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Abandoned)
    );
    assert_eq!(
        registry.publish_with(
            owed.token,
            completion_ack(owed.command, XAuthorityControlOutcome::Delivered),
            |_| panic!("an abandoned operation must not reach the emission"),
        ),
        Err(crate::ControlPublicationRefusal::Abandoned)
    );

    // Until something establishes that nothing is owed, it stays owed, and
    // so does its credit.
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1
    );
    assert_eq!(private.reclaim_settled(), 0, "so the credit stays too");

    // Retiring it is not an outcome, but it is the end of what is owed.
    assert_eq!(registry.reconcile_unstarted().discharged, 1);
    assert!(registry.cleanups_owed().expect("a readable registry").is_empty());
    assert_eq!(
        private.reclaim_settled(),
        1,
        "and exactly one credit is released for it"
    );
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        registry.reconcile_unstarted().discharged,
        0,
        "and not a second time"
    );
    let _ = channels;
}

#[test]
fn dropping_the_writers_stops_them_even_if_nobody_shut_them_down() {
    let client = XServerFrontendClientId(326);
    let surface = SurfaceId::new(326, 1);
    let state = writer_runtime(surface);
    let (routes, control) = sync_channel(4);
    let (acknowledgements, _acks) = sync_channel(4);
    let (control_writer, _peer) = writer_start(
        None,
        &state,
        control,
        None,
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    let stop = control_writer.stop.clone();

    // A setup failure between two spawns leaves by dropping, not by calling
    // anything. Whatever has already started is still owned.
    drop(X11ClientWriters {
        input: None,
        control: Some(control_writer),
        protocol: None,
        drain: None,
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    });
    assert!(
        stop.load(Ordering::Acquire),
        "a writer nobody shut down is stopped by losing the thing that owned it"
    );
    let _ = routes;
}

#[test]
fn an_unreadable_registry_reconciles_nothing_and_says_so() {
    let client = XServerFrontendClientId(327);
    let surface = SurfaceId::new(327, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(client, surface, 34001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // Finding nothing because nothing could be looked at is not finding
    // nothing. A caller that read this as a clean teardown would walk away
    // from an operation still mid-application.
    let reconciled = registry.reconcile_client(client);
    assert!(!reconciled.readable);
    assert_eq!(reconciled.abandoned, 0);
    assert!(
        !registry.reconcile_unstarted().readable,
        "and settling nothing because nothing could be read says so"
    );
}

#[test]
fn only_an_abandoned_operation_with_nothing_queued_can_be_retired() {
    let client = XServerFrontendClientId(328);
    let surface = SurfaceId::new(328, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let applying = accepted(&registry, configure(client, surface, 35001));
    assert_eq!(
        registry.claim_execution(applying),
        crate::ControlExecutionClaim::Claimed
    );
    let accepted_only = accepted(&registry, configure(client, surface, 35002));

    // An operation whose executor is still there, and one that never started,
    // are not waiting on a cleanup. Recording one for either would retire a
    // record that is still owed something else entirely.
    let report = registry.reconcile_unstarted();
    assert_eq!(report.discharged, 0);
    for token in [applying, accepted_only] {
        assert_eq!(
            registry.state_of(token),
            crate::ControlRecordState::Outstanding
        );
    }
    assert_eq!(registry.outstanding().expect("a readable registry"), 2);

    // And another registry settles only its own: it holds no record for this
    // one, whatever the local identity happens to be.
    let other = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    assert_eq!(other.reconcile_unstarted().discharged, 0);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );
}

#[test]
fn a_writer_that_stops_mid_operation_leaves_it_owed_a_cleanup() {
    let client = XServerFrontendClientId(329);
    let surface = SurfaceId::new(329, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    assert!(routing.install_control_completion(registry.clone()));

    // An operation a writer claimed and is inside.
    let token = accepted(&registry, configure(client, surface, 36001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty()
    );

    // The writer goes. It is the executor, so it does not have to guess
    // whether one is still there, and nothing else will establish an outcome
    // for what it was applying.
    drop(X11ControlWriterSeal {
        routing: Some(&routing),
        client,
    });

    let owed = registry.cleanups_owed().expect("a readable registry");
    assert_eq!(owed.len(), 1, "the writer's exit is what establishes it");
    assert_eq!(
        owed[0].command.command.transaction(),
        TransactionId::from_raw(36001)
    );
    assert_eq!(
        registry.resume_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Abandoned),
        "and nothing picks it up afterwards"
    );
    drop(registration);
}

#[test]
fn an_unreadable_registry_owes_an_answer_rather_than_an_empty_list() {
    let client = XServerFrontendClientId(330);
    let surface = SurfaceId::new(330, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 37001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    assert_eq!(registry.cleanups_owed().map(|owed| owed.len()), Ok(1));

    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // An empty list means nothing is owed. It must never also mean nobody
    // could look: an owner told the first would walk away from a cleanup it
    // is holding.
    assert_eq!(
        registry.cleanups_owed(),
        Err(crate::ControlCleanupRefusal::Unavailable)
    );
    assert_eq!(registry.outstanding(), None);
    assert_eq!(registry.owed(), None);
}
