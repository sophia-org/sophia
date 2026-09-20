//! What a submitter learns about its own request's internal processing.
//!
//! The barrier is the thing FakeInput's next-request rule rests on, and the
//! rule is exact: the next request waits until internal processing completes,
//! not until the work was accepted and not until it reached a socket. These
//! pin the two halves that make that safe to wait on. The slot is storage, so
//! an outcome survives a wake that is coalesced, lost, or raised at a
//! connection that already went; the notifier is only a wakeup, so it can fail
//! without costing anyone an answer.

use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use sophia_input_authority::{RegistrationError, RequestCompletion};
use sophia_x_authority::{
    ConnectionNotifier, ConnectionWait, ConnectionWake, PrivateRequestBarrier,
};

fn barrier() -> (ConnectionNotifier, PrivateRequestBarrier) {
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let barrier = PrivateRequestBarrier::over(&notifier);
    (notifier, barrier)
}

#[test]
fn an_unanswered_request_is_outstanding_and_yields_nothing() {
    let (_notifier, barrier) = barrier();
    assert!(!barrier.armed(), "a fresh barrier owes nobody an answer");

    let _ticket = barrier.arm(1);

    assert!(barrier.armed());
    // None means not yet, never refused. A refusal is an outcome and arrives
    // as one, so a waiter that treated None as an answer would report success
    // for work that had not happened.
    assert_eq!(barrier.take(), None);
    assert!(barrier.armed(), "taking nothing does not end the wait");
}

#[test]
fn the_outcome_reported_is_the_outcome_taken_and_the_wait_ends() {
    for completion in [
        RequestCompletion::Processed,
        RequestCompletion::Cancelled,
        RequestCompletion::Refused(RegistrationError::RoutingUnavailable),
        RequestCompletion::FailedAfterApplication(RegistrationError::RoutingUnavailable),
    ] {
        let (_notifier, barrier) = barrier();
        let ticket = barrier.arm(7);

        assert!(ticket.report(completion));

        assert_eq!(barrier.take(), Some(completion));
        assert!(!barrier.armed(), "a taken outcome ends the request");
        assert_eq!(barrier.take(), None, "an outcome answers once");
    }
}

#[test]
fn the_first_answer_wins_rather_than_the_last() {
    let (_notifier, barrier) = barrier();
    let ticket = barrier.arm(3);

    assert!(ticket.report(RequestCompletion::Processed));
    // The same request is answered from its execution and again from the
    // later observation that reclaims its cell. Keeping the second would
    // overwrite what really happened with a cancellation that only describes
    // the cell being freed.
    assert!(!ticket.report(RequestCompletion::Cancelled));

    assert_eq!(barrier.take(), Some(RequestCompletion::Processed));
}

#[test]
fn a_ticket_cannot_answer_the_request_that_replaced_its_own() {
    let (_notifier, barrier) = barrier();
    let abandoned = barrier.arm(11);
    let current = barrier.arm(12);

    // A completion answers exactly one request, and this ticket's request is
    // over. Letting it through would hand one request's outcome to another's
    // waiter, which is worse than that waiter waiting.
    assert!(!abandoned.report(RequestCompletion::Processed));
    assert_eq!(barrier.take(), None);

    assert!(current.report(RequestCompletion::Cancelled));
    assert_eq!(barrier.take(), Some(RequestCompletion::Cancelled));
}

#[test]
fn rearming_discards_an_outcome_nobody_took() {
    let (_notifier, barrier) = barrier();
    let first = barrier.arm(20);
    assert!(first.report(RequestCompletion::Processed));

    // The previous request ended without its answer being read. Delivering it
    // to the next request's waiter would be a lie about the next request.
    let _second = barrier.arm(21);

    assert_eq!(barrier.take(), None);
    assert!(barrier.armed());
}

#[test]
fn a_stored_outcome_survives_a_wake_nobody_could_receive() {
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let barrier = PrivateRequestBarrier::over(&notifier);
    let ticket = barrier.arm(5);
    assert!(ticket.report(RequestCompletion::Processed));

    // The connection departs, taking the descriptor the wake would have gone
    // to. The runner still raises it, because a failed wake is not a reason
    // to skip the store and the store already happened.
    drop(notifier);
    ticket.flush();

    assert_eq!(
        barrier.take(),
        Some(RequestCompletion::Processed),
        "a departed waiter must not cost the runner its record of the outcome"
    );
}

#[test]
fn storing_then_waking_releases_a_connection_parked_on_its_socket() {
    let (ours, _peer) = UnixStream::pair().expect("a socket pair");
    let (notifier, barrier) = barrier();
    let ticket = barrier.arm(42);

    // The order the runner uses: commit the outcome under the guard, then
    // raise the wake outside it. A waiter that is roused therefore always
    // finds an answer rather than having to park again.
    assert!(ticket.report(RequestCompletion::Processed));
    ticket.flush();

    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    let wake = wait
        .wait_until(Some(Instant::now() + Duration::from_secs(5)))
        .expect("the wait to complete");

    assert_eq!(wake, ConnectionWake::Notified);
    assert_eq!(barrier.take(), Some(RequestCompletion::Processed));
}

#[test]
fn a_wake_raised_before_the_park_is_not_slept_through() {
    let (ours, _peer) = UnixStream::pair().expect("a socket pair");
    let (notifier, barrier) = barrier();
    let ticket = barrier.arm(43);

    // The whole race the eventfd counter exists to close: work can finish
    // between a submitter releasing its guards and reaching the wait. The
    // wake is pending, so the park returns immediately instead of blocking on
    // an answer that already arrived.
    ticket.report(RequestCompletion::Cancelled);
    ticket.flush();

    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    assert_eq!(
        wait.wait_until(Some(Instant::now() + Duration::from_secs(5)))
            .expect("the wait to complete"),
        ConnectionWake::Notified
    );
    assert_eq!(barrier.take(), Some(RequestCompletion::Cancelled));
}
