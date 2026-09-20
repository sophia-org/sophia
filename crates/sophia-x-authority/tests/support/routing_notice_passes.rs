// Notices and the passes that carry them: the slot each takes, the promoted
// connection that serves nothing yet, and the worker owned before permitted.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_promoted_connection_is_ready_and_serves_nothing() {
    // READY, DRIVEN BY NOBODY. Promotion makes an owner exist. It does not
    // start a worker, receive anything, write anything or answer anything, and
    // this checks each of those rather than the absence of a thread.
    let client = XServerFrontendClientId(8681);
    let (registration, runner, _durable, _output, peer, _keeper) = bound_connection(client);
    let private = runner.frontend.as_ref().expect("a live runner");

    // A capsule is accepted for it before promotion, so there is something a
    // promotion could wrongly consume. Foreign fixture custody: it is here to
    // be work, not an admission to this endpoint.
    let sender = capture_gated_sender(private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(86810);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let finalizer = Arc::downgrade(capsule.finalizer().expect("carried"));
    let frames = order_pass_frames(&capsule);
    gated_send(&sender, capsule).expect("an open endpoint");

    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::Ready
    );
    assert_eq!(payload_shape(&registration), Some("serving"));

    registration
        .ordered_home
        .borrow(|payload| {
            let PrivateOrderedContinuation::Serving { owner, evidence } = payload else {
                panic!("promoted")
            };
            assert!(
                owner.in_flight().is_none() && owner.refused().is_none(),
                "it received nothing"
            );
            assert!(
                owner.retained_unanswered().is_empty() && owner.retained_foreign().is_empty(),
                "and is holding nothing"
            );
            assert!(owner.closing().is_none() && !owner.ending_ended());
            assert_eq!(
                evidence.worker,
                PrivateOrderedWorkerExit::NeverStarted,
                "nothing was started, and that is what is recorded"
            );
            assert!(evidence.fence.is_none() && !evidence.source_poisoned);
        })
        .expect("its own home");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "it wrote nothing"
    );
    assert!(cell.answer().is_none(), "and answered nothing");

    // AND IT RECEIVED NOTHING, which empty slots do not establish: they hold
    // equally after a capsule is received and thrown away. The queue still has
    // the exact one, by the finalizer it was built with.
    assert!(
        finalizer.upgrade().is_some(),
        "nothing dropped it on the way through"
    );
    registration
        .ordered_home
        .borrow(|payload| {
            let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                panic!("promoted")
            };
            let survived = owner
                .queue
                .try_recv()
                .expect("its queue still holds the capsule");
            assert!(Arc::ptr_eq(
                &cell,
                &survived.finalizer().expect("carried").completion
            ));
            assert_eq!(order_pass_frames(&survived), frames);
            assert!(
                owner.queue.try_recv().is_err(),
                "and holds nothing else: nothing was added either"
            );
        })
        .expect("its own home");
    assert!(cell.answer().is_none());
    drop(registration);
}

#[test]
fn a_second_promotion_leaves_the_first_owner_exactly_as_it_was() {
    // THE FIRST OWNER STANDS. A second ask does not rebuild it and does not
    // reset what it has been through: identity, close state, attempt budget
    // and held admissions are the ones it had.
    let client = XServerFrontendClientId(8691);
    let (registration, runner, _durable, _output, _peer, _keeper) = bound_connection(client);
    let private = runner.frontend.as_ref().expect("a live runner");
    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::Ready
    );

    // Give the first owner a history: a close it began, with an attempt spent.
    let first = {
        registration
            .ordered_home
            .borrow(|payload| {
                let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                    panic!("promoted")
                };
                owner.closing = Some(X11OrderedClosing {
                    cause: X11OrderedCloseCause::ConnectionEnded,
                    termination: X11OrderedTermination::Refused(
                        std::io::ErrorKind::PermissionDenied,
                    ),
                    attempts: 2,
                    answered: 0,
                    already: 0,
                    deferred: 0,
                    drained: false,
                });
                (
                    &**owner as *const X11OrderedServingOwner as usize,
                    owner.closing().expect("staged").termination,
                )
            })
            .expect("its own home")
    };

    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::AlreadyServing,
        "the second ask is refused rather than served"
    );
    registration
        .ordered_home
        .borrow(|payload| {
            let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                panic!("still promoted")
            };
            assert_eq!(
                &**owner as *const X11OrderedServingOwner as usize,
                first.0,
                "THE SAME OWNER, not a new one at the same address by luck"
            );
            let closing = owner.closing().expect("its close");
            assert_eq!(closing.attempts, 2, "its attempt budget did not reset");
            assert_eq!(
                closing.termination, first.1,
                "and neither did its close state"
            );
        })
        .expect("its own home");
    drop(registration);
}

#[test]
fn a_closed_endpoint_starts_nothing() {
    // THE LIVE ENDPOINT DECIDES, not what teardown once recorded. Closing this
    // endpoint through the real interface, while the registration lives,
    // leaves the payload's evidence untouched -- that field is teardown's
    // history. An eligibility check reading it admitted an owner onto an
    // endpoint that was already closed.
    let client = XServerFrontendClientId(8701);
    let (registration, runner, _durable, _output, _peer, _keeper) = bound_connection(client);
    let private = runner.frontend.as_ref().expect("a live runner");

    assert_eq!(
        registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established,
        "closed through the real interface"
    );
    assert_eq!(
        registration.ordered_handovers_fenced(),
        Some(true),
        "and the gate says so"
    );
    assert_eq!(
        registration
            .ordered_home
            .borrow(|payload| match payload {
                PrivateOrderedContinuation::Setup { evidence, .. }
                | PrivateOrderedContinuation::Serving { evidence, .. } => evidence.fence,
            }),
        Some(None),
        "while the payload's evidence is untouched, because no teardown ran"
    );

    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::Closing
    );
    assert_eq!(
        payload_shape(&registration),
        Some("transport"),
        "its transport is untouched"
    );
    drop(registration);
}

#[test]
fn an_endpoint_whose_gate_cannot_be_read_starts_nothing_either() {
    // CLOSED AND UNREADABLE ARE DIFFERENT REFUSALS. One says this endpoint is
    // done; the other says a holder panicked inside its gate, so what the flag
    // says cannot be trusted. Neither is a reason to start an owner, and
    // reporting one as the other would send a reader to the wrong question.
    let client = XServerFrontendClientId(8731);
    let (registration, runner, _durable, _output, _peer, _keeper) = bound_connection(client);
    let private = runner.frontend.as_ref().expect("a live runner");

    let gate = registration.ordered_gate.clone();
    let holder = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a holder unwound inside this gate");
    });
    assert!(holder.join().is_err(), "the holder unwound");
    assert_eq!(
        registration.ordered_handovers_fenced(),
        None,
        "the gate is neither open nor closed as far as anything can tell"
    );

    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::EndpointUnreadable
    );
    assert_eq!(
        payload_shape(&registration),
        Some("transport"),
        "and nothing was taken to find that out"
    );
    drop(registration);
}

#[test]
fn a_promoted_connection_torn_down_without_a_worker_retains_what_it_is() {
    // NO WORKER, NO JOIN. A connection that was never served reaches retention
    // by its own quiet path: nothing waits for a join nobody can perform, and
    // no successful join is manufactured to make the account look complete.
    let client = XServerFrontendClientId(8711);
    let (registration, runner, durable, _output, _peer, _keeper) = bound_connection(client);
    assert_eq!(
        registration.promote_ordered_serving(runner.frontend.as_ref().expect("live")),
        PrivateOrderedPromotion::Ready
    );
    let private = runner;

    drop(registration);
    drop(private);
    let reading = durable.retained_dispositions().expect("a readable store")[0]
        .1
        .clone()
        .expect("a readable record");
    assert_eq!(
        reading.closure,
        Some(PrivateHandoverFence::Established),
        "teardown closed the endpoint and carried that"
    );
    assert_eq!(
        reading.worker,
        PrivateOrderedWorkerExit::NeverStarted,
        "and carried that nothing was ever started, rather than a join"
    );
    assert!(!reading.source_poisoned);
}


#[test]
fn a_home_a_holder_panicked_in_stays_unreadable_rather_than_reading_as_ordinary() {
    // THE POISON USED TO HAVE TO TRAVEL. Teardown read the payload out of
    // storage that had panicked and put it in a record with a lock of its own
    // -- a readable one -- so without carrying the fact, the move laundered
    // it: the reading showed an ordinary row for a connection whose storage
    // had been left mid-something.
    //
    // NOTHING MOVES NOW, so nothing launders it. The home a holder panicked in
    // is the home the place holds, and a reader of that place finds it
    // unreadable rather than ordinary. `source_poisoned` is still written --
    // teardown knows the fact and records it where the payload lives -- but it
    // is no longer what carries it, and this control no longer reads it back:
    // the value is inside the storage whose unreadability it describes.
    let client = XServerFrontendClientId(8721);
    let (registration, runner, durable, _output, _peer, _keeper) = bound_connection(client);

    // A holder panics inside this connection's payload storage.
    let storage = Arc::clone(&registration.ordered_home);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        storage.borrow(|_| panic!("a holder unwound inside this payload's storage"))
    }));
    assert!(panicked.is_err(), "the holder unwound");
    assert!(storage.unreadable(), "so the storage is poisoned");

    // TEARDOWN STILL RUNS THROUGH IT. Refusing to act on a poisoned home would
    // strand accepted work to make a point, so the connection is still said to
    // have ended and the place is still accounted for.
    drop(registration);
    drop(runner);
    assert!(storage.unreadable(), "and it is still the same poisoned home");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "the place is still taken"
    );
    assert_eq!(
        durable.continuations_retained(),
        None,
        "and what is in it is unknown rather than absent"
    );
    let reading = durable.retained_dispositions().expect("a readable store");
    assert_eq!(reading.len(), 1, "the connection is still in the account");
    assert!(
        reading[0].1.is_none(),
        "reported as a row nothing can be read from, not as an ordinary one"
    );
}

/// What a connection's notice currently says.
fn wake_snapshot(wake: &Arc<PrivateOrderedWake>) -> (bool, usize, bool) {
    let state = wake
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (state.pending, state.senders, state.gone)
}

#[test]
fn a_waiter_asleep_when_the_last_sender_goes_is_woken_by_it() {
    // THE COUNTEREXAMPLE THIS EXISTS FOR. An owner that finds its queue empty
    // and sleeps cannot notice its senders disappearing: the receive that
    // would tell it is the receive it is not making. Nothing in the channel
    // wakes it, so the disappearance has to be published by whoever causes it.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8751);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let wake = Arc::clone(&channels.ordered.wake);
    assert_eq!(wake_snapshot(&wake), (false, 1, false), "one sender: the row's");

    // A waiter asleep on an empty queue, exactly as an owner would be.
    let waiting = Arc::clone(&wake);
    let (woken, wakes) = sync_channel(1);
    let waiter = std::thread::spawn(move || {
        let mut state = waiting.state.lock().expect("a readable notice");
        while !state.pending && !state.gone {
            state = waiting.ready.wait(state).expect("a readable notice");
        }
        woken.send(state.gone).expect("the control is listening");
    });
    // WHAT THIS OBSERVES: no answer has arrived. It does NOT establish that
    // the waiter reached the condvar, or even that it has been scheduled --
    // an answer that has not arrived looks the same from here whatever the
    // waiter is doing. Arrival needs a handshake this control does not have.
    assert_eq!(
        wakes.recv_timeout(std::time::Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "no answer yet"
    );

    // Its senders go. The row's is the last one.
    drop(registration);
    assert_eq!(
        wakes.recv_timeout(std::time::Duration::from_secs(5)),
        Ok(true),
        "and the disappearance woke it"
    );
    waiter.join().expect("the waiting thread");

    // THE NOTICE IS A HINT, NOT A FINDING. What establishes that the producers
    // are finished is a receive, and this one is the parent's, after the
    // waiter joined -- so it says the channel is finished, not that the waiter
    // saw it finished at the moment it woke.
    assert!(matches!(
        channels.ordered.receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn the_notice_is_published_only_after_the_last_sender_is_actually_gone() {
    // A DROP BODY RUNS BEFORE ITS FIELDS ARE DESTROYED. Publishing from there
    // and letting the sender field fall away afterwards says "all gone" while
    // this one still exists: a waiter woken then receives, finds the queue
    // merely empty rather than finished, and sleeps again -- and the drop that
    // really ends it signals nobody.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8761);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let wake = Arc::clone(&channels.ordered.wake);

    // A waiter that does what an owner does: on waking, it RECEIVES, and
    // reports what the channel told it.
    let waiting = Arc::clone(&wake);
    let (answered, answers) = sync_channel(1);
    let receiver = channels.ordered.receiver;
    let waiter = std::thread::spawn(move || {
        let mut state = waiting.state.lock().expect("a readable notice");
        while !state.pending && !state.gone {
            state = waiting.ready.wait(state).expect("a readable notice");
        }
        drop(state);
        answered
            .send(matches!(
                receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            ))
            .expect("the control is listening");
    });

    drop(registration);
    assert_eq!(
        answers.recv_timeout(std::time::Duration::from_secs(5)),
        Ok(true),
        "woken by the notice, it finds the channel FINISHED and not merely empty"
    );
    waiter.join().expect("the waiting thread");

    // WHAT THIS DOES NOT ESTABLISH. If the notice were published first and the
    // sender dropped immediately after, this waiter would still almost always
    // find the channel finished: it has to be woken, reacquire the notice and
    // then receive, and the drop it is racing is the next instruction. The
    // window is real but not observable from here without a rendezvous inside
    // that Drop, which this crate has no way to place.
    //
    // So the order is written the way it is because the reasoning says so --
    // a Drop body runs before its fields are destroyed -- and not because this
    // control could tell the difference.
}

#[test]
fn a_clone_going_is_not_the_senders_going() {
    // A NOTICE PER DISAPPEARANCE, NOT PER DROP. Producers clone a connection's
    // sender constantly; saying the senders are gone each time one of those
    // clones is dropped would wake an owner to find a queue that is perfectly
    // alive, over and over.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8771);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let wake = Arc::clone(&channels.ordered.wake);

    // Producers take copies the way the real ones do.
    let first = capture_gated_sender(&private, client);
    let second = capture_gated_sender(&private, client);
    assert_eq!(wake_snapshot(&wake), (false, 3, false));

    drop(first);
    assert_eq!(
        wake_snapshot(&wake),
        (false, 2, false),
        "one producer finished, and the connection is not"
    );
    drop(second);
    assert_eq!(wake_snapshot(&wake), (false, 1, false));

    // The exact capsule on the queue is untouched by any of that.
    let sender = capture_gated_sender(&private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(87710);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    gated_send(&sender, capsule).expect("an open endpoint");
    drop(sender);
    assert_eq!(wake_snapshot(&wake), (false, 1, false));
    // Taken here, BEFORE the last sender goes. So what this shows is that
    // producers finishing does not disturb queued work -- not that work
    // survives the final disappearance, which is a different moment and is
    // not covered by this control.
    let survived = channels
        .ordered
        .receiver
        .try_recv()
        .expect("still queued, and still exactly itself");
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));

    drop(registration);
    assert!(wake_snapshot(&wake).2, "and only the last one says so");
}

#[test]
fn a_refused_publication_does_not_disturb_the_live_connections_notice() {
    // ISOLATION IS WHAT THIS ESTABLISHES. A publication that refuses drops the
    // senders it had built, and those were its own: this connection's notice
    // is not touched by them going.
    //
    // It does NOT observe the refused attempt's own notice. That notice is
    // minted inside the call that refuses and goes with it, so reading it
    // needs a hold on the mint itself, which this control does not have.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8781);
    let (first, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let wake = Arc::clone(&channels.ordered.wake);

    // A duplicate: publication refuses after its senders were made.
    let refused = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .err()
        .expect("the client is already registered");
    assert!(matches!(
        refused,
        XServerFrontendRouteError::DuplicateClient { .. }
    ));

    // ISOLATION: the refused attempt's senders were its own. This connection's
    // notice is untouched by them going.
    assert_eq!(
        wake_snapshot(&wake),
        (false, 1, false),
        "the live connection still has its own sender and is not finished"
    );
    drop(first);
    assert!(wake_snapshot(&wake).2);
}


#[test]
fn a_promoted_owner_keeps_the_notice_its_senders_publish_to() {
    // TAKING THE RECEIVER LEAVES ITS WRAPPER BEHIND, and the notice with it.
    // An owner built that way would hold a queue it could be told about and no
    // way to be told: every disappearance would be published to a notice
    // nothing was holding.
    let client = XServerFrontendClientId(8791);
    let (registration, runner, _durable, _output, _peer, _keeper) = bound_connection(client);
    let private = runner.frontend.as_ref().expect("a live runner");

    // The notice the senders were counted against, taken from the row.
    let minted = {
        let guard = private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry");
        Arc::clone(&guard.get(&client).expect("its row").ordered.wake)
    };

    assert_eq!(
        registration.promote_ordered_serving(private),
        PrivateOrderedPromotion::Ready
    );

    let before = registration
        .ordered_home
        .borrow(|payload| {
            let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                panic!("promoted")
            };
            assert!(
                Arc::ptr_eq(&owner.wake, &minted),
                "THE SAME NOTICE, not a fresh one nothing publishes to"
            );
            // And it is live: a disappearance published by the senders reaches
            // it.
            wake_snapshot(&owner.wake)
        })
        .expect("its own home");
    assert!(!before.2 && before.1 > 0);
    drop(runner);
    drop(registration);
    assert!(
        wake_snapshot(&minted).2,
        "the senders going published to the notice the owner is holding"
    );
}


#[test]
fn a_notice_published_before_a_waiter_arrives_is_still_there_to_find() {
    // A SIGNAL IS NOT RETAINED; A LEVEL IS. This is the case a bare signal
    // loses: the handover happens while the owner is between looking at its
    // queue and entering its wait, so the signal reaches nobody. What the
    // owner finds when it does arrive is the level, and that is why the level
    // is published rather than only signalled.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8801));
    attempt_run(&mut f, 88010, 272, true);
    let wake = Arc::clone(&f.channels.ordered.wake);
    {
        let mut state = wake.state.lock().expect("a readable notice");
        state.pending = false;
    }

    // The handover happens with nobody waiting at all.
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));

    // A waiter arriving afterwards finds it set, and does not wait.
    let state = wake.state.lock().expect("a readable notice");
    assert!(
        state.pending,
        "the notice is still there for a waiter that had not arrived yet"
    );
}

#[test]
fn a_handover_interrupted_after_acceptance_still_publishes_its_notice() {
    // ACCEPTED WORK MUST NOT BE LEFT ASLEEP. A handover can be accepted and
    // then unwind before anything is written down; a notification made only on
    // success would be the one not made in exactly that case.
    //
    // THE UNWIND IS STAGED AT THE GUARD, not inside the producer: this crate
    // has no way to panic mid-producer without a hook, so the control arms the
    // real notice, sends through the real admission, and then unwinds.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8811);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let wake = Arc::clone(&channels.ordered.wake);
    {
        let mut state = wake.state.lock().expect("a readable notice");
        state.pending = false;
    }
    let sender = capture_gated_sender(&private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(88110);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let notify = sender.arm_wake();
        let admitted = sender.admit().expect("an open endpoint");
        admitted.try_send(capsule).expect("accepted");
        let _ = &notify;
        panic!("interrupted between acceptance and the owned report");
    }));
    assert!(unwound.is_err(), "it unwound");

    assert!(
        wake.state.lock().expect("a readable notice").pending,
        "and the notice was published on the way out"
    );
    // The accepted capsule is on the queue, exactly itself, unanswered.
    let arrived = channels
        .ordered
        .receiver
        .try_recv()
        .expect("accepted before the unwind");
    assert!(Arc::ptr_eq(
        &cell,
        &arrived.finalizer().expect("carried").completion
    ));
    assert!(cell.answer().is_none());
    drop(registration);
}

#[test]
fn a_refused_handover_publishes_a_recheck_and_keeps_its_capsule() {
    // A REFUSAL IS STILL A REASON TO LOOK, and costs a waiter one look at its
    // own queue. What it is not is evidence: nothing was accepted here, and
    // the capsule is where it was.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8821));
    attempt_run(&mut f, 88210, 272, true);
    let cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 88210);
    let wake = Arc::clone(&f.channels.ordered.wake);
    assert_eq!(
        f.registration.fence_ordered_handovers(),
        PrivateHandoverFence::Established
    );
    {
        let mut state = wake.state.lock().expect("a readable notice");
        state.pending = false;
    }

    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(
        private.dispatch_one_press(),
        Some(false),
        "the endpoint is closed, so nothing is handed over"
    );
    assert!(
        wake.state.lock().expect("a readable notice").pending,
        "and the notice is published anyway: a recheck request, not evidence"
    );
    assert_eq!(
        handover_phase(private, &cell),
        Some(PrivateDispatchPhase::Pending),
        "with the capsule still offerable"
    );
    assert!(cell.answer().is_none());
    assert!(f.channels.ordered.try_recv().is_err(), "and nothing queued");
}

#[test]
fn publishing_a_notice_needs_nothing_that_a_handover_holds() {
    // THE NOTICE TAKES ITS OWN LOCK AND NO OTHER. If publishing reached for
    // the gate, the payload, the output, common or the client table, it would
    // be doing so on a path where somebody already holds one of them -- and
    // the one it would most obviously reach for is the gate it was armed
    // beside.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8831);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let sender = capture_gated_sender(&private, client);
    let wake = Arc::clone(&channels.ordered.wake);
    {
        let mut state = wake.state.lock().expect("a readable notice");
        state.pending = false;
    }

    // The gate is held by something else entirely, and the client table too.
    let entered = registration
        .ordered_gate
        .entered()
        .unwrap_or_else(|_| panic!("an open endpoint"));
    let clients = private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry");

    // Publishing completes regardless.
    drop(sender.arm_wake());
    assert!(
        wake.state.lock().expect("a readable notice").pending,
        "published while the gate and the client table are both held elsewhere"
    );
    drop(clients);
    drop(entered);
    drop(registration);
}


/// An attention roll with room for two connections, and both admitted.
fn attention_for_two() -> (
    Arc<PrivateAttention>,
    PrivateAttentionIdentity,
    PrivateAttentionIdentity,
    std::time::Instant,
) {
    let now = std::time::Instant::now();
    let roll = Arc::new(
        PrivateAttention::with_connections(NonZeroUsize::new(2).unwrap(), now)
            .expect("room for two connections"),
    );
    let first = roll.admit(0).expect("a slot");
    let second = roll.admit(1).expect("a slot");
    (roll, first, second, now)
}

#[test]
fn a_release_during_a_failed_attempt_is_not_parked_away() {
    // THE ERASURE THIS EXISTS TO PREVENT. A pass takes a slot, fails to take
    // the record, and parks it -- while the holder released the record during
    // that very attempt. Parking it then buries the one event that says it is
    // worth trying again, and only a timer would ever find it.
    //
    // The interest is armed by the claim itself, before the attempt: the slot
    // is in flight from that moment, so the release lands on something that
    // remembers it.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    assert_eq!(roll.waiting(), Some(1));

    let claim = roll.claim_next().expect("a slot waiting");
    assert_eq!(claim.who(), first);
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::InFlight { dirty: false })
    );

    // The holder releases while the attempt is in flight.
    assert!(roll.released(first));
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::InFlight { dirty: true })
    );

    // And the attempt then fails.
    assert!(claim.could_not());
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::Ready),
        "the release survived the parking that followed it"
    );
    assert_eq!(roll.waiting(), Some(1));
}

#[test]
fn a_notice_arriving_during_a_successful_pass_outlives_its_conclusion() {
    // The same rule on the other outcome: a pass that succeeded concluded
    // about what it saw, and something arrived after it looked.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    let claim = roll.claim_next().expect("a slot waiting");
    assert!(roll.flag(first));
    assert!(claim.took_it());
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::Ready),
        "done with what it saw, and there is something newer"
    );
}

#[test]
fn a_failed_attempt_with_nothing_new_parks_rather_than_spins() {
    // WHAT STOPS THE SPIN. A slot that could not be taken is not counted as
    // waiting, so a supervisor's predicate goes false and it has something to
    // sleep on. A pass that re-marked it ready would take it again at once,
    // for ever.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    let claim = roll.claim_next().expect("a slot waiting");
    assert!(claim.could_not());
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Deferred));
    assert_eq!(roll.waiting(), Some(0), "nothing is waiting to be looked at");
    assert!(
        roll.claim_next().is_none(),
        "and a pass finds nothing to take"
    );
}

#[test]
fn a_pass_that_is_abandoned_parks_its_slot_rather_than_finishing_it() {
    // A claim dropped without an outcome reported nothing. Treating that as
    // done would record a connection as looked at by a pass that did not
    // finish; it is parked instead, and anything that arrived still revives it.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    drop(roll.claim_next().expect("a slot waiting"));
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Deferred));
}

#[test]
fn a_stale_pass_cannot_conclude_about_the_slot_it_no_longer_holds() {
    // A slot can be retired and given to someone else while a pass is in
    // flight. A conclusion written then would be written about a connection
    // that pass never saw.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    let stale = roll.claim_next().expect("a slot waiting");

    // Its occupant goes, and the slot is given to another.
    assert!(roll.retire(first, false));
    let successor = roll.admit(0).expect("the slot again");
    assert_ne!(successor.generation, first.generation);
    assert!(roll.flag(successor));

    // The successor has a pass of its own in flight, which is the state a
    // stale conclusion could actually damage: anything else is refused by the
    // state check before the identity is even consulted.
    let current = roll.claim_next().expect("the successor's own pass");
    assert_eq!(current.who(), successor);
    assert!(roll.flag(successor));
    assert_eq!(
        roll.state_of(successor),
        Some(PrivateAttentionState::InFlight { dirty: true })
    );

    assert!(
        !stale.took_it(),
        "the stale pass is refused rather than applied"
    );
    assert_eq!(
        roll.state_of(successor),
        Some(PrivateAttentionState::InFlight { dirty: true }),
        "and the successor's own pass is exactly where it was"
    );

    // Which its own conclusion then finishes, still carrying what arrived.
    assert!(current.took_it());
    assert_eq!(roll.state_of(successor), Some(PrivateAttentionState::Ready));
    assert_eq!(roll.waiting(), Some(1));
}

#[test]
fn a_slot_whose_generations_are_spent_is_retired_rather_than_wrapped() {
    // A WRAPPED GENERATION IS A STALE NOTICE THAT PASSES THE CHECK. One
    // connection's worth of capacity is a smaller cost than an identity that
    // lies about who it is.
    let now = std::time::Instant::now();
    let roll = Arc::new(
        PrivateAttention::with_connections(NonZeroUsize::new(1).unwrap(), now)
            .expect("room for one"),
    );
    {
        let mut held = roll.roll.lock().expect("readable");
        held.slots[0].next_generation = Some(u32::MAX);
    }
    let last = roll.admit(0).expect("the last generation");
    assert_eq!(last.generation, u32::MAX);

    // ASKED AT THE HANDING-OUT, not only at the giving-up: a slot sitting at
    // the last generation refuses to name another occupant at all, so nothing
    // depends on a retirement having happened first.
    assert!(
        roll.admit(0).is_none(),
        "there is no next name for this slot"
    );

    assert!(roll.retire(last, false));
    assert!(
        roll.admit(0).is_none(),
        "and giving it up does not make one either"
    );
}

#[test]
fn the_sweep_deadline_is_not_pushed_back_by_other_traffic() {
    // A DEADLINE RECOMPUTED ON EVERY WAKE IS NEVER REACHED under load, and
    // load is exactly when work nobody revived has been waiting longest.
    let (roll, first, second, now) = attention_for_two();
    assert!(roll.flag(first));
    let claim = roll.claim_next().expect("a slot waiting");
    assert!(claim.could_not());
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Deferred));

    // Unrelated traffic, repeatedly, at ADVANCING times -- all before the
    // deadline. Asking with one frozen instant would pass equally against a
    // deadline that was being pushed back on every ask, which is the thing
    // this is about.
    for step in 1..=6 {
        assert!(roll.flag(second));
        let other = roll.claim_next().expect("the other slot");
        assert!(other.took_it());
        let later = now + std::time::Duration::from_millis(40 * step);
        assert!(
            !roll.sweep_due(later),
            "not due yet at {}ms, and not rescheduled by any of this",
            40 * step
        );
    }
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::Deferred),
        "still parked, because nothing revived it"
    );

    // The deadline arrives on its own schedule, unmoved by any of that.
    assert!(roll.sweep_due(now + std::time::Duration::from_millis(250)));
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Ready));
    assert_eq!(roll.waiting(), Some(1));
}


#[test]
fn a_second_admission_does_not_move_into_a_live_connections_place() {
    // OCCUPANCY IS NOT A STATE. A slot with nothing to do and a slot with
    // nobody in it look identical from the state alone, and treating them as
    // one let a second admission walk into a live connection's place -- taking
    // its name, its readiness and everything it had been told, while the
    // connection it took them from was still there.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Ready));
    assert_eq!(roll.waiting(), Some(1));

    assert!(
        roll.admit(0).is_none(),
        "somebody lives here, so there is nothing to hand out"
    );
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::Ready),
        "and the occupant keeps its name and its notice"
    );
    assert_eq!(roll.waiting(), Some(1), "and its readiness");

    // The same while a pass is in flight, which is the worse case: a pass with
    // a claim outstanding for an occupant that was replaced under it.
    let claim = roll.claim_next().expect("a slot waiting");
    assert!(roll.admit(0).is_none());
    assert_eq!(claim.who(), first);
    assert!(claim.took_it());
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Idle));
}

#[test]
fn a_retired_occupants_notices_stop_counting_immediately() {
    // GONE AT ONCE, not when a successor arrives. Leaving the name valid in
    // between let a late notice make an empty slot ready, and a pass then took
    // it -- work invented for a connection that had already left, before
    // anybody had moved in.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.retire(first, false));
    assert_eq!(
        roll.state_of(first),
        None,
        "its name means nothing the moment it goes"
    );

    // The interval with nobody in the slot at all.
    assert!(
        !roll.released(first),
        "a release for somebody who has gone is refused"
    );
    assert!(!roll.flag(first), "and so is a notice");
    assert_eq!(roll.waiting(), Some(0));
    assert!(
        roll.claim_next().is_none(),
        "so no pass is manufactured for an empty slot"
    );

    // Retiring again says nothing happened, because nothing did.
    assert!(!roll.retire(first, false), "there is nobody to retire");

    // And a successor is a different connection, not a continuation.
    let successor = roll.admit(0).expect("the slot is free now");
    assert_ne!(successor.generation, first.generation);
    assert!(!roll.flag(first), "the old name still means nothing");
    assert_eq!(roll.waiting(), Some(0));
    assert!(roll.flag(successor));
    assert_eq!(roll.waiting(), Some(1));
}

#[test]
fn a_pass_outstanding_when_its_occupant_goes_concludes_about_nobody() {
    // A claim held across a retirement is a pass for a connection that has
    // left. Whatever it reports -- and whether it reports at all, or is simply
    // dropped -- it must not touch the slot it no longer holds.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.flag(first));
    let outstanding = roll.claim_next().expect("a slot waiting");
    assert!(roll.retire(first, false));

    // Dropped without an outcome: the abandoned-pass path, over a slot with
    // nobody in it.
    drop(outstanding);
    assert_eq!(roll.state_of(first), None);
    assert_eq!(roll.waiting(), Some(0));
    assert!(
        roll.claim_next().is_none(),
        "nothing was parked, because there was nobody to park"
    );

    // And the same for a pass that does report.
    let successor = roll.admit(0).expect("the slot is free");
    assert!(roll.flag(successor));
    let reporting = roll.claim_next().expect("the successor's pass");
    assert!(roll.retire(successor, false));
    assert!(!reporting.could_not(), "refused, not applied");
    assert_eq!(roll.waiting(), Some(0));
}


#[test]
fn no_pass_is_made_for_a_slot_with_nobody_in_it() {
    // DEFENCE IN DEPTH, AND STAGED AS SUCH. Retirement clears the state as
    // well as the occupant, so a slot that is waiting with nobody in it is not
    // reachable through the ordinary operations -- this control sets that
    // state directly to check the guard that would catch it if some later path
    // did produce it. Manufacturing a pass for an empty slot hands a
    // supervisor a connection that is not there.
    let (roll, first, _second, _now) = attention_for_two();
    assert!(roll.retire(first, false));
    {
        let mut held = roll.roll.lock().expect("readable");
        held.slots[0].state = PrivateAttentionState::Ready;
        held.ready = 1;
    }
    assert_eq!(roll.waiting(), Some(1), "the roll believes one is waiting");
    assert!(
        roll.claim_next().is_none(),
        "and no pass is made for it, because nobody lives there"
    );
}


/// The pieces a startup transaction needs, with nothing started.
fn startup_fixture() -> (
    Mutex<PrivateWorkerSlot>,
    Arc<AtomicBool>,
    Arc<PrivateOrderedWake>,
) {
    (
        Mutex::new(PrivateWorkerSlot::empty()),
        Arc::new(AtomicBool::new(false)),
        Arc::new(PrivateOrderedWake::for_first_sender()),
    )
}

/// A worker body that waits for a permit or a stop and reports which it saw.
fn permit_waiter(
    stop: Arc<AtomicBool>,
    wake: Arc<PrivateOrderedWake>,
    saw: std::sync::mpsc::SyncSender<&'static str>,
) -> impl FnOnce() + Send + 'static {
    move || {
        // A POISONED NOTICE IS RECOVERED HERE, NOT UNWRAPPED. A worker that
        // panicked on one would take a cancellation it was meant to observe
        // and turn it into a second failure. What a real worker does with a
        // poisoned notice -- leave and report it -- is policy that belongs
        // with the worker, which is not landed; this fixture only has to stay
        // alive long enough to see its stop.
        let mut state = wake
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !state.started && !stop.load(std::sync::atomic::Ordering::Acquire) {
            state = wake
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        // STOP IS ASKED FIRST AND WINS. A permit says a transaction finished,
        // never that a worker should still be running.
        let outcome = if stop.load(std::sync::atomic::Ordering::Acquire) {
            "stopped"
        } else {
            "permitted"
        };
        drop(state);
        saw.send(outcome).expect("the control is listening");
    }
}

#[test]
fn a_started_worker_is_owned_before_it_is_permitted() {
    // THE PERMIT IS LAST. A worker scheduled the instant it exists cannot
    // serve before its handle is owned, because what it waits for is published
    // after the handle is stored.
    //
    // THAT ORDER IS NOT OBSERVED HERE, and cannot be from outside: the
    // transaction holds the destination throughout, so nothing else can look
    // at the slot while it runs, and by the time a permitted worker reports,
    // the store has happened either way. It rests on the two being adjacent in
    // one function with nothing between them. What this control does observe
    // is the rest: that the notice is free while the thread is made, that the
    // handle is owned afterwards, and that the worker was permitted rather
    // than stopped.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);

    // OBSERVED FROM INSIDE THE SPAWN: the destination is held and the notice
    // is not, so stopping and waking this connection do not queue behind
    // somebody else's thread creation.
    let notice_free = Arc::new(AtomicBool::new(false));
    let seen_free = Arc::clone(&notice_free);
    let spawning = Arc::clone(&wake);
    let outcome = start_connection_worker(&slot, &stop, &wake, move || {
        seen_free.store(spawning.state.try_lock().is_ok(), std::sync::atomic::Ordering::Release);
        std::thread::Builder::new().spawn(body)
    });
    assert_eq!(outcome, PrivateStartupOutcome::Started);
    assert!(
        notice_free.load(std::sync::atomic::Ordering::Acquire),
        "the notice was free while the thread was being made"
    );

    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );
    let handle = slot
        .lock()
        .expect("readable")
        .handle
        .take()
        .expect("its handle is owned here");
    handle.join().expect("the worker");
    assert!(!stop.load(std::sync::atomic::Ordering::Acquire));
}
