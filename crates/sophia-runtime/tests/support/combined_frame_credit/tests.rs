use super::*;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::time::Duration;

pub(super) const WAIT: Duration = Duration::from_secs(10);

pub(super) fn source(bytes: &[u8]) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-credit",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

pub(super) fn completion(worker: &mut IsbValidationWorker) -> IsbValidationCompletion {
    use rustix::event::{PollFd, PollFlags, Timespec};
    let end = Instant::now() + WAIT;
    loop {
        if let Some(result) = worker.take_completed().unwrap() {
            return result;
        }
        assert!(
            Instant::now() < end,
            "validation did not return to the owner"
        );
        let mut fds = [PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN)];
        rustix::event::poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            }),
        )
        .unwrap();
    }
}

pub(super) fn reserve(worker: &mut IsbValidationWorker) -> Reservation {
    worker.reserve_combined(8, Instant::now() + WAIT).unwrap()
}

fn plan(id: u64) -> ProbePlan {
    ProbePlan {
        id,
        descriptor: source(&id.to_le_bytes()),
        lifetime: Arc::new(()),
    }
}

fn submit(
    worker: &mut IsbValidationWorker,
    reservation: &Reservation,
) -> ReturnedValidation<ProbePlan> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let _returned_plan = worker
        .submit_combined(reservation, source(&[0; 8]), plan(reservation.job), sender)
        .ok()
        .expect("combined submission");
    receiver.recv_timeout(WAIT).unwrap()
}

#[test]
fn ordinary_and_combined_reservations_share_the_real_four_slots() {
    let mut worker = IsbValidationWorker::new(4, 8).unwrap();
    let ordinary = worker.reserve(8, Instant::now() + WAIT).unwrap();
    let combined: Vec<_> = (0..3).map(|_| reserve(&mut worker)).collect();
    assert_eq!(worker.pending(), 4);
    assert_eq!(
        worker.reserve(8, Instant::now() + WAIT),
        Err(IsbSubmitFailure::Capacity)
    );
    assert_eq!(
        worker.reserve_combined(8, Instant::now() + WAIT).err(),
        Some(IsbSubmitFailure::Capacity)
    );
    assert!(worker.release_reserved(ordinary));
    let replacement = reserve(&mut worker);
    assert!(replacement.job > combined[2].job);
    for token in combined.into_iter().chain([replacement]) {
        assert!(worker.release_reserved(token.job));
    }
    assert_eq!(worker.pending(), 0);
}

#[test]
fn returned_validation_clears_exactly_one_credit_and_preserves_backing() {
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let token = reserve(&mut worker);
    let returned = submit(&mut worker, &token);
    assert_eq!(
        worker.pending(),
        1,
        "forwarding must retain the owner credit"
    );
    assert!(!worker.release_reserved(token.job));
    // One observation before take_completed can clear the wake; not a race proof.
    {
        use rustix::event::{PollFd, PollFlags, Timespec};
        let mut fds = [PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN)];
        rustix::event::poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )
        .unwrap();
        assert!(!fds[0].revents().contains(PollFlags::IN));
    }
    assert!(
        worker.take_completed().unwrap().is_none(),
        "forwarded success is not an ordinary completion"
    );
    assert!(
        worker.cancel(token.job),
        "cancellation must not remove a returned obligation"
    );
    // Deliberately forge a replay: the public seam types do not implement Clone.
    let replay = Reservation {
        job: token.job,
        identity: Arc::clone(&token.identity),
    };
    let identity = Arc::clone(&returned.identity);
    let job = returned.job;
    let (pixels, plan) = worker
        .settle_combined(token, returned)
        .ok()
        .expect("matching returned validation");
    assert_eq!(pixels.bytes(), &[0; 8]);
    assert_eq!(plan.id, job);
    assert_eq!(worker.pending(), 0);
    let next = reserve(&mut worker);
    let duplicate = ReturnedValidation {
        job,
        identity,
        pixels,
        plan,
    };
    let (_, duplicate) = worker
        .settle_combined(replay, duplicate)
        .err()
        .expect("duplicate must refuse");
    assert_eq!(worker.pending(), 1, "replay must not clear the next row");
    assert_eq!(duplicate.pixels.bytes(), &[0; 8]);
    assert!(worker.release_reserved(next.job));
}

#[test]
fn cross_allocator_and_wrong_job_proofs_return_their_payloads_without_clearing() {
    let mut first = IsbValidationWorker::new(2, 8).unwrap();
    let mut second = IsbValidationWorker::new(1, 8).unwrap();
    let a = reserve(&mut first);
    let foreign = reserve(&mut second);
    assert_eq!(
        a.job, foreign.job,
        "numeric ids alone must not bind allocators"
    );
    let returned_a = submit(&mut first, &a);
    let returned_foreign = submit(&mut second, &foreign);
    let (a, returned_a) = second
        .settle_combined(a, returned_a)
        .err()
        .expect("foreign allocator");
    assert_eq!(first.pending(), 1);
    assert_eq!(second.pending(), 1);
    let b = reserve(&mut first);
    let returned_b = submit(&mut first, &b);
    let (a, returned_b) = first
        .settle_combined(a, returned_b)
        .err()
        .expect("wrong job");
    assert_eq!(first.pending(), 2);
    assert!(first.settle_combined(a, returned_a).is_ok());
    assert!(first.settle_combined(b, returned_b).is_ok());
    assert!(second.settle_combined(foreign, returned_foreign).is_ok());
}

#[test]
fn stale_submit_and_original_byte_deadline_bounds_are_preserved() {
    let mut worker = IsbValidationWorker::new(2, 8).unwrap();
    let stale = reserve(&mut worker);
    assert!(worker.release_reserved(stale.job));
    let live = reserve(&mut worker);
    let fd = source(&[0; 8]);
    let raw = fd.as_raw_fd();
    let (sender, receiver) = mpsc::sync_channel(1);
    let submitted_plan = plan(stale.job);
    let plan_raw = submitted_plan.descriptor.as_raw_fd();
    let (error, fd, returned_plan) = worker
        .submit_combined(&stale, fd, submitted_plan, sender)
        .expect_err("stale submission");
    assert_eq!(error, IsbSubmitFailure::Invalid);
    assert_eq!(fd.as_raw_fd(), raw);
    assert_eq!(returned_plan.id, stale.job);
    assert_eq!(returned_plan.descriptor.as_raw_fd(), plan_raw);
    assert!(!stale.was_forwarded());
    assert!(receiver.try_recv().is_err());
    assert_eq!(worker.pending(), 1);
    assert!(worker.release_reserved(live.job));

    for (deadline, bytes, expected) in [
        (
            Instant::now() - Duration::from_secs(1),
            vec![0; 8],
            IsbValidationFailure::Timeout,
        ),
        (
            Instant::now() + WAIT,
            vec![0; 4],
            IsbValidationFailure::SizeMismatch,
        ),
        (
            Instant::now() + WAIT,
            vec![1, 0, 0, 0, 0, 0, 0, 0],
            IsbValidationFailure::Malformed,
        ),
    ] {
        let token = worker.reserve_combined(8, deadline).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let submitted_plan = plan(token.job);
        let raw = submitted_plan.descriptor.as_raw_fd();
        let returned_plan = worker
            .submit_combined(&token, source(&bytes), submitted_plan, sender)
            .ok()
            .expect("submitted validation failure");
        let result = completion(&mut worker);
        assert_eq!(result.job, token.job);
        assert_eq!(result.result.err(), Some(expected));
        let returned_plan = returned_plan.try_recv().expect("plan before completion");
        assert_eq!(returned_plan.id, token.job);
        assert_eq!(returned_plan.descriptor.as_raw_fd(), raw);
        assert!(!token.was_forwarded());
        assert!(
            receiver.try_recv().is_err(),
            "validation failure was forwarded"
        );
        assert_eq!(worker.pending(), 0);
        assert!(worker.take_completed().unwrap().is_none());
    }
}

#[test]
fn cancelled_before_submission_uses_the_unchanged_completion_route() {
    let mut worker = IsbValidationWorker::new(1, 8).unwrap();
    let token = reserve(&mut worker);
    assert!(worker.cancel(token.job));
    let (sender, receiver) = mpsc::sync_channel(1);
    let returned_plan = worker
        .submit_combined(&token, source(&[0; 8]), plan(token.job), sender)
        .ok()
        .expect("cancelled submission");
    assert_eq!(worker.pending(), 1);
    let result = completion(&mut worker);
    assert_eq!(result.job, token.job);
    assert_eq!(result.result.err(), Some(IsbValidationFailure::Cancelled));
    assert_eq!(
        returned_plan.try_recv().expect("cancelled plan").id,
        token.job
    );
    assert!(!token.was_forwarded());
    assert_eq!(worker.pending(), 0);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn dropping_a_forwarded_destination_loses_the_return_but_keeps_credit() {
    let mut worker = IsbValidationWorker::new(2, 8).unwrap();
    let token = reserve(&mut worker);
    let (sender, receiver) = mpsc::sync_channel(1);
    let returned_plan = worker
        .submit_combined(&token, source(&[0; 8]), plan(token.job), sender)
        .ok()
        .expect("combined submission");
    // FIFO barrier proves the destination holds the result before it is dropped.
    let barrier = worker
        .submit(source(&[0; 8]), 8, Instant::now() + WAIT)
        .unwrap();
    assert_eq!(completion(&mut worker).job, barrier);
    assert!(
        token.was_forwarded(),
        "successful move publishes its witness"
    );
    assert!(returned_plan.try_recv().is_err());
    drop(receiver);
    assert_eq!(worker.pending(), 1);
    assert!(!worker.release_reserved(token.job));
    assert!(worker.cancel(token.job));
    assert!(
        token.was_forwarded(),
        "cancellation cannot erase forwarding"
    );
    assert!(worker.take_completed().unwrap().is_none());
    assert_eq!(worker.pending(), 1, "stage 1 cannot reclaim a lost return");
    let remaining = worker.reserve(8, Instant::now() + WAIT).unwrap();
    assert_eq!(
        worker.reserve(8, Instant::now() + WAIT),
        Err(IsbSubmitFailure::Capacity)
    );
    assert!(worker.release_reserved(remaining));
    assert_eq!(worker.pending(), 1);
}

#[test]
fn full_or_disconnected_destination_preserves_the_old_owner_return() {
    let mut worker = IsbValidationWorker::new(4, 8).unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let first = reserve(&mut worker);
    let first_plan = worker
        .submit_combined(&first, source(&[0; 8]), plan(first.job), sender.clone())
        .ok()
        .expect("first submission");
    // FIFO barrier: first is in the destination before the second can try_send.
    let barrier = worker
        .submit(source(&[0; 8]), 8, Instant::now() + WAIT)
        .unwrap();
    assert_eq!(completion(&mut worker).job, barrier);
    assert!(first.was_forwarded());
    assert!(first_plan.try_recv().is_err());
    let second = reserve(&mut worker);
    let second_plan = worker
        .submit_combined(&second, source(&[0; 8]), plan(second.job), sender)
        .ok()
        .expect("second submission");
    let result = completion(&mut worker);
    assert_eq!(result.job, second.job);
    assert_eq!(result.result.unwrap().bytes(), &[0; 8]);
    assert_eq!(
        second_plan.try_recv().expect("full plan return").id,
        second.job
    );
    assert!(!second.was_forwarded(), "Full is not a successful move");
    assert_eq!(worker.pending(), 1);
    let returned = receiver.recv_timeout(WAIT).unwrap();
    assert!(worker.settle_combined(first, returned).is_ok());
    assert_eq!(worker.pending(), 0);

    let third = reserve(&mut worker);
    let (sender, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    let third_plan = worker
        .submit_combined(&third, source(&[0; 8]), plan(third.job), sender)
        .ok()
        .expect("third submission");
    let result = completion(&mut worker);
    assert_eq!(result.job, third.job);
    assert_eq!(result.result.unwrap().bytes(), &[0; 8]);
    assert_eq!(
        third_plan.try_recv().expect("disconnected plan return").id,
        third.job
    );
    assert!(
        !third.was_forwarded(),
        "Disconnected is not a successful move"
    );
    assert_eq!(worker.pending(), 0);
}
