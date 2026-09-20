// Spawning and stopping workers: the refused spawn with nothing to join, and
// the instrument that takes the admission asked for and keeps the others.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_refused_spawn_leaves_nothing_to_stop_or_join() {
    // Nothing exists, so there is nothing to cancel and nothing to join. The
    // slot is as empty as it was found, and the connection's stop is untouched.
    let (slot, stop, wake) = startup_fixture();
    let outcome = start_connection_worker(&slot, &stop, &wake, || {
        Err(std::io::Error::other("no thread for you"))
    });
    assert_eq!(outcome, PrivateStartupOutcome::SpawnRefused);
    assert!(!slot.lock().expect("readable").running());
    assert!(
        !stop.load(std::sync::atomic::Ordering::Acquire),
        "nothing was running, so nothing was stopped"
    );
    assert!(
        !wake.state.lock().expect("readable").started,
        "and nothing was permitted"
    );
}

#[test]
fn a_transaction_that_fails_after_the_spawn_cancels_rather_than_forgets() {
    // AFTER A SPAWN THERE IS NO GOING BACK TO "NO WORKER". A thread may be
    // running; saying otherwise loses it, and dropping its handle detaches a
    // thread nobody can join.
    //
    // THE UNWIND IS STAGED AT THE GUARD, and the control says so: the
    // transaction has nothing fallible between its store and its commit, so
    // there is no way to make the real one fail there. What is exercised is
    // the guard those paths share.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    let handle = std::thread::Builder::new()
        .spawn(body)
        .expect("a thread for this control");
    {
        let mut held = slot.lock().expect("readable");
        held.handle = Some(handle);
        let _guard = PrivateStartupGuard {
            slot: &mut held,
            stop: &stop,
            wake: &wake,
            committed: false,
        };
        // and it goes out of scope without committing.
    }

    assert!(
        stop.load(std::sync::atomic::Ordering::Acquire),
        "the connection's own stop is set -- the one its serving consults"
    );
    assert!(
        !wake.state.lock().expect("readable").started,
        "and no permit was ever published"
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("stopped"),
        "so the worker leaves rather than waiting for a permission nobody will give"
    );
    let handle = slot
        .lock()
        .expect("readable")
        .handle
        .take()
        .expect("THE HANDLE IS STILL HERE, not detached");
    handle.join().expect("the worker");
}

#[test]
fn a_second_start_is_refused_from_the_handle_that_is_already_owned() {
    // Asked of the handle itself rather than a flag beside it: a flag can be
    // stale in exactly the window that matters, and would then say no worker
    // exists while one does.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );

    let mut spawned_again = false;
    let outcome = start_connection_worker(&slot, &stop, &wake, || {
        spawned_again = true;
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(outcome, PrivateStartupOutcome::AlreadyStarted);
    assert!(!spawned_again, "nothing was even attempted");

    let handle = slot
        .lock()
        .expect("readable")
        .handle
        .take()
        .expect("the first worker's handle, untouched");
    handle.join().expect("the worker");
}

#[test]
fn a_stop_after_a_permit_still_takes_the_worker_out() {
    // A PERMIT IS NOT A RIGHT TO CONTINUE. It says a transaction finished; the
    // connection's stop says whether it should still be running, and it is
    // asked every time the worker wakes rather than once at the start.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let waiting_stop = Arc::clone(&stop);
    let waiting_wake = Arc::clone(&wake);
    // A body that is permitted, then waits again -- the shape a serving loop
    // has, without any serving in it.
    let body = move || {
        {
            let mut state = waiting_wake.state.lock().expect("a readable notice");
            while !state.started {
                state = waiting_wake.ready.wait(state).expect("a readable notice");
            }
        }
        saw.send("permitted").expect("listening");
        let mut state = waiting_wake.state.lock().expect("a readable notice");
        while !waiting_stop.load(std::sync::atomic::Ordering::Acquire) {
            state = waiting_wake.ready.wait(state).expect("a readable notice");
        }
        saw.send("stopped").expect("listening");
    };
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );

    // Through the real cancellation: the connection's own stop, and a wake so
    // it looks. A cancellation that only cleared the permit would leave this
    // worker exactly where it is.
    cancel_connection_worker(&stop, &wake);
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("stopped"),
        "the permit did not keep it running"
    );
    let handle = slot.lock().expect("readable").handle.take().expect("owned");
    handle.join().expect("the worker");
}


#[test]
fn a_worker_that_cannot_be_permitted_is_cancelled_rather_than_declared_started() {
    // A PERMIT TAKEN FROM A POISONED NOTICE IS NOT A PERMIT. Recovering that
    // guard and writing into it grants a worker permission on the strength of
    // a lock whose contents nobody stands behind -- and reports Started, with
    // the connection's stop never set, over a thread that is actually running.
    //
    // The poisoning is real and happens inside the spawner, which then returns
    // a real worker: exactly the window between the store and the commit that
    // I had claimed nothing could refuse in.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    let poisoning = Arc::clone(&wake);
    let outcome = start_connection_worker(&slot, &stop, &wake, move || {
        let holder = std::thread::spawn(move || {
            let _inside = poisoning.state.lock().expect("a readable notice");
            panic!("a holder unwound inside this notice");
        });
        assert!(holder.join().is_err(), "the holder unwound");
        std::thread::Builder::new().spawn(body)
    });

    assert_eq!(
        outcome,
        PrivateStartupOutcome::PermitRefused,
        "its own answer: a worker exists and could not be permitted"
    );
    assert!(
        stop.load(std::sync::atomic::Ordering::Acquire),
        "the connection's own stop is set"
    );
    let state = wake
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(!state.started, "and nothing was permitted");
    drop(state);
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("stopped"),
        "so the worker leaves"
    );
    let handle = slot
        .lock()
        .expect("readable")
        .handle
        .take()
        .expect("and its handle is here to be joined");
    handle.join().expect("the worker");
}

#[test]
fn a_worker_handed_on_to_be_joined_does_not_leave_its_slot_free() {
    // AN EMPTY SLOT IS NOT AN UNUSED ONE. Handing a handle to whoever joins it
    // empties the slot while the thread is still running, and a slot that read
    // that as "nobody was ever started" would start a second worker for a
    // connection whose first is alive.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(2);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );

    // The handle goes to a joiner, and nothing is joined yet.
    let handed = hand_worker_to_joiner(&slot);
    assert!(!handed.source_poisoned);
    let handed = handed.handle.expect("its handle");
    assert!(
        !slot.lock().expect("readable").running(),
        "the slot holds no handle now"
    );
    assert!(
        wake.state.lock().expect("readable").started,
        "AND THE GRANT IS UNTOUCHED. Moving a handle is not a decision about \
         whether the worker should still be running"
    );

    let mut spawned_again = false;
    let outcome = start_connection_worker(&slot, &stop, &wake, || {
        spawned_again = true;
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(
        outcome,
        PrivateStartupOutcome::NoLongerStartable,
        "the slot remembers that it had one"
    );
    assert!(!spawned_again, "and nothing was attempted");

    // Both threads are accounted for before anything is concluded, and
    // stopping is its own deliberate act rather than a side effect of the
    // handoff.
    cancel_connection_worker(&stop, &wake);
    handed.join().expect("the first worker");
}

#[test]
fn a_worker_not_yet_at_its_first_look_is_not_stranded_by_the_handoff() {
    // THE GRANT SURVIVES THE MOVE. A worker held before it has reached its
    // first look has not seen its permit yet; a handoff that revoked the
    // permit left it waiting for a permission taken back, with nothing telling
    // it otherwise and its handle already somewhere else.
    //
    // A real predicate-lock handshake holds the worker there: the control owns
    // the notice until it chooses to let go, so this does not depend on
    // timing.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);

    // Held here, so the worker cannot reach its predicate.
    let held_notice = wake.state.lock().expect("a readable notice");
    let started = std::thread::Builder::new()
        .spawn(body)
        .expect("a thread for this control");
    {
        let mut slotted = slot.lock().expect("readable");
        slotted.handle = Some(started);
        slotted.life = PrivateWorkerLife::Running;
    }
    // Permit it, and KEEP HOLDING THE NOTICE. Releasing here would let the
    // worker reach its predicate before the handoff, which is exactly the
    // ordering this control is about -- and a release followed by a handoff
    // establishes nothing about a worker that has not looked, because it may
    // well have.
    let mut state = held_notice;
    state.started = true;

    // The handle goes while the notice is still held, so the worker provably
    // has not observed anything. Handing on does not take this notice, which
    // is why holding it across the call is possible at all.
    let handed = hand_worker_to_joiner(&slot).handle.expect("its handle");

    // Only now may it look. It is not stranded: the grant it was given is
    // still there.
    drop(state);
    wake.ready.notify_all();
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted"),
        "the worker got the permission it was granted"
    );
    handed.join().expect("the first worker");
    assert!(
        !stop.load(std::sync::atomic::Ordering::Acquire),
        "and nothing stopped it, because nothing asked it to"
    );
}

#[test]
fn a_poisoned_slot_still_hands_over_the_worker_it_owns() {
    // ACCEPTED CUSTODY IS NOT ABSENCE. A slot somebody panicked inside still
    // owns whatever it owns, and answering "nothing here" would leave a
    // running thread with no route to a join at all -- while reporting that
    // there was nothing to join.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );

    // A holder panics inside the slot.
    let holder = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _inside = slot.lock().expect("readable");
        panic!("a holder unwound inside this slot");
    }));
    assert!(holder.is_err(), "the holder unwound");
    assert!(slot.lock().is_err(), "so the slot is poisoned");

    let handed = hand_worker_to_joiner(&slot);
    assert!(
        handed.source_poisoned,
        "and that is reported, beside the handle rather than instead of it"
    );
    let handle = handed
        .handle
        .expect("THE WORKER IS STILL HANDED OVER, because it is still owned");
    cancel_connection_worker(&stop, &wake);
    handle.join().expect("the worker");
}


#[test]
fn a_release_is_news_only_where_somebody_is_waiting_on_it() {
    // EVERY STATE, because the narrowing is about which of them a release
    // means anything in. Announcing into a slot an actor's own pass just
    // finished with would make it ready, be claimed again, find nothing again,
    // and announce again -- a loop with no progress in it.
    let (roll, first, second, _now) = attention_for_two();

    // Idle: an actor's own concluded pass leaves this. Nothing to tell.
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Idle));
    assert!(roll.released(first), "the identity is live");
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::Idle),
        "and stays idle: no new readiness from a release nobody awaited"
    );
    assert_eq!(roll.waiting(), Some(0));

    // Ready: already has a reason to be looked at, and does not need two.
    assert!(roll.flag(first));
    assert_eq!(roll.waiting(), Some(1));
    assert!(roll.released(first));
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Ready));
    assert_eq!(roll.waiting(), Some(1), "counted once, not twice");

    // InFlight: a pass is running and may be failing to take the record, so a
    // release is exactly what it needs to know.
    let claim = roll.claim_next().expect("a slot waiting");
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::InFlight { dirty: false })
    );
    assert!(roll.released(first));
    assert_eq!(
        roll.state_of(first),
        Some(PrivateAttentionState::InFlight { dirty: true })
    );
    assert!(claim.could_not());
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Ready));

    // Deferred: a pass gave up, and the release is what revives it.
    let parked = roll.claim_next().expect("a slot waiting");
    assert!(parked.could_not());
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Deferred));
    assert_eq!(roll.waiting(), Some(0));
    assert!(roll.released(first));
    assert_eq!(roll.state_of(first), Some(PrivateAttentionState::Ready));
    assert_eq!(roll.waiting(), Some(1));

    // A stale identity is refused outright, whatever the slot is doing now.
    let claim = roll.claim_next().expect("a slot waiting");
    assert!(claim.could_not());
    assert!(roll.retire(first, false));
    let successor = roll.admit(0).expect("the slot is free");
    assert!(roll.flag(successor));
    let current = roll.claim_next().expect("the successor's pass");
    assert!(
        !roll.released(first),
        "a release for somebody who has gone is not a release"
    );
    assert_eq!(
        roll.state_of(successor),
        Some(PrivateAttentionState::InFlight { dirty: false }),
        "and the occupant's own pass is untouched by it"
    );
    assert!(current.took_it());
    let _ = second;
}


#[test]
fn a_departure_stops_a_connection_without_waiting_for_a_spawn_in_flight() {
    // THE SLOT IS HELD ACROSS A SPAWN. Creating a thread is somebody else's
    // latency, and a departure that took that lock before telling the
    // connection anything would wait behind it -- stopping a connection
    // queueing behind starting one.
    //
    // The spawner here is held by the control, so the lock is provably held
    // while the departure runs: this does not depend on timing.
    let (slot, stop, wake) = startup_fixture();
    let slot = Arc::new(slot);
    let (inside, entered) = sync_channel(1);
    let (go, wait_here) = sync_channel(1);
    let starting = Arc::clone(&slot);
    let start_stop = Arc::clone(&stop);
    let start_wake = Arc::clone(&wake);
    let starter = std::thread::spawn(move || {
        start_connection_worker(&starting, &start_stop, &start_wake, || {
            inside.send(()).expect("the control is listening");
            wait_here.recv().expect("the control lets go");
            std::thread::Builder::new().spawn(|| {})
        })
    });
    entered.recv().expect("the spawn is in flight, holding the slot");
    assert!(
        slot.try_lock().is_err(),
        "and the slot really is held while it is"
    );

    // The departure runs now, with that lock held by the spawn.
    let departing_slot = Arc::clone(&slot);
    let departing_stop = Arc::clone(&stop);
    let departing_wake = Arc::clone(&wake);
    let departure = std::thread::spawn(move || {
        depart_connection(&departing_slot, &departing_stop, &departing_wake)
    });

    // THE STOP ARRIVES WHILE THE SPAWN IS STILL PAUSED. Nothing has released
    // the slot, and the control has not let the spawner go.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "the stop must not wait for the spawn"
        );
        std::thread::yield_now();
    }
    assert!(
        slot.try_lock().is_err(),
        "and it arrived with the spawn still holding the slot"
    );

    // Only now does the spawn finish, and the departure's decision follows it.
    go.send(()).expect("the spawner is waiting");
    let started = starter.join().expect("the starting thread");
    assert_eq!(started, PrivateStartupOutcome::Started);
    assert_eq!(
        departure.join().expect("the departing thread"),
        PrivateDeparture::WorkerRunning,
        "and it found the worker that had just been started"
    );

    let handle = slot
        .lock()
        .expect("readable")
        .handle
        .take()
        .expect("its handle");
    handle.join().expect("the worker");
}

#[test]
fn a_departure_says_which_of_the_three_histories_it_found() {
    // AN EMPTY SLOT CAN BE ANY OF THREE THINGS, and they need different things
    // done for them: nothing to join, a handle to take, or a handle already
    // taken. A departure that collapsed them into one state would leave
    // whoever acts next guessing.
    let (slot, stop, wake) = startup_fixture();
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::NothingStarted
    );
    assert!(stop.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::AlreadyDeparting,
        "the first departure stands"
    );

    // A connection whose worker is here.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::WorkerRunning
    );
    let handed = hand_worker_to_joiner(&slot).handle.expect("its handle");

    // A repeated departure says what it is: the first one stands. It does not
    // re-read the history, which is why the third case below is a connection
    // of its own rather than this one with a field put back.
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::AlreadyDeparting
    );
    handed.join().expect("the worker");

    // The third history, through a fresh connection: started, handed on, and
    // only then departed for the first time.
    let (slot, stop, wake) = startup_fixture();
    let (saw, seen) = sync_channel(1);
    let body = permit_waiter(Arc::clone(&stop), Arc::clone(&wake), saw);
    assert_eq!(
        start_connection_worker(&slot, &stop, &wake, || {
            std::thread::Builder::new().spawn(body)
        }),
        PrivateStartupOutcome::Started
    );
    assert_eq!(
        seen.recv_timeout(std::time::Duration::from_secs(5)),
        Ok("permitted")
    );
    let handed = hand_worker_to_joiner(&slot).handle.expect("its handle");
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::WorkerHandedOn,
        "its handle is with a joiner, and the departure says so"
    );
    handed.join().expect("the worker");

    // WHAT THESE ARE: facts about a handle and a history. Neither says the
    // thread is executing now, and neither says it has been joined.
}

#[test]
fn a_departing_connection_starts_nothing_whatever_its_history() {
    // The serialized transition is the slot's own lock: a start takes it and
    // refuses a departing slot, a departure takes it and marks one. Neither
    // can interleave with the other, and this is the refusal.
    let (slot, stop, wake) = startup_fixture();
    assert_eq!(
        depart_connection(&slot, &stop, &wake),
        PrivateDeparture::NothingStarted
    );
    let mut spawned = false;
    let outcome = start_connection_worker(&slot, &stop, &wake, || {
        spawned = true;
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(
        outcome,
        PrivateStartupOutcome::NoLongerStartable,
        "departing, however little ever happened here"
    );
    assert!(!spawned, "and nothing was attempted");
}

#[test]
fn a_full_recipient_does_not_consume_a_live_recipients_turn() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7601));
    // Four ORIGINAL events fill A: each pair is actually reserved, executed,
    // observed at the common boundary, built and enqueued by production code.
    // Each release's actual sealed native proof is recorded once. No receipt
    // or settlement is invented, and the A receiver stays live and undrained.
    for (id,button) in [(76010,272),(76012,273)] {
        attempt_release(&mut f,id,button);
        let p=f.runner.frontend.as_mut().unwrap();
        assert!(p.terminal.settling.last().unwrap().native().unwrap().proof().is_some());
        assert_eq!(p.dispatch_one_press(),Some(true));
        assert_eq!(p.record_one_native(),Some(true));
        assert_eq!(p.attempt_one_delivery(),Some(true));
    }
    attempt_run(&mut f,76014,274,true);
    let (original,frames,order)={
        let p=f.runner.frontend.as_mut().unwrap();
        assert_eq!(p.broker.registry.per_client_input_capacity.get(),4);
        assert_eq!(p.terminal.holds.len(),1);
        assert_eq!(p.terminal.settling.len(),2);
        assert!(p.terminal.settling.iter().all(|r|r.native_recorded() && r.dispatch()==PrivateDispatchPhase::Enqueued && r.attempt().is_some()));
        let recovery=p.broker.registry.input_recovery.clone();
        let record=&mut p.terminal.holds[0];
        let original=record.custody.completion.as_ref().unwrap().clone();
        let emission=record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody,emission,&recovery,f.client);
        let Some(PrivatePendingDelivery::Capsule(capsule))=record.custody.pending.take() else{panic!("the actual fifth event built a capsule")};
        let frames=order_pass_frames(&capsule);
        let sender=p.broker.registry.clients.lock().unwrap().get(&f.client).unwrap().ordered.clone();
        // Classify the actual send refusal. Return the exact original capsule
        // to custody; this is neither synthetic filler nor a fake Full result.
        let capsule=match gated_send(&sender,capsule) {
            Err(std::sync::mpsc::TrySendError::Full(c))=>c,
            Err(std::sync::mpsc::TrySendError::Disconnected(_))=>panic!("A receiver remains live"),
            Ok(())=>panic!("four original capsules must fill A's four slots"),
        };
        assert!(Arc::ptr_eq(&original,&capsule.finalizer().unwrap().completion));
        record.custody.pending=Some(PrivatePendingDelivery::Capsule(capsule));
        assert_eq!(record.custody.dispatch,PrivateDispatchPhase::Pending);
        (original,frames,record.custody.order)
    };

    // B is a genuine second recipient in the same namespace, admitted with
    // its own selection/connection. Real A-ungrab/B-grab changes subsequent
    // source resolution; no foreign custody or fabricated native hold is used.
    let other=XServerFrontendClientId(7602);
    let other_window=XResourceId::new(0x307602,1);
    let (other_registration,other_channels)={
        let p=f.runner.frontend.as_mut().unwrap();let registry=&p.broker.registry;
        let context=namespaced(other,f.namespace);
        let (registration,channels)=registry.register_client_with_admission(other,Some(context)).unwrap();
        registry.attach_private_lifecycle(&registration,context).unwrap();
        let selected=Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry.attach_connection_state(&registration,f.namespace,selected.clone(),Arc::new(AtomicU64::new(0))).unwrap();
        {
            let mut s=selected.lock().unwrap();
            s.register(other_window,XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT),1),Rect{x:0,y:0,width:200,height:100});
            s.observe_mapped(other_window);s.update(other_window,Some((1<<2)|(1<<3)),None);
        }
        let mut grabs=registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace,f.client.raw());
        grabs.grab_pointer(f.namespace,crate::XActiveInputGrab{owner:other.raw(),window:other_window,owner_events:false,pointer_mode:1,keyboard_mode:1,event_mask:u16::MAX,xi_event_mask:[0;8],xi_event_mask_words:0,route_lease:None}).unwrap();
        (registration,channels)
    };
    attempt_run(&mut f,76015,275,true);
    let p=f.runner.frontend.as_mut().unwrap();
    assert_eq!(p.terminal.holds.len(),2);
    assert_eq!(p.terminal.holds[0].reached.client(),f.client);
    assert_eq!(p.terminal.holds[1].reached.client(),other);
    let original_native=p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize;
    let incarnation=p.terminal.holds[0].incarnation;
    assert_eq!(p.terminal.holds[0].custody.order,order);
    assert!(order<p.terminal.holds[1].custody.order);
    let later=p.terminal.holds[1].custody.completion.as_ref().unwrap().clone();
    let recovery=p.broker.registry.input_recovery.clone();
    let queue_cells=[76010,76011,76012,76013].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let mut seen_b=Vec::new();let mut refused=0;let mut idle=0;
    for _ in 0..32 {
        match p.deliver_one(None, &mut |_,_|Ok(())).unwrap() {
            PrivateDeliveryStep::Dispatched{enqueued:false,..}=>refused+=1,
            PrivateDeliveryStep::Idle=>idle+=1,
            _=>{},
        }
        // Only B is drainable during measurement. A's original four remain
        // untouched until after every actual terminal visit has completed.
        while let Ok(c)=other_channels.ordered.try_recv() {
            assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(76015));
            assert_eq!(c.client(),other);
            assert!(Arc::ptr_eq(&later,&c.finalizer().unwrap().completion));
            seen_b.push(c.delivery());
        }
    }
    let old=&p.terminal.holds[0];
    assert_eq!(old.incarnation,incarnation);
    assert_eq!(old.native.as_ref().unwrap() as *const _ as usize,original_native);
    assert_eq!(old.custody.dispatch,PrivateDispatchPhase::Pending);
    assert_eq!(old.custody.order,order);
    let Some(PrivatePendingDelivery::Capsule(c))=old.custody.pending.as_ref() else{panic!("the original blocked capsule remains inventory-owned")};
    assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(76014));
    assert_eq!(c.client(),f.client);
    assert!(Arc::ptr_eq(&original,&c.finalizer().unwrap().completion));
    assert_eq!(order_pass_frames(c),frames);
    assert!(original.answer().is_none() && later.answer().is_none());
    assert!(queue_cells.iter().all(|c|c.answer().is_none()));
    assert!(p.terminal.settling.iter().all(|r|r.dispatch()==PrivateDispatchPhase::Enqueued && r.attempt().is_some()));
    let mut seen_a=Vec::new();
    while let Ok(c)=f.channels.ordered.try_recv() {
        let index=seen_a.len();assert!(index<queue_cells.len());
        assert_eq!(c.client(),f.client);
        assert!(Arc::ptr_eq(&queue_cells[index],&c.finalizer().unwrap().completion));
        seen_a.push(c.delivery());
    }
    assert_eq!(seen_a,[76010,76011,76012,76013].map(XAuthorityInputDeliveryId::from_raw));
    assert!(
        refused + idle > 0,
        "the visits were real: {refused} refused dispatches, {idle} idle"
    );
    assert_eq!(seen_b,[XAuthorityInputDeliveryId::from_raw(76015)],"a genuinely Full older recipient must not consume every later live recipient's turn");

    // AND THE RETAINED CAPSULE GOES ONCE, NOW THAT THERE IS ROOM. Draining A's
    // four freed its slots; what was refused is offered again unchanged. The
    // capsule is the one that was built before the refusal -- same completion,
    // same recipient, same bytes -- because a refused queue is a handover that
    // did not happen, not one to redo.
    let mut inbox = OrderedInbox::default();
    let retried = inbox
        .accepted(p, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("the retained capsule is accepted once its recipient has room");
    assert_eq!(
        retried.delivery(),
        XAuthorityInputDeliveryId::from_raw(76014)
    );
    assert_eq!(retried.client(), f.client);
    assert!(Arc::ptr_eq(
        &original,
        &retried.finalizer().unwrap().completion
    ));
    assert_eq!(
        order_pass_frames(&retried),
        frames,
        "the same encoded event, not one built again"
    );
    assert_eq!(
        retried.emission().incarnation(),
        Some(incarnation),
        "still named by the hold it came from"
    );
    assert!(
        inbox.taken.is_empty(),
        "and nothing else of A's was taken while finding it"
    );

    // Once. Further visits produce no duplicate for either recipient.
    for _ in 0..8 {
        let _ = p.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "an accepted handover is not repeated after capacity opens"
    );
    assert!(other_channels.ordered.try_recv().is_err());
    assert!(original.answer().is_none() && later.answer().is_none());
    drop(other_registration);
}

#[test]
fn an_unrecorded_release_at_the_head_spends_its_connections_turn() {
    // A release whose native half is not in yet cannot be given a ledger
    // attempt, so its handover is unfinished and it is its connection's head.
    // The press path must recognise that and leave it alone: the release
    // belongs to the attempt path, and reaching into it from here would act on
    // a debt the ledger has not authorised.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7611));
    attempt_release(&mut f, 76110, 272);
    // The press it ends comes first and is handed over, which is what leaves
    // the release alone at the head of that connection.
    {
        let p = f.runner.frontend.as_mut().unwrap();
        assert_eq!(p.dispatch_one_press(), Some(true));
    }
    assert_eq!(
        f.channels.ordered.try_iter().count(),
        1,
        "the carried press, and only it"
    );

    // A second connection with its own press, behind the release in stamp
    // order, so a release head that stopped everything would show here.
    let other = XServerFrontendClientId(7612);
    let other_window = XResourceId::new(0x307612, 1);
    let (other_registration, other_channels) = {
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
    attempt_run(&mut f, 76112, 273, true);

    let p = f.runner.frontend.as_mut().unwrap();
    // The state this control exists for: one settling release, native not yet
    // recorded, so the ledger will refuse it an attempt and its own handover
    // has not been taken.
    assert_eq!(p.terminal.settling.len(), 1);
    assert!(!p.terminal.settling[0].native_recorded());
    assert_eq!(
        p.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "the release owes a handover, so it is its connection's head"
    );
    assert!(p.terminal.settling[0].attempt().is_none());
    assert_eq!(p.terminal.holds.len(), 1);
    assert_eq!(p.terminal.holds[0].reached.client(), other);

    let recovery = p.broker.registry.input_recovery.clone();
    let press_cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(76110))
        .unwrap()
        .unwrap();
    let release_cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(76111))
        .unwrap()
        .unwrap();
    let other_cell = p.terminal.holds[0].custody.completion.as_ref().unwrap().clone();

    // The other connection's own press goes first: the turn moved on from A
    // when A was last offered, which is what keeps a stuck connection from
    // consuming every visit.
    assert_eq!(p.dispatch_one_press(), Some(true));
    let handed: Vec<_> = other_channels.ordered.try_iter().collect();
    assert_eq!(handed.len(), 1, "the connection behind it is not blocked by it");

    // Now only the release is left unfinished, so it is what the next visit is
    // offered. The press path must recognise it and leave it alone: reaching
    // into it would act on a debt the ledger never authorised.
    assert_eq!(
        p.dispatch_one_press(),
        None,
        "a release head is left to the attempt path, and costs the visit"
    );
    assert_eq!(
        p.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "nothing was taken from it"
    );
    assert_eq!(
        handed[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(76112)
    );
    assert_eq!(handed[0].client(), other);
    assert!(Arc::ptr_eq(
        &other_cell,
        &handed[0].finalizer().expect("carried").completion
    ));
    assert!(f.channels.ordered.try_recv().is_err(), "and nothing of A's moved");
    assert!(
        press_cell.answer().is_none()
            && release_cell.answer().is_none()
            && other_cell.answer().is_none()
    );
    drop(other_registration);
}

#[test]
fn a_recipients_complete_event_order_is_preserved_across_press_and_release() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7551));
    attempt_release(&mut f,75510,272);
    attempt_run(&mut f,75512,273,true);
    let p=f.runner.frontend.as_mut().unwrap();
    let recovery=p.broker.registry.input_recovery.clone();
    let cells=[75510,75511,75512].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let mut seen=Vec::new();
    // Drive the actual terminal arbiter, including native recording/release
    // dispatch. Drain ALL queue entries after every visit, not merely presses.
    for _ in 0..16 {
        let _=p.deliver_one(None, &mut |_,_|Ok(())).unwrap();
        while let Ok(capsule)=f.channels.ordered.try_recv() {
            let id=capsule.delivery();
            let expected=[75510,75511,75512].iter().position(|n|id==XAuthorityInputDeliveryId::from_raw(*n)).expect("only these actual events exist");
            assert!(Arc::ptr_eq(&cells[expected],&capsule.finalizer().unwrap().completion));
            seen.push(id);
        }
    }
    assert!(cells.iter().all(|c|c.answer().is_none()));
    assert_eq!(seen,[75510,75511,75512].map(XAuthorityInputDeliveryId::from_raw),"sorting only presses cannot preserve the recipient's complete event order");
}

#[test]
fn an_indeterminate_head_keeps_its_custody_while_another_recipient_progresses() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7561));
    attempt_run(&mut f,75610,272,true);attempt_run(&mut f,75611,273,true);
    let other=XServerFrontendClientId(7562);let other_window=XResourceId::new(0x307562,1);
    let (other_registration,other_channels)={
        let p=f.runner.frontend.as_mut().unwrap();let registry=&p.broker.registry;
        let context=namespaced(other,f.namespace);
        let (registration,channels)=registry.register_client_with_admission(other,Some(context)).unwrap();
        registry.attach_private_lifecycle(&registration,context).unwrap();
        let selected=Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry.attach_connection_state(&registration,f.namespace,selected.clone(),Arc::new(AtomicU64::new(0))).unwrap();
        {
            let mut s=selected.lock().unwrap();
            s.register(other_window,XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT),1),Rect{x:0,y:0,width:200,height:100});
            s.observe_mapped(other_window);s.update(other_window,Some((1<<2)|(1<<3)),None);
        }
        // Genuine owner operations change subsequent source resolution. Native
        // A holds remain retained; no foreign inventory or fake recipient is
        // inserted into this executor. We make no cleanup/lifecycle claim.
        let mut grabs=registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace,f.client.raw());
        grabs.grab_pointer(f.namespace,crate::XActiveInputGrab{owner:other.raw(),window:other_window,owner_events:false,pointer_mode:1,keyboard_mode:1,event_mask:u16::MAX,xi_event_mask:[0;8],xi_event_mask_words:0,route_lease:None}).unwrap();
        (registration,channels)
    };
    attempt_run(&mut f,75612,274,true);
    let p=f.runner.frontend.as_mut().unwrap();assert_eq!(p.terminal.holds.len(),3);
    assert_eq!(p.terminal.holds[0].reached.client(),f.client);assert_eq!(p.terminal.holds[1].reached.client(),f.client);assert_eq!(p.terminal.holds[2].reached.client(),other);
    let recovery=p.broker.registry.input_recovery.clone();
    let cells=[75610,75611,75612].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let record=&mut p.terminal.holds[0];let emission=record.native.as_mut().unwrap().take_press_emission().unwrap();
    PrivateXServerFrontend::stow_press_capsule(&mut record.custody,emission,&recovery,f.client);
    // Explicitly staged at the state between the dispatch phase write and
    // capsule take. No actual interrupted send or already-sent byte is claimed.
    record.custody.dispatch=PrivateDispatchPhase::Indeterminate;
    let mut seen_a=Vec::new();let mut seen_b=Vec::new();
    for _ in 0..8 {
        let _=p.deliver_one(None, &mut |_,_|Ok(())).unwrap();
        while let Ok(c)=f.channels.ordered.try_recv(){
            let index=if c.delivery()==XAuthorityInputDeliveryId::from_raw(75610){0}else{assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(75611));1};
            assert!(Arc::ptr_eq(&cells[index],&c.finalizer().unwrap().completion));seen_a.push(c.delivery());
        }
        while let Ok(c)=other_channels.ordered.try_recv(){
            assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(75612));assert_eq!(c.client(),other);
            assert!(Arc::ptr_eq(&cells[2],&c.finalizer().unwrap().completion));seen_b.push(c.delivery());
        }
    }
    let old_phase=p.terminal.holds[0].custody.dispatch;
    let old_owned=matches!(p.terminal.holds[0].custody.pending.as_ref(),Some(PrivatePendingDelivery::Capsule(c)) if c.delivery()==XAuthorityInputDeliveryId::from_raw(75610) && Arc::ptr_eq(&cells[0],&c.finalizer().unwrap().completion));
    assert!(cells.iter().all(|c|c.answer().is_none()));
    assert_eq!(seen_b,[XAuthorityInputDeliveryId::from_raw(75612)],"a distinct exact recipient must progress under retained arbitration");
    assert!(seen_a.is_empty(),"neither the indeterminate head nor any later event may enqueue for its recipient");
    assert_eq!(old_phase,PrivateDispatchPhase::Indeterminate);assert!(old_owned,"the exact unknown-handover capsule stays inventory-owned");
    drop(other_registration);
}

fn output_refused(f:&mut PreparedOrderedFixture,route:XAuthorityRoutedInput)->PrivateExecutionRefusal {
    f.ingress.submit(&f.keeper.lease(), route).unwrap();
    let PrivatePreparedRunner{frontend,keyboards,watch,..}=&mut f.runner;let p=frontend.as_mut().unwrap();
    assert!(matches!(p.step_once(keyboards,&mut |_,_|Ok(()),watch.as_ref().unwrap()).unwrap(),PrivateOrderedStep::Decided(_)));
    let Some(PrivateOrderedItem::Refused{refusal,custody,..})=p.terminal.turn.pop() else{panic!("typed pre-effect refusal required")};
    assert!(custody.observe().unwrap().is_some());refusal
}

#[test]
fn an_exhausted_event_order_refuses_before_any_effect() {
    for release in [false,true] {
        let mut f=prepared_ordered_fixture(XServerFrontendClientId(if release{7572}else{7571}));
        if release{attempt_run(&mut f,75720,272,true);}
        let p=f.runner.frontend.as_mut().unwrap();
        let held=if release{Some((p.terminal.holds[0].incarnation,p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize,p.terminal.holds[0].custody.completion.as_ref().unwrap().clone()))}else{None};
        // Counter exhaustion only is staged. The reservation, completion cell,
        // native source call boundary and typed refusal are real.
        p.terminal.next_event_order=u64::MAX;
        let id=XAuthorityInputDeliveryId::from_raw(if release{75721}else{75710});
        let route=button_to(f.surface,id,272,!release);
        assert!(matches!(output_refused(&mut f,route),PrivateExecutionRefusal::OrderExhausted));
        let p=f.runner.frontend.as_ref().unwrap();
        assert_eq!(p.terminal.next_event_order,u64::MAX);
        assert!(p.terminal.pending_custody.is_none() && p.terminal.native_pending.is_none() && p.terminal.settling.is_empty());
        assert_eq!(p.terminal.holds.len(),usize::from(release));
        let mask=p.broker.registry.pointer_state.lock().unwrap().get(&(f.namespace,SeatId::from_raw(1))).expect("existing mapper").state();
        assert_eq!(mask,if release{256}else{0});
        if let Some((incarnation,native,cell))=held {
            assert_eq!(p.terminal.holds[0].incarnation,incarnation);assert_eq!(p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize,native);
            assert!(Arc::ptr_eq(&cell,p.terminal.holds[0].custody.completion.as_ref().unwrap()));assert_eq!(Arc::strong_count(&cell),3);assert!(cell.answer().is_none());
        }
        let recovery=&p.broker.registry.input_recovery;let cell=recovery.completion_for(id).unwrap().unwrap();
        assert!(cell.answer().is_none());assert_eq!(Arc::strong_count(&cell),2);
        let (claimed,applied)={let s=recovery.state.lock().unwrap();let e=s.tickets.get(&id).unwrap();(e.claimed,e.may_have_applied)};
        assert!(!claimed && !applied);assert!(f.channels.ordered.try_recv().is_err());
    }
}

#[test]
fn an_instrument_recognises_its_admission_after_the_ticket_is_pruned() {
    // The helper used to ask the recovery for the cell when it was called, so
    // an admission whose ticket had been answered and pruned read as one that
    // was never handed over. The identity has to be fixed where the admission
    // mints it, and a lookup that returns nothing is not evidence.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7621));
    attempt_run(&mut f, 76210, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let id = XAuthorityInputDeliveryId::from_raw(76210);
    let original = admitted_cell(private, 76210);
    let mut inbox = OrderedInbox::default();

    let capsule = inbox
        .accepted(private, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("the actual recipient accepted the original capsule");
    assert_eq!(capsule.delivery(), id);
    assert_eq!(capsule.client(), f.client);
    assert!(Arc::ptr_eq(
        &original,
        &capsule.finalizer().unwrap().completion
    ));
    assert_eq!(
        handover_phase(private, &original),
        Some(PrivateDispatchPhase::Enqueued)
    );

    // A real finish and an ordinary observation prune the ticket. Nothing
    // about the handover changes: the capsule and the custody still carry the
    // cell this admission minted.
    recovery
        .finish(f.client, Some(id), XAuthorityInputDeliveryOutcome::WriteFailed)
        .unwrap();
    let answer = original
        .answer()
        .expect("the original admission owns the established answer");
    assert_eq!(answer.delivery, id);
    assert_eq!(answer.outcome, XAuthorityInputDeliveryOutcome::WriteFailed);
    assert!(
        recovery.observe(answer),
        "an ordinary observer consumes and prunes the exact answered ticket"
    );
    assert!(
        recovery.completion_for(id).unwrap().is_none(),
        "the lookup this instrument must not depend on is gone"
    );

    assert_eq!(
        handover_phase(private, &original),
        Some(PrivateDispatchPhase::Enqueued)
    );
    assert!(Arc::ptr_eq(
        &original,
        private.terminal.holds[0].custody.completion.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        &original,
        &capsule.finalizer().unwrap().completion
    ));

    // And the retained capsule is still recognised as this admission's, with
    // nothing replayed to find it again.
    inbox.taken.push(capsule);
    let found = inbox
        .accepted(private, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("an instrument holding the admission's own cell still knows it");
    assert_eq!(found.delivery(), id);
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "the actual accepted event is never replayed"
    );
}

#[test]
fn an_instrument_takes_the_admission_asked_for_and_keeps_the_others() {
    // Three events owed to one recipient, asked for out of order. An
    // instrument that returned whatever capsule it found first would answer
    // every one of these with the press, and a control built on it would be
    // asserting about an event it never named.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7631));
    let cells: Vec<_> = {
        let mut cells = Vec::new();
        for (id, button, pressed) in [(76310u64, 272u32, true), (76311, 272, false), (76312, 273, true)] {
            f.ingress
                .submit(&f.keeper.lease(), button_to(
                    f.surface,
                    XAuthorityInputDeliveryId::from_raw(id),
                    button,
                    pressed,
                ))
                .unwrap();
            let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut f.runner;
            let private = frontend.as_mut().unwrap();
            assert!(matches!(
                private
                    .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
                    .unwrap(),
                PrivateOrderedStep::Decided(_)
            ));
            cells.push(admitted_cell(private, id));
            let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
                panic!("an accepted request runs")
            };
            assert!(custody.observe().unwrap().is_some());
        }
        cells
    };
    let private = f.runner.frontend.as_mut().unwrap();

    // THE BUDGET STOPS AT THE ANSWER. Nothing is queued yet, so visits have to
    // be driven -- but only until this admission's capsule appears. Spending
    // the rest would hand over events this question never mentioned, which a
    // control asking about the first one has no business causing.
    let mut inbox = OrderedInbox::default();
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "nothing has been handed over before this"
    );
    let first = inbox
        .accepted(private, &f.channels.ordered, &cells[0], 8)
        .expect("a readable terminal step")
        .expect("the press is handed over and recognised");
    assert_eq!(
        first.delivery(),
        XAuthorityInputDeliveryId::from_raw(76310)
    );
    assert_ne!(
        handover_phase(private, &cells[2]),
        Some(PrivateDispatchPhase::Enqueued),
        "and the events nobody asked about are still owed"
    );
    assert!(
        inbox.taken.is_empty(),
        "nothing else was taken off that queue"
    );

    // Now let the rest go, so the whole stream is in hand.
    for _ in 0..12 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }
    inbox.collect(&f.channels.ordered);
    assert_eq!(
        inbox
            .taken
            .iter()
            .map(XAuthorityOrderedDelivery::delivery)
            .collect::<Vec<_>>(),
        [76311, 76312].map(XAuthorityInputDeliveryId::from_raw),
        "in their own order, with the one already taken out"
    );

    // ASKED FOR THE LAST ONE, out of order and with no budget at all.
    let third = inbox
        .accepted(private, &f.channels.ordered, &cells[2], 0)
        .expect("a readable terminal step")
        .expect("what is already queued counts without driving anything");
    assert_eq!(
        third.delivery(),
        XAuthorityInputDeliveryId::from_raw(76312),
        "the admission asked for, not the first capsule to hand"
    );
    assert!(Arc::ptr_eq(&cells[2], &third.finalizer().unwrap().completion));

    // The one it was not asked about is kept.
    assert_eq!(
        inbox
            .taken
            .iter()
            .map(XAuthorityOrderedDelivery::delivery)
            .collect::<Vec<_>>(),
        [76311].map(XAuthorityInputDeliveryId::from_raw),
        "unrelated capsules are retained, not consumed by someone else's question"
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
}
