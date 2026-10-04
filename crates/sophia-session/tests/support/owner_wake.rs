//! The owner's wait against producers that publish, then ring.
//!
//! Elapsed-time bounds are generous: they separate "woken" from "waited out
//! a thirty-second deadline", not one scheduling delay from another.
#![cfg(test)]

use super::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::sync_channel;
use std::thread;

const LONG: Duration = Duration::from_secs(30);
const PROMPT: Duration = Duration::from_secs(5);

#[test]
fn wait_attribution_separates_queued_work_readiness_and_expiry() {
    let owner = OwnerWake::new().unwrap();
    let (sender, queue) = sync_channel::<u32>(1);
    let mut plan = WaitPlan::new(Duration::ZERO, WaitReason::Frames);
    plan.pending(WaitReason::Frames);
    plan.pending(WaitReason::Lifecycle);
    sender.send(7).unwrap();
    owner.plan_wait(plan);
    assert_eq!(owner.receive(&queue, Duration::ZERO), Ok(7));
    assert_eq!(
        owner.wait_attribution(),
        attribution::WaitAttribution::default()
    );

    owner.plan_wait(plan);
    assert_eq!(
        owner.receive(&queue, Duration::ZERO),
        Err(RecvTimeoutError::Timeout)
    );
    let row = owner.wait_attribution().to_string();
    assert!(row.contains(" selected_frames=1 expired_frames=1 pending_frames=1"));
    assert!(row.contains(" selected_lifecycle=0 expired_lifecycle=0 pending_lifecycle=1"));

    owner.notifier().notify();
    owner.plan_wait(WaitPlan::new(LONG, WaitReason::Present));
    assert_eq!(owner.receive(&queue, LONG), Err(RecvTimeoutError::Timeout));
    let row = owner.wait_attribution().to_string();
    assert!(row.contains(" selected_present=1 expired_present=0 pending_present=0"));
    let reduced = crate::diagnostics::reduced_record(&format!(
        "sophia_owner_wait schema=1 owner_tid=123 observed_monotonic_usec=456{row}"
    ))
    .unwrap();
    assert!(reduced.ends_with(&row));
}

#[test]
fn wait_caps_keep_the_first_tie_and_then_the_earliest_deadline() {
    let mut plan = WaitPlan::new(Duration::from_millis(25), WaitReason::Maintenance);
    plan.cap(Duration::from_millis(1), WaitReason::Frames);
    plan.cap(Duration::from_millis(1), WaitReason::Controls);
    assert_eq!(plan.reason, WaitReason::Frames);
    plan.cap(Duration::ZERO, WaitReason::Present);
    assert_eq!(plan.reason, WaitReason::Present);
    assert_eq!(plan.timeout, Duration::ZERO);
}

/// A producer's view of the owner, as Session hands it to each worker.
fn attached(owner: &OwnerWake) -> sophia_wake::WakeSlot {
    let slot = sophia_wake::WakeSlot::default();
    slot.set(owner.notifier());
    slot
}

#[test]
fn a_publication_between_inspection_and_wait_ends_the_wait() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    let (input, input_queue) = sync_channel::<u32>(1);
    let input = sophia_wake::SignalSender::new(input, attached(&owner));
    owner.begin_pass().unwrap();
    // The pass inspects input, finds nothing, and moves on to its wait. The
    // producer publishes in that gap; its ring must not be lost.
    assert_eq!(input_queue.try_recv(), Err(TryRecvError::Empty));
    input.send(7).unwrap();
    let started = Instant::now();
    assert_eq!(
        owner.receive(&authority_queue, LONG),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() < PROMPT, "the ring was lost");
    // The next pass clears first, then finds the work the ring announced.
    owner.begin_pass().unwrap();
    assert_eq!(input_queue.try_recv(), Ok(7));
}

#[test]
fn work_published_before_the_pass_is_found_by_its_inspection() {
    let owner = OwnerWake::new().unwrap();
    let (authority, authority_queue) = sync_channel::<u32>(1);
    let authority = sophia_wake::SignalSender::new(authority, attached(&owner));
    authority.send(3).unwrap();
    // Clearing consumes the ring, never the work.
    owner.begin_pass().unwrap();
    let started = Instant::now();
    assert_eq!(owner.receive(&authority_queue, LONG), Ok(3));
    assert!(started.elapsed() < PROMPT);
}

#[test]
fn input_published_during_a_long_idle_wait_ends_it() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    let (input, input_queue) = sync_channel::<u32>(4);
    let input = sophia_wake::SignalSender::new(input, attached(&owner));
    owner.begin_pass().unwrap();
    let producer = thread::spawn(move || {
        // Usually lands while the owner sleeps; landing first is also correct.
        thread::sleep(Duration::from_millis(50));
        input.send(1).unwrap();
        input
    });
    let started = Instant::now();
    assert_eq!(
        owner.receive(&authority_queue, LONG),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(
        started.elapsed() < PROMPT,
        "idle wait ignored the input ring"
    );
    let _input = producer.join().unwrap();
    owner.begin_pass().unwrap();
    assert_eq!(input_queue.try_recv(), Ok(1));
}

#[test]
fn authority_published_during_the_wait_is_returned_by_it() {
    let owner = OwnerWake::new().unwrap();
    let (authority, authority_queue) = sync_channel::<u32>(1);
    let authority = sophia_wake::SignalSender::new(authority, attached(&owner));
    owner.begin_pass().unwrap();
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        authority.send(9).unwrap();
        authority
    });
    let started = Instant::now();
    assert_eq!(owner.receive(&authority_queue, LONG), Ok(9));
    assert!(started.elapsed() < PROMPT);
    drop(producer.join().unwrap());
}

#[test]
fn the_deadline_bounds_an_unrung_wait() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    owner.begin_pass().unwrap();
    let budget = Duration::from_millis(20);
    let started = Instant::now();
    assert_eq!(
        owner.receive(&authority_queue, budget),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() >= budget);
}

#[test]
fn a_stale_ring_is_cleared_and_does_not_shorten_the_next_wait() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    owner.notifier().notify();
    owner.notifier().notify();
    owner.begin_pass().unwrap();
    let budget = Duration::from_millis(20);
    let started = Instant::now();
    assert_eq!(
        owner.receive(&authority_queue, budget),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() >= budget);
}

#[test]
fn producer_disconnection_wakes_the_owner_and_reads_as_disconnected() {
    let owner = OwnerWake::new().unwrap();
    let (authority, authority_queue) = sync_channel::<u32>(1);
    let authority = sophia_wake::SignalSender::new(authority, attached(&owner));
    owner.begin_pass().unwrap();
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        drop(authority);
    });
    let started = Instant::now();
    assert_eq!(
        owner.receive(&authority_queue, LONG),
        Err(RecvTimeoutError::Disconnected)
    );
    assert!(started.elapsed() < PROMPT, "disconnection did not ring");
    producer.join().unwrap();
}

#[test]
fn producers_outliving_the_owner_publish_without_ringing_a_closed_wake() {
    let owner = OwnerWake::new().unwrap();
    let (input, input_queue) = sync_channel::<u32>(1);
    let input = sophia_wake::SignalSender::new(input, attached(&owner));
    // Session shutdown drops the owner before every worker has joined.
    drop(owner);
    input.send(5).unwrap();
    drop(input);
    assert_eq!(input_queue.try_recv(), Ok(5));
}

/// A socket the owner serves inline, as it holds a shell wire, and its peer.
fn served() -> (UnixStream, UnixStream) {
    let (served, peer) = UnixStream::pair().unwrap();
    served.set_nonblocking(true).unwrap();
    (served, peer)
}

fn readable(socket: &UnixStream) -> Vec<PollFd<'_>> {
    vec![PollFd::new(socket, PollFlags::IN)]
}

#[test]
fn an_idle_socket_lets_the_deadline_end_the_wait() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    let (socket, _peer) = served();
    owner.begin_pass().unwrap();
    let budget = Duration::from_millis(20);
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, budget, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() >= budget);
}

#[test]
fn a_request_during_a_long_idle_wait_ends_it() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    let (mut socket, mut peer) = served();
    owner.begin_pass().unwrap();
    let client = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        peer.write_all(b"request").unwrap();
        peer
    });
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, LONG, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(
        started.elapsed() < PROMPT,
        "idle wait ignored the socket's request"
    );
    let _peer = client.join().unwrap();
    // The next pass serves the socket.
    owner.begin_pass().unwrap();
    let mut request = [0; 7];
    socket.read_exact(&mut request).unwrap();
    assert_eq!(&request, b"request");
}

#[test]
fn consumed_readiness_does_not_shorten_the_next_wait() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, authority_queue) = sync_channel::<u32>(1);
    let (mut socket, mut peer) = served();
    peer.write_all(b"once").unwrap();
    owner.begin_pass().unwrap();
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, LONG, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() < PROMPT);
    owner.begin_pass().unwrap();
    let mut request = [0; 4];
    socket.read_exact(&mut request).unwrap();
    // Served, the socket is idle again: the owner sleeps out its budget
    // rather than spinning on readiness it already consumed.
    let budget = Duration::from_millis(20);
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, budget, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(
        started.elapsed() >= budget,
        "consumed readiness ended the wait"
    );
}

#[test]
fn rings_and_authority_still_end_a_wait_that_watches_sockets() {
    let owner = OwnerWake::new().unwrap();
    let (authority, authority_queue) = sync_channel::<u32>(1);
    let authority = sophia_wake::SignalSender::new(authority, attached(&owner));
    let (input, input_queue) = sync_channel::<u32>(1);
    let input = sophia_wake::SignalSender::new(input, attached(&owner));
    let (socket, _peer) = served();
    owner.begin_pass().unwrap();
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        input.send(1).unwrap();
        input
    });
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, LONG, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(
        started.elapsed() < PROMPT,
        "the ring was lost beside a socket"
    );
    let _input = producer.join().unwrap();
    owner.begin_pass().unwrap();
    assert_eq!(input_queue.try_recv(), Ok(1));
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        authority.send(4).unwrap();
        authority
    });
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_fds(&authority_queue, LONG, readable(&socket))
            .unwrap(),
        Ok(4)
    );
    assert!(started.elapsed() < PROMPT);
    drop(producer.join().unwrap());
}

#[test]
fn either_sender_drives_session_helpers() {
    let owner = OwnerWake::new().unwrap();
    let (bare, bare_queue) = sync_channel::<u32>(1);
    let (notifying, notifying_queue) = sync_channel::<u32>(1);
    let notifying = sophia_wake::SignalSender::new(notifying, attached(&owner));
    owner.begin_pass().unwrap();
    let rung = || owner.wake.wait(Some(Instant::now())).unwrap();

    let bare: &dyn SessionSender<u32> = &bare;
    bare.try_send(1).unwrap();
    assert!(matches!(bare.try_send(2), Err(TrySendError::Full(2))));
    assert_eq!(bare_queue.try_recv(), Ok(1));
    assert!(!rung(), "a bare sender has no ring to give");

    let notifying: &dyn SessionSender<u32> = &notifying;
    notifying.try_send(1).unwrap();
    assert!(rung());
    owner.begin_pass().unwrap();
    // A refused publication rings nothing: nothing new was queued.
    assert!(matches!(notifying.try_send(2), Err(TrySendError::Full(2))));
    assert!(!rung());
    assert_eq!(notifying_queue.try_recv(), Ok(1));
}

#[test]
fn measurements_distinguish_coalesced_rings_ready_fds_deadlines_and_queued_work() {
    let owner = OwnerWake::new().unwrap();
    let (authority, queue) = sync_channel::<u32>(1);
    let (socket, mut peer) = served();
    owner.begin_pass().unwrap();
    owner.notifier().notify();
    owner.notifier().notify();
    peer.write_all(b"ready").unwrap();
    assert_eq!(
        owner
            .receive_with_fds(&queue, Duration::ZERO, readable(&socket))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert_eq!(
        owner.statistics(),
        OwnerWakeStatistics {
            passes: 1,
            waits: 1,
            ring_ready: 1,
            fd_ready: 1,
            wait_deadlines: 0,
            immediate_items: 0,
            ..OwnerWakeStatistics::default()
        }
    );
    // Clearing the coalesced ring, then waiting with no borrowed descriptors,
    // gives a deadline. Readiness is counted per poll return, not per notify.
    owner.begin_pass().unwrap();
    assert_eq!(
        owner.receive(&queue, Duration::ZERO),
        Err(RecvTimeoutError::Timeout)
    );
    authority.send(7).unwrap();
    assert_eq!(owner.receive(&queue, Duration::ZERO), Ok(7));
    assert_eq!(
        owner.statistics(),
        OwnerWakeStatistics {
            passes: 2,
            waits: 2,
            ring_ready: 1,
            fd_ready: 1,
            wait_deadlines: 1,
            immediate_items: 1,
            ..OwnerWakeStatistics::default()
        }
    );
}

#[test]
fn native_readiness_wakes_without_consuming_and_returns_to_idle_after_service() {
    let owner = OwnerWake::new().unwrap();
    let (_authority, queue) = sync_channel::<u32>(1);
    let (mut card, mut kernel) = served();
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        kernel.write_all(b"flip").unwrap();
        kernel
    });
    owner.begin_pass().unwrap();
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_native(&queue, LONG, vec![], readable(&card), Some((7, 0)))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() < PROMPT);
    let _kernel = producer.join().unwrap();
    assert_eq!(owner.statistics().native_ready, 1);
    assert_eq!(
        owner.statistics().fd_ready,
        0,
        "shell readiness remains separate"
    );
    // Only the native service consumes the event. Readiness grants no retirement.
    let mut event = [0; 4];
    card.read_exact(&mut event).unwrap();
    assert_eq!(&event, b"flip");
    owner.observe_native_progress(Some((7, 1)), true).unwrap();
    assert_eq!(owner.statistics().native_ready_consumed, 1);
    owner.begin_pass().unwrap();
    let idle = Duration::from_millis(25);
    let started = Instant::now();
    assert_eq!(
        owner
            .receive_with_native(&queue, idle, vec![], vec![], Some((7, 1)))
            .unwrap(),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(started.elapsed() >= idle, "there must be no polling tail");
    assert_eq!(owner.statistics().waits, 2);
    assert_eq!(owner.statistics().wait_deadlines, 1);
}

#[test]
fn native_ready_without_consumption_is_counted_separately() {
    let owner = OwnerWake::new().unwrap();
    let (card, mut kernel) = served();
    kernel.write_all(b"flip").unwrap();
    owner
        .wait_for_service(LONG, readable(&card), Some((9, 3)))
        .unwrap();
    owner.observe_native_progress(Some((9, 3)), true).unwrap();
    assert_eq!(owner.statistics().native_ready_idle, 1);
    assert_eq!(owner.statistics().native_ready_consumed, 0);
    owner.observe_native_progress(Some((9, 4)), true).unwrap();
    assert_eq!(
        owner.statistics().native_ready_idle,
        1,
        "count once per wake"
    );
}

#[test]
fn native_hup_is_a_state_change_before_the_next_subscription() {
    for (active, next_owner, must_fail) in [(false, 7, false), (true, 7, true), (true, 8, false)] {
        let owner = OwnerWake::new().unwrap();
        let (card, kernel) = served();
        drop(kernel);
        owner
            .wait_for_service(LONG, readable(&card), Some((7, 0)))
            .unwrap();
        assert_eq!(owner.statistics().native_errors, 1);
        assert_eq!(
            owner
                .observe_native_progress(Some((next_owner, 0)), active)
                .is_err(),
            must_fail
        );
        // An inactive or replaced owner does not subscribe the revoked card again.
        let idle = Duration::from_millis(20);
        let started = Instant::now();
        owner
            .wait_for_service(idle, vec![], Some((next_owner, 0)))
            .unwrap();
        assert!(started.elapsed() >= idle);
        assert_eq!(owner.statistics().native_errors, 1);
    }
}

#[test]
fn fair_service_wait_does_not_consume_busy_authority_and_is_interruptible() {
    let owner = OwnerWake::new().unwrap();
    let (authority, queue) = sync_channel(1);
    authority.send(42).unwrap();
    let notifier = owner.notifier();
    let producer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        notifier.notify();
    });
    let started = Instant::now();
    owner.wait_for_service(LONG, vec![], None).unwrap();
    assert!(started.elapsed() < PROMPT);
    assert_eq!(queue.try_recv(), Ok(42));
    assert_eq!(owner.statistics().service_waits, 1);
    assert_eq!(owner.statistics().ring_ready, 1);
    producer.join().unwrap();
}

#[test]
fn stalled_native_descriptor_sleeps_until_the_watchdog_budget() {
    let owner = OwnerWake::new().unwrap();
    let (card, _kernel) = served();
    let bound = Duration::from_millis(25);
    let started = Instant::now();
    owner
        .wait_for_service(bound, readable(&card), Some((3, 0)))
        .unwrap();
    assert!(started.elapsed() >= bound);
    assert_eq!(owner.statistics().waits, 1);
    assert_eq!(owner.statistics().native_ready, 0);
    assert_eq!(owner.statistics().wait_deadlines, 1);
}
