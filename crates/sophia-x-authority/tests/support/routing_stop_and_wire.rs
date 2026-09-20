// Stopping mid-frame: why the wire is left unusable rather than yielding, and
// what reading the store does not take beneath it.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_stop_mid_frame_leaves_the_wire_unusable_rather_than_yielding() {
    // Keeping custody is not enough when bytes are already on the wire: the
    // next writer to take the serialization puts its own event directly after
    // a prefix. The stop exit that the shared wait answers was reached without
    // ever asking about a begun frame, so that is exactly what happened.
    //
    // The prefix is real -- the source built the frame and five of its bytes
    // actually went -- and the stored progress is explicitly staged, because
    // nothing here produces a naturally interrupted send.
    let client = XServerFrontendClientId(7891);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78910, 272, true);
    // A second admission, so one can be served and another staged mid-frame.
    attempt_run(&mut f, 78912, 273, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78912);
    for _ in 0..16 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
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
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // THE FIRST CAPSULE IS FINISHED THROUGH THE OWNER, not written and then
    // overwritten. Accepting Advanced and moving on would leave that delivery
    // owned and awaiting its own publication, and reading its bytes is not
    // that publication -- the staging below would be discarding custody rather
    // than adding to it.
    let first_cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 78910);
    let mut flushed = false;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("its own first event is served: {other:?}"),
        }
    }
    assert!(flushed, "the first delivery finished through this owner");
    assert_eq!(
        first_cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "and was published for its own admission"
    );
    assert!(owner.in_flight().is_none(), "so its slot is free");
    let mut drained = [0u8; 4096];
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}
    let taken = owner.queue.try_recv().expect("its next event");
    let frame = taken
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 7)
        .expect("its own first frame");
    {
        let mut socket = output.lock().expect("the connection's output");
        (*socket)
            .write_all(&frame.as_bytes()[..5])
            .expect("five real bytes of it go");
    }
    let staged_len = frame.as_bytes().len();
    assert!(staged_len > 5, "the frame really is longer than its prefix");
    owner.in_flight = Some(X11OrderedInFlight {
        delivery: taken,
        frame: 0,
        send: X11OrderedSendState {
            frame: Some(X11OrderedFrame {
                bytes: frame,
                progress: X11OrderedSendProgress::Sent(5),
            }),
            blocked: Duration::ZERO,
        },
    });
    assert!(owner.mid_frame(), "a frame is begun and not finished");

    // A WRITER IS ALREADY INSIDE. It passed the permission and holds the
    // serialization; setting the flag stops later admissions and retracts
    // nothing from this one. If the stop exit only barred, this writer would
    // resume and put its event straight after the prefix.
    let holder_output = output.clone();
    let holder_left = Arc::new(AtomicBool::new(false));
    let holder_flag = holder_left.clone();
    let (inside, resumed) = std::sync::mpsc::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let mut socket = holder_output.lock().expect("the connection's output");
        inside.send(()).expect("inside");
        wait_release.recv().expect("released");
        let wrote = (*socket).write_all(&[0xABu8; 32]).is_ok();
        // WHAT THIS FLAG WITNESSES, exactly: that this writer's write attempt
        // finished. It is stored before the guard goes, so it does not witness
        // the release -- and moving it after the drop would only trade one
        // scheduling assumption for another. What the assertion below rests on
        // is the narrower fact that the step cannot have decided before this
        // writer had done its work, which is what "a flag retracts nothing
        // from a writer already inside" means.
        holder_flag.store(true, Ordering::Release);
        drop(socket);
        wrote
    });
    resumed.recv().expect("the other writer holds the output");

    // Told to stop, with control pending so the shared wait is the exit taken.
    pending.store(1, Ordering::Release);
    stop.store(true, Ordering::Release);
    let observed = holder_left.clone();
    let stepper = std::thread::spawn(move || {
        let step = owner.serve_one(XByteOrder::LittleEndian, 7);
        // Read the instant the step decided, not after the join. The sleep
        // above does not prove the stepper had reached acquisition, so this
        // does not establish where it was waiting -- only that it did not
        // decide before the writer already inside had finished writing.
        let had_left = observed.load(Ordering::Acquire);
        (step, had_left, owner)
    });
    std::thread::sleep(Duration::from_millis(100));
    release.send(()).expect("let the inside writer go");
    let _inside_wrote = holder.join().expect("the inside writer finished");
    let (step, had_left, mut owner) = stepper.join().expect("the serving thread finished");
    assert!(
        matches!(step, X11OrderedServeStep::Stopped),
        "the writer is leaving: {step:?}"
    );
    assert!(
        had_left,
        "exclusion was claimed before the writer already inside had finished: \
         a flag stops later admissions and retracts nothing from one already in"
    );
    let _ = &mut owner;

    // AND THE WIRE IS NOT LEFT FOR ANYONE ELSE.
    assert!(
        wire.barred(),
        "a prefix with no writer to finish it makes the wire unusable"
    );
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &AtomicU16::new(1),
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "so no control event lands directly after those five bytes"
    );
    // The obligation is not what ended. The connection is.
    assert!(owner.in_flight().is_some(), "the delivery stays owned");
    assert!(cell.answer().is_none(), "and nobody answered for it");
}

#[test]
fn a_stop_arriving_after_admission_also_ends_a_wire_mid_frame() {
    // The other stop exit: admitted because nothing was pending and no stop was
    // set, then told to stop before the recheck under serialization. With a
    // frame begun it must end the wire there too, not simply hand the guard
    // back. Same staging note as the sibling control -- the prefix is real,
    // the stored progress is staged.
    let client = XServerFrontendClientId(7901);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 79010, 272, true);
    attempt_run(&mut f, 79012, 273, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 79012);
    for _ in 0..16 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
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
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // THE FIRST CAPSULE IS FINISHED THROUGH THE OWNER, not written and then
    // overwritten. Accepting Advanced and moving on would leave that delivery
    // owned and awaiting its own publication, and reading its bytes is not
    // that publication -- the staging below would be discarding custody rather
    // than adding to it.
    let first_cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 79010);
    let mut flushed = false;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("its own first event is served: {other:?}"),
        }
    }
    assert!(flushed, "the first delivery finished through this owner");
    assert_eq!(
        first_cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "and was published for its own admission"
    );
    assert!(owner.in_flight().is_none(), "so its slot is free");
    let mut drained = [0u8; 4096];
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}
    let taken = owner.queue.try_recv().expect("its next event");
    let frame = taken
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 7)
        .expect("its own first frame");
    {
        let mut socket = output.lock().expect("the connection's output");
        (*socket)
            .write_all(&frame.as_bytes()[..5])
            .expect("five real bytes of it go");
    }
    owner.in_flight = Some(X11OrderedInFlight {
        delivery: taken,
        frame: 0,
        send: X11OrderedSendState {
            frame: Some(X11OrderedFrame {
                bytes: frame,
                progress: X11OrderedSendProgress::Sent(5),
            }),
            blocked: Duration::ZERO,
        },
    });
    assert!(owner.mid_frame());

    // Nothing pending and no stop, so the helper admits; the parent holds the
    // output, so the step blocks after that admission. The stop is set while
    // it is blocked, which only the recheck can see.
    let held = output.lock().expect("the connection's own output");
    let serving_stop = stop.clone();
    let (started, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        started.send(()).expect("started");
        let step = owner.serve_one(XByteOrder::LittleEndian, 7);
        (step, owner)
    });
    wait.recv().expect("the serving thread started");
    std::thread::sleep(Duration::from_millis(50));
    serving_stop.store(true, Ordering::Release);
    drop(held);

    let (step, owner) = server.join().expect("the serving thread finished");
    assert!(
        matches!(step, X11OrderedServeStep::Stopped),
        "the writer is leaving: {step:?}"
    );
    assert!(
        wire.barred(),
        "and the wire it left a prefix on is unusable"
    );
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &AtomicU16::new(1),
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "so nothing lands after those five bytes"
    );
    assert!(owner.in_flight().is_some(), "the delivery stays owned");
    assert!(cell.answer().is_none());
}

#[test]
fn a_stop_that_cannot_take_the_output_claims_nothing_and_says_why() {
    // Failing to acquire the output is not a shutdown error, and recording one
    // would describe a syscall never made. Exclusion is not established, so it
    // is not claimed, and the reason is kept as its own kind.
    let client = XServerFrontendClientId(7911);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 79110, 272, true);
    attempt_run(&mut f, 79112, 273, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 79112);
    for _ in 0..16 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
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
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    let first_cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 79110);
    let mut flushed = false;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("its own first event is served: {other:?}"),
        }
    }
    assert!(flushed);
    assert!(first_cell.answer().is_some());
    let mut drained = [0u8; 4096];
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}

    let taken = owner.queue.try_recv().expect("its next event");
    let frame = taken
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 7)
        .expect("its own first frame");
    {
        let mut socket = output.lock().expect("the connection's output");
        (*socket)
            .write_all(&frame.as_bytes()[..5])
            .expect("five real bytes of it go");
    }
    owner.in_flight = Some(X11OrderedInFlight {
        delivery: taken,
        frame: 0,
        send: X11OrderedSendState {
            frame: Some(X11OrderedFrame {
                bytes: frame,
                progress: X11OrderedSendProgress::Sent(5),
            }),
            blocked: Duration::ZERO,
        },
    });

    // The real output mutex, poisoned without touching the socket.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = output.lock().unwrap();
            panic!("intentional output-lock poison for acquisition fixture");
        }))
        .is_err()
    );
    assert!(output.is_poisoned());

    pending.store(1, Ordering::Release);
    stop.store(true, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Unterminated
        ),
        "exclusion could not be established, so nothing is claimed"
    );
    assert_eq!(
        owner.unterminated_cause(),
        Some(X11OrderedUnterminatedCause::OutputUnavailable),
        "and the reason is its own kind, not an invented shutdown error"
    );
    assert!(
        !wire.barred(),
        "no exclusion was claimed over a wire this could not take"
    );
    assert!(owner.in_flight().is_some(), "the delivery stays owned");
    assert!(cell.answer().is_none());
}

#[test]
fn a_continuation_place_is_taken_before_exposure_and_kept_while_work_remains() {
    // The bound covers live connections and retained leftovers together. A
    // connection that returned its place while still owing work would let a
    // churn of connections grow retention without limit, which is the whole
    // reason the two share one number.
    let client = XServerFrontendClientId(7921);
    let f = prepared_ordered_fixture(client);
    // UNCONFIGURED ADMITS NOTHING. A connection bound is not the
    // abandoned-work capacity; an owner that inherited one for the other would
    // allow as many connections as it happens to allow obligations, which is a
    // number nobody chose.
    let unconfigured = PrivateSettlementOwner::with_capacity(4);
    assert!(
        matches!(
            unconfigured.reserve_ordered_continuation(),
            Err(AdmissionRefusal::Saturated)
        ),
        "no connection is exposed against a bound nobody set"
    );

    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    assert_eq!(durable.continuations_reserved(), Some(0));

    let first = durable
        .reserve_ordered_continuation()
        .expect("a place for the first connection");
    let second = durable
        .reserve_ordered_continuation()
        .expect("a place for the second");
    assert_eq!(durable.continuations_reserved(), Some(2));

    // EXHAUSTED BEFORE PUBLICATION. A third connection cannot be exposed,
    // because there would be nowhere for what it accepted to go.
    assert!(
        matches!(
            durable.reserve_ordered_continuation(),
            Err(AdmissionRefusal::Saturated)
        ),
        "a connection that cannot be handed over is not built"
    );

    // A connection that finished owing nothing gives its place back.
    first.finish();
    assert_eq!(durable.continuations_reserved(), Some(1));
    let third = durable
        .reserve_ordered_continuation()
        .expect("the returned place is reusable");
    assert_eq!(durable.continuations_reserved(), Some(2));

    // AND ONE THAT STILL OWES WORK KEEPS ITS PLACE. Installing is not
    // returning: the leftovers occupy the place they were promised.
    // A real admission accepted into this connection's queue before anything
    // could be built for it -- the window the Setup case exists for.
    let sender = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();
    let (emission, _endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(79210);
    let stranded = XAuthorityOrderedDelivery::from_emission(emission).unwrap();
    gated_send(&sender, stranded).expect("accepted into its queue");

    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(second, &mut source), PrivateContinuationCommit::Retained);
    assert!(source.is_none(), "it left the caller's slot");
    // AND IT IS STILL THERE. The queue came with the place, so an admission
    // accepted before a serving owner existed is not lost with the setup that
    // failed.
    let survived = durable
        .with_ordered_continuation(1, |continuation| {
            continuation
                .queue()
                .try_recv()
                .map(|capsule| capsule.delivery())
        })
        .expect("the place holds it");
    assert_eq!(
        survived.expect("the stranded admission"),
        XAuthorityInputDeliveryId::from_raw(79210),
        "accepted before the owner existed, and retained with its queue"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(2),
        "retained work still occupies its place"
    );
    assert_eq!(durable.continuations_retained(), Some(1));
    assert!(
        matches!(
            durable.reserve_ordered_continuation(),
            Err(AdmissionRefusal::Saturated)
        ),
        "so a reconnect cannot take a place the leftovers still hold"
    );
    assert_eq!(durable.continuations_abandoned(), Some(0));
    drop(third);
    assert_eq!(
        durable.continuations_abandoned(),
        Some(1),
        "a place whose holder disposed of neither is marked, not handed out again"
    );
    assert_eq!(
        durable.continuations_reserved(),
        Some(2),
        "and stays taken"
    );
}

#[test]
fn a_whole_serving_owner_moves_into_its_place_with_everything_it_held() {
    // One ownership move. Nothing is unpacked, rebuilt, reselected or looked
    // up again: what arrives is what left, including how far a frame got and
    // why an ending refused.
    let client = XServerFrontendClientId(7931);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 79310, 272, true);
    attempt_run(&mut f, 79312, 273, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let staged_cell = admitted_cell(private, 79312);
    for _ in 0..16 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
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
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // Its first event finishes properly, so what is staged below adds to this
    // owner's custody rather than replacing it.
    let first_cell = admitted_cell(f.runner.frontend.as_ref().unwrap(), 79310);
    let mut flushed = false;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("its own first event is served: {other:?}"),
        }
    }
    assert!(flushed && first_cell.answer().is_some());
    let mut drained = [0u8; 4096];
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}

    // A real capsule with a real prefix written and its progress staged.
    let taken = owner.queue.try_recv().expect("its next event");
    let staged_finalizer = Arc::downgrade(taken.finalizer().expect("carried"));
    let frame = taken
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 7)
        .expect("its own first frame");
    let staged_bytes = frame.as_bytes().to_vec();
    {
        let mut socket = output.lock().expect("the connection's output");
        (*socket)
            .write_all(&staged_bytes[..5])
            .expect("five real bytes of it go");
    }
    owner.in_flight = Some(X11OrderedInFlight {
        delivery: taken,
        frame: 0,
        send: X11OrderedSendState {
            frame: Some(X11OrderedFrame {
                bytes: frame,
                progress: X11OrderedSendProgress::Sent(5),
            }),
            blocked: Duration::ZERO,
        },
    });
    // And a failed ending, so a typed cause travels with it.
    owner.unterminated = true;
    owner.unterminated_cause =
        Some(X11OrderedUnterminatedCause::Shutdown(std::io::ErrorKind::PermissionDenied));

    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before this connection was exposed");
    let mut source = Some(PrivateOrderedContinuation::Serving {
        owner: home_holding(owner),
        // STAGED, NOT OBSERVED. No teardown ran in this control; this is the
        // value a record installed by one would carry, set directly because
        // the subject here is what arrives in the place, not what writes this
        // field.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
    assert!(source.is_none(), "it left the caller's slot");
    assert_eq!(durable.continuations_retained(), Some(1));

    // EVERYTHING ARRIVED. Read through a borrow of the stored record.
    let checked = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("a serving owner was installed")
            };
            let held = owner.in_flight().expect("its in-flight record");
            let frame = held
                .send
                .frame
                .as_ref()
                .expect("the frame it was part way through");
            (
                held.delivery().delivery(),
                frame.progress,
                frame.bytes.as_bytes().to_vec(),
                owner.unterminated_cause(),
                Arc::ptr_eq(
                    held.delivery().finalizer().expect("carried"),
                    &staged_finalizer.upgrade().expect("still alive"),
                ),
            )
        })
        .expect("the place holds it");
    assert_eq!(checked.0, XAuthorityInputDeliveryId::from_raw(79312));
    assert_eq!(
        checked.1,
        X11OrderedSendProgress::Sent(5),
        "how far its bytes got came with it"
    );
    assert_eq!(checked.2, staged_bytes, "and the exact bytes, not re-encoded");
    assert_eq!(
        checked.3,
        Some(X11OrderedUnterminatedCause::Shutdown(
            std::io::ErrorKind::PermissionDenied
        )),
        "and why its ending refused"
    );
    assert!(checked.4, "carrying its own original finalizer");
    assert!(
        staged_cell.answer().is_none(),
        "a transfer answers nobody"
    );
}

#[test]
fn an_unreadable_owner_does_not_make_an_accepted_transfer_optional() {
    // The work has been accepted and the place is this connection's. Skipping
    // the move because the aggregate lock is unreadable would drop it, which
    // is the one thing a reserved destination exists to prevent.
    let client = XServerFrontendClientId(7941);
    let f = prepared_ordered_fixture(client);
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");

    // The real settlement mutex, poisoned without touching any record.
    let poisoner = durable.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("intentional settlement-lock poison for transfer fixture");
        }))
        .is_err()
    );
    assert!(durable.inner.is_poisoned());
    assert_eq!(
        durable.continuations_retained(),
        None,
        "and an ordinary read of it now refuses"
    );

    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    // It went in anyway, and can be read back through the same fail-closed
    // discipline every other already-accepted move here uses.
    let held = durable
        .with_ordered_continuation(0, |continuation| {
            matches!(
                continuation,
                PrivateOrderedContinuation::Setup {
                    refusal: X11OrderedServingRefusal::TransportUnavailable,
                    ..
                }
            )
        })
        .expect("the place holds it");
    assert!(held, "the accepted transfer happened despite the poison");
}

#[test]
fn driving_a_continuation_does_not_hold_the_store_behind_it() {
    // The helper's comment promised the aggregate lock was released and the
    // code held it across the callback, so every other retained connection sat
    // behind whichever one was being driven -- and a close that waits on its
    // output would have taken that wait under the settlement lock.
    let client = XServerFrontendClientId(7951);
    let f = prepared_ordered_fixture(client);
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    // Asked from inside the callback, about the real store.
    let store_free = durable
        .with_ordered_continuation(0, |_| durable.inner.try_lock().is_ok())
        .expect("the place holds it");
    assert!(
        store_free,
        "the settlement lock is released before a record is driven"
    );

    // And another place can still be reserved while one is being driven.
    let reserved_during = durable
        .with_ordered_continuation(0, |_| durable.reserve_ordered_continuation().is_ok())
        .expect("the place holds it");
    assert!(
        reserved_during,
        "one retained connection does not block the store for the rest"
    );
}

#[test]
fn a_bound_transport_that_could_not_be_served_keeps_its_ending_handle() {
    // How far setup got decides what there is to keep. A receiver that was
    // published but never bound has its queue; one that was bound has the
    // connection's output, its permission and -- the part that matters -- the
    // independent handle that can still end a wire nobody will serve. Keeping
    // only the queue there would discard the one thing able to terminate it.
    let client = XServerFrontendClientId(7971);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 79710, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 79710);
    for _ in 0..8 {
        private.deliver_one(None, &mut |_, _| Ok(())).unwrap();
    }

    let PreparedOrderedFixture {
        mut runner,
        durable: _fixture_durable,
        registration,
        channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport = XAuthorityOrderedTransport::bind(
        &registration,
        channels.ordered,
        &output,
        &wire,
        &pending,
        None,
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));

    // The registration stops being the current row, so serving cannot be
    // prepared -- with the transport already bound and holding real work.
    //
    // NO REPLACEMENT IS MADE, AND NONE IS NEEDED. What this control is about
    // is a registration that is no longer current, which removing the row
    // establishes on its own. A successor at the same number could not exist
    // here anyway: this registration still holds that number.
    private
        .participant
        .revoke_admission(client, admitted(client).client_id)
        .expect("the admission this fixture made is the one it revokes");
    private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .remove(&client);
    let (refusal, returned) =
        X11OrderedServingOwner::for_registration(private, &registration, transport)
            .err()
            .expect("a stale registration prepares no writer");

    // RETAINED WHOLE, ending handle included.
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Transport(Box::new(returned)),
        refusal,
        // Torn down, with its endpoint closed: the only way a record reaches
        // a place.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
    assert!(source.is_none());

    let ended = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(transport),
                ..
            } = continuation
            else {
                panic!("a bound transport was retained")
            };
            // Its queue came with it, and so did the handle that can end the
            // wire without the output lock.
            let queued = transport.ordered.try_recv().map(|capsule| capsule.delivery());
            let ended = transport.shutdown.shutdown(Shutdown::Both).is_ok();
            (queued, ended)
        })
        .expect("the place holds it");
    assert_eq!(
        ended.0.expect("the accepted admission came with it"),
        XAuthorityInputDeliveryId::from_raw(79710)
    );
    assert!(
        ended.1,
        "and the retained transport can still end the connection"
    );
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "which the peer sees"
    );
    assert!(cell.answer().is_none(), "retention answers nobody");
    drop(registration);
    drop(runner);
}

#[test]
fn a_handover_waits_for_the_destination_reserved_for_it() {
    // The record was made while installing, so the one interval that must
    // contain nothing fallible contained an allocation: between taking a
    // connection's work out of its source and putting it somewhere. The
    // aggregate's reserved place and the record are different storage, and
    // reserving has to make both.
    let client = XServerFrontendClientId(7981);
    let f = prepared_ordered_fixture(client);
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");

    // WHAT THIS ESTABLISHES: that a home exists and is empty from the moment
    // its place is reserved, that a binding does not COMPLETE while something
    // holds that home, and that it completes once released. One owner, and a
    // handle is a way to ask rather than a way in.
    //
    // It does NOT establish where the other thread has got to -- a signal
    // before the attempt plus a sleep proves neither entry nor ordering -- and
    // there is no longer any take to order against an acquisition: the payload
    // goes into its home when the connection binds and is never anywhere
    // else.
    let record = {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(record) = &held.continuations[0] else {
            panic!("reserving made the record")
        };
        record.clone()
    };
    assert!(
        !record.occupied(),
        "made empty, at reservation"
    );

    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });

    // Held directly, because what is being established is that nothing else
    // gets at this home while a holder has it.
    let blocker = record.state.lock().expect("hold the destination");
    let (started, wait) = std::sync::mpsc::channel();
    let (checked, report) = std::sync::mpsc::channel();
    let installer = std::thread::spawn(move || {
        started.send(()).expect("started");
        assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
        checked.send(source.is_none()).expect("reported");
    });
    wait.recv().expect("the binding thread started");
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        report.try_recv().is_err(),
        "a binding does not complete while something holds the home"
    );
    drop(blocker);
    let took = report.recv().expect("the binding finished");
    installer.join().expect("the binding thread finished");
    assert!(took, "and then it went in");

    assert_eq!(durable.continuations_retained(), Some(1));
    assert_eq!(durable.continuations_abandoned(), Some(0));
}

#[test]
fn a_handover_with_nothing_to_hand_over_is_recorded_rather_than_counted_done() {
    // Installing an empty source left the place looking taken by someone who
    // would come back for it, while no holder or driver remained. It is not
    // installed and not released; it is reported.
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut empty = None;
    assert_eq!(retain_into(slot, &mut empty), PrivateContinuationCommit::NothingBound);
    assert_eq!(durable.continuations_retained(), Some(0), "nothing installed");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "and the place is not handed out again"
    );
    assert_eq!(
        durable.continuations_abandoned(),
        Some(1),
        "it is recorded as having no holder, not counted as done"
    );
}

#[test]
fn reading_the_store_does_not_take_a_record_beneath_it() {
    // Driving holds a record and may enter settlement. A reader that took a
    // record while holding the store would close that cycle from the other
    // side, so the store must be released before any record is read.
    let client = XServerFrontendClientId(7991);
    let f = prepared_ordered_fixture(client);
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let PreparedOrderedFixture { channels, .. } = f;
    let mut source = Some(PrivateOrderedContinuation::Setup {
        accepted: PrivateOrderedSetupCustody::Receiver(Box::new(channels.ordered)),
        refusal: X11OrderedServingRefusal::TransportUnavailable,
        // A staged precondition: the value a torn-down record carries, set
        // directly rather than observed, because this control is about what
        // happens to a record that has one.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
        retained: Vec::new(),
        drained: false,
        ended: false,
        ending_refused: None,
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    let record = {
        let held = durable.records_even_if_poisoned();
        let PrivateOrderedContinuationPlace::Taken(record) = &held.continuations[0] else {
            panic!("its place holds the record")
        };
        record.clone()
    };

    // The record is held, as a driver would hold it. The store must stay
    // available to everyone else.
    let blocker = record.state.lock().expect("hold the record");
    assert!(
        durable.inner.try_lock().is_ok(),
        "the store is not behind a record"
    );
    let reporting = durable.clone();
    let reader = std::thread::spawn(move || reporting.continuations_retained());
    // The sleep does NOT establish that the reader reached the record lock --
    // a descheduled thread may not have started. What this half asserts is the
    // weaker fact that the store stays available while a reader is outstanding
    // and the record is held; the stronger claim about that exact interval is
    // not established here.
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        durable.inner.try_lock().is_ok(),
        "the store stays available while a reader is outstanding"
    );
    drop(blocker);
    assert_eq!(
        reader.join().expect("the reader finished"),
        Some(1),
        "and it reports once the record is free"
    );
}
