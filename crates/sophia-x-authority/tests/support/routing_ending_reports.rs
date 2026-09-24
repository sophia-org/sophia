// Endings, reported: the attempt handed back, the place that was never
// consumed, and the preparation whose refusal leaves the transport untouched.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.



/// One real grant from the ledger, taken the way the executor takes one.
///
/// Not a fabricated token: the authority chooses the debt and issues the
/// claim, so what is held afterwards is the ledger's own slot.
fn real_attempt_claim(
    private: &crate::PrivateXServerFrontend,
) -> sophia_input_authority::AttemptClaim {
    let mut cursor = 0;
    private
        .authority()
        .under_common_as_origin(|authority, issuer| {
            authority.claim_next_attempt(issuer, &mut cursor)
        })
        .expect("a readable authority")
        .expect("this issuer answers for it")
        .expect("a debt to attempt")
}

#[test]
fn an_unplaced_attempt_goes_back_and_the_ledger_hands_the_debt_out_again() {
    // UNPLACED IS PROVABLY UNUSED. Nothing was written onto the record and
    // nothing was taken, so this grant reached no recipient and may go back --
    // and going back is what the LEDGER confirms, not what a cleared slot
    // here suggests. The debt being offered again is the ledger saying so.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8521));
    attempt_release(&mut f, 85210, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));

    let claim = real_attempt_claim(private);
    let incarnation = claim.hold;
    assert_eq!(
        private.terminal.settling[0].incarnation(),
        incarnation,
        "the ledger chose this release's own debt"
    );
    private.terminal.attempt_custody = Some(PrivateAttemptCustody {
        token: claim.token,
        phase: PrivateAttemptPhase::Unplaced,
    });

    assert_eq!(
        private.relinquish_one_attempt(),
        Some(true),
        "an unplaced grant goes back, and the ledger confirmed it"
    );
    assert!(
        private.terminal.attempt_custody.is_none(),
        "the executor stops naming it only once that confirmation landed"
    );

    // THE LEDGER'S OWN ACCOUNT, not this executor's. The same debt is granted
    // again, which it could not be while an attempt on it was outstanding.
    let again = real_attempt_claim(private);
    assert_eq!(
        again.hold, incarnation,
        "the debt is outstanding again, so the return really did land"
    );
    assert_ne!(
        again.token, claim.token,
        "and it is a new grant, not the old token handed back"
    );
    assert!(
        private.relinquish_outstanding_attempt(again.token),
        "this control's own cleanup is confirmed too, so it leaves the ledger \
         as it found it"
    );
}

#[test]
fn a_dispatching_attempt_is_never_given_back_as_unused() {
    // DISPATCHING MEANS IT MAY HAVE REACHED THE RECIPIENT. The record names it
    // and the phase says the handover was begun, so no path may hand this
    // grant back as an unused reservation -- that would tell the ledger
    // nothing happened for a delivery that may already be on a queue.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8531));
    attempt_release(&mut f, 85310, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));

    let claim = real_attempt_claim(private);
    let incarnation = claim.hold;
    // The write-ahead state: named on the record, phase says begun. Staged,
    // because an unwind is what leaves it and this crate cannot unwind a
    // producer mid-call; the token and the debt are the ledger's own.
    private.terminal.attempt_custody = Some(PrivateAttemptCustody {
        token: claim.token,
        phase: PrivateAttemptPhase::Dispatching,
    });
    private.terminal.settling[0].custody.attempt = Some(claim.token);
    private.terminal.settling[0].custody.dispatch = PrivateDispatchPhase::Indeterminate;

    assert!(
        private.relinquish_one_attempt().is_none(),
        "a grant that may have been used is not an unused reservation"
    );
    assert_eq!(
        private
            .terminal
            .attempt_custody
            .map(|custody| (custody.token, custody.phase)),
        Some((claim.token, PrivateAttemptPhase::Dispatching)),
        "it stays held, as exactly the token and phase it was"
    );
    assert_eq!(
        private.terminal.settling[0].custody.attempt,
        Some(claim.token),
        "and the record goes on naming that same token"
    );

    // THE LEDGER STILL HAS IT. The debt is not offered again, which is what an
    // outstanding attempt looks like from the other side.
    let mut cursor = 0;
    let next = private
        .authority()
        .under_common_as_origin(|authority, issuer| {
            authority.claim_next_attempt(issuer, &mut cursor)
        })
        .expect("a readable authority")
        .expect("this issuer answers for it");
    // One debt, one attempt: with that attempt outstanding there is nothing
    // left for the ledger to grant at all.
    assert!(
        next.is_none(),
        "a debt with an attempt outstanding is not granted again, and this \
         fixture has no other debt to offer"
    );
    let _ = incarnation;
}


#[test]
fn an_attempt_whose_return_was_not_confirmed_stays_named_here() {
    // THE LEDGER'S CONFIRMATION IS THE EVIDENCE, not the call. A give-back
    // that got no answer has established nothing: the ledger may still hold
    // the slot, and an executor that stopped naming the token on the strength
    // of having asked would have lost the only handle on it.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8541));
    attempt_release(&mut f, 85410, 272);
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(private.record_one_native(), Some(true));
    let claim = real_attempt_claim(private);
    private.terminal.attempt_custody = Some(PrivateAttemptCustody {
        token: claim.token,
        phase: PrivateAttemptPhase::Unplaced,
    });

    // The authority cannot be read: a holder panicked inside it. Asking is
    // still possible; getting an answer is not.
    let common = Arc::clone(&private.authority().common);
    let holder = std::thread::spawn(move || {
        let _inside = common.lock().expect("a readable authority");
        panic!("a holder unwound inside the authority");
    });
    assert!(holder.join().is_err(), "the holder unwound");

    assert_eq!(
        private.relinquish_one_attempt(),
        Some(false),
        "asked, and not answered"
    );
    assert_eq!(
        private
            .terminal
            .attempt_custody
            .map(|custody| custody.token),
        Some(claim.token),
        "so the executor goes on naming exactly that token, unchanged"
    );
    assert_eq!(
        private
            .terminal
            .attempt_custody
            .map(|custody| custody.phase),
        Some(PrivateAttemptPhase::Unplaced),
        "and goes on knowing it was never placed"
    );
}


#[test]
fn a_record_that_cannot_finish_says_which_things_are_stopping_it() {
    // THE REASONS COEXIST AND ARE REPORTED SEPARATELY. A connection can have
    // no way to end its wire AND a closure nobody could establish. They are
    // independent facts of one bounded record: reporting either alone loses
    // the other, and collapsing them into "not finished" sends whoever reads
    // it looking for the wrong thing.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8551);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let other = XServerFrontendClientId(8552);
    let (other_registration, other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a second place and row");

    // A real refusal leaves it holding a receiver and no way to reach the
    // connection at all.
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        registration
            .bind_ordered_output(other_channels.ordered, &output, &wire, &pending)
            .unwrap_or_else(|_| panic!("a fresh registration holds no custody")),
        Some(X11OrderedServingRefusal::ForeignReceiver)
    );
    // And a producer unwinds inside its gate, so the closure establishes
    // nothing either.
    let gate = registration.ordered_gate.clone();
    let holder = std::thread::spawn(move || {
        let _inside = gate.fenced.lock().expect("an open gate");
        panic!("a handover unwound inside this gate");
    });
    assert!(holder.join().is_err(), "the holder unwound");
    drop(channels);
    drop(registration);
    drop(other_registration);
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }

    let readings = durable
        .retained_dispositions()
        .expect("a readable store");
    let stuck = readings
        .iter()
        .find_map(|(index, reading)| (*index == 0).then_some(reading.clone()))
        .expect("the connection that could not finish")
        .expect("and it is readable");
    assert_eq!(
        stuck.ending,
        PrivateRetainedEnding::NoCapability,
        "no handle on the connection: not a refused shutdown, and not an ending"
    );
    assert_eq!(
        stuck.closure,
        Some(PrivateHandoverFence::Unreadable),
        "and separately, a closure nobody could establish"
    );
    assert!(!stuck.settled, "so it owes something");
    assert_eq!(
        (stuck.drained, stuck.retained),
        (true, 0),
        "while its queue did finish and it is holding nothing, which is why \
         neither of those can be what a reader is told"
    );
}

#[test]
fn a_reading_reports_an_ending_and_a_closure_that_were_established() {
    // The same reading over a connection that got everything it needed, so
    // the one above is not simply reporting sadness at everything.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8561);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));
    drop(registration);
    drop(private);

    // Read before it is driven: closed, nothing ended yet, nothing held.
    let before = durable.retained_dispositions().expect("a readable store");
    assert_eq!(before.len(), 1);
    let reading = before[0].1.clone().expect("a readable record");
    assert_eq!(reading.closure, Some(PrivateHandoverFence::Established));
    assert_eq!(
        reading.ending,
        PrivateRetainedEnding::Unattempted,
        "it has a handle and has not used it: that is not the same as having none"
    );
    assert!(!reading.settled);

    // Driven, and then gone: a settled record returns its place, so there is
    // nothing left to report.
    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    assert!(
        durable
            .retained_dispositions()
            .expect("a readable store")
            .is_empty(),
        "a place that came back is not a connection, and is not reported as one"
    );

    // NEITHER IS A PLACE THAT WAS PROMISED AND NEVER FILLED. A connection that
    // ended without ever binding keeps its place -- what was accepted for it
    // is unknown, so it is not given back -- but the place holds no record,
    // and there is no connection there to report. A reading that invented one
    // would put a row in front of an operator with nothing behind it.
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let never_bound = XServerFrontendClientId(8562);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(never_bound, Some(admitted(never_bound)))
        .expect("a place and a row");
    drop(channels);
    drop(registration);
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "its place is held, because what was accepted for it is unknown"
    );
    assert!(
        durable
            .retained_dispositions()
            .expect("a readable store")
            .is_empty(),
        "and yet there is no connection there to read"
    );
}


/// A reading of one serving record, installed in a store of its own.
fn serving_reading(
    owner: X11OrderedServingOwner,
) -> (PrivateSettlementOwner, PrivateRetainedDisposition) {
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut source = Some(PrivateOrderedContinuation::Serving {
        owner: home_holding(owner),
        // A staged precondition: what a torn-down record carries.
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
    let reading = durable
        .retained_dispositions()
        .expect("a readable store")[0]
        .1
        .clone()
        .expect("a readable record");
    (durable, reading)
}

#[test]
fn a_refused_ending_is_reported_as_refused_and_not_as_untried() {
    // A CLOSE THAT TRIED AND WAS REFUSED IS NOT A CLOSE THAT NEVER TRIED. The
    // refusal is written on the close record itself, and a reading that only
    // consulted the owner's separate cause reported it as unattempted --
    // sending whoever read it looking for a syscall that had already happened
    // and failed.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8571));
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    // Staged: this host gives no honest way to make a shutdown refuse, so the
    // close record is written as begin_close's own failure branch writes it --
    // one attempt made, refused, and no ending established.
    owner.closing = Some(X11OrderedClosing {
        cause: X11OrderedCloseCause::ConnectionEnded,
        termination: X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied),
        attempts: 1,
        answered: 0,
        already: 0,
        deferred: 0,
        drained: false,
    });
    assert!(
        !owner.ending_ended(),
        "nothing ended this wire, which is the state being reported"
    );
    assert!(
        owner.unterminated_cause().is_none(),
        "and the owner carries no separate cause, so the close record is the \
         only thing that knows"
    );

    let (_durable, reading) = serving_reading(owner);
    assert_eq!(
        reading.ending,
        PrivateRetainedEnding::Refused(std::io::ErrorKind::PermissionDenied),
        "the close record says it was refused, and that is what is reported"
    );
}

#[test]
fn an_ending_that_was_established_outranks_a_cause_recorded_before_it() {
    // A LATER ESTABLISHED ENDING IS THE ENDING. An earlier attempt that
    // refused leaves its cause standing on the owner; reading that first
    // reported a wire as unterminated after it had actually been ended.
    //
    // The cause is not discharged by being outranked: it still says what
    // happened, and the report saying Ended authorises nothing about it.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8581));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    // Staged history: an earlier attempt refused.
    owner.unterminated = true;
    owner.unterminated_cause = Some(X11OrderedUnterminatedCause::Shutdown(
        std::io::ErrorKind::PermissionDenied,
    ));
    // And then a real close, over a real peer that really goes.
    drop(peer);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("this socket ends");

    assert!(
        owner.unterminated_cause().is_some(),
        "the earlier cause is still on the owner, undischarged"
    );
    let (_durable, reading) = serving_reading(owner);
    assert_eq!(
        reading.ending,
        PrivateRetainedEnding::Ended,
        "what is reported is the ending that was established"
    );
}

#[test]
fn an_ending_through_a_part_written_frame_is_reported_as_an_ending() {
    // THIS PATH LEAVES NO CLOSE RECORD. Ending a part-written frame ends the
    // wire and writes nothing about having done so, and a reading that looked
    // only at close records called that unattempted.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8591));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    drop(peer);
    owner.end_partial_frame();
    assert!(
        owner.closing().is_none(),
        "no close was begun, which is the whole point of this path"
    );

    let (_durable, reading) = serving_reading(owner);
    assert_eq!(reading.ending, PrivateRetainedEnding::Ended);
}

#[test]
fn a_reading_counts_every_slot_that_is_holding_something() {
    // A CAPSULE HELD IN THE REFUSED SLOT IS CUSTODY. It is unanswered and it
    // is this owner's to answer for, exactly as one in the retained list is,
    // and a count that left it out reported nothing held while an unanswered
    // completion sat in the writer.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8601));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let sender = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .get(&f.client)
        .expect("its row")
        .ordered
        .clone();
    let (foreign, _endpoint, _recovery, _receipts) = answerable_capsule(86010);
    let cell = Arc::clone(&foreign.finalizer().expect("carried").completion);
    gated_send(&sender, foreign).expect("this owner's queue accepts it");
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(_)
    ));
    assert!(
        owner.refused().is_some(),
        "the writer is holding it, unanswered"
    );
    assert!(cell.answer().is_none());

    let (_durable, reading) = serving_reading(owner);
    assert_eq!(
        reading.retained, 1,
        "so the reading says one thing is held, rather than nothing"
    );
    assert!(!reading.settled);
}

#[test]
fn a_record_that_cannot_be_read_is_reported_as_unreadable() {
    // NOT SKIPPED, AND NOT GUESSED AT. A poisoned record may well hold a
    // connection; what it does not hold is a reading. Treating it as a normal
    // row would put an account in front of a reader with nothing behind it,
    // and skipping it would lose a connection from the account entirely.
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8611));
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (owner, _output) = serving_owner_for(&mut f, socket);
    let mut source = Some(PrivateOrderedContinuation::Serving {
        owner: home_holding(owner),
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);

    // A holder panics inside this record.
    let record = {
        let held = durable.inner.lock().expect("a readable store");
        let PrivateOrderedContinuationPlace::Taken(record) = &held.continuations[0] else {
            panic!("its place holds a record")
        };
        record.clone()
    };
    let holder = std::thread::spawn(move || {
        record.borrow(|_| panic!("a holder unwound inside this record"))
    });
    assert!(holder.join().is_err(), "the holder unwound");

    let readings = durable.retained_dispositions().expect("a readable store");
    assert_eq!(readings.len(), 1, "the connection is still in the account");
    assert!(
        readings[0].1.is_none(),
        "and is reported as unreadable rather than as anything in particular"
    );
}


#[test]
fn an_ordinary_stop_ends_nothing_and_is_not_reported_as_an_ending() {
    // A STOP WITH NOTHING PART-WRITTEN LEAVES THE WIRE ALONE. No shutdown is
    // made, the connection is still usable and still unbarred, and its queue
    // still holds what was accepted.
    //
    // WHICH STOP THIS IS: the early one, where stop is already set when
    // serve_one is entered, so it returns without taking the output at all.
    // The other is a stop that arrives AFTER that check, which reaches the
    // exit under serialization with nothing part-written; "nothing refused" is
    // true there too, and reading an ending from it reported a live connection
    // as ended. Reaching that one needs a rendezvous inside serve_one, which
    // this crate has no way to make, so it is not established here.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8621));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (mut owner, output) = serving_owner_for(&mut f, socket);
    let stop = Arc::new(AtomicBool::new(true));
    owner.stop = Some(stop);
    assert!(!owner.mid_frame(), "nothing is part-written");

    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::Stopped
    ));
    assert!(
        !owner.ending_ended(),
        "nothing attempted an ending, so nothing may record one"
    );
    assert!(
        !owner.wire.barred(),
        "the connection's own permission was never barred"
    );
    let (_durable, reading) = serving_reading(owner);
    assert_eq!(reading.ending, PrivateRetainedEnding::Unattempted);

    // AND THE CONNECTION REALLY IS STILL THERE. Two separate facts: the
    // permission was never barred, and the socket was never shut down. The
    // write below goes through the raw output and so proves only the second;
    // the permission is asked directly, because a writer going through
    // admission would be stopped by a bar this control must rule out itself.
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "nothing was written and nothing was ended"
    );
    std::io::Write::write_all(&mut *output.lock().expect("its output"), b"alive")
        .expect("a wire nobody ended still carries bytes");
    let mut alive = [0u8; 5];
    std::io::Read::read_exact(&mut (&peer), &mut alive).expect("and they arrive");
    assert_eq!(&alive, b"alive");
}

#[test]
fn an_ending_reached_by_an_ordinary_serving_exit_is_reported_as_one() {
    // THE SAME FACT, REACHED THE ORDINARY WAY. A send that finds the peer gone
    // ends the wire on its way out and says so in the step it returns.
    // Recording an ending only on the close paths meant an owner whose wire
    // this had ended read as never having attempted one.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8631));
    attempt_run(&mut f, 86310, 272, true);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    {
        let private = f.runner.frontend.as_mut().unwrap();
        assert_eq!(private.dispatch_one_press(), Some(true));
    }
    // The peer really goes, so the write really fails.
    drop(peer);
    let mut ended = None;
    for _ in 0..8 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            step @ X11OrderedServeStep::Ended { .. } => {
                ended = Some(step);
                break;
            }
            X11OrderedServeStep::Idle => break,
            _ => continue,
        }
    }
    assert!(
        matches!(ended, Some(X11OrderedServeStep::Ended { shutdown: true, .. })),
        "the exit ended this wire and said so: {ended:?}"
    );
    assert!(
        owner.ending_ended(),
        "so the owner knows its wire was ended"
    );

    let (_durable, reading) = serving_reading(owner);
    assert_eq!(reading.ending, PrivateRetainedEnding::Ended);
}


#[test]
fn an_ending_this_owner_made_itself_is_reported_as_one() {
    // THE OWNER'S OWN FALLBACK. When the delivery writer finds the producers
    // gone it reports the end without shutting anything down, and this owner
    // ends the wire itself on the way out. That is a second site, and an
    // ending recorded at only one of them left the other reading as never
    // attempted.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8641));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);

    // Its producers really go: the row and everything holding a sender for it.
    let PreparedOrderedFixture {
        registration,
        runner,
        channels,
        ..
    } = f;
    drop(channels);
    drop(registration);
    drop(runner);

    let step = owner.serve_one(XByteOrder::LittleEndian, 7);
    assert!(
        matches!(
            step,
            X11OrderedServeStep::Ended {
                shutdown: true,
                ..
            }
        ),
        "this owner ended the wire itself: {step:?}"
    );
    assert!(owner.ending_ended(), "so it knows it did");
    let (_durable, reading) = serving_reading(owner);
    assert_eq!(reading.ending, PrivateRetainedEnding::Ended);

    // The peer sees the end, which is the fact being reported.
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "the wire really was ended"
    );
}


/// A serving record installed in a store, with its close staged as refused.
///
/// The refusal is staged -- this host gives no honest way to make a shutdown
/// fail -- and written exactly as begin_close's own failure branch writes it,
/// with the attempts already spent. Everything after it is real: the retry is
/// the production visit, and the shutdown it makes is a real one.
fn retained_refused_close(
    owner: X11OrderedServingOwner,
    attempts: u8,
) -> PrivateSettlementOwner {
    let mut owner = owner;
    owner.closing = Some(X11OrderedClosing {
        cause: X11OrderedCloseCause::ConnectionEnded,
        termination: X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied),
        attempts,
        answered: 0,
        already: 0,
        deferred: 0,
        drained: false,
    });
    let durable = PrivateSettlementOwner::with_capacities(2, 2);
    let slot = durable
        .reserve_ordered_continuation()
        .expect("a place, reserved before exposure");
    let mut source = Some(PrivateOrderedContinuation::Serving {
        owner: home_holding(owner),
        evidence: PrivateOrderedEvidence {
            fence: Some(PrivateHandoverFence::Established),
            ..PrivateOrderedEvidence::unstarted()
        },
    });
    assert_eq!(retain_into(slot, &mut source), PrivateContinuationCommit::Retained);
    durable
}

#[test]
fn a_refused_close_is_attempted_again_by_the_next_visit() {
    // NOBODY WAS RETRYING. The visit asked for a close only when none existed,
    // so a close whose shutdown refused was never attempted again: serving
    // excluded, the wire possibly still carrying bytes, and the recipient
    // waiting for an ending that was not coming.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8651));
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (owner, _output) = serving_owner_for(&mut f, socket);
    let durable = retained_refused_close(owner, 1);

    // One visit, and the attempt is made again -- for real, on a real socket,
    // which is why it succeeds this time.
    //
    // WHAT THIS CONTROL DOES NOT COVER: this owner is holding nothing, so what
    // it establishes about publication is that a place is kept, not that
    // custody was withheld. The exhausted control beside it holds a real
    // capsule and an unanswered completion, and is where that is said.
    durable.drive_ordered_continuations(1);
    let held = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("a serving record")
            };
            let closing = owner.closing().expect("its close");
            (closing.cause, closing.termination, closing.attempts)
        })
        .expect("the place holds it");
    assert_eq!(
        held.1,
        X11OrderedTermination::Established,
        "the retry established the termination the first attempt could not"
    );
    assert_eq!(
        held.0,
        X11OrderedCloseCause::ConnectionEnded,
        "THE ORIGINAL CAUSE, not the one the visit would have opened with"
    );
    assert_eq!(
        held.2, 2,
        "and the attempt count carried on rather than starting again"
    );
}

#[test]
fn a_close_with_no_attempts_left_says_so_and_is_not_retried() {
    // EXHAUSTION IS EXPLICIT. A close that refused with attempts left will be
    // tried again by the next visit; one with none left will not, and no
    // amount of driving changes it. Reporting both as refused leaves a reader
    // waiting for a retry that is never coming.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(8661));
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);

    // IT IS HOLDING SOMETHING. Without custody this control could only say a
    // place was kept, which is not the same as saying nothing was offered from
    // it. A real capsule goes onto its queue and into its refused slot, with a
    // completion nobody has answered.
    let sender = f
        .runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .get(&f.client)
        .expect("its row")
        .ordered
        .clone();
    let (held_capsule, _endpoint, _recovery, _receipts) = answerable_capsule(86610);
    let delivery = held_capsule.delivery();
    let cell = Arc::clone(&held_capsule.finalizer().expect("carried").completion);
    gated_send(&sender, held_capsule).expect("this owner's queue accepts it");
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(_)
    ));
    assert!(owner.refused().is_some(), "the writer is holding it");

    let durable = retained_refused_close(owner, X11_ORDERED_CLOSE_ATTEMPTS);

    for _ in 0..8 {
        durable.drive_ordered_continuations(4);
    }
    let held = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("a serving record")
            };
            let closing = owner.closing().expect("its close");
            (closing.termination, closing.attempts, owner.ending_ended())
        })
        .expect("the place holds it");
    assert_eq!(
        held.0,
        X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied),
        "the original refusal stands, with its original cause"
    );
    assert_eq!(
        held.1, X11_ORDERED_CLOSE_ATTEMPTS,
        "NO ATTEMPT WAS MADE and none was invented: the count did not move"
    );
    assert!(!held.2, "and nothing was ended");

    let reading = durable.retained_dispositions().expect("a readable store")[0]
        .1
        .clone()
        .expect("a readable record");
    assert!(
        reading.retries_exhausted,
        "so the reading says the retries are spent, not merely that it refused"
    );
    assert_eq!(
        reading.ending,
        PrivateRetainedEnding::Refused(std::io::ErrorKind::PermissionDenied)
    );
    assert!(!reading.settled, "and it is not finished");
    assert_eq!(
        durable.continuations_reserved(),
        Some(1),
        "NOTHING IS PUBLISHED BEFORE A CONFIRMED TERMINATION: the place stays"
    );

    // AND NEITHER IS WHAT IT HOLDS. THE EXACT CAPSULE, BY IDENTITY: a delivery
    // number is not an identity -- another real admission can carry the same
    // one -- so what is compared is the completion this capsule was built
    // with, by pointer.
    //
    // WHAT THIS CAPSULE IS: another endpoint's, refused on admission and held
    // unanswered. So what is established here is that a close which could not
    // establish an ending offers nothing derived from one, over FOREIGN
    // custody. It is not independently the publication guard for this
    // endpoint's own admission.
    assert_eq!(reading.retained, 1);
    let still = durable
        .with_ordered_continuation(0, |continuation| {
            let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
                panic!("a serving record")
            };
            owner.refused().map(|held| {
                (
                    held.delivery().delivery(),
                    Arc::clone(&held.delivery().finalizer().expect("carried").completion),
                )
            })
        })
        .expect("the place holds it")
        .expect("and the writer still holds the capsule");
    assert_eq!(still.0, delivery);
    assert!(
        Arc::ptr_eq(&still.1, &cell),
        "the exact one, by the completion it was built with"
    );
    assert!(
        cell.answer().is_none(),
        "and nobody answered for it on the strength of a close that failed"
    );
}


/// A home holding an owner a control built directly.
///
/// Production makes one only through `commit`, where the allocation happens
/// before any custody is taken. A control that builds an owner some other way
/// needs somewhere to put it, and this is that -- it is not the promotion
/// path, and a fixture using it is not exercising one.
fn home_holding(owner: X11OrderedServingOwner) -> PrivateServingHome {
    PrivateServingHome(Box::new(Some(owner)))
}

/// A prepared connection whose ordered output is bound, ready to promote.
///
/// Built on the ordered fixture because promotion asks the frontend for this
/// connection's endpoint, and that is answerable only for a connection the
/// instance has actually admitted and published.
// THE SERVICE KEEPER COMES BACK WITH THE REGISTRATION. It is the outer owner
// whose lifetime the private service guarantees around every connection, and
// a registration destroyed after it is gone can establish nothing about its
// worker and defers instead of tearing down. A caller that wants the
// never-started teardown to run at its drop holds the keeper across it.
fn bound_connection(
    client: XServerFrontendClientId,
) -> (
    XServerFrontendClientRouteRegistration,
    PrivatePreparedRunner,
    PrivateSettlementOwner,
    Arc<Mutex<X11ClientOutput>>,
    UnixStream,
    crate::PrivateServiceOwner,
) {
    let f = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        registration,
        runner,
        channels,
        durable,
        keeper,
        ..
    } = f;
    let (stream, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));
    (registration, runner, durable, output, peer, keeper)
}

/// What this registration's ordered payload is, by shape.
fn payload_shape(
    registration: &XServerFrontendClientRouteRegistration,
) -> Option<&'static str> {
    registration
        .ordered_home
        .borrow(|payload| match payload {
            PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Transport(_),
                ..
            } => "transport",
            PrivateOrderedContinuation::Setup {
                accepted: PrivateOrderedSetupCustody::Receiver(_),
                ..
            } => "receiver",
            PrivateOrderedContinuation::Serving { .. } => "serving",
        })
}

#[test]
fn a_receiver_alone_is_not_something_to_promote() {
    // A binding that refused keeps a receiver and no connection, so there is
    // nothing to make an owner from. This says only that; the control below is
    // where a refusal INSIDE preparation is answered for.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8671);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let other = XServerFrontendClientId(8672);
    let (other_registration, other_channels) = private
        .broker
        .registry
        .register_client_with_admission(other, Some(admitted(other)))
        .expect("a second place and row");
    let (stream, _peer) = UnixStream::pair().expect("a socket pair");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(other_channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));
    assert_eq!(payload_shape(&registration), Some("receiver"));

    assert_eq!(
        registration.promote_ordered_serving(&private),
        PrivateOrderedPromotion::Unbound
    );
    assert_eq!(
        payload_shape(&registration),
        Some("receiver"),
        "and nothing moved"
    );
    drop(channels);
    drop(registration);
    drop(other_registration);
}

#[test]
fn a_preparation_that_refuses_leaves_the_transport_and_its_queue_untouched() {
    // THE REFUSAL HAPPENS INSIDE PREPARATION, over a connection that really
    // has a bound transport with a real capsule on its queue. That is the only
    // arrangement in which "a refusal consumes nothing" says anything: a
    // refusal reached before preparation runs proves nothing about what
    // preparation does with what it borrows.
    //
    // It refuses because this connection's private lifecycle and connection
    // state were never attached, so the endpoint lookup has nothing to answer
    // with -- a real refusal from the real path, not an injected one.
    //
    // Its ROW is published, by the registration itself, and that is exactly
    // why this control can hold accepted custody: the queue is reachable, so
    // a capsule can be on it. Publication and attachment are different
    // boundaries and only the second is missing here.
    let durable = PrivateSettlementOwner::default();
    let service_keeper = service_owner(&durable, 2);
    let private = private_over(&service_keeper, 2);
    let client = XServerFrontendClientId(8741);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place and a row");
    let (stream, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = X11ClientOutput::shared(stream, 0);
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    registration
        .bind_ordered_output(channels.ordered, &output, &wire, &pending)
        .unwrap_or_else(|_| panic!("a fresh registration holds no custody"));
    assert_eq!(payload_shape(&registration), Some("transport"));

    // A real capsule on its queue. Foreign fixture custody: it is here to be
    // work that a refusal could destroy, not an admission to this endpoint.
    let sender = capture_gated_sender(&private, client);
    let (capsule, _endpoint, _recovery, _receipts) = answerable_capsule(87410);
    let cell = Arc::clone(&capsule.finalizer().expect("carried").completion);
    let finalizer = Arc::downgrade(capsule.finalizer().expect("carried"));
    let frames = order_pass_frames(&capsule);
    gated_send(&sender, capsule).expect("an open endpoint");

    assert_eq!(
        registration.promote_ordered_serving(&private),
        PrivateOrderedPromotion::Refused(X11OrderedServingRefusal::Unadmitted(
            PrivateAdmissionRefusal::NotAdmitted
        )),
        "refused inside preparation, by the endpoint lookup"
    );

    // NOTHING WAS CONSUMED. The transport is still here, over the same output,
    // and its queue still holds the exact capsule -- by the finalizer it was
    // built with, which is still alive.
    assert_eq!(payload_shape(&registration), Some("transport"));
    assert!(
        finalizer.upgrade().is_some(),
        "the capsule was not dropped on the way out"
    );
    let survived = registration.ordered_home.borrow(|payload| {
        let PrivateOrderedContinuation::Setup {
            accepted: PrivateOrderedSetupCustody::Transport(transport),
            ..
        } = payload
        else {
            panic!("its transport is still here")
        };
        assert!(
            Arc::ptr_eq(&transport.output, &output),
            "over the same output it was bound to"
        );
        assert!(
            transport.ordered.minted_by(&registration),
            "and the same receiver, still this registration's"
        );
        transport.ordered.receiver.try_recv().ok()
    })
    .expect("its own home")
    .expect("its queue still holds the capsule");
    assert!(Arc::ptr_eq(
        &cell,
        &survived.finalizer().expect("carried").completion
    ));
    assert_eq!(order_pass_frames(&survived), frames);
    assert!(cell.answer().is_none());

    // AND THE ENDING CAPABILITY IS USED, not inferred from dropping things.
    // Letting go of every descriptor would end the socket whatever handle the
    // transport had kept -- including one for some other connection -- so the
    // retained handle is called while the real output is still open, and the
    // peer this connection belongs to is the one that has to see the end.
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).map_err(|error| error.kind()),
        Err(std::io::ErrorKind::WouldBlock),
        "still connected"
    );
    {
        registration
            .ordered_home
            .borrow(|payload| {
                let PrivateOrderedContinuation::Setup {
                    accepted: PrivateOrderedSetupCustody::Transport(transport),
                    ..
                } = payload
                else {
                    panic!("its transport is still here")
                };
                transport
                    .shutdown
                    .shutdown(Shutdown::Both)
                    .expect("the retained handle ends this connection");
            })
            .expect("its own home");
    }
    assert!(
        Arc::strong_count(&output) >= 2,
        "the real output is still open, so nothing ended by being dropped"
    );
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "and THIS connection's peer sees the end, so the handle kept was its own"
    );
    drop(sender);
    drop(registration);
    drop(private);
    drop(durable);
    drop(output);
}
