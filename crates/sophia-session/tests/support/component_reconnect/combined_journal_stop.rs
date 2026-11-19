//! G3-5d d-2b3d-2, beside `combined_journal.rs` for the file-size limit:
//! FJ-stop, the next admission's Stop at the reservation refusal, on the
//! parent's lane, helpers and fill (REV8 disposition: the final filler's
//! ResourceReleased parked at 255; source-02 R2: the Stop witness is the
//! identified frame commit left queued by `Step::Stop`, read with the
//! admission's own preconditions from its embedded offer).
use super::*;

/// FJ-stop: 63 fillers to 252 and the final filler whose SlotReleased
/// publishes at 255 while its ResourceReleased parks; the probe's
/// admission refused at its first reservation hold: `Step::Stop`
/// (owned_copy/frame.rs), the offer waits, nothing is appended or queued,
/// holds stay H. The other blockers are shown absent. The ACK, the one
/// parked record's drain, then the admission.
#[test]
fn fj_stop_the_next_admission_stops_at_the_reservation_refusal_until_an_ack() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let mut fill = baseline(&mut lane, &mut a, "FJ-stop phase 0");
    let r = fillers(
        &mut lane,
        &wake,
        &mut a,
        &mut fill,
        1,
        STOP_FILLERS,
        "FJ-stop",
    );
    assert_eq!(r, 252, "FJ-stop: the fillers reached 252");
    // The final filler: T1 as every filler; T2 publishes SlotReleased(64)
    // at 255 and parks the release.
    let f = filler_start(
        &mut lane,
        &wake,
        &mut a,
        &mut fill,
        STOP_FINAL,
        "FJ-stop 64",
    );
    assert_eq!(f.r0, 252);
    let d = filler_t1(&mut lane, &mut a, &mut fill, &f, "FJ-stop 64");
    assert_eq!(d.outcome, d.status + 1, "status 253, outcome 254");
    lane.slot_freed("FJ-stop 64");
    budget(&fill, "FJ-stop T2(64)");
    a.peer.poll_io().unwrap();
    let (steps, observed) = lane.observed_turn(f.late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, d.c1.unchanged(), "FJ-stop T2(64): nothing more");
    let s2 = lane.snapshot(&a, "FJ-stop O2(64)");
    let w2 = window(&d.s1, &s2, "FJ-stop T2(64)");
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)"],
        "FJ-stop T2(64): the slot's release alone"
    );
    let event = slot_released_of(&w2[0]).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (f.identity, f.resource, f.credit.offer.request.slot, 0)
    );
    let slot_released_64 = w2[0].sequence;
    let j2 = s2.journal();
    assert_eq!((j2.records, j2.held), (255, H), "FJ-stop O2(64): {j2:?}");
    assert_all_visible(j2, "FJ-stop O2(64): published with lean 0");
    assert!(s2.releasing.is_empty());
    assert!(s2.facts.owned_live_credits.is_empty(), "both holds spent");
    assert!(
        s2.facts.output_blocked,
        "FJ-stop O2(64): the release was refused at 255"
    );
    assert_eq!(outbox_kinds(&s2.facts.outbox), ["ResourceReleased"]);
    assert_parked(&s2, 0, &f.release, "FJ-stop O2(64)");
    assert_eq!(s2.facts.owned_publication_sequence, Some(slot_released_64));
    assert_eq!(s2.facts.lean_frames_in_flight, Some(0));
    s2.assert_stores_empty("FJ-stop O2(64)");
    let charged = usage(&lane).backing;
    assert!(
        charged > fill.backing_0,
        "FJ-stop O2(64): the parked release keeps its charge"
    );
    assert!(lane.states().is_empty());
    assert_eq!(lane.leased(), 0);
    assert_eq!(counters(&lane), d.c1);
    eprintln!("FJ-stop O2(64): journal {j2:?}; usage {:?}", usage(&lane));
    // The probe: turns until the owner holds frame 65's commit queued,
    // unadmitted (`Step::Stop` leaves it in the inbound queue; the
    // ordinary offer queue stays empty on the lean path), then one more
    // turn in which it stays queued.
    let resource = resource_n(STOP_PROBE);
    let probe_transaction = TransactionId::from_raw(40 + STOP_PROBE);
    boundary(&lane, &wake, &a, "FJ-stop probe");
    budget(&fill, "FJ-stop probe submission");
    client_facts(&a, &fill, "FJ-stop probe submission");
    let offer_65 = lane.submit_at(&mut a, STOP_PROBE, PIXEL, &[0]);
    offered(
        &mut fill,
        &a,
        STOP_PROBE,
        offer_65,
        RingOfferState::Settled,
        "FJ-stop probe",
    );
    assert!(!a.stream.has_credit());
    let deadline = Instant::now() + WAIT;
    let mut turns = 0;
    let probe_turn = |lane: &mut Lane, a: &mut Attempt, turns: usize| {
        budget(&fill, &format!("FJ-stop admission pass {turns}"));
        a.peer.poll_io().unwrap();
        let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
        assert_eq!(steps, TURN);
        assert_eq!(accepted, [0]);
        observe_peer(a);
        assert!(
            lane.states().is_empty(),
            "FJ-stop: admitted: {:?}",
            lane.states()
        );
        let queued = lane.h.owner.processes.queued_frame_facts(0);
        if let Some(q) = &queued {
            assert_eq!(
                (
                    q.transaction,
                    q.candidate_generation,
                    q.offer.request.resource
                ),
                (probe_transaction, STOP_PROBE, resource),
                "FJ-stop: frame 65's commit is the queued frame: {q:?}"
            );
        }
        queued
    };
    let queued = loop {
        turns += 1;
        if let Some(q) = probe_turn(&mut lane, &mut a, turns) {
            break q;
        }
        assert!(
            Instant::now() < deadline,
            "FJ-stop: frame 65 never reached the owner: {:?}",
            lane.states()
        );
        std::thread::yield_now();
    };
    let s_first = lane.snapshot(&a, "FJ-stop first stop");
    let j_first = s_first.journal();
    admission_facts(&lane, &a, &s2, &s_first, "FJ-stop first stop");
    assert_eq!(committed(j_first), committed(j2), "nothing appended");
    turns += 1;
    let still = probe_turn(&mut lane, &mut a, turns)
        .expect("FJ-stop: frame 65 stays queued through another turn");
    assert_eq!(still, queued, "FJ-stop: the same facts in both turns");
    let s_stop = lane.snapshot(&a, "FJ-stop stop");
    let j_stop = s_stop.journal();
    assert_eq!(
        s_stop.facts.owned_pending_offer, None,
        "the lean commit carries its offer; the ordinary offer queue is empty"
    );
    assert!(
        lane.states().is_empty(),
        "no admission: {:?}",
        lane.states()
    );
    assert!(
        s_stop.facts.owned_live_credits.is_empty(),
        "no hold was taken and kept: {:?}",
        s_stop.facts.owned_live_credits
    );
    assert_eq!(j_stop.held, H, "{j_stop:?}");
    assert_eq!(committed(j_stop), committed(j2), "nothing appended");
    assert_eq!(j_stop.records, 255);
    assert!(
        s_stop.facts.output_blocked,
        "the parked release still blocks"
    );
    assert_eq!(
        outbox_kinds(&s_stop.facts.outbox),
        ["ResourceReleased"],
        "no refusal status was appended or parked"
    );
    assert_parked(&s_stop, 0, &f.release, "FJ-stop stop");
    s_stop.assert_stores_empty("FJ-stop stop: no refusal status queued");
    assert!(s_stop.releasing.is_empty());
    assert_eq!(s_stop.facts.lean_frames_in_flight, Some(0));
    assert_eq!(lane.leased(), 0);
    assert_eq!(
        usage(&lane).backing,
        charged,
        "the parked charge, nothing else"
    );
    // The other admission preconditions, read from the queued frame's own
    // offer as `admit_peeked_commit` reads them: the grants, the worker's
    // capacity and the backing budget with RR(64) still charged, the copy
    // owner's pending count (< 4) and the control room.
    assert!(queued.grant_current, "{queued:?}");
    assert_eq!(
        queued.available,
        Ok(true),
        "worker capacity and the backing budget admit the offer: {queued:?}"
    );
    assert_eq!(queued.worker_pending, 0, "{queued:?}");
    assert_eq!(queued.room, 4, "control room for the admission: {queued:?}");
    assert_eq!(
        lane.h.owner.processes.image_pending(0),
        0,
        "no copy job pending"
    );
    eprintln!(
        "FJ-stop: R 255 held {}, {turns} turn(s); queued frame {queued:?}; by the source's order the status hold (255 not < 256 - 1) refused: an inference; journal {j_stop:?}",
        j_stop.held
    );
    // The unblock: ACK, PP, IT (the one parked release drains), then
    // the admission.
    budget(&fill, "FJ-stop unblock");
    let acked = acknowledge(&mut a, &fill, &d, slot_released_64, "FJ-stop");
    let after_drain = match acked {
        Acked::SlotReleased => 1,
        Acked::Outcome => 2,
    };
    budget(&fill, "FJ-stop IT");
    lane.input_turn(&a);
    let s_u = lane.snapshot(&a, "FJ-stop after IT");
    let j_u = s_u.journal();
    assert!(j_u.floor > j_stop.floor, "the ACK released retention");
    assert!(!s_u.facts.output_blocked);
    let w_u = window(&s_stop, &s_u, "FJ-stop IT");
    assert_eq!(
        kinds(&w_u),
        ["ResourceReleased"],
        "the one parked release drained"
    );
    assert_eq!(w_u[0].identity, release_identity(&f.release));
    assert_eq!(j_u.records, after_drain, "{acked:?}: {j_u:?}");
    assert_eq!(j_u.held, H);
    assert!(s_u.facts.outbox.is_empty());
    assert_eq!(
        usage(&lane).backing,
        fill.backing_0,
        "the charge released with the commit"
    );
    let job = admit(&mut lane, &mut a, &fill, "FJ-stop after the ACK");
    let s_adm = lane.snapshot(&a, "FJ-stop admitted");
    admission_facts(&lane, &a, &s_u, &s_adm, "FJ-stop admitted");
    assert_eq!(s_adm.journal().held, H + 2, "{:?}", s_adm.journal());
    assert_eq!(
        lane.h.owner.processes.queued_frame_facts(0),
        None,
        "the admission consumed frame 65's commit"
    );
    assert!(credit_of(&s_adm, resource, "FJ-stop admitted").status_reserved);
    assert!(wake.wait(Some(Instant::now() + WAIT)).unwrap());
    assert_eq!(lane.states(), [(job, "in flight")]);
    lane.trace_end(&[(job, "in flight")], &[]);
    forget_observed(&mut fill, &mut a, "FJ-stop end");
    eprintln!("FJ-stop: admitted after the ACK: {:?}", s_adm.journal());
}
