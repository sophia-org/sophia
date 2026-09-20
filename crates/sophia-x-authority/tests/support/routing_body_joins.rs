// Worker bodies and joins: the body that finds nothing and is woken by an
// actual producer, and the second fencing that asks and replaces nothing.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_body_that_finds_nothing_waits_and_is_woken_by_an_actual_producer() {
    // THE REAL WAKE PATH, AND THE ANTI-SPIN CONTROL IN ONE. An idle body waits
    // on this connection's notice; what ends the wait is the production
    // producer publishing to that same notice, not a timeout and not a poll.
    //
    // AND IT WAITS RATHER THAN RETURNING AT ONCE. A body that woke on the
    // sticky permit, or on a level nothing cleared, would not have needed this
    // control's level to get through its wait. WHAT IS OBSERVED is that the
    // level was consumed and the body had not left. That it still has steps in
    // hand is NOT observed -- a paused body looks the same -- and nothing here
    // rests on it.
    let mut f = worker_fixture(XServerFrontendClientId(8393));
    f.permit();
    attempt_run(&mut f.fixture, 83930, 272, true);
    let cell = admitted_cell(f.fixture.runner.frontend.as_ref().unwrap(), 83930);
    let exit = PrivateWorkerExit::unstarted();
    let (home, wake, stop, sequence) = f.handles();

    std::thread::scope(|scope| {
        let exit = &exit;
        let worker = scope.spawn(move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit,
                steps: 8,
            }
            .run()
        });
        let _stopper = PrivateWorkerStopper(&f.stop, &f.wake);
        // IT WENT THROUGH A WAIT, established rather than timed: a level
        // published under that wait's own mutex, consumed only by a waiter.
        assert!(
            waited_and_consumed(&f.wake),
            "the body reached its idle wait"
        );
        assert!(
            !exit.left(),
            "and has not left, so it did not run its budget out on nothing"
        );
        assert!(cell.answer().is_none(), "having served nothing");

        // The production producer, which arms this connection's notice itself.
        assert_eq!(
            f.fixture
                .runner
                .frontend
                .as_mut()
                .unwrap()
                .dispatch_one_press(),
            Some(true)
        );
        // Its bytes reach the peer, which is what says the wake produced a
        // serve rather than merely a return.
        let mut seen = [0u8; 32];
        std::io::Read::read_exact(&mut (&f.peer), &mut seen)
            .expect("its peer reads what the woken body sent");
        assert_eq!(seen[0], 4);
        // ITS RECEIPT, BEFORE THE CANCELLATION. Finalising is the step after
        // the one that finishes the write, so a body stopped between them
        // would be asked for something nobody had written.
        assert!(
            published(&cell),
            "the woken body answered for what it served"
        );
        cancel_connection_worker(&f.stop, &f.wake);
        let outcome = worker.join().expect("the body finished");
        assert!(
            stopped_by_cancellation(&outcome),
            "the owner's own word, whichever of the two saw the stop: {outcome:?}"
        );
    });
    assert!(
        f.wake.state.lock().expect("a readable notice").started,
        "while the permit stayed set, having never been a reason to wake"
    );
    drop(f.fixture);
}

#[test]
fn a_body_told_to_stop_while_idle_asks_its_owner_what_that_means() {
    // A STOP IS A TRIGGER, NOT A RESULT. Reading the flag back as `Stopped`
    // would report a wire outcome nobody established; the owner is asked once,
    // and what it says is kept beside the trigger rather than instead of it.
    let f = worker_fixture(XServerFrontendClientId(8396));
    f.permit();
    let exit = PrivateWorkerExit::unstarted();
    std::thread::scope(|scope| {
        let (home, wake, stop, sequence) = f.handles();
        let exit = &exit;
        let worker = scope.spawn(move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit,
                steps: 16,
            }
            .run()
        });
        let _stopper = PrivateWorkerStopper(&f.stop, &f.wake);
        // A WAIT HAPPENED FIRST, so this is a stop told to a body that got
        // past startup -- not one that happened to land before it. Where the
        // body is when the stop arrives is not established and nothing here
        // rests on it: a stop is asked before every visit and inside every
        // wait, so either finds it.
        assert!(
            waited_and_consumed(&f.wake),
            "the body has been through its idle wait"
        );
        // THROUGH THE PRODUCTION CANCELLATION, which is how a running worker
        // is told: a bare write beside the predicate mutex can be lost by a
        // waiter between its check and its wait.
        cancel_connection_worker(&f.stop, &f.wake);
        let outcome = worker.join().expect("the body finished");
        // EITHER OF TWO LEGAL ANSWERS. A body told to stop before its own
        // check departs itself; one told between that check and its visit has
        // the owner observe the stop instead. Requiring one of them asserts a
        // schedule. What both carry is the owner's own word, which is what
        // this control is about.
        assert!(
            stopped_by_cancellation(&outcome),
            "the owner is what said so: {outcome:?}"
        );
    });
    drop(f.fixture);
}

#[test]
fn a_body_that_cannot_read_its_notice_stops_the_connection_and_asks_once() {
    // A NOTICE NOBODY STANDS BEHIND IS NOT A WAIT. Recovering the guard and
    // sleeping on it would put this worker to sleep on a level a panic left,
    // with nothing able to wake it. So the same authoritative stop is set and
    // the notice woken -- the connection's own stop, not a fresh flag -- and
    // the owner is asked once for its departure.
    for while_running in [false, true] {
        let f = worker_fixture(XServerFrontendClientId(8397));
        f.permit();
        let exit = PrivateWorkerExit::unstarted();
        if while_running {
            // POISONED WHILE THE BODY IS RUNNING, after it has been through a
            // wait. WHICH ACQUISITION FINDS IT IS NOT ESTABLISHED: consuming
            // the handshake level ends a wait, so the body may take another
            // ordinary visit and meet the poison on the next wait's initial
            // lock rather than on a Condvar reacquisition. Both are the same
            // classification and this control does not separate them. That the
            // reacquisition branch itself is taken is established elsewhere,
            // not here.
            std::thread::scope(|scope| {
                let body = f.body(&exit, 16);
                let worker = scope.spawn(move || body.run());
                assert!(
                    waited_and_consumed(&f.wake),
                    "the body has been through its idle wait"
                );
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _held = f.wake.state.lock().expect("a readable notice");
                        f.wake.ready.notify_all();
                        panic!("a holder unwound inside this connection's notice");
                    }))
                    .is_err(),
                    "the holder unwound"
                );
                let outcome = worker.join().expect("the body finished");
                assert_eq!(outcome.trigger, PrivateWorkerTrigger::NoticeUnreadable);
            });
        } else {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _held = f.wake.state.lock().expect("a readable notice");
                    panic!("a holder unwound inside this connection's notice");
                }))
                .is_err(),
                "the holder unwound"
            );
            let outcome = f.body(&exit, 16).run();
            assert_eq!(outcome.trigger, PrivateWorkerTrigger::NoticeUnreadable);
            assert_eq!(
                outcome.last,
                Some(PrivateWorkerAsk::Said(X11OrderedServeStep::Stopped)),
                "the owner's own word about a connection now stopped"
            );
        }
        assert!(
            f.stop.load(Ordering::SeqCst),
            "the connection's own stop is what was set"
        );
        drop(f.fixture);
    }
}

#[test]
fn a_body_ends_on_the_owners_word_when_the_last_sender_disappears() {
    // GONE IS A HINT, AND THE OWNER'S RECEIVE IS THE FINDING. That every
    // sender has disappeared is a reason to look again; what says the queue is
    // finished is the owner receiving from it, and the outcome it gives is the
    // owner's rather than a flag this body read back.
    //
    // THE LAST SENDER GOES WITHOUT THE REGISTRATION GOING. The registry's own
    // routing removes a client's row when its protocol receiver has gone, so
    // dropping that receiver and driving the real routing seam takes the last
    // counted wrapper with it. No table surgery, no invented level, and no
    // registration torn down under a body still borrowing its home -- which is
    // the integration boundary, not this body's to cross.
    let f = worker_fixture(XServerFrontendClientId(8404));
    f.permit();
    let exit = PrivateWorkerExit::unstarted();
    let sender = f.sender;
    let protocol = f.fixture.channels.protocol;
    let (home, wake, stop, sequence) = (
        Arc::clone(&f.home),
        Arc::clone(&f.wake),
        Arc::clone(&f.stop),
        Arc::clone(&f.sequence),
    );
    let outcome = std::thread::scope(|scope| {
        let exit = &exit;
        let worker = scope.spawn(move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit,
                steps: 16,
            }
            .run()
        });
        let _stopper = PrivateWorkerStopper(&f.stop, &f.wake);
        assert!(
            waited_and_consumed(&f.wake),
            "the body is in its idle wait before anything disappears"
        );

        // The connection's protocol receiver goes, and the ordered sender this
        // control was holding with it.
        drop((protocol, sender));
        // THE REAL ROUTING SEAM: it finds the protocol queue disconnected and
        // removes the row, which is what releases the last ordered wrapper.
        f.fixture
            .runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .route_protocol(
                f.fixture.client,
                XClientEvent::UnmapNotify {
                    sequence: 1,
                    event: f.fixture.window,
                    window: f.fixture.window,
                    from_configure: false,
                },
            )
            .expect("a disconnected protocol queue is not this caller's error");
        let outcome = worker.join().expect("the body finished");
        // Read HERE, before the control's own cleanup stops anything: what is
        // being asked is whether the body ended on the owner's word without
        // the connection having been told to stop.
        (outcome, f.stop.load(Ordering::SeqCst), f.home.standing())
    });
    let (outcome, stopped, standing) = outcome;
    assert_eq!(outcome.trigger, PrivateWorkerTrigger::OwnerStep);
    assert_eq!(
        outcome.last,
        Some(PrivateWorkerAsk::Said(X11OrderedServeStep::Ended {
            outcome: XAuthorityInputDeliveryOutcome::ClientDisconnected,
            shutdown: true,
        })),
        "the owner's own ending, with its exact outcome and what it shut down"
    );
    // The connection is still live and was never told to stop: what ended was
    // its queue, established by the owner receiving from it.
    assert_eq!(standing, PrivateHomeStanding::Live);
    assert!(!stopped, "and nothing had told the connection to stop");
    drop(f.fixture.registration);
}

#[test]
fn a_departure_that_cannot_ask_its_owner_says_so_rather_than_nothing() {
    // A REFUSAL IS NOT AN ABSENCE. When a departure's ask cannot be made --
    // the home unreadable by the time the body is told to stop -- reporting
    // "nothing said" loses the one fact a caller has to act on, and leaves the
    // departure looking ordinary.
    //
    // THE SEAM IS CALLED DIRECTLY, and the control says so rather than
    // pretending to reach it through a running body. Poisoning a home under a
    // running worker does not decide which of two legal things happens: a body
    // that meets the poison on its next ordinary visit refuses eligibly and
    // never departs at all, which is correct and is not what this is about.
    // Waiting for a schedule to produce the one is a race, not a witness.
    //
    // WHAT IS REAL HERE: the connection, its owner, its home, the poison and
    // the cancellation. What is staged is only which of the body's own
    // entry points is entered.
    let f = worker_fixture(XServerFrontendClientId(8405));
    f.permit();
    let exit = PrivateWorkerExit::unstarted();
    let body = f.body(&exit, 16);
    assert!(
        body.credentials().is_ok(),
        "this body may serve this connection, a moment before it cannot"
    );

    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = f.home.state.lock().expect("a readable home");
            panic!("a holder unwound inside this connection's home");
        }))
        .is_err(),
        "the holder unwound"
    );
    assert!(f.home.unreadable());
    cancel_connection_worker(&f.stop, &f.wake);

    let outcome = body.depart(PrivateWorkerTrigger::Stopped);
    assert_eq!(outcome.trigger, PrivateWorkerTrigger::Stopped);
    assert_eq!(
        outcome.last,
        Some(PrivateWorkerAsk::Refused(
            PrivateWorkerRefusal::HomeUnreadable
        )),
        "what could not be asked, not silence standing in for it"
    );
    drop(f.fixture);
}

#[test]
fn a_body_that_spends_its_budget_still_says_what_it_was_doing() {
    // EXHAUSTION IS NOT INNOCENCE. A body that used every step it was given
    // has been serving, and reporting that nothing was ever asked would read
    // as one that never did -- which is exactly what a caller deciding
    // whether work is owed must not be told.
    let mut f = worker_fixture(XServerFrontendClientId(8406));
    attempt_run(&mut f.fixture, 84060, 272, true);
    let cell = admitted_cell(f.fixture.runner.frontend.as_ref().unwrap(), 84060);
    assert_eq!(
        f.fixture
            .runner
            .frontend
            .as_mut()
            .unwrap()
            .dispatch_one_press(),
        Some(true)
    );
    f.permit();

    // Two steps: enough to take the delivery and finish it, and no more, so
    // the budget is what ends this rather than the connection.
    let exit = PrivateWorkerExit::unstarted();
    let outcome = f.body(&exit, 2).run();
    let mut seen = [0u8; 32];
    std::io::Read::read_exact(&mut (&f.peer), &mut seen).expect("its peer reads what was sent");
    assert_eq!(seen[0], 4);
    assert_eq!(
        cell.answer().expect("answered for").outcome,
        XAuthorityInputDeliveryOutcome::Flushed
    );
    assert_eq!(outcome.trigger, PrivateWorkerTrigger::Exhausted);
    assert!(
        matches!(outcome.last, Some(PrivateWorkerAsk::Said(_))),
        "its last step, not a claim that it never took one: {outcome:?}"
    );
    drop(f.fixture);
}

#[test]
fn a_body_refuses_a_home_or_an_owner_it_may_not_serve() {
    // EACH REFUSAL IS A DIFFERENT FACT, and none of them is idleness. A body
    // that reported no work for any of these would say this connection had
    // nothing owed when what happened was that it could not serve it.
    //
    // NOTHING IS CONSUMED BY A REFUSAL: no receive, no write, no answer.
    // No stop: the production binding's own shape, refused rather than run.
    let unstoppable = worker_fixture_bound(XServerFrontendClientId(8398), false);
    let no_stop = PrivateWorkerExit::unstarted();
    let refused = unstoppable.body(&no_stop, 8).run();
    assert_eq!(
        refused.trigger,
        PrivateWorkerTrigger::Ineligible(PrivateWorkerRefusal::NoStop),
        "an owner with no stop is a worker nothing could end"
    );
    assert_eq!(
        refused.last,
        Some(PrivateWorkerAsk::Refused(PrivateWorkerRefusal::NoStop)),
        "and the refusal is kept rather than reported as nothing said"
    );
    drop(unstoppable.fixture);

    // A notice that is not this owner's.
    let f = worker_fixture(XServerFrontendClientId(8399));
    let foreign = Arc::new(PrivateOrderedWake::for_first_sender());
    let exit = PrivateWorkerExit::unstarted();
    assert_eq!(
        PrivateWorkerBody {
            home: &f.home,
            wake: &foreign,
            stop: &f.stop,
            byte_order: XByteOrder::LittleEndian,
            sequence: &f.sequence,
            exit: &exit,
            steps: 8,
        }
        .run()
        .trigger,
        PrivateWorkerTrigger::Ineligible(PrivateWorkerRefusal::ForeignNotice)
    );
    // A stop that is not this owner's.
    let other_stop = Arc::new(AtomicBool::new(false));
    assert_eq!(
        PrivateWorkerBody {
            home: &f.home,
            wake: &f.wake,
            stop: &other_stop,
            byte_order: XByteOrder::LittleEndian,
            sequence: &f.sequence,
            exit: &exit,
            steps: 8,
        }
        .run()
        .trigger,
        PrivateWorkerTrigger::Ineligible(PrivateWorkerRefusal::ForeignStop)
    );

    // A home whose connection has ended: whoever finishes it owns it now.
    let ended = {
        let g = worker_fixture(XServerFrontendClientId(8400));
        let home = Arc::clone(&g.home);
        let wake = Arc::clone(&g.wake);
        let stop = Arc::clone(&g.stop);
        let sequence = Arc::clone(&g.sequence);
        drop((g.fixture, g.sender));
        let exit = PrivateWorkerExit::unstarted();
        PrivateWorkerBody {
            home: &home,
            wake: &wake,
            stop: &stop,
            byte_order: XByteOrder::LittleEndian,
            sequence: &sequence,
            exit: &exit,
            steps: 8,
        }
        .run()
        .trigger
    };
    assert_eq!(
        ended,
        PrivateWorkerTrigger::Ineligible(PrivateWorkerRefusal::HomeRetained)
    );
    drop(f.fixture);
}


/// Take custody of this connection's evidence, before anything produces any.
///
/// THE ORDER IS THE SUBJECT. Every reaping below is handed a home this already
/// owns, in a scope that outlives the operation -- which is what makes losing
/// that operation cost the operation and not the result.
/// The custody this connection's REGISTRATION reserved, pinned.
///
/// NOT A FRESH ONE BESIDE IT. The home a control publishes into has to be the
/// one the service owner set aside before this connection's row went in --
/// otherwise every control would be exercising a custody no registration ever
/// knew about, and the reservation this component exists for would be
/// untested.
fn custody_for<'o>(
    f: &PrivateWorkerFixture,
    keeper: &'o crate::PrivateServiceOwner,
) -> PrivateCustodyPin<'o> {
    // THE KEEPER IS BORROWED, NOT THE FIXTURE. A pin tied to the whole fixture
    // would stop a control touching anything else in it, which is a borrow
    // about this helper's shape rather than about the owner this pin depends
    // on.
    let PrivateCustodyReach::Reached(pin) = f
        .fixture
        .registration
        .registered_custody(&keeper.lease())
        .expect("a private registration reserves a custody")
    else {
        panic!("its own service owner still keeps it")
    };
    pin
}

/// A started worker, through the real startup transaction, whose handle a
/// reaping can take.
///
/// THE ACTUAL PAIR: the body runs in the thread the startup transaction
/// spawned, writing its departure into the exit record the reaping reads.
/// Nothing asserts that pairing from the types -- they cannot establish it --
/// which is why it is built from one startup rather than assembled from parts.
/// Start one worker into THIS CONNECTION'S OWN registered slot.
///
/// The destination is the custody's, not a local of this helper: that is the
/// component's whole subject, and a control that started into a slot beside it
/// would be exercising the old shape.
fn started_worker<F>(custody: &PrivateEvidenceCustody, f: &PrivateWorkerFixture, body: F)
where
    F: FnOnce() + Send + 'static,
{
    assert_eq!(
        start_connection_worker(custody.worker_slot(), &f.stop, &f.wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
}

/// The payload a publication home holds, as a string, if it holds one.
fn panic_payload_of(evidence: &PrivateJoinEvidence) -> Option<String> {
    let PrivateJoinResult::Panicked(payload) = evidence.result()? else {
        return None;
    };
    let held = payload.lock().expect("a readable payload");
    held.downcast_ref::<&str>()
        .map(|carried| (*carried).to_owned())
}

/// The payload a reaping kept, as a string, if it kept one.
fn panic_payload(record: &PrivateReapingRecord<'_>) -> Option<String> {
    panic_payload_of(&record.join_evidence())
}

#[test]
fn a_reaping_keeps_what_a_worker_that_returned_actually_returned() {
    // THE ORDINARY CASE, end to end: a real startup, a real body over this
    // connection's own home, a real stop, and a join whose result is in the
    // caller's record before the attempt reports anything.
    let f = worker_fixture(XServerFrontendClientId(8411));
    f.permit();
    let custody = custody_for(&f, &f.fixture.keeper);
    let (home, wake, stop, sequence) = f.handles();
    let running = Arc::clone(custody.exit_sink());
    started_worker(&custody, &f, move || {
        PrivateWorkerBody {
            home: &home,
            wake: &wake,
            stop: &stop,
            byte_order: XByteOrder::LittleEndian,
            sequence: &sequence,
            exit: &running,
            steps: 16,
        }
        .run();
    });
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.phase(), PrivateReapingPhase::NotBegun);

    cancel_connection_worker(&f.stop, &f.wake);
    let reaping = record.reap();

    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert!(!reaping.slot_poisoned);
    assert_eq!(record.phase(), PrivateReapingPhase::Joined);
    assert!(
        matches!(record.result(), Some(PrivateJoinResult::Returned)),
        "the frame returned, and the record says which"
    );
    // THE BODY'S OWN EVIDENCE IS BESIDE IT, not derived from the join.
    let Some(PrivateExitReading::Classified(outcome)) = reaping.exit else {
        panic!("the body left a classification: {:?}", reaping.exit)
    };
    assert!(
        stopped_by_cancellation(&outcome),
        "the owner's own word: {outcome:?}"
    );
    drop(f.fixture);
}

#[test]
fn a_reaping_keeps_the_payload_a_worker_panicked_with() {
    // A JOIN THAT REPORTS A PANIC IS A COMPLETED JOIN, and what the frame was
    // carrying is kept rather than reduced to the fact that it panicked.
    let f = worker_fixture(XServerFrontendClientId(8412));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || {
        panic!("a worker frame carried this out with it");
    });
    let record = PrivateReapingRecord::bound_to(&custody);
    let reaping = record.reap();

    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert_eq!(record.phase(), PrivateReapingPhase::Joined);
    assert_eq!(
        panic_payload(&record).as_deref(),
        Some("a worker frame carried this out with it"),
        "the exact payload, still there to be read"
    );
    // AND NOTHING WAS INVENTED FOR THE BODY. This thread never ran one, so
    // there is no classification and the join does not supply one.
    assert_eq!(reaping.exit, Some(PrivateExitReading::NotLeft));
    drop(f.fixture);
}

#[test]
fn a_departure_noticed_is_not_a_thread_collected() {
    // LEFT IS A HINT, NOT A JOIN. A departure is published as a frame goes,
    // which is before the thread has finished going; only the join says the
    // thread is quiescent.
    //
    // WHAT THIS ESTABLISHES is that the two facts are separate and that a
    // reaping does not take the first for the second. It does NOT establish
    // that an active reaper waits on a still-running worker: this control
    // releases the thread before asking. The control beside it, where two
    // threads ask at once, is where a reaping runs against a live worker.
    //
    // THE GUARD IS INSTALLED BY HAND, so the departure can be published while
    // the thread is deliberately held open. No body runs here.
    let f = worker_fixture(XServerFrontendClientId(8413));
    let custody = custody_for(&f, &f.fixture.keeper);
    let (release, held) = std::sync::mpsc::channel::<()>();
    let departing = Arc::clone(custody.exit_sink());
    started_worker(&custody, &f, move || {
        {
            let _leaving = PrivateWorkerLeaving(&departing);
        }
        let _ = held.recv();
    });
    let record = PrivateReapingRecord::bound_to(&custody);
    assert!(
        waited_for(|| custody.exit_sink().left()),
        "the frame published that it had gone"
    );
    assert_eq!(
        custody.exit_sink().reading(),
        PrivateExitReading::Unclassified,
        "departure noticed, classification absent -- and that is not a panic"
    );
    assert_eq!(
        record.phase(),
        PrivateReapingPhase::NotBegun,
        "and nothing has been joined"
    );

    drop(release);
    let reaping = record.reap();
    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert!(
        matches!(record.result(), Some(PrivateJoinResult::Returned)),
        "the join is what establishes the thread finished, and how"
    );
    assert_eq!(
        reaping.exit,
        Some(PrivateExitReading::Unclassified),
        "and the join did not manufacture a classification"
    );
    drop(f.fixture);
}

#[test]
fn two_asks_at_once_join_a_worker_once() {
    // ONE ATTEMPT OWNS THE HANDLE, and this is the case the claim exists for:
    // two threads asking the same record while the worker is still running.
    //
    // THE OVERLAP IS WITNESSED, NOT HOPED FOR. The worker is held until the
    // slot itself says its handle has gone to a joiner AND the losing ask has
    // come back; releasing it and trusting the scheduler would let both asks
    // run serially after the thread had already finished, which would prove
    // nothing about either.
    let f = worker_fixture(XServerFrontendClientId(8418));
    let custody = custody_for(&f, &f.fixture.keeper);
    let (release, held) = std::sync::mpsc::channel::<()>();
    started_worker(&custody, &f, move || {
        let _ = held.recv();
        panic!("what exactly one of them keeps");
    });
    let record = PrivateReapingRecord::bound_to(&custody);

    std::thread::scope(|scope| {
        let (report, asked) = std::sync::mpsc::channel();
        for _ in 0..2 {
            let record = &record;
            let report = report.clone();
            scope.spawn(move || report.send(record.reap()));
        }
        drop(report);

        // ONE OF THEM HAS THE HANDLE, said by the slot rather than assumed.
        assert!(
            waited_for(|| {
                custody.worker_slot().lock().expect("a readable slot").life == PrivateWorkerLife::HandedToJoiner
            }),
            "an ask took the handle while its worker is still running"
        );
        // AND THE OTHER HAS ALREADY COME BACK, while that worker is still
        // held: the only ask that can return now is the one that found the
        // record taken, because the one that joined is waiting on the thread.
        let losing = asked
            .recv_timeout(Duration::from_secs(3))
            .expect("the losing ask returned");
        assert_eq!(losing.reaped, PrivateReaped::AlreadyAsked);
        assert_eq!(losing.exit, None, "it read nothing, having done nothing");
        assert_eq!(
            record.phase(),
            PrivateReapingPhase::InProgress,
            "a handle is consumed and no result is confirmed"
        );
        assert!(record.result().is_none());

        // Only now may the worker finish.
        drop(release);
        let winning = asked
            .recv_timeout(Duration::from_secs(3))
            .expect("the joining ask returned");
        assert_eq!(winning.reaped, PrivateReaped::Joined);
    });

    assert_eq!(record.phase(), PrivateReapingPhase::Joined);
    assert_eq!(
        panic_payload(&record).as_deref(),
        Some("what exactly one of them keeps"),
        "one join, one result"
    );
    let held = custody.worker_slot().lock().expect("a readable slot");
    assert!(held.handle.is_none());
    assert_eq!(held.life, PrivateWorkerLife::HandedToJoiner);
    drop(held);
    drop(f.fixture);
}

#[test]
fn a_second_ask_joins_nothing_and_changes_nothing() {
    // A second ask must not join twice, detach another handle, overwrite what
    // the first established, or leave the slot looking startable over a thread
    // that has already run.
    let f = worker_fixture(XServerFrontendClientId(8414));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || {
        panic!("what the first ask keeps");
    });
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);

    let again = record.reap();
    assert_eq!(again.reaped, PrivateReaped::AlreadyAsked);
    assert_eq!(again.exit, None, "it read nothing, having done nothing");
    assert_eq!(record.phase(), PrivateReapingPhase::Joined);
    assert_eq!(
        panic_payload(&record).as_deref(),
        Some("what the first ask keeps")
    );
    let held = custody.worker_slot().lock().expect("a readable slot");
    assert!(held.handle.is_none());
    assert_eq!(held.life, PrivateWorkerLife::HandedToJoiner);
    drop(held);
    drop(f.fixture);
}

#[test]
fn an_ask_that_consumes_nothing_says_which_nothing_it_found() {
    // THREE DIFFERENT NOTHINGS, and a caller deciding whether to expect a join
    // needs them apart. None of them consumes anything, so each leaves this
    // connection's one publication right where it found it.
    //
    // ALL THREE ARE ASKED OF ONE REGISTERED SOURCE, in the order a single slot
    // can actually reach them: a reaping view has no slot of its own to be
    // given any more, so the states are produced in this connection's own slot
    // rather than staged in three slots beside it.
    let f = worker_fixture(XServerFrontendClientId(8415));
    let custody = custody_for(&f, &f.fixture.keeper);

    // NEVER STARTED, which is true of this source until something starts it.
    let first = PrivateReapingRecord::bound_to(&custody);
    let reaping = first.reap();
    assert_eq!(reaping.reaped, PrivateReaped::NothingStarted);
    assert_eq!(reaping.exit, None);
    assert_eq!(
        first.phase(),
        PrivateReapingPhase::NotBegun,
        "the intent is withdrawn: this attempt consumed nothing"
    );

    // HANDED ELSEWHERE. A real worker is started into this connection's own
    // slot and its handle is taken by this control, which is what any other
    // joiner would have done.
    started_worker(&custody, &f, || {});
    let handle = hand_worker_to_joiner(custody.worker_slot())
        .handle
        .expect("the started worker's handle");
    let second = PrivateReapingRecord::bound_to(&custody);
    let elsewhere = second.reap();
    assert_eq!(elsewhere.reaped, PrivateReaped::HandedElsewhere);
    assert_eq!(second.phase(), PrivateReapingPhase::NotBegun);
    assert!(second.result().is_none());
    handle.join().expect("this control joins what it took");

    // A HANDLE GONE WITH NOTHING SAYING IT WAS HANDED ON. STAGED, and only
    // this: the connection's own slot is written into the state something
    // would leave if it took a handle without recording that it had. No path
    // here produces it, which is why it is written rather than reached.
    *custody
        .worker_slot()
        .lock()
        .expect("a readable slot") = PrivateWorkerSlot {
        handle: None,
        departing: false,
        life: PrivateWorkerLife::Running,
    };
    let third = PrivateReapingRecord::bound_to(&custody);
    let missing = third.reap();
    assert_eq!(
        missing.reaped,
        PrivateReaped::HandleMissing,
        "neither never-started nor handed on, and a caller told either \
         would be wrong"
    );
    assert_eq!(third.phase(), PrivateReapingPhase::NotBegun);
    assert!(
        third.result().is_none(),
        "three looks, and nothing published by any of them"
    );
    drop(custody);
    drop(f.fixture);
}

#[test]
fn a_poisoned_slot_still_gives_up_its_handle_and_says_it_was_poisoned() {
    // A THREAD LEFT UNJOINABLE BECAUSE SOMEBODY PANICKED NEAR ITS SLOT IS THE
    // WORSE OUTCOME. The slot is recovered far enough to get the handle out,
    // and the poison is reported beside the answer rather than absorbed.
    let f = worker_fixture(XServerFrontendClientId(8416));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || {});
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = custody.worker_slot().lock().expect("a readable slot");
            panic!("a holder unwound inside this connection's worker slot");
        }))
        .is_err(),
        "the holder unwound"
    );
    assert!(custody.worker_slot().is_poisoned());

    let record = PrivateReapingRecord::bound_to(&custody);
    let reaping = record.reap();
    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert!(reaping.slot_poisoned, "and the poison is reported");
    assert!(matches!(record.result(), Some(PrivateJoinResult::Returned)));
    drop(f.fixture);
}

#[test]
fn a_joined_result_publishes_while_its_exit_record_is_held() {
    // THE RESULT IS NOT HOSTAGE TO A DIAGNOSTIC. The exit record is read after
    // the join result has been retained and published, and never as a
    // condition of it -- so a record another thread is holding costs a caller
    // the diagnostic and not the join.
    //
    // HELD ACROSS THE REAPING, not merely poisoned: a reaping that took this
    // lock on its way to publishing would wait here, and the phase would not
    // move until it was released.
    let f = worker_fixture(XServerFrontendClientId(8417));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || {});
    let record = PrivateReapingRecord::bound_to(&custody);

    let reaping = std::thread::scope(|scope| {
        let (taken, wait) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let holder = Arc::clone(custody.exit_sink());
        let keeper = scope.spawn(move || {
            let _guard = holder.outcome.lock().expect("a readable exit record");
            taken.send(()).expect("the exit record is held");
            let _ = held.recv();
        });
        wait.recv().expect("the exit record is held");
        let record = &record;
        let reaper = scope.spawn(move || record.reap());
        // The join and its publication happen while the diagnostic is held.
        assert!(
            waited_for(|| record.phase() == PrivateReapingPhase::Joined),
            "the result is published without waiting for the exit record"
        );
        drop(release);
        keeper.join().expect("the holder finished");
        reaper.join().expect("the reaping finished")
    });

    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert!(matches!(record.result(), Some(PrivateJoinResult::Returned)));
    drop(f.fixture);
}

/// A connection whose worker has been started, run and joined, with its gate
/// captured while the registration still had one to give.
///
/// THE PAIRING IS THE CALLER'S: the gate is this registration's own and the
/// join is over the thread that was serving through it. Nothing in the types
/// establishes that, which is why it is built from one connection here.
struct PrivateFenceFixture {
    f: PrivateWorkerFixture,
    gate: Arc<PrivateHandoverGate>,
}

fn fence_fixture(client: XServerFrontendClientId) -> PrivateFenceFixture {
    let f = worker_fixture(client);
    f.permit();
        let custody = custody_for(&f, &f.fixture.keeper);
    let gate = f.fixture.registration.handover_gate();
    let (home, wake, stop, sequence) = f.handles();
    {
        // THE WORKER GOES INTO THE CONNECTION'S OWN SLOT, and is handed only
        // the sink it writes its classification into -- not the custody, not
        // the registration and not the owner.
        let running = Arc::clone(custody.exit_sink());
        started_worker(&custody, &f, move || {
            PrivateWorkerBody {
                home: &home,
                wake: &wake,
                stop: &stop,
                byte_order: XByteOrder::LittleEndian,
                sequence: &sequence,
                exit: &running,
                steps: 16,
            }
            .run();
        });
    }
    PrivateFenceFixture { f, gate }
}

#[test]
fn a_fence_waits_for_the_join_that_makes_it_eligible() {
    // A JOINED THREAD IS ONE WHOSE OWN SERVING HAS FINISHED, which is the
    // sequencing this component waits for. It is not a claim that nothing can
    // hand anything over afterwards -- producers are what the gate holds back,
    // and they outlive a consumer. Every weaker sign leaves this connection
    // still being served: a departure published, an empty slot, an attempt
    // that may have taken a handle.
    let g = fence_fixture(XServerFrontendClientId(8421));
    let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);

    // ASKED TOO EARLY: nothing is entered and nothing is spent.
    assert_eq!(fence.record_fence(), PrivateFenced::JoinIncomplete);
    assert_eq!(fence.phase(), PrivateFencePhase::NotAttempted);
    assert_eq!(fence.fence(), None);
    assert_eq!(
        g.f.fixture.registration.ordered_handovers_fenced(),
        Some(false),
        "the gate is untouched and this connection still admits handovers"
    );

    // THE SAME RECORD IS STILL ELIGIBLE once that same join completes.
    cancel_connection_worker(&g.f.stop, &g.f.wake);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert_eq!(fence.phase(), PrivateFencePhase::FenceRecorded);
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));
    assert_eq!(
        g.f.fixture.registration.ordered_handovers_fenced(),
        Some(true),
        "and now it does not"
    );
    drop(g.f.fixture);
}

#[test]
fn an_unconfirmed_join_is_not_a_joined_one() {
    // AN ATTEMPT THAT MAY HAVE TAKEN A HANDLE IS NOT A FINISHED THREAD. The
    // worker is held here, so the reaping is genuinely in flight: its record
    // says InProgress, its slot says the handle has gone, and neither is a
    // reason to close anything.
    let g = fence_fixture(XServerFrontendClientId(8422));
    let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
    let fence = PrivateFenceRecord::bound_to(&custody);

    std::thread::scope(|scope| {
        let record = &record;
        let reaper = scope.spawn(move || record.reap());
        // ARMED BEFORE ANY ASSERTION THAT CAN FAIL. A failing assertion below
        // would otherwise skip the cancellation, and the scope would join a
        // reaper that is waiting on a worker nothing had told to stop.
        let _stopper = PrivateWorkerStopper(&g.f.stop, &g.f.wake);
        // THE SLOT IS WHAT SAYS THE HANDLE HAS GONE, not the record's phase.
        // The intent is written BEFORE the handoff -- that is what write-ahead
        // means -- so a reaping paused between them has an InProgress record
        // over a slot that is still Running with its handle in it. Waiting on
        // the phase and then asserting the slot would be asserting a schedule.
        assert!(
            waited_for(|| {
                custody.worker_slot().lock().expect("a readable slot").life
                    == PrivateWorkerLife::HandedToJoiner
            }),
            "the reaping took the handle"
        );
        assert_eq!(
            record.phase(),
            PrivateReapingPhase::InProgress,
            "a handle is consumed and no result is confirmed"
        );

        assert_eq!(
            fence.record_fence(),
            PrivateFenced::JoinIncomplete,
            "an unconfirmed attempt is not a completed join"
        );
        assert_eq!(fence.phase(), PrivateFencePhase::NotAttempted);
        assert_eq!(
            g.f.fixture.registration.ordered_handovers_fenced(),
            Some(false),
            "and the gate is untouched"
        );

        cancel_connection_worker(&g.f.stop, &g.f.wake);
        assert_eq!(
            reaper.join().expect("the reaping finished").reaped,
            PrivateReaped::Joined
        );
    });

    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));
    drop(g.f.fixture);
}

#[test]
fn a_join_that_reported_a_panic_is_a_completed_join() {
    // BOTH RESULTS ARE COMPLETED JOINS. A thread that panicked is as finished
    // as one that returned, and the payload is neither inspected nor locked to
    // decide it -- a fence that had to read one would be hostage to whoever
    // was reading the payload at the time.
    let f = worker_fixture(XServerFrontendClientId(8423));
    let custody = custody_for(&f, &f.fixture.keeper);
    started_worker(&custody, &f, || panic!("what the join kept"));
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    let fence = PrivateFenceRecord::bound_to(&custody);

    // The payload is held by this control for the whole of the fencing, which
    // a fence that needed it could not have got past.
    let PrivateJoinResult::Panicked(payload) = record.result().expect("a completed join") else {
        panic!("this worker panicked")
    };
    let held = payload.lock().expect("a readable payload");
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
    assert_eq!(fence.fence(), Some(PrivateHandoverFence::Established));
    assert_eq!(
        held.downcast_ref::<&str>().copied(),
        Some("what the join kept"),
        "and the payload is exactly as it was"
    );
    drop(held);
    drop(f.fixture);
}

#[test]
fn a_fence_keeps_the_three_things_a_gate_can_say() {
    // ESTABLISHED, ALREADY ESTABLISHED AND UNREADABLE ARE THREE FACTS. A
    // closure somebody else made is not one this made, and a gate whose lock
    // carried a panic out of somebody's handover is not a fence at all.
    let established = {
        let g = fence_fixture(XServerFrontendClientId(8424));
        cancel_connection_worker(&g.f.stop, &g.f.wake);
        let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let seen = fence.fence();
        drop(g.f.fixture);
        seen
    };
    assert_eq!(established, Some(PrivateHandoverFence::Established));

    // ALREADY ESTABLISHED, reached by a real close this component did not
    // make: the registration's own fencing runs first.
    let already = {
        let g = fence_fixture(XServerFrontendClientId(8425));
        cancel_connection_worker(&g.f.stop, &g.f.wake);
        let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        assert_eq!(
            g.f.fixture.registration.fence_ordered_handovers(),
            PrivateHandoverFence::Established,
            "somebody else closed it first, through the real API"
        );
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let seen = fence.fence();
        drop(g.f.fixture);
        seen
    };
    assert_eq!(already, Some(PrivateHandoverFence::AlreadyEstablished));

    // UNREADABLE: a holder panicked inside the gate. The lock is acquired --
    // that is what poisoning means -- and what could not be established is
    // closure over custody nobody stands behind.
    let unreadable = {
        let g = fence_fixture(XServerFrontendClientId(8426));
        cancel_connection_worker(&g.f.stop, &g.f.wake);
        let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
        assert_eq!(record.reap().reaped, PrivateReaped::Joined);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _inside = g.gate.fenced.lock().expect("a readable gate");
                panic!("a holder unwound inside this connection's gate");
            }))
            .is_err(),
            "the holder unwound"
        );
        let fence = PrivateFenceRecord::bound_to(&custody);
        assert_eq!(fence.record_fence(), PrivateFenced::Recorded);
        let seen = fence.fence();
        // NOT CLEARED AND NOT REOPENED: the gate is left exactly as it was
        // found.
        assert!(g.gate.fenced.is_poisoned());
        drop(g.f.fixture);
        seen
    };
    assert_eq!(unreadable, Some(PrivateHandoverFence::Unreadable));
}

#[test]
fn a_second_fencing_asks_nothing_and_replaces_nothing() {
    // A REPEATED ASK MUST NOT ASK THE GATE AGAIN. The second call would get
    // AlreadyEstablished from a gate this record itself had closed, and
    // writing that over the first Established would turn a closure this made
    // into one it merely found.
    let g = fence_fixture(XServerFrontendClientId(8427));
    cancel_connection_worker(&g.f.stop, &g.f.wake);
    let custody = custody_for(&g.f, &g.f.fixture.keeper);
    let record = PrivateReapingRecord::bound_to(&custody);
    assert_eq!(record.reap().reaped, PrivateReaped::Joined);
    let fence = PrivateFenceRecord::bound_to(&custody);
    assert_eq!(fence.record_fence(), PrivateFenced::Recorded);

    assert_eq!(fence.record_fence(), PrivateFenced::AlreadyAttempted);
    assert_eq!(
        fence.fence(),
        Some(PrivateHandoverFence::Established),
        "the first attempt's answer, not a second one over the top of it"
    );
    assert_eq!(fence.phase(), PrivateFencePhase::FenceRecorded);
    drop(g.f.fixture);
}
