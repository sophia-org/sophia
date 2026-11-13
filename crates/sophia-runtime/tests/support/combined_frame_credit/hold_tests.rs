//! G3-5d d-2a: the validator's test-support instrumentation, through the
//! public admission. A hold armed with `arm_next_combined_checkpoint` rides
//! the next real `submit_combined`; the loop-head progress counter counts a
//! job's iteration only after its completion send and notification, or after
//! its forward.

use super::super::hold::checkpoint;
use super::tests::{WAIT, plan, source};
use super::*;
use rustix::event::{PollFd, PollFlags};

fn readable(worker: &Worker) -> bool {
    let mut fds = [PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN)];
    sophia_wake::wait(&mut fds, Some(Instant::now())).unwrap();
    fds[0].revents().contains(PollFlags::IN)
}

fn admit(
    worker: &mut Worker,
    bytes: &[u8],
) -> (
    CombinedReservation,
    mpsc::Receiver<ForwardedValidation<ProbePlan>>,
    mpsc::Receiver<ProbePlan>,
) {
    let reservation = worker.reserve_combined(8, Instant::now() + WAIT).unwrap();
    let (sender, forwarded) = mpsc::sync_channel(1);
    let owner_plan = worker
        .submit_combined(
            &reservation,
            source(bytes),
            plan(reservation.job),
            inbox(sender),
        )
        .ok()
        .expect("combined admission");
    (reservation, forwarded, owner_plan)
}

#[test]
fn an_armed_checkpoint_holds_the_next_real_admission_before_validation() {
    let mut worker = Worker::for_combined_plans(2, 8).unwrap();
    let (hold, mut handle) = checkpoint();
    worker.arm_next_combined_checkpoint(hold);
    let (first, first_forwarded, _first_plan) = admit(&mut worker, &[0; 8]);
    assert!(
        handle.entered(WAIT),
        "the armed hold rides the next real admission"
    );
    assert!(
        !first.was_forwarded() && first_forwarded.try_recv().is_err(),
        "the held job is not validated or forwarded"
    );
    let (second, second_forwarded, _second_plan) = admit(&mut worker, &[0; 8]);
    handle.release();
    let first_returned = first_forwarded
        .recv_timeout(WAIT)
        .expect("the held job forwards once released");
    let second_returned = second_forwarded
        .recv_timeout(WAIT)
        .expect("the next admission forwards without a hold");
    assert!(
        !handle.entered(Duration::from_millis(50)),
        "one arming holds one admission"
    );
    assert!(worker.settle_combined(first, first_returned).is_ok());
    assert!(worker.settle_combined(second, second_returned).is_ok());
    assert_eq!(worker.pending(), 0);
}

#[test]
fn the_progress_counter_follows_the_completion_send_and_notification() {
    let mut worker = Worker::for_combined_plans(2, 8).unwrap();
    let progress = worker.validator_progress();
    assert!(
        progress.wait_for(1, WAIT),
        "an idle validator reached its loop head once"
    );
    assert_eq!(progress.value(), 1);
    assert!(!readable(&worker), "nothing completed yet");
    // A malformed frame returns through the ordinary completion path.
    let (token, _forwarded, owner_plan) = admit(&mut worker, &[1, 0, 0, 0, 0, 0, 0, 0]);
    assert!(
        progress.wait_for(2, WAIT),
        "the failed job's iteration completes"
    );
    assert!(
        readable(&worker),
        "the completion's notification precedes the next loop-head count"
    );
    let completion = worker
        .take_completed()
        .unwrap()
        .expect("the ordinary completion");
    assert_eq!(completion.job, token.job);
    assert!(matches!(
        completion.result,
        Err(IsbValidationFailure::Malformed)
    ));
    assert!(
        owner_plan.try_recv().is_ok(),
        "the plan came back to its owner"
    );
}

#[test]
fn a_forwarded_job_leaves_readiness_quiet_after_its_whole_iteration() {
    let mut worker = Worker::for_combined_plans(2, 8).unwrap();
    let progress = worker.validator_progress();
    assert!(worker.take_completed().unwrap().is_none());
    assert!(!readable(&worker));
    let (hold, mut handle) = checkpoint();
    worker.arm_next_combined_checkpoint(hold);
    let (first, forwarded, owner_plan) = admit(&mut worker, &[0; 8]);
    assert!(
        handle.entered(WAIT),
        "the admission is held before validation"
    );
    // Measured baseline: the held iteration has counted its loop head.
    let p0 = progress.value();
    assert!(!readable(&worker), "nothing is readable while held");
    handle.release();
    assert!(
        progress.wait_for(p0 + 1, WAIT),
        "the forwarded job's iteration completes"
    );
    let returned = forwarded
        .try_recv()
        .expect("the job was forwarded before the loop head");
    assert!(first.was_forwarded());
    assert!(
        !readable(&worker),
        "readiness stays quiet after a forwarded job"
    );
    assert!(worker.take_completed().unwrap().is_none());
    assert!(owner_plan.try_recv().is_err(), "the plan travelled on");
    assert!(worker.settle_combined(first, returned).is_ok());
}

#[test]
fn each_validator_has_its_own_serial() {
    let first = Worker::for_combined_plans(1, 8).unwrap();
    let second = Worker::for_combined_plans(1, 8).unwrap();
    assert_ne!(first.validator_serial(), second.validator_serial());
}
