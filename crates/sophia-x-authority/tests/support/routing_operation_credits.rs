// An operation's credit: queued focus output that keeps it, settlement that
// releases it, and the poisoned owner that must still release an answered one.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn publishing_an_outcome_does_not_free_a_credit_while_its_focus_out_is_queued() {
    let focused = XServerFrontendClientId(351);
    let claimant = XServerFrontendClientId(352);
    let focused_surface = SurfaceId::new(351, 1);
    let claimant_surface = SurfaceId::new(352, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, focused, focused_surface);
    let (claimant_registration, _claimant_channels) = private
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
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(51001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());

    let command = XAuthorityClientControlCommand {
        client: claimant,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(51002),
            surface: claimant_surface,
        },
    };
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), command)
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // The operation is answered while the FocusOut it queued on the other
    // client still sits in that client's writer queue.
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    channels
        .send_ack_for(
            claimant,
            completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
            Some(token),
        )
        .expect("a free channel");

    // Answering it is not everything it started being over. Freeing the
    // credit here frees storage for work an effect of this is still queued
    // against.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "no credit is released while its queued FocusOut can still run"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(registry.dependents_outstanding(token), Some(1));

    // The other client's writer takes it, or its queue goes. Either way the
    // effect is over, and only then is the credit free.
    let queued = held_channels
        .control
        .try_recv()
        .expect("the previously focused client is told");
    drop(queued);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    drop(claimant_registration);
}

#[test]
fn a_count_that_cannot_advance_refuses_rather_than_saturating() {
    let client = XServerFrontendClientId(353);
    let surface = SurfaceId::new(353, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 52001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    // The counter is placed at its end from the test, not from a setter in
    // production src.
    registry.inner.lock().unwrap().records[0].dependents = usize::MAX;

    // Saturating here would take responsibility for an effect and then lose
    // it the moment the first guard ended, leaving the rest uncounted.
    assert!(matches!(
        registry.track_dependent(token),
        Err(crate::ControlDependentRefusal::Exhausted)
    ));
    assert_eq!(registry.dependents_outstanding(token), Some(usize::MAX));
}

/// Reached the way production reaches it: a genuine submit and route_pending
/// that claims, parks, and finds the registry unreadable when it comes to
/// count the effect it is about to queue.
#[test]
fn a_governed_focus_out_that_cannot_be_counted_is_not_queued() {
    let focused = XServerFrontendClientId(354);
    let claimant = XServerFrontendClientId(355);
    let focused_surface = SurfaceId::new(354, 1);
    let claimant_surface = SurfaceId::new(355, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, held_channels, _registration, _deliveries) =
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
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(53001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    private
        .control_producer()
        .submit(&owner_of_durable.lease(), XAuthorityClientControlCommand {
            client: claimant,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(53002),
                surface: claimant_surface,
            },
        })
        .expect("the shared admission to accept control");

    // Park the router after it has claimed and before it produces any effect.
    let focus_lock = Arc::clone(&private.broker.registry.focused_surface);
    let focused_guard = focus_lock.lock().unwrap();
    let routed = std::thread::spawn(move || {
        let outcome = private.route_pending(&owner_of_durable.lease());
        (private, outcome)
    });
    let parked = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < parked {
        std::thread::yield_now();
    }

    // The registry becomes unreadable while it is parked, so the effect it is
    // about to queue cannot be counted against the operation that causes it.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );
    drop(focused_guard);
    let (private, outcome) = routed.join().expect("the routing thread");

    // Refused before either effect. Queueing it untracked would make the work
    // look ungoverned, which is a real state for ordinary work and a false one
    // here, and the operation would then be settled while that effect could
    // still happen.
    assert!(
        matches!(
            outcome,
            Err(XServerFrontendRouteError::DependentNotTracked {
                refusal: crate::ControlDependentRefusal::Unavailable,
                ..
            })
        ),
        "authority unavailability is reported as what it is: {outcome:?}"
    );
    assert!(
        held_channels.control.try_recv().is_err(),
        "the previously focused client is not told it lost focus"
    );
    assert!(
        claimant_channels.control.try_recv().is_err(),
        "and the target is not queued either"
    );
    drop(claimant_registration);
    drop(private);
}

#[test]
fn a_published_outcome_is_not_sent_again_while_its_record_survives() {
    let client = XServerFrontendClientId(356);
    let surface = SurfaceId::new(356, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 54001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let queued = registry.track_dependent(token).expect("an applying record");
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );

    // The record survives only for the work it queued elsewhere. Its outcome
    // has gone out, so nothing reaches the emitter again -- not the same
    // acknowledgement, and not a different one.
    for outcome in [
        XAuthorityControlOutcome::Delivered,
        XAuthorityControlOutcome::AuthorityRejected,
    ] {
        for publication in [
            ControlPublication::Delivered,
            ControlPublication::Retained,
            ControlPublication::ReceiverGone,
        ] {
            assert_eq!(
                registry.publish_with(token, completion_ack(command, outcome), |_| {
                    panic!("a published outcome must not reach the emitter again")
                }),
                Err(crate::ControlPublicationRefusal::AlreadyPublished),
                "{outcome:?} as {publication:?}"
            );
        }
    }

    // And the retry path finds nothing owed: a retained one here would have
    // turned a published record back into one that still owes publication.
    assert_eq!(registry.owed(), Some(0));
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding
    );

    drop(queued);
    assert_eq!(registry.state_of(token), crate::ControlRecordState::Retired);
}

#[test]
fn a_retry_that_lands_settles_where_it_stands() {
    let client = XServerFrontendClientId(357);
    let surface = SurfaceId::new(357, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = |transaction| configure(client, surface, transaction);
    let with_work = accepted(&registry, command(55001));
    let without = accepted(&registry, command(55002));
    for token in [with_work, without] {
        assert_eq!(
            registry.claim_execution(token),
            crate::ControlExecutionClaim::Claimed
        );
    }
    let queued = registry
        .track_dependent(with_work)
        .expect("an applying record");
    for (token, transaction) in [(with_work, 55001), (without, 55002)] {
        assert_eq!(
            registry.publish_with(
                token,
                completion_ack(command(transaction), XAuthorityControlOutcome::Delivered),
                |_| ControlPublication::Retained,
            ),
            Ok(ControlPublication::Retained)
        );
    }
    assert_eq!(registry.owed(), Some(2));

    // One retry pass: the one with nothing outstanding goes, and the one with
    // queued work is settled in the same pass rather than revisited.
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        2
    );
    assert_eq!(registry.owed(), Some(0));
    assert_eq!(registry.state_of(without), crate::ControlRecordState::Retired);
    assert_eq!(
        registry.state_of(with_work),
        crate::ControlRecordState::Outstanding
    );

    drop(queued);
    assert_eq!(
        registry.state_of(with_work),
        crate::ControlRecordState::Retired
    );
}

#[test]
fn an_operation_that_finished_applying_reports_it_and_owes_nothing() {
    let client = XServerFrontendClientId(358);
    let surface = SurfaceId::new(358, 1);
    let (acknowledgements, acks) = sync_channel(1);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
    let state = writer_runtime(surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Fill the one acknowledgement slot so the writer applies and then fails
    // to publish, leaving the operation unanswered but fully applied.
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 56001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    writer_configure_notify(&mut peer, 80);
    assert!(!writer_join(writer), "a full channel fails this writer");

    // It reported both steps as it took them, and the writer going is what
    // makes the operation abandoned.
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::Completed
        })
    );
    // Its outcome is established even though the channel was full, so it is
    // not abandoned and is not a cleanup candidate: what it is waiting for is
    // publication, not a reconciliation.
    let reconciled = registry.reconcile_client(client);
    assert_eq!(reconciled.abandoned, 0);
    assert_eq!(reconciled.owed, 1);
    assert_eq!(private.reconcile_abandoned().discharged, 0);
    assert_eq!(private.reclaim_settled(), 0, "and its credit stays with it");

    // Draining lets the retained outcome out, and only then is it over.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
}

#[test]
fn an_operation_that_reported_finishing_is_still_not_proved_to_agree() {
    let client = XServerFrontendClientId(361);
    let surface = SurfaceId::new(361, 1);
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
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 59001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // It reported finishing both steps, and then its executor went without
    // establishing an outcome.
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
        crate::ControlProgress::ProjectionBegun,
        crate::ControlProgress::ProjectionApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Both steps finished, and that is history rather than a statement about
    // now. The runtime guard was released before the projection was brought
    // into line and neither report carries a revision, so a projection that
    // agreed can have been overtaken; and the operation continues through
    // fallible work after it that these reports say nothing about.
    let report = private.reconcile_abandoned();
    assert!(report.readable);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_operation_whose_first_step_never_began_owes_nothing() {
    let client = XServerFrontendClientId(364);
    let surface = SurfaceId::new(364, 1);
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
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 62001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);

    // It never began anything, and beginning is recorded before an effect can
    // happen, so this is evidence that nothing happened rather than an absence
    // of evidence.
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let report = private.reconcile_abandoned();
    assert_eq!(report.discharged, 1);
    assert_eq!(report.retained_in_progress, 0);
    assert_eq!(
        private.reclaim_settled(),
        1,
        "and its credit is released once"
    );
    assert_eq!(private.reclaim_settled(), 0);
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_operation_interrupted_inside_a_step_is_not_one_that_never_began() {
    let client = XServerFrontendClientId(365);
    let surface = SurfaceId::new(365, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 63001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Begun and never reported finished: the writer went between noting the
    // intent and the change succeeding. Reporting only after success cannot
    // tell this from an operation that never began, and that gap is where the
    // runtime ends up changed while the record says nothing happened.
    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let report = private.reconcile_abandoned();
    assert_eq!(report.discharged, 0, "the effect may have happened");
    assert_eq!(report.retained_in_progress, 1);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
}

#[test]
fn an_operation_caught_between_its_steps_keeps_its_obligation() {
    let client = XServerFrontendClientId(359);
    let surface = SurfaceId::new(359, 1);
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
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 57001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Caught between changing shared state and the state derived from it
    // catching up. This is what the writer reports having done at that point.
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // It changed something that outlives the connection and left the
    // projection of it behind. Nothing here can discharge that, so it keeps
    // the obligation and the credit.
    let report = private.reconcile_abandoned();
    assert_eq!(report.retained_half_applied, 1);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_in_progress, 0);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "and it is still owed one"
    );
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn a_kind_whose_steps_are_not_reported_is_retained_rather_than_discharged() {
    let client = XServerFrontendClientId(360);
    let surface = SurfaceId::new(360, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::CloseSurface {
                transaction: TransactionId::from_raw(58001),
                surface,
            },
        })
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Nothing reports what closing a surface did, and an absent report is not
    // a report of nothing. Discharging it would be reading silence as proof.
    let report = private.reconcile_abandoned();
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_half_applied, 0);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
}

#[test]
fn an_unreadable_registry_settles_nothing_and_says_so() {
    let client = XServerFrontendClientId(362);
    let surface = SurfaceId::new(362, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 60001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&owner_of_durable.lease()).expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // Settling nothing because nothing could be looked at is not settling
    // nothing. A caller told the first would walk away from an operation that
    // is still owed something.
    let report = private.reconcile_abandoned();
    assert!(!report.readable);
    assert_eq!(report.discharged, 0);
    assert_eq!(private.reclaim_settled(), 0, "and no credit is released");
}

#[test]
fn only_an_operation_being_applied_can_report_progress() {
    let client = XServerFrontendClientId(363);
    let surface = SurfaceId::new(363, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 61001);

    // A reservation is its producer's and an accepted command has not
    // started, so neither has anything to report.
    let token = registry.register(command).expect("a fresh registry");
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    registry.writer_started(client);
    registry
        .begin_acceptance(token)
        .expect("a fresh reservation")
        .commit();
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    assert_eq!(registry.steps_of(token), Some(crate::ControlSteps::default()));

    // Applying is when there is something to report.
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::NotStarted
        })
    );

    // And once it is answered, nothing more is reported against it: a step
    // recorded after the outcome would describe work the outcome did not
    // cover.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::ProjectionBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::NotStarted
        })
    );
}

#[test]
fn progress_cannot_be_taken_back_or_skipped() {
    let client = XServerFrontendClientId(366);
    let surface = SurfaceId::new(366, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 64001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let progressed = |steps: crate::ControlSteps| registry.steps_of(token) == Some(steps);

    // A step cannot finish before it begins, and a projection cannot be
    // claimed without the runtime change it projects. Both were reachable
    // when a caller could set the state directly, and either one lets an
    // operation report work nothing did.
    for out_of_order in [
        crate::ControlProgress::RuntimeApplied,
        crate::ControlProgress::ProjectionBegun,
        crate::ControlProgress::ProjectionApplied,
    ] {
        assert_eq!(
            registry.record_progress(token, out_of_order),
            Err(crate::ControlProgressRefusal::OutOfOrder),
            "{out_of_order:?} before what it depends on"
        );
    }
    assert!(progressed(crate::ControlSteps::default()));

    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("the first step");
    // And it cannot begin twice, which would take the record backwards from
    // whatever the first beginning already established.
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::OutOfOrder)
    );
    assert!(progressed(crate::ControlSteps {
        runtime: crate::ControlStepState::InProgress,
        projection: crate::ControlStepState::NotStarted,
    }));

    registry
        .record_progress(token, crate::ControlProgress::RuntimeApplied)
        .expect("the runtime change");
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::ProjectionApplied),
        Err(crate::ControlProgressRefusal::OutOfOrder),
        "the projection cannot finish before it begins"
    );
    registry
        .record_progress(token, crate::ControlProgress::ProjectionBegun)
        .expect("the projection");
    registry
        .record_progress(token, crate::ControlProgress::ProjectionApplied)
        .expect("the projection finishing");
    assert!(progressed(crate::ControlSteps {
        runtime: crate::ControlStepState::Completed,
        projection: crate::ControlStepState::Completed,
    }));
}

#[test]
fn an_effect_whose_intent_cannot_be_recorded_does_not_happen() {
    let client = XServerFrontendClientId(367);
    let surface = SurfaceId::new(367, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 65001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // A registration whose registry a writer cannot reach. Continuing would
    // produce an effect nobody noted the intent for, and afterwards nothing
    // could tell it from one that never happened -- which is exactly the
    // state a discharge is read from.
    let orphaned = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(1).0,
        completion: None,
    };
    assert_eq!(
        orphaned.record_progress(Some(token), crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::Unavailable),
        "so the caller is failed rather than allowed to continue"
    );

    // An operation no record governs is not gated by this at all.
    assert_eq!(
        orphaned.record_progress(None, crate::ControlProgress::RuntimeBegun),
        Ok(())
    );
}

/// A private instance whose accepted Configure never began a step, with its
/// writer gone: the state the never-started proof is about.
///
/// Composed rather than driven. Real accepted work is routed, but the writer
/// lifecycle is stated through the registry: no writer thread runs and no
/// surface-map failure produces it. An independent review reaches the same
/// state through an actual writer that exits before its first step, and
/// through an unwind injected after a native effect; those are its evidence
/// and not this. What these establish is the owner and phase composition
/// around that state.
#[cfg(unix)]
fn unstarted_after_its_writer_went(
    keeper: &crate::PrivateServiceOwner,
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    transaction: u64,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteChannels,
    XServerFrontendClientRouteRegistration,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (mut private, channels, registration, deliveries) =
        private_with_client(acknowledgements, keeper, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(&keeper.lease(), configure(client, surface, transaction))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending(&keeper.lease()).expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    (private, channels, registration, deliveries)
}

#[test]
fn the_never_started_proof_survives_shutdown_and_reaches_the_retained_handle() {
    let client = XServerFrontendClientId(368);
    let surface = SurfaceId::new(368, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&owner_of_durable, acknowledgements, client, surface, 66001);

    // Shut down without reconciling first. The frontend is consumed, so
    // nothing that only it could do will ever be done.
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    let mut report = private.shutdown();
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "still owed until something settles it");

    // The retained handle applies the same proof.
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    // Driving again finds nothing left, rather than releasing twice.
    assert_eq!(report.reclaim_outstanding(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(
        acks.try_recv().is_err(),
        "and nothing was answered for it"
    );
    // The command it was routed with is still in the queue of the writer that
    // went, which is where it was left. Settling it queued nothing further:
    // nothing is replayed.
    assert!(channels.control.try_recv().is_ok());
    assert!(
        channels.control.try_recv().is_err(),
        "and there is only ever the one"
    );
}

#[test]
fn the_never_started_proof_reaches_the_durable_owner_when_the_handle_goes() {
    let client = XServerFrontendClientId(369);
    let surface = SurfaceId::new(369, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&owner_of_durable, acknowledgements, client, surface, 67001);

    // The only handle goes before anything drives it, so the work is now the
    // durable owner's and the proof has to reach it there.
    drop(private.shutdown());
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(durable.outstanding().expect("a readable owner"), 1);

    assert!(durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "released exactly once");
    assert_eq!(durable.outstanding().expect("a readable owner"), 0);
    // Driven twice, and the second finds nothing.
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_interrupted_operation_keeps_its_credit_through_the_same_transfers() {
    let client = XServerFrontendClientId(370);
    let surface = SurfaceId::new(370, 1);
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
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 68001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Begun and never reported finished: the effect may have happened. Seeded
    // through the registry rather than by interrupting a writer inside its
    // effect, which is an independent review's control and not this one.
    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Through every transfer the never-started one is released by, this one
    // keeps its credit, because nothing about it is established.
    let mut report = private.shutdown();
    assert_eq!(report.reclaim_outstanding(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    drop(report);
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "still owed after the durable owner too");
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert!(acks.try_recv().is_err());
}

#[test]
fn one_instances_reconciliation_does_not_reach_anothers_identical_identity() {
    let client = XServerFrontendClientId(371);
    let surface = SurfaceId::new(371, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();

    // Two instances, each issuing its own registrations from its own counter,
    // so their local identities collide.
    let owner_of_durable = service_owner(&durable, 16);
    let (mine, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&owner_of_durable, acknowledgements.clone(), client, surface, 69001);
    // The same client, surface and transaction as well as the same local
    // completion counter, so nothing but the origin distinguishes the two.
    let theirs_client = client;
    let theirs_surface = surface;
    let owner_of_durable = service_owner(&durable, 16);
    let (mut theirs, _their_channels, _their_registration, _their_deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, theirs_client, theirs_surface);
    let their_registry = theirs
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    theirs
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(theirs_client, theirs_surface, 69001))
        .expect("the shared admission to accept control");
    let ran = theirs.route_pending(&owner_of_durable.lease()).expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(their_token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    their_registry
        .record_progress(their_token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    their_registry.writer_started(theirs_client);
    their_registry.writer_stopped(theirs_client);
    assert_eq!(their_registry.reconcile_client(theirs_client).abandoned, 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 2);

    // Settling mine reaches only mine. The other instance's operation has the
    // same local identity and a different origin, and it is the origin that
    // decides whose records these are.
    let mut report = mine.shutdown();
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        1,
        "the other instance's interrupted operation still owes its credit"
    );
    assert_eq!(
        their_registry.steps_of(their_token).map(|steps| steps.runtime),
        Some(crate::ControlStepState::InProgress),
        "and is untouched"
    );
    drop(theirs);
}

#[test]
fn reconciliation_settles_only_abandoned_operations_with_nothing_still_queued() {
    let client = XServerFrontendClientId(373);
    let surface = SurfaceId::new(373, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");

    // Applying, with no step begun. Its executor is still there, so it is not
    // abandoned and nothing about it is being settled yet.
    let applying = accepted(&registry, configure(client, surface, 70001));
    assert_eq!(
        registry.claim_execution(applying),
        crate::ControlExecutionClaim::Claimed
    );
    assert_eq!(registry.reconcile_unstarted().discharged, 0);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );

    // Abandoned with no step begun, but holding work it queued elsewhere.
    // What it left is not established whatever its own steps say.
    let queued = registry.track_dependent(applying).expect("an applying record");
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    let report = registry.reconcile_unstarted();
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );

    // Only once nothing it started can still run.
    drop(queued);
    assert_eq!(registry.reconcile_unstarted().discharged, 1);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Retired
    );
}

#[test]
fn a_poisoned_owner_still_takes_work_that_has_nowhere_else_to_go() {
    let client = XServerFrontendClientId(374);
    let surface = SurfaceId::new(374, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&owner_of_durable, acknowledgements, client, surface, 71001);
    assert_eq!(durable.reserved(), Some(1));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // Unreadable is not empty. Answering zero here would tell a caller nothing
    // is owed by an owner that cannot say.
    assert_eq!(durable.reserved(), None);
    assert_eq!(durable.owed(), None);
    assert_eq!(durable.outstanding(), None);
    assert_eq!(durable.failed_instances(), None);

    // The handle goes, and its work moves to the owner. This move cannot
    // refuse: the credits were taken before the work was accepted, so the
    // space is already its own and declining would lose both. Doing nothing on
    // a poisoned lock was exactly that loss.
    drop(private.shutdown());
    let held = durable.records_even_if_poisoned();
    assert_eq!(held.outstanding.len(), 1, "the work was taken, not dropped");
    assert_eq!(held.reserved, 1, "and it still holds its credit");
    drop(held);
    assert!(acks.try_recv().is_err());
}

#[test]
fn a_poisoned_owner_still_releases_a_credit_that_is_answered() {
    let durable = crate::PrivateSettlementOwner::with_capacities(2, 2);
    durable.reserve().expect("a fresh owner");
    assert_eq!(durable.reserved(), Some(1));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // A release that does not happen is capacity lost for as long as this
    // owner lives.
    durable.release();
    assert_eq!(durable.records_even_if_poisoned().reserved, 0);

    // But taking one still refuses, because that is a refusal before
    // acceptance and the caller keeps what it has.
    assert!(matches!(
        durable.reserve(),
        Err(crate::AdmissionRefusal::Unavailable)
    ));
    assert!(matches!(
        durable.reserve_failure_slot(),
        Err(crate::AdmissionRefusal::Unavailable)
    ));
}
