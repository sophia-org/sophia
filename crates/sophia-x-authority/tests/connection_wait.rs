use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

use sophia_protocol::NamespaceId;
use sophia_x_authority::{
    ConnectionNotifier, ConnectionWait, ConnectionWake, NotifierRegistry, XInputAuthorityState,
};

fn pair() -> (UnixStream, UnixStream) {
    UnixStream::pair().expect("a socket pair")
}

fn registry_with(notifier: &ConnectionNotifier) -> NotifierRegistry {
    let mut registry = NotifierRegistry::default();
    registry.register(notifier);
    registry
}

#[test]
fn a_wake_raised_before_the_wait_begins_is_still_delivered() {
    let (ours, _peer) = pair();
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let mut registry = registry_with(&notifier);
    // The wake happens first, with nobody yet waiting.
    registry.notify_all();

    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    let wake = wait
        .wait_until(Some(Instant::now() + Duration::from_secs(5)))
        .expect("the wait to complete");

    assert_eq!(wake, ConnectionWake::Notified);
}

#[test]
fn unread_pipelined_bytes_are_not_a_wake_reason() {
    let (ours, mut peer) = pair();
    peer.write_all(b"a pipelined request").expect("the write");
    let notifier = ConnectionNotifier::new().expect("an eventfd");

    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    let wake = wait
        .wait_until(Some(Instant::now() + Duration::from_millis(50)))
        .expect("the wait to complete");

    // A wait that included POLLIN would report readiness immediately and
    // forever, though the peer has done nothing that concerns this wait.
    assert_eq!(wake, ConnectionWake::Deadline);
    assert!(!wait.half_closed());
    // One round, spent blocked in poll for the whole deadline. Asking for
    // POLLIN would return it at once with the buffered bytes set and spin
    // here until the deadline instead, which the outcome alone would hide.
    assert_eq!(wait.poll_rounds(), 1);
}

#[test]
fn a_departed_peer_ends_the_wait() {
    let (ours, peer) = pair();
    drop(peer);
    let notifier = ConnectionNotifier::new().expect("an eventfd");

    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    let wake = wait
        .wait_until(Some(Instant::now() + Duration::from_secs(5)))
        .expect("the wait to complete");

    assert_eq!(wake, ConnectionWake::Departed);
}

#[test]
fn a_half_closed_peer_neither_ends_the_wait_nor_spins_it() {
    let (mut ours, mut peer) = pair();
    peer.write_all(b"buffered").expect("the write");
    peer.shutdown(std::net::Shutdown::Write)
        .expect("the half close");
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let mut registry = registry_with(&notifier);

    let waker = thread::spawn(move || {
        thread::sleep(Duration::from_millis(120));
        registry.notify_all();
    });
    let mut wait = ConnectionWait::new(ours.as_fd(), &notifier);
    let wake = wait
        .wait_until(Some(Instant::now() + Duration::from_secs(5)))
        .expect("the wait to complete");
    waker.join().expect("the waking thread");

    // Half-close is recorded, not treated as departure: the peer is still
    // owed replies for what it already sent.
    assert_eq!(wake, ConnectionWake::Notified);
    assert!(wait.half_closed());
    // POLLRDHUP stays set once raised. Without dropping interest in it the
    // loop would run for the whole 120ms at the speed of poll; with the
    // latch it runs exactly twice, once per genuine event.
    assert_eq!(wait.poll_rounds(), 2);

    // And the half close discarded nothing the peer had already sent.
    let mut buffered = [0u8; 8];
    ours.read_exact(&mut buffered).expect("the buffered bytes");
    assert_eq!(&buffered, b"buffered");
}

#[test]
fn a_registry_forgets_a_subscriber_that_departed() {
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let mut registry = registry_with(&notifier);
    assert_eq!(registry.subscriber_count(), 1);

    drop(notifier);
    registry.notify_all();

    assert_eq!(registry.subscriber_count(), 0);
}

#[test]
fn parking_repeatedly_does_not_grow_the_subscriber_list() {
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let mut registry = NotifierRegistry::default();
    for _ in 0..32 {
        registry.register(&notifier);
    }
    assert_eq!(registry.subscriber_count(), 1);
}

/// The three ways a server grab can end, each of which must wake whoever
/// parked behind it. A release that forgets to wake parks the waiter
/// forever, because the wait has no backstop timer by design.
fn parked_waiter() -> (NamespaceId, ConnectionNotifier, XInputAuthorityState) {
    let namespace = NamespaceId::from_raw(1);
    let notifier = ConnectionNotifier::new().expect("an eventfd");
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_server(namespace, 7)
        .expect("the grab to be taken");
    authority.await_server_grab(namespace, &notifier);
    (namespace, notifier, authority)
}

fn woke(notifier: &ConnectionNotifier) -> bool {
    let (ours, _peer) = pair();
    let mut wait = ConnectionWait::new(ours.as_fd(), notifier);
    wait.wait_until(Some(Instant::now() + Duration::from_millis(50)))
        .expect("the wait to complete")
        == ConnectionWake::Notified
}

#[test]
fn releasing_the_server_grab_wakes_a_parked_connection() {
    let (namespace, notifier, mut authority) = parked_waiter();
    authority.ungrab_server(namespace, 7);
    assert!(woke(&notifier));
}

#[test]
fn revoking_the_security_epoch_wakes_a_parked_connection() {
    let (_namespace, notifier, mut authority) = parked_waiter();
    authority.advance_security_epoch();
    assert!(woke(&notifier));
}

#[test]
fn the_grab_holder_disconnecting_wakes_a_parked_connection() {
    let (_namespace, notifier, mut authority) = parked_waiter();
    authority.cleanup_owner(7);
    assert!(woke(&notifier));
}

#[test]
fn another_clients_ungrab_does_not_wake_a_connection_still_blocked() {
    let (namespace, notifier, mut authority) = parked_waiter();
    // Client 9 never held the grab, so its release changes nothing and
    // the parked connection must stay parked.
    authority.ungrab_server(namespace, 9);
    assert!(!woke(&notifier));
}
