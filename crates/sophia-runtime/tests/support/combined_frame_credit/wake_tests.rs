use super::tests::{WAIT, plan, source};
use super::*;
use rustix::event::{PollFd, PollFlags};

struct HeldJob {
    token: CombinedReservation,
    entered: mpsc::Receiver<()>,
    resume: mpsc::SyncSender<()>,
    returned: mpsc::Receiver<ForwardedValidation<ProbePlan>>,
    owner_plan: mpsc::Receiver<ProbePlan>,
}

fn reserve(worker: &mut Worker) -> CombinedReservation {
    worker
        .reserve_combined(8, Instant::now() + CHECKPOINT_WAIT * 2)
        .unwrap()
}

fn hold(worker: &mut Worker) -> HeldJob {
    let token = reserve(worker);
    let (entered, arrival) = mpsc::sync_channel(1);
    let (resume, release) = mpsc::sync_channel(1);
    let (sender, returned) = mpsc::sync_channel(1);
    let owner_plan = worker
        .submit_combined_checkpoint(
            &token,
            source(&[0; 8]),
            plan(token.job),
            sender,
            Some(Checkpoint {
                entered,
                resume: release,
            }),
        )
        .ok()
        .expect("checkpointed submission");
    HeldJob {
        token,
        entered: arrival,
        resume,
        returned,
        owner_plan,
    }
}

impl HeldJob {
    fn entered(&self) {
        self.entered
            .recv_timeout(WAIT)
            .expect("validator reached the checkpoint");
    }

    fn finish(self, worker: &mut Worker) {
        self.resume.send(()).expect("resume the held job");
        let returned = self
            .returned
            .recv_timeout(WAIT)
            .expect("held job forwarded after release");
        assert_eq!(returned.job, self.token.job);
        assert!(self.owner_plan.try_recv().is_err());
        assert!(worker.settle_combined(self.token, returned).is_ok());
    }
}

fn readable(worker: &Worker, deadline: Instant) -> bool {
    let mut fds = [PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN)];
    sophia_wake::wait(&mut fds, Some(deadline)).unwrap();
    fds[0].revents().contains(PollFlags::IN)
}

fn baseline(worker: &mut Worker) {
    assert!(worker.take_completed().unwrap().is_none());
    assert!(!readable(worker, Instant::now()));
}

#[test]
fn forwarded_success_is_quiet_after_the_next_job_enters_and_control_still_wakes() {
    let mut worker = Worker::for_combined_plans(2, 8).unwrap();
    baseline(&mut worker);
    let first = reserve(&mut worker);
    let (sender, receiver) = mpsc::sync_channel(1);
    let owner_plan = worker
        .submit_combined(&first, source(&[0; 8]), plan(first.job), inbox(sender))
        .ok()
        .expect("first job submitted");
    let returned = receiver.recv_timeout(WAIT).unwrap();
    // B is immediately after A; entry orders all of A, without an ordinary wake.
    let next = hold(&mut worker);
    next.entered();
    assert!(first.was_forwarded());
    assert!(
        !readable(&worker, Instant::now()),
        "forwarded iteration must leave validation readiness quiet"
    );
    assert_eq!(worker.pending(), 2, "quiet is not cleared custody");
    assert!(!worker.release_reserved(first.job));
    assert!(owner_plan.try_recv().is_err());

    let control = Wake::new().unwrap();
    control.notifier().notify();
    {
        let mut fds = [
            PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN),
            PollFd::new(&control, PollFlags::IN),
        ];
        assert!(sophia_wake::wait(&mut fds, Some(Instant::now() + WAIT)).unwrap());
        assert!(!fds[0].revents().contains(PollFlags::IN));
        assert!(
            fds[1].revents().contains(PollFlags::IN),
            "control remains independently readable"
        );
    }
    control.clear().unwrap();
    assert!(worker.take_completed().unwrap().is_none());
    assert!(worker.settle_combined(first, returned).is_ok());
    next.finish(&mut worker);
    assert_eq!(worker.pending(), 0);
}

#[test]
fn validation_errors_ring_before_the_ordered_completion_drain() {
    for (cancelled, bytes, failure) in [
        (true, [0; 8], IsbValidationFailure::Cancelled),
        (
            false,
            [1, 0, 0, 0, 0, 0, 0, 0],
            IsbValidationFailure::Malformed,
        ),
    ] {
        let mut worker = Worker::for_combined_plans(2, 8).unwrap();
        baseline(&mut worker);
        let token = reserve(&mut worker);
        if cancelled {
            assert!(worker.cancel(token.job));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let owner_plan = worker
            .submit_combined(&token, source(&bytes), plan(token.job), inbox(sender))
            .ok()
            .expect("failing job submitted");
        let next = hold(&mut worker);
        next.entered();
        assert!(
            readable(&worker, Instant::now()),
            "ordinary completion notification must be readable after the checkpoint"
        );
        let completion = worker
            .take_completed()
            .unwrap()
            .expect("real failure completion");
        assert_eq!(completion.job, token.job);
        assert_eq!(completion.result.err(), Some(failure));
        assert_eq!(
            owner_plan.try_recv().expect("plan precedes completion").id,
            token.job
        );
        assert!(receiver.try_recv().is_err());
        assert!(!token.was_forwarded());
        assert!(worker.take_completed().unwrap().is_none());
        assert_eq!(worker.pending(), 1);
        next.finish(&mut worker);
        assert_eq!(worker.pending(), 0);
    }
}

#[test]
fn full_and_disconnected_forwarding_keep_the_ordinary_notification() {
    for disconnected in [false, true] {
        let mut worker = Worker::for_combined_plans(4, 8).unwrap();
        baseline(&mut worker);
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut receiver = Some(receiver);
        let first = if disconnected {
            drop(receiver.take());
            None
        } else {
            let first = reserve(&mut worker);
            let _owner_plan = worker
                .submit_combined(
                    &first,
                    source(&[0; 8]),
                    plan(first.job),
                    inbox(sender.clone()),
                )
                .ok()
                .expect("fill the destination before the following job");
            Some(first)
        };
        let failed_move = reserve(&mut worker);
        let owner_plan = worker
            .submit_combined(
                &failed_move,
                source(&[0; 8]),
                plan(failed_move.job),
                inbox(sender),
            )
            .ok()
            .expect("failed move uses ordinary completion");
        let next = hold(&mut worker);
        next.entered();
        assert!(
            readable(&worker, Instant::now()),
            "failed move must keep the ordinary wake"
        );
        let completion = worker
            .take_completed()
            .unwrap()
            .expect("real successful validation returned");
        assert_eq!(completion.job, failed_move.job);
        assert_eq!(completion.result.unwrap().bytes(), &[0; 8]);
        assert_eq!(owner_plan.try_recv().unwrap().id, failed_move.job);
        assert!(!failed_move.was_forwarded());
        assert!(worker.take_completed().unwrap().is_none());
        next.finish(&mut worker);
        if let Some(first) = first {
            let returned = receiver.unwrap().recv_timeout(WAIT).unwrap();
            assert!(worker.settle_combined(first, returned).is_ok());
        }
        assert_eq!(worker.pending(), 0);
    }
}

#[test]
fn completion_published_after_clear_remains_observable() {
    let mut worker = Worker::for_combined_plans(1, 8).unwrap();
    baseline(&mut worker);
    let held = hold(&mut worker);
    held.entered();
    assert!(worker.cancel(held.token.job));
    assert!(
        worker.take_completed().unwrap().is_none(),
        "clear and queue check precede publication"
    );
    held.resume.send(()).unwrap();
    assert!(
        readable(&worker, Instant::now() + WAIT),
        "completion after clear must wake"
    );
    let completion = worker
        .take_completed()
        .unwrap()
        .expect("cancelled job returned");
    assert_eq!(completion.job, held.token.job);
    assert_eq!(
        completion.result.err(),
        Some(IsbValidationFailure::Cancelled)
    );
    assert_eq!(held.owner_plan.try_recv().unwrap().id, held.token.job);
    assert!(!held.token.was_forwarded());
    assert!(held.returned.try_recv().is_err());
    assert!(worker.take_completed().unwrap().is_none());
    assert_eq!(worker.pending(), 0);
}

#[test]
fn normal_validator_exit_rings_while_the_owner_keeps_its_wake_alive() {
    let mut worker = Worker::for_combined_plans(1, 8).unwrap();
    baseline(&mut worker);
    let (dummy, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    drop(std::mem::replace(&mut worker.sender, dummy));
    assert!(
        readable(&worker, Instant::now() + WAIT),
        "normal validator exit must ring before drain"
    );
    assert_eq!(
        worker.take_completed().err().unwrap().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(worker.pending(), 0);
}

#[test]
fn validator_unwind_rings_but_does_not_clear_an_unreturned_job() {
    let mut worker = Worker::for_combined_plans(1, 8).unwrap();
    baseline(&mut worker);
    let held = hold(&mut worker);
    held.entered();
    assert!(!readable(&worker, Instant::now()));
    // Expected validator-thread panic: the test asserts the real exit guard's effects.
    drop(held.resume);
    assert!(
        readable(&worker, Instant::now() + WAIT),
        "validator unwind must ring before drain"
    );
    assert_eq!(
        worker.take_completed().err().unwrap().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        worker.pending(),
        1,
        "exit notification is not a settlement proof"
    );
    assert!(!worker.release_reserved(held.token.job));
    assert!(worker.cancel(held.token.job));
    assert!(!held.token.was_forwarded());
    assert!(matches!(
        held.returned.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(matches!(
        held.owner_plan.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn a_forwarded_frame_rings_the_renderer_inbox_once_after_the_move() {
    let mut worker = Worker::for_combined_plans(1, 8).unwrap();
    let token = reserve(&mut worker);
    let renderer = sophia_wake::Wake::new().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let _owner_plan = worker
        .submit_combined(
            &token,
            source(&[0; 8]),
            plan(token.job),
            CombinedInbox::new(sender, super::ring(&renderer)),
        )
        .ok()
        .expect("combined submission");
    assert!(
        renderer.wait(Some(Instant::now() + WAIT)).unwrap(),
        "a successful move rings the renderer inbox"
    );
    let forwarded = receiver
        .try_recv()
        .expect("the frame is in the inbox once the ring is seen");
    renderer.clear().unwrap();
    assert!(
        worker.settle_combined(token, forwarded).is_ok(),
        "exact settlement"
    );
    assert!(
        !renderer.wait(Some(Instant::now())).unwrap(),
        "dropping the route does not ring the renderer again"
    );
}
