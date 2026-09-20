// An authority nobody can read: what an attempt answers, what the order gives
// back, and why unavailable is a different answer from nothing to do.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_poisoned_owner_reports_unavailable_rather_than_nothing_to_do() {
    let durable = crate::PrivateSettlementOwner::with_capacities(2, 2);
    durable.reserve().expect("a fresh owner");

    // Readable and idle first, so the difference below is the poison and not
    // the emptiness.
    let idle = durable.drive();
    assert!(idle.readable);
    assert!(!idle.made_progress());
    assert_eq!(durable.recover_failed(), Some(0));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // A drive that could not look is not a drive that found nothing, and
    // recovering nothing is not being unable to try. Only one of each says a
    // later attempt might do something.
    let blind = durable.drive();
    assert!(!blind.readable);
    assert!(!blind.made_progress());
    assert_eq!(durable.recover_failed(), None);
}

#[test]
fn a_poisoned_owner_still_takes_pending_work_from_a_dropping_handle() {
    let client = XServerFrontendClientId(375);
    let surface = SurfaceId::new(375, 1);
    // One slot, filled, so the instance cannot answer what it accepted and
    // the obligation is carried rather than discharged.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 72001))
        .expect("the shared admission to accept control");

    let report = private.shutdown();
    assert_eq!(report.pending.len(), 1, "it could not be answered");

    // The owner becomes unreadable before the only handle goes.
    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // Dropping is the handover, and a drop cannot keep what it is handing over
    // or report that it failed to. Declining here loses an accepted obligation
    // and the credit it holds.
    drop(report);
    let held = durable.records_even_if_poisoned();
    assert_eq!(held.held.len(), 1, "the obligation was taken, not dropped");
    assert!(
        matches!(
            held.held[0].1,
            PrivateOperation::Control(control, _)
                if control.command.transaction() == TransactionId::from_raw(72001)
        ),
        "and it is the command itself, with its own identity"
    );
    assert_eq!(held.reserved, 1, "still holding its credit");
    drop(held);

    // The channel is still full, so nothing was answered on the way.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert!(acks.try_recv().is_err());
}

#[test]
fn a_sweep_that_unwinds_leaves_its_work_owned_and_returnable() {
    let client = XServerFrontendClientId(376);
    let surface = SurfaceId::new(376, 1);
    // One slot, filled, so the obligation is carried rather than answered.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 73001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));
    assert_eq!(durable.reserved(), Some(1));

    // A sweep interrupted after moving its work out and before settling it.
    // The obligation is in the owner's own in-flight list, which is where it
    // has to be: a local vector goes with the frame.
    {
        let mut held = durable.records_even_if_poisoned();
        let carried = std::mem::take(&mut held.held);
        held.in_flight.extend(carried);
    }
    assert_eq!(durable.owed(), Some(0), "not in the list it settles from");
    assert_eq!(durable.reserved(), Some(1), "and still holding its credit");

    // Returning it answers nothing. An obligation found in flight is not
    // evidence of what happened to it, only that a sweep did not finish.
    assert_eq!(durable.restore_interrupted(), 1);
    assert_eq!(durable.owed(), Some(1), "returned, not answered");
    assert_eq!(durable.reserved(), Some(1));
    assert!(
        acks.try_recv().is_ok(),
        "the slot still holds what was there before"
    );
    assert!(acks.try_recv().is_err(), "and nothing was published");

    // Twice does nothing the second time.
    assert_eq!(durable.restore_interrupted(), 0);
    assert_eq!(durable.owed(), Some(1));

    // And now it can be driven, answering exactly the original obligation
    // once, with its credit returned.
    let progress = durable.drive();
    assert!(progress.readable);
    assert_eq!(progress.answered, 1);
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(73001),
        "the exact obligation that was accepted"
    );
    assert!(acks.try_recv().is_err(), "and only once");
    assert_eq!(durable.reserved(), Some(0), "its credit returned");
    assert_eq!(durable.owed(), Some(0));
}

#[test]
fn restoring_reaches_through_the_poison_the_interruption_caused() {
    let client = XServerFrontendClientId(378);
    let surface = SurfaceId::new(378, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::with_capacities(2, 2);
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 75001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Staged as an interrupted sweep leaves it, then poisoned the way the
    // interruption itself poisons it: a sweep unwinds while holding this lock,
    // so the stranded work and the poison are one event.
    {
        let mut held = durable.records_even_if_poisoned();
        let AbandonedSettlements {
            held: settling,
            in_flight,
            ..
        } = &mut *held;
        in_flight.append(settling);
    }
    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );
    assert_eq!(durable.owed(), None, "an ordinary read still refuses");

    // A restore that declined here would decline in every case it exists for
    // and succeed only when there was nothing to do.
    assert_eq!(
        durable.restore_interrupted(),
        1,
        "the obligation comes back despite the poison that stranded it"
    );
    {
        let held = durable.records_even_if_poisoned();
        assert_eq!(held.held.len(), 1, "returned to the list a drive settles");
        assert!(held.in_flight.is_empty());
        assert_eq!(held.reserved, 1, "still holding its credit");
        assert!(held.indeterminate.is_empty(), "it never reached an attempt");
    }
    assert_eq!(durable.restore_interrupted(), 0, "and only once");
    assert!(acks.try_recv().is_ok(), "the slot is untouched");
    assert!(acks.try_recv().is_err(), "restoring published nothing");
}

#[test]
fn an_unreadable_completion_registry_is_not_permission_to_answer() {
    let client = XServerFrontendClientId(379);
    let surface = SurfaceId::new(379, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 76001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));

    // Freed, so the channel is not what stops the next drive. The only thing
    // standing between this obligation and an acknowledgement is whether
    // anyone can show it is owed exactly one.
    assert!(acks.try_recv().is_ok());
    let _ = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = completion.inner.lock().expect("the registry");
                panic!("poisoning the completion registry");
            })
            .join()
    });

    let progress = durable.drive();
    assert!(progress.readable, "the settlement owner is still readable");
    assert_eq!(
        progress.answered, 0,
        "a registry nobody can read cannot say this has one owner"
    );
    assert!(
        acks.try_recv().is_err(),
        "so nothing was published on the strength of an unreadable record"
    );
    assert_eq!(durable.owed(), Some(1), "kept whole");
    assert_eq!(durable.reserved(), Some(1), "and still holding its credit");
    assert_eq!(
        durable.indeterminate(),
        Some(0),
        "never attempted, so not unproved -- just unresolved"
    );
}

#[test]
fn an_attempt_interrupted_while_emitting_is_not_returned_as_retryable() {
    let client = XServerFrontendClientId(380);
    let surface = SurfaceId::new(380, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 77001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Staged as an unwind inside the emitting call leaves it: moved out of the
    // list a drive settles from, and marked as having reached the attempt.
    {
        let mut held = durable.records_even_if_poisoned();
        let AbandonedSettlements {
            held: settling,
            in_flight,
            ..
        } = &mut *held;
        in_flight.append(settling);
        held.settling = true;
    }

    assert_eq!(durable.restore_interrupted(), 1);
    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "whether its acknowledgement went out is what the unwind destroyed"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "so it is not put back where a drive would send it again"
    );
    assert_eq!(
        durable.reserved(),
        Some(1),
        "and its credit is not released, because nobody observed an outcome"
    );

    // Driving now must find nothing to do with it.
    let progress = durable.drive();
    assert_eq!(progress.answered, 0);
    assert!(acks.try_recv().is_ok(), "the slot still holds what was there");
    assert!(
        acks.try_recv().is_err(),
        "an unproved outcome is never published a second time"
    );
    assert_eq!(durable.indeterminate(), Some(1), "it stays what it is");
}

#[test]
fn an_obligation_a_live_record_still_answers_for_is_not_published_here() {
    let client = XServerFrontendClientId(381);
    let surface = SurfaceId::new(381, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");

    // A record that has begun applying cannot be handed over: a writer is
    // inside the command and will publish for it.
    let command = configure(client, surface, 78001);
    let token = accepted(&completion, command);
    assert_eq!(
        completion.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        !completion.discard(token),
        "an applying record is not free to take"
    );

    // Staged carrying that token, which is the state this gate exists for: the
    // obligation is here, and so is someone else who can answer it.
    {
        let mut held = durable.records_even_if_poisoned();
        held.held.push((
            private.broker.registry.clone(),
            PrivateOperation::Control(command, Some(token)),
        ));
        held.reserved = held.reserved.saturating_add(1);
    }

    let progress = durable.drive();
    assert!(progress.readable);
    assert_eq!(progress.answered, 0, "not ours to answer");
    assert!(
        acks.try_recv().is_err(),
        "publishing here would be the second outcome for one operation"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "and it is not kept as a command that could be sent again"
    );
    assert_eq!(
        durable.outstanding(),
        Some(1),
        "carried as an identity, so the credit is tracked without the payload"
    );
    assert_eq!(durable.reserved(), Some(1), "nothing observed an outcome yet");

    // Once the record's real owner answers, the credit is free -- released by
    // observing the registry rather than by anything here sending a receipt.
    assert_eq!(
        completion.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );
    let progress = durable.drive();
    assert_eq!(progress.reclaimed, 1, "its record retired");
    assert_eq!(progress.answered, 0, "reclaiming is not answering");
    assert_eq!(durable.reserved(), Some(0));
    assert!(acks.try_recv().is_err(), "and still nothing published here");
}

#[test]
fn a_full_channel_is_congestion_and_the_obligation_survives_it() {
    let client = XServerFrontendClientId(382);
    let surface = SurfaceId::new(382, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 79001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Driven repeatedly against a full channel. Each attempt establishes
    // ownership and fails to send; none of them may conclude anything from
    // that, because a full channel with a live receiver is congestion.
    for attempt in 0..3 {
        let progress = durable.drive();
        assert!(progress.readable);
        assert_eq!(progress.answered, 0, "attempt {attempt} answered nothing");
        assert_eq!(durable.owed(), Some(1), "and kept it");
        assert_eq!(durable.reserved(), Some(1), "with its credit");
        assert_eq!(durable.indeterminate(), Some(0), "no attempt was interrupted");
    }

    // Drained, and the same obligation is answered once.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    let progress = durable.drive();
    assert_eq!(progress.answered, 1);
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(79001),
        "the exact obligation that was accepted"
    );
    assert_eq!(durable.owed(), Some(0));
    assert_eq!(durable.reserved(), Some(0), "its credit returned");

    // Repeat control: nothing is answered twice and no credit is released
    // twice, whether by driving again or by restoring.
    for _ in 0..3 {
        assert_eq!(durable.drive().answered, 0);
        assert_eq!(durable.restore_interrupted(), 0);
    }
    assert!(acks.try_recv().is_err(), "and only one acknowledgement went out");
    assert_eq!(durable.reserved(), Some(0));
}

#[test]
fn a_token_from_another_registry_is_not_permission_and_is_not_taken() {
    let client = XServerFrontendClientId(383);
    let surface = SurfaceId::new(383, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mine, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
    let (theirs, _their_channels, _their_registration, _their_deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let their_completion = theirs
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");

    // Same client, same surface, same transaction. Only the registry that
    // issued the registration differs, which is the whole identity here: a
    // command's fields are not unique to one operation.
    let command = configure(client, surface, 80001);
    let foreign = accepted(&their_completion, command);
    assert_eq!(their_completion.outstanding(), Some(1));

    {
        let mut held = durable.records_even_if_poisoned();
        held.held.push((
            mine.broker.registry.clone(),
            PrivateOperation::Control(command, Some(foreign)),
        ));
        held.reserved = held.reserved.saturating_add(1);
    }

    let progress = durable.drive();
    assert_eq!(
        progress.answered, 0,
        "one registry cannot answer for another's registration"
    );
    assert!(
        acks.try_recv().is_err(),
        "and must not publish on the strength of a record it did not issue"
    );
    assert_eq!(
        their_completion.outstanding(),
        Some(1),
        "nor take a record belonging to another registry"
    );
    assert_eq!(durable.owed(), Some(1), "kept whole");
    assert_eq!(durable.reserved(), Some(1), "with its credit");
    assert_eq!(durable.outstanding(), Some(0), "and not counted as routed");

    drop(mine);
    drop(theirs);
}

#[test]
fn a_dying_handle_parks_an_attempt_that_never_returned() {
    let client = XServerFrontendClientId(384);
    let surface = SurfaceId::new(384, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 81001))
        .expect("the shared admission to accept control");

    // The channel has room, so shutdown answers it and the handle carries
    // nothing. Refilled first so the obligation survives into the handle.
    let mut settlement = private.shutdown();
    assert_eq!(settlement.pending.len(), 0, "answered on the way out");
    let _ = acks.try_recv();

    // Staged as an attempt that did not return leaves a handle: the obligation
    // is still in the list and the marker says one of them reached the call
    // that emits.
    settlement.pending.push(PrivateOperation::Control(
        configure(client, surface, 81002),
        None,
    ));
    settlement.settling = Some(0);
    drop(settlement);

    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "a dying handle hands over what it cannot account for"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "and not as work something would send again"
    );
}

#[test]
fn a_transfer_guard_pays_out_when_the_attempt_unwinds() {
    let client = XServerFrontendClientId(385);
    let surface = SurfaceId::new(385, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let origin = private.broker.registry.clone();
    drop(private.shutdown());
    let _ = acks.try_recv();

    // Two obligations: one the attempt is inside, one it has not reached. A
    // real unwind crosses the guard, which is the path that matters -- the
    // handle's fields are destroyed immediately afterwards, so anything not
    // transferred by then is gone.
    //
    // The panic is raised here rather than from inside `settle_one`: nothing
    // on that path panics of its own accord, so injecting one needs a
    // modified copy of the source. This exercises the guard's contract, which
    // is the mechanism that makes the injected case survivable.
    let mut pending = vec![
        PrivateOperation::Control(configure(client, surface, 82001), None),
        PrivateOperation::Control(configure(client, surface, 82002), None),
    ];
    let mut settling = None;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let transfer = SettlementTransfer {
            execution: None,
            origin: &origin,
            durable: &durable,
            pending: &mut pending,
            settling: &mut settling,
        };
        // As the marker stands when the emitting call is entered.
        *transfer.settling = Some(1);
        panic!("interrupting the attempt");
    }));
    assert!(unwound.is_err(), "the attempt unwound");

    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "the one it was inside is unproved"
    );
    assert_eq!(
        durable.owed(),
        Some(1),
        "and the one it never reached is ordinary owed work"
    );
    assert!(
        acks.try_recv().is_err(),
        "an unwinding transfer publishes nothing"
    );
}

#[test]
fn an_emptied_failed_record_cannot_release_a_second_instances_slot() {
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (sender, _receiver) = sync_channel(4);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    // Kept alive for the whole test. Its failure slot was reserved before it
    // was exposed and it holds it for its life.
    let service_keeper = service_owner(&durable, 16);
    let live = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender.clone(),
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    let (failing_authority, failing_issuer, failing_submit) = private_authority();
    let service_keeper = service_owner(&durable, 16);
    let failing = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority: failing_authority,
            issuer: failing_issuer,
            submit: failing_submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a second slot: {refusal:?}"));
    let failing_origin = failing.broker.registry.clone();
    let failing_queue = std::sync::Arc::clone(&failing.admission.ready);
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        2,
        "one slot each, reserved before either was exposed"
    );

    let admission = std::sync::Arc::clone(&failing.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();
    drop(failing.shutdown());
    assert_eq!(durable.failed_instances().expect("readable"), 1);

    assert_eq!(durable.recover_failed().expect("readable"), 0, "it had accepted nothing");
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        1,
        "the failed instance gave its slot back; the live one keeps its own"
    );

    // As an interrupted recovery leaves it: the record is back in the
    // inventory, already marked as having given its slot up. Restoring an
    // obligation must not restore a release that already happened.
    {
        let mut held = durable.records_even_if_poisoned();
        held.failed.push(FailedInstance {
            origin: failing_origin,
            queue: failing_queue,
            uncollected: Vec::new(),
            slot: FailureSlot::Released,
        });
    }
    assert_eq!(durable.recover_failed().expect("readable"), 0);
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        1,
        "the live instance still holds the slot it reserved"
    );
    drop(live);
}

#[test]
fn a_private_frontend_gates_the_authority_it_actually_owns() {
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    // Read before the parts are handed over; the instance is moved in.
    let owned = authority
        .authority_identity(&issuer)
        .expect("an authority to name itself");
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
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
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    // The property, not a spelling of the constructor: whatever the frontend
    // stamps and admits under is the same identity it executes against. A gate
    // built elsewhere and handed in could name another authority, and every
    // stamp this instance issued would then describe a coordinator driving
    // something else.
    assert_eq!(
        private.control_gate().authority(),
        owned,
        "the derived gate serves the authority this instance owns"
    );
    assert_eq!(
        private
            .authority()
            .identity()
            .expect("the held authority to name itself"),
        owned,
        "and the instance it kept is the one that was read"
    );

    // A separately built authority is a different identity, which is exactly
    // what pairing one with this gate would have hidden.
    let (other, other_issuer, _other_submit) = private_authority();
    assert_ne!(
        other
            .authority_identity(&other_issuer)
            .expect("a second authority to name itself"),
        owned,
        "two authorities are never one identity"
    );
}

/// The session generation these tests admit clients under.
const ROLE_SESSION_GENERATION: u64 = 5;

/// A client admitted the way the production lookup expects to find one.
fn admitted(client: XServerFrontendClientId) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        sophia_protocol::NamespaceContext::new(
            NamespaceId::from_raw(client.raw()),
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            ROLE_SESSION_GENERATION,
        )
        .unwrap(),
    )
    .unwrap()
}

/// The same admission, in a chosen namespace.
fn namespaced(
    client: XServerFrontendClientId,
    namespace: NamespaceId,
) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        sophia_protocol::NamespaceContext::new(
            namespace,
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            ROLE_SESSION_GENERATION,
        )
        .unwrap(),
    )
    .unwrap()
}

/// Admit a client the way the production boundary expects.
///
/// Both halves, because they are different things: the frontend registers the
/// client's routes, and the admission participant is where the producer of
/// admission and revocation binds it. Execution consults the second, so a test
/// that only did the first would be exercising a path that no longer decides
/// anything.
fn admit_role_client(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
) -> XServerFrontendClientRouteRegistration {
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .broker.registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("a fresh client to be admitted to the boundary");
    registration
}

fn private_for_roles(keeper: &crate::PrivateServiceOwner) -> crate::PrivateXServerFrontend {
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"))
}

/// Put a capsule on a recipient's queue the way production does: through the
/// gate that serializes handovers with that registration's closing.
///
/// Panics if the endpoint is closed, which no caller staging a queue intends.
/// A control about closing uses `admit` directly and inspects the refusal.
#[allow(clippy::result_large_err)] // The capsule comes back in the error, as in production.
fn gated_send(
    sender: &PrivateGatedOrderedSender,
    capsule: XAuthorityOrderedDelivery,
) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityOrderedDelivery>> {
    sender
        .admit()
        .expect("an open endpoint")
        .try_send(capsule)
}

/// The connection identity the ordered path will read back for this client.
///
/// Built the same way on both sides on purpose: the recipient is the client
/// and the generation is the one Session admitted it under. A test that
/// invented either would be checking that two of its own constants match.
fn role_connection(recipient: u64) -> sophia_input_authority::ConnectionIdentity {
    sophia_input_authority::ConnectionIdentity {
        recipient,
        connection_generation: ROLE_SESSION_GENERATION,
    }
}

#[test]
fn one_producer_cannot_consume_another_producers_completion() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    // Admitted, because execution reads the live registration table rather
    // than trusting what the request remembers.
    let _first_admitted = admit_role_client(&private, XServerFrontendClientId(501));
    let _second_admitted = admit_role_client(&private, XServerFrontendClientId(502));
    let first = private
        .reservation_role(XServerFrontendClientId(501), DeviceId::from_raw(1))
        .expect("a capability for the first producer");
    let second = private
        .reservation_role(XServerFrontendClientId(502), DeviceId::from_raw(2))
        .expect("a capability for the second producer");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let first_held = first.reserve(stamp, 1).expect("the first reservation");
    let second_held = second.reserve(stamp, 1).expect("the second reservation");
    let first_request = first_held.accepted();
    let second_request = second_held.accepted();

    // Both execute, so both have an outcome waiting. The connection handed to
    // execution is the evidence a caller read now, not the one custody
    // remembers -- here they agree, because nothing has revoked either.
    for (request, client) in [
        (&first_request, XServerFrontendClientId(501)),
        (&second_request, XServerFrontendClientId(502)),
    ] {
        assert!(
            private
                .execute_ordered(request, client, |permit, _bindings| permit.begin_external_effect())
                .is_ok(),
            "each producer's own request executes"
        );
    }

    // The right is carried, not named. There is no call that lets one producer
    // point at another's request: `observe` takes the token and the connection
    // from the custody value it is invoked on. A shared submit handle
    // establishes only which authority is being addressed, and the connection
    // test compares against whatever the caller passed in -- so an API that
    // accepted both from the caller let either producer consume the other's
    // outcome, and the one whose outcome was taken then saw a stale request.
    assert!(
        matches!(
            first_request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the first producer observes its own outcome"
    );
    assert!(
        matches!(
            second_request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "and the second still has its own to observe"
    );
}

#[test]
fn a_controller_refuses_an_authority_paired_with_another_issuer() {
    let (authority, _issuer, _submit) = private_authority();
    let (_other, other_issuer, _other_submit) = private_authority();

    // Accepting this pairing lets a reservation be taken and then never
    // disposed: disposal is an issuer act, and this issuer answers for a
    // different instance, so the cell is stranded with nothing able to
    // publish, consume or reissue it.
    let refused = crate::PrivateAuthorityController::new(authority, other_issuer);
    let Err((refusal, authority, issuer)) = refused else {
        panic!("a mismatched authority and issuer must be refused");
    };
    assert!(matches!(
        refusal,
        crate::PrivateAuthorityRefusal::Authority(_)
    ));

    // The parts come back, so a caller can still build the right pairing.
    assert!(
        crate::PrivateAuthorityController::new(authority, issuer).is_err(),
        "the returned parts are the mismatched ones, unchanged"
    );
}

#[test]
fn a_reservation_dropped_under_common_is_disposed_rather_than_deadlocking() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = private_for_roles(&service_keeper);
    let _admitted = admit_role_client(&private, XServerFrontendClientId(503));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(504));
    let role = private
        .reservation_role(XServerFrontendClientId(503), DeviceId::from_raw(3))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let running = role.reserve(stamp, 1).expect("a reservation to execute");
    let request = running.accepted();

    // A second producer's reservation, taken OUTSIDE any execution, and never
    // published. Its own grant, because one grant holds one cell.
    let other = private
        .reservation_role(XServerFrontendClientId(504), DeviceId::from_raw(4))
        .expect("a capability for the second producer");
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Moved in and dropped while this thread holds common. Taking common again
    // to dispose is a deadlock rather than a rank question, so the debt is
    // recorded and paid by the next caller that holds it.
    let completion = private.execute_ordered(&request, XServerFrontendClientId(503), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(
        completion.is_ok(),
        "the execution returned rather than hanging"
    );

    assert!(
        matches!(
            request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the executed request still answers to its own custody"
    );
    // Deferred, not skipped: the stranded cell is free for the next request on
    // that grant.
    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the dropped reservation's cell was released"
    );
}

#[test]
fn two_detached_producers_reserve_against_one_authority() {
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = private_for_roles(&service_keeper);
    let _first_admitted = admit_role_client(&private, XServerFrontendClientId(511));
    let _second_admitted = admit_role_client(&private, XServerFrontendClientId(512));
    let first = private
        .ingress_for(XServerFrontendClientId(511), DeviceId::from_raw(1))
        .expect("an ingress for the first producer");
    let second = private
        .ingress_for(XServerFrontendClientId(512), DeviceId::from_raw(2))
        .expect("an ingress for the second producer");

    // Detached is the point: each is handed off and used on its own, and they
    // contend for one order against one authority.
    let first_sequence = first
        .submit(&service_keeper.lease(), motion_to(
            SurfaceId::new(511, 1),
            XAuthorityInputDeliveryId::from_raw(511),
        ))
        .expect("the first producer's work to be accepted");
    let second_sequence = second
        .submit(&service_keeper.lease(), motion_to(
            SurfaceId::new(512, 1),
            XAuthorityInputDeliveryId::from_raw(512),
        ))
        .expect("the second producer's work to be accepted");
    assert_ne!(
        first_sequence, second_sequence,
        "two producers take distinct places in one order"
    );

    // Each reserved against its own grant, so neither refused the other. A
    // shared grant would have made the second submission fail for want of a
    // completion cell, because a grant holds exactly one.
    //
    // That same bound is why a producer cannot get ahead of execution: its
    // first request still holds its cell until that request is executed and
    // its outcome consumed, so a second submission on the same grant is
    // refused rather than queued behind it. Recorded here as the property it
    // is -- a producer streaming input needs its requests executed, not just
    // accepted.
    let again = first.submit(&service_keeper.lease(), motion_to(
        SurfaceId::new(511, 1),
        XAuthorityInputDeliveryId::from_raw(513),
    ));
    // Saturated, not denied: the grant's one cell is busy until the first
    // request's outcome is observed, and busy is worth retrying. Denial would
    // tell a producer to stop when it should wait.
    assert!(
        matches!(again, Err(PrivateSendError::Saturated(_))),
        "a second request on one grant is busy while the first holds its cell, got {again:?}"
    );

    // And the refusal did not disturb the other producer, which still has its
    // own grant and its own cell.
    assert!(
        matches!(
            second.submit(&service_keeper.lease(), motion_to(
                SurfaceId::new(512, 1),
                XAuthorityInputDeliveryId::from_raw(514),
            )),
            Err(PrivateSendError::Saturated(_))
        ),
        "the same bound applies to each producer independently"
    );
}

#[test]
fn work_refused_by_the_order_takes_its_reservation_back() {
    let (sender, _receiver) = sync_channel(64);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    // The order is filled from OTHER grants, one request each, so nothing here
    // is refused for want of a completion cell. Filling it from one producer
    // could not work: that producer's second request is refused at reservation
    // long before the order is full, which is a different refusal entirely and
    // proves nothing about the queue.
    let mut fillers = Vec::new();
    let mut filler_admissions = Vec::new();
    for index in 0..32u64 {
        filler_admissions.push(admit_role_client(
            &private,
            XServerFrontendClientId(600 + index),
        ));
        let filler = private
            .ingress_for(
                XServerFrontendClientId(600 + index),
                DeviceId::from_raw(index + 1),
            )
            .expect("a capability per filling producer");
        let outcome = filler.submit(&service_keeper.lease(), motion_to(
            SurfaceId::new(600, 1),
            XAuthorityInputDeliveryId::from_raw(800 + index),
        ));
        match outcome {
            Ok(_) => fillers.push(filler),
            Err(PrivateSendError::Saturated(_)) => break,
            Err(other) => panic!("filling the order should saturate, got {other:?}"),
        }
    }
    assert!(!fillers.is_empty(), "the order accepted work before filling");

    // A fresh grant, so its own cell is free and the only thing that can
    // refuse this is the order itself.
    let _fresh_admitted = admit_role_client(&private, XServerFrontendClientId(699));
    let fresh = private
        .ingress_for(XServerFrontendClientId(699), DeviceId::from_raw(60))
        .expect("a capability for the fresh producer");
    let refused = fresh.submit(&service_keeper.lease(), motion_to(
        SurfaceId::new(699, 1),
        XAuthorityInputDeliveryId::from_raw(899),
    ));
    let Err(PrivateSendError::Saturated(route)) = refused else {
        panic!("a full order must saturate a fresh grant's submission, got {refused:?}");
    };
    assert_eq!(
        route.delivery,
        Some(XAuthorityInputDeliveryId::from_raw(899)),
        "the refused work is handed back intact"
    );

    // Only that reservation went back. Everything the order accepted before it
    // filled is still there -- drained across as many turns as the service
    // budget needs, and counted, so a refusal that quietly consumed an earlier
    // item would show up as a short count rather than being invisible.
    let mut keyboards = private.keyboards().expect("this instance's state");
    let mut drained = 0usize;
    loop {
        let ran = private
            .route_pending_ordered(&mut keyboards, &control_watchdog())
            .expect("a readable order")
            .len();
        if ran == 0 {
            break;
        }
        drained += ran;
    }
    assert_eq!(
        drained,
        fillers.len(),
        "the order still held exactly the work it had accepted"
    );

    // The same grant and the same delivery id go through once the order has
    // room. Both halves matter: the cell was released, so the grant can
    // reserve again, and the delivery reservation was rolled back, so the id
    // is not still tracked as live. Either one left behind would refuse this.
    assert!(
        fresh
            .submit(&service_keeper.lease(), motion_to(
                SurfaceId::new(699, 1),
                XAuthorityInputDeliveryId::from_raw(899),
            ))
            .is_ok(),
        "the refused submission released both its cell and its delivery id"
    );
}
