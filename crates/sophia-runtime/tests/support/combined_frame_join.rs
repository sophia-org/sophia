//! G3-1 joined admission: a combined copy spends one credit, carries its
//! row's description, and its result settles only through the exact row.
use super::*;
use std::sync::mpsc;

type Worker = IsbValidationWorker<u32>;
type Envelope = ForwardedValidation<u32>;
const WAIT: Duration = Duration::from_secs(10);

struct Joined {
    f: Fixture,
    worker: Worker,
}

fn joined() -> Joined {
    Joined {
        f: Fixture::new(),
        // Two credits, so a second credit taken per copy shows in `pending`.
        worker: Worker::for_combined_plans(2, 32000).unwrap(),
    }
}

fn inbox() -> (CombinedInbox<u32>, mpsc::Receiver<Envelope>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        CombinedInbox::new(sender, sophia_wake::WakeSlot::default()),
        receiver,
    )
}

impl Joined {
    fn admit(&mut self, generation: u64) -> (u64, CombinedReservation) {
        let mut input = Some(frame(generation, true));
        let (admission, reservation) = self
            .f
            .jobs
            .admit_frame_commit_combined(
                &mut self.f.registry,
                &mut self.worker,
                Some(&self.f.ring),
                &mut input,
                4,
                1,
                deadline(),
            )
            .unwrap();
        let job = job(admission);
        let reservation = reservation.expect("an accepted copy keeps its combined credit");
        assert_eq!(reservation.job(), job);
        (job, reservation)
    }

    fn description(&self, job: u64) -> ContentResourceBegin {
        self.f
            .jobs
            .combined_admitted(job)
            .expect("an admitted unsent row")
            .description
            .clone()
    }

    /// Submit plan 7 and wait for the validator to move it to the inbox.
    fn forward(
        &mut self,
        reservation: &CombinedReservation,
    ) -> (Envelope, mpsc::Receiver<u32>, mpsc::Receiver<Envelope>) {
        publish(&self.f.file, request(3));
        let (inbox, arrived) = inbox();
        let source = self.f.ring.offer(request(3).slot, 16000).unwrap();
        let returned = self
            .f
            .jobs
            .submit_combined(&mut self.worker, reservation, source, 7, inbox)
            .unwrap_or_else(|_| panic!("exact row accepts its combined send"));
        let envelope = arrived.recv_timeout(WAIT).expect("validator forwards");
        (envelope, returned, arrived)
    }

    /// Staging and reserved bytes the copy's transfer still owes.
    fn charged(&self) -> u64 {
        let usage = self.f.registry.resources(grant()).unwrap().usage();
        usage.staging + usage.reserved_resident
    }
}

#[test]
fn combined_admission_spends_one_credit_and_binds_the_admitted_job() {
    let mut j = joined();
    let (job, _reservation) = j.admit(1);
    assert_eq!(j.worker.pending(), 1, "one credit, not one per path");
    let admitted = j.f.jobs.combined_admitted(job).unwrap();
    assert_eq!(admitted.job, job);
    assert_eq!(admitted.binding.generation, 1);
    assert_eq!(admitted.description.resource, request(3).resource);
    assert_eq!(admitted.description.total_bytes, 16000);
    assert!(j.f.jobs.combined_admitted(job + 1).is_none());
}

#[test]
fn forwarded_result_reaches_reconcile_only_through_its_exact_row() {
    let mut j = joined();
    let (job, reservation) = j.admit(1);
    let description = j.description(job);
    let held = j.charged();
    assert!(held > 0, "the copy holds its staging charge");
    let (envelope, returned, _arrived) = j.forward(&reservation);
    assert!(j.f.jobs.combined_admitted(job).is_none(), "the row is sent");
    assert_eq!(envelope.job(), job);
    assert!(!envelope.stopped());
    assert_eq!(envelope.description(), Some(&description));
    let (pixels, plan) = j
        .worker
        .settle_combined(reservation, envelope)
        .unwrap_or_else(|_| panic!("own token settles"));
    assert_eq!(plan, 7);
    assert_eq!(j.worker.pending(), 0);
    assert!(
        returned.try_recv().is_err(),
        "a forwarded plan never returns"
    );
    let pixels = match j.f.jobs.receive_forwarded(job + 1, Ok(pixels)) {
        Err(Ok(pixels)) => pixels,
        _ => panic!("a foreign job returns the result intact"),
    };
    assert_eq!(j.charged(), held, "a refused receive settles nothing");
    assert!(j.f.jobs.receive_forwarded(job, Ok(pixels)).is_ok());
    assert!(matches!(
        j.f.jobs
            .receive_forwarded(job, Err(IsbValidationFailure::Malformed)),
        Err(Err(IsbValidationFailure::Malformed))
    ));
    let mut settlement = j.f.jobs.reconcile(&mut j.f.registry, 2).unwrap();
    assert_eq!(settlement.outcome, Ok(()));
    settlement.finish_frame_copy(&mut j.f.registry, 2).unwrap();
    assert!(j.f.jobs.reconcile(&mut j.f.registry, 2).is_none());
    assert_eq!(j.f.ring.pending_release(0), Some(request(3).slot));
}

#[test]
fn a_stop_after_forwarding_is_visible_and_keeps_charges_until_reconcile() {
    let mut j = joined();
    let (job, reservation) = j.admit(1);
    let held = j.charged();
    let (envelope, _returned, _arrived) = j.forward(&reservation);
    assert!(!envelope.stopped());
    j.f.jobs
        .withdraw(
            &mut j.f.registry,
            &mut j.worker,
            job,
            IsbImageWithdrawal::Cancelled,
            2,
        )
        .unwrap();
    assert!(envelope.stopped(), "the envelope reads the job's own flag");
    assert_eq!(j.charged(), held, "stopping settles no charge");
    let (pixels, _) = j
        .worker
        .settle_combined(reservation, envelope)
        .unwrap_or_else(|_| panic!("own token settles"));
    assert!(
        j.f.jobs.receive_forwarded(job, Ok(pixels)).is_ok(),
        "a stopped row still takes its late result"
    );
    assert_eq!(j.charged(), held);
    let settlement = j.f.jobs.reconcile(&mut j.f.registry, 3).unwrap();
    assert!(settlement.stopped.is_some());
    assert!(j.charged() < held, "complete_isb settled the charge");
    assert!(j.f.jobs.reconcile(&mut j.f.registry, 3).is_none());
}

#[test]
fn a_mismatch_reported_as_malformed_settles_once_through_the_row() {
    let mut j = joined();
    let (job, reservation) = j.admit(1);
    let held = j.charged();
    let (envelope, _returned, _arrived) = j.forward(&reservation);
    let (pixels, _) = j
        .worker
        .settle_combined(reservation, envelope)
        .unwrap_or_else(|_| panic!("own token settles"));
    drop(pixels);
    assert_eq!(j.charged(), held, "dropping the payload settles nothing");
    assert!(
        j.f.jobs
            .receive_forwarded(job, Err(IsbValidationFailure::Malformed))
            .is_ok()
    );
    let settlement = j.f.jobs.reconcile(&mut j.f.registry, 2).unwrap();
    assert_eq!(
        settlement.validation_failure,
        Some(IsbValidationFailure::Malformed)
    );
    assert!(settlement.outcome.is_err());
    assert!(j.charged() < held);
    assert!(j.f.jobs.reconcile(&mut j.f.registry, 2).is_none());
}

#[test]
fn a_refused_inbox_returns_the_plan_once_and_the_result_takes_the_ordinary_path() {
    let mut j = joined();
    let (job, reservation) = j.admit(1);
    publish(&j.f.file, request(3));
    let (inbox, arrived) = inbox();
    drop(arrived);
    let source = j.f.ring.offer(request(3).slot, 16000).unwrap();
    let returned =
        j.f.jobs
            .submit_combined(&mut j.worker, &reservation, source, 7, inbox)
            .unwrap_or_else(|_| panic!("exact row accepts its combined send"));
    assert_eq!(returned.recv_timeout(WAIT), Ok(7));
    let completion = completed_any(&mut j.worker);
    assert_eq!(completion.job, job);
    assert!(j.f.jobs.receive(completion).is_ok());
    assert!(returned.try_recv().is_err(), "the plan returns once");
    let settlement = j.f.jobs.reconcile(&mut j.f.registry, 2).unwrap();
    assert_eq!(settlement.outcome, Ok(()));
}

#[test]
fn a_mismatched_send_returns_source_and_plan_and_leaves_the_row_unsent() {
    let mut j = joined();
    let (job, reservation) = j.admit(1);
    publish(&j.f.file, request(4));
    let (inbox, _arrived) = inbox();
    let wrong = j.f.ring.offer(request(4).slot, 16000).unwrap();
    let Err((failure, _source, plan)) =
        j.f.jobs
            .submit_combined(&mut j.worker, &reservation, wrong, 7, inbox)
    else {
        panic!("a foreign slot cannot supply the row");
    };
    assert_eq!(failure, IsbSubmitFailure::Invalid);
    assert_eq!(plan, 7);
    assert!(j.f.jobs.combined_admitted(job).is_some());
    assert_eq!(j.worker.pending(), 1, "the credit stays with the row");
}

fn state(j: &Joined, binding: &FrameCopyBinding, job: u64) -> FrameCandidateState {
    j.f.registry
        .active_candidates(grant())
        .unwrap()
        .frame_candidate_state(binding, job)
}

#[test]
fn candidate_state_reads_live_tables_and_ends_only_on_proof() {
    let mut j = joined();
    let (job, _reservation) = j.admit(1);
    let binding = j.f.jobs.combined_admitted(job).unwrap().binding;
    assert_eq!(state(&j, &binding, job), FrameCandidateState::Assembling);
    assert_eq!(state(&j, &binding, job + 1), FrameCandidateState::Unknown);
    let earlier = FrameCopyBinding {
        generation: 0,
        ..binding
    };
    assert_eq!(state(&j, &earlier, job), FrameCandidateState::Ended);
    let later = FrameCopyBinding {
        generation: 2,
        ..binding
    };
    assert_eq!(state(&j, &later, job), FrameCandidateState::Unknown);
    let replaced = FrameCopyBinding {
        key: FrameStreamKey {
            stream_id: binding.key.stream_id + 1,
            ..binding.key
        },
        ..binding
    };
    assert_eq!(
        state(&j, &replaced, job),
        FrameCandidateState::Ended,
        "a live assembly under another stream key is not this binding"
    );
    j.f.registry
        .active_candidates_mut(grant())
        .unwrap()
        .revoke();
    assert_eq!(state(&j, &binding, job), FrameCandidateState::Ended);
}

#[test]
fn candidate_state_ends_on_a_closing_stream_that_keeps_its_key() {
    let mut j = joined();
    let (job, _reservation) = j.admit(1);
    let binding = j.f.jobs.combined_admitted(job).unwrap().binding;
    let candidates = j.f.registry.active_candidates_mut(grant()).unwrap();
    candidates
        .close_frame_stream(tx(40), binding.key, 1)
        .unwrap();
    candidates.take_frame_stream_event().unwrap();
    assert_eq!(
        state(&j, &binding, job),
        FrameCandidateState::Assembling,
        "the copy still owns its live assembly through the close"
    );
    assert_eq!(
        state(&j, &binding, job + 1),
        FrameCandidateState::Ended,
        "no live match on a closing stream is proof of its end"
    );
}

#[test]
fn a_begin_isb_refusal_after_the_combined_credit_releases_it_for_the_next_admission() {
    let mut j = joined();
    // Replacement generations of a lower id fill the open transfers, so the
    // copy's own resource stays fresh and only begin_isb refuses it.
    let resources = j.f.registry.resources_mut(grant()).unwrap();
    for generation in 2..=5 {
        resources
            .begin_isb(
                tx(200 + generation),
                ContentResourceBegin {
                    grant: grant(),
                    resource: ContentResourceId { id: 1, generation },
                    width_px: 1,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 4,
                },
                1,
            )
            .unwrap();
    }
    let charged = j.charged();
    let mut input = Some(frame(1, true));
    let (admission, reservation) =
        j.f.jobs
            .admit_frame_commit_combined(
                &mut j.f.registry,
                &mut j.worker,
                Some(&j.f.ring),
                &mut input,
                4,
                1,
                deadline(),
            )
            .unwrap();
    assert_eq!(
        admission,
        ContentFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    assert!(reservation.is_none());
    assert!(input.is_none());
    assert_eq!(j.worker.pending(), 0, "the combined credit was released");
    assert_eq!(j.f.jobs.pending(), 0);
    assert_eq!(j.charged(), charged, "the refused copy holds no charge");
    j.f.refusal(1, ContentReason::Budget);
    j.f.registry
        .resources_mut(grant())
        .unwrap()
        .cancel(
            tx(210),
            &ContentResourceCancel {
                grant: grant(),
                resource: ContentResourceId {
                    id: 1,
                    generation: 5,
                },
            },
        )
        .unwrap();
    let (_job, _reservation) = j.admit(2);
    assert_eq!(j.worker.pending(), 1, "the next admission takes the credit");
}

fn completed_any(worker: &mut Worker) -> IsbValidationCompletion {
    let bound = Instant::now() + WAIT;
    loop {
        if let Some(done) = worker.take_completed().unwrap() {
            return done;
        }
        assert!(Instant::now() < bound, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}
