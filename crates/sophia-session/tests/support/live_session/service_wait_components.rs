// The owner's bounded Service wait with the component descriptors in it,
// driven through the production helpers against a real connected component.
// Included by `live_session/service_wait_components/tests.rs`; the module
// paths below stay relative to this file's directory.
use super::*;
use crate::owner_wait_causes::{FD, RING, SERVICE, WIRE};
use crate::shell_component_connections::ComponentConnectionKey;
use rustix::event::PollFlags;
use sophia_protocol::*;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../component_files.rs"]
mod component_files;
// The capture owner tests load the same peer; the harness needs it as a sibling.
#[allow(clippy::duplicate_mod)]
#[path = "../../../../sophia-runtime/tests/support/shell_file_peer.rs"]
mod shell_file_peer;
use component_files::Harness;

/// Long enough that a wait ended by readiness is clearly not a deadline.
const WAIT: Duration = Duration::from_millis(300);
const PROMPT: Duration = Duration::from_millis(200);

fn allocation_request(key: ComponentConnectionKey) -> ShellContentRecord {
    ShellContentRecord::AllocationRequest(ContentAllocationRequest {
        grant: key.grant,
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        allocation_request_id: 1,
        operation: 1,
        role: 1,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: 200,
        desired_height: 20,
        margins: ContentMargins::default(),
    })
}

fn turns(h: &Harness) -> (u64, u64) {
    let readiness = h.owner.fd_readiness();
    (readiness.turns_observed, readiness.turns_blind)
}

/// Let a fresh connection's startup traffic drain the way the owner loop
/// would, so the wait under test starts from a quiet component.
fn settle(h: &mut Harness) {
    for _ in 0..200 {
        assert!(h.owner.turn_connected().iter().all(Option::is_none));
        let mut fds = smallvec::SmallVec::<[PollFd<'_>; 64]>::new();
        let _waits = h.owner.subscribe_wait(&mut fds);
        rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap();
        if fds.iter().all(|fd| fd.revents().is_empty()) {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the component never went quiet");
}

fn relay() -> (UnixStream, UnixStream) {
    let (relay, writer) = UnixStream::pair().unwrap();
    relay.set_nonblocking(true).unwrap();
    (relay, writer)
}

#[test]
fn a_component_request_ends_the_service_wait_and_steers_the_next_turn() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(0).unwrap();
    let mut peer = h.connect(key);
    let owner = OwnerWake::new().unwrap();
    settle(&mut h);
    // Turns without an observation probed the wire while settling.
    let (observed, blind) = turns(&h);
    assert_eq!(observed, 0);
    assert!(blind >= 1);
    let base = h.owner.fd_readiness();

    owner.begin_pass().unwrap();
    let (relay, _quiet) = relay();
    let mut fds = smallvec::SmallVec::<[PollFd<'_>; 64]>::new();
    let waits = subscribe(Some(&h.owner), true, &mut fds);
    let subscribed = waits.as_ref().map_or(0, ComponentWaits::len);
    let relay_index = fds.len();
    fds.push(PollFd::new(&relay, PollFlags::IN));
    let request = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        peer.send_content(TransactionId::from_raw(80), &allocation_request(key))
            .unwrap();
        peer
    });
    let started = Instant::now();
    owner
        .wait_for_service_observed(WAIT, fds, Vec::new(), None, |fds| {
            assert!(fds[relay_index].revents().is_empty(), "quiet relay fired");
            observe(&owner, waits, fds);
        })
        .unwrap();
    let elapsed = started.elapsed();
    assert!(h.owner.turn_connected().iter().all(Option::is_none));
    let peer = request.join().unwrap();
    let now = h.owner.content_clock_msec();
    let served = h
        .owner
        .with_connection(key, |t| {
            t.service_content_allocation_requests(&[], now).unwrap()
        })
        .unwrap();
    assert_eq!(served, 1, "the request was not served");
    assert_eq!(
        turns(&h),
        (1, blind),
        "the turn after the Service wait was not observed"
    );
    assert!(
        elapsed < PROMPT,
        "component readiness did not end the Service wait"
    );
    assert_eq!(
        (subscribed, relay_index > 0),
        (1, true),
        "component prefix is empty"
    );
    assert_eq!(owner.wait_causes().count(WIRE | SERVICE), 1);
    let readiness = h.owner.fd_readiness();
    assert_eq!(
        (
            readiness.waits - base.waits,
            readiness.any - base.any,
            readiness.wire - base.wire
        ),
        (1, 1, 1)
    );
    drop(peer);
    h.owner.close(key).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn a_ring_ends_a_quiet_subscribed_wait_and_the_next_turn_is_still_observed() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(0).unwrap();
    let peer = h.connect(key);
    let owner = OwnerWake::new().unwrap();
    settle(&mut h);
    let base = h.owner.fd_readiness();
    owner.begin_pass().unwrap();
    let mut fds = smallvec::SmallVec::<[PollFd<'_>; 64]>::new();
    let waits = subscribe(Some(&h.owner), true, &mut fds);
    let subscribed = fds.len();
    let notifier = owner.notifier();
    let ring = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        notifier.notify();
    });
    let started = Instant::now();
    owner
        .wait_for_service_observed(WAIT, fds, Vec::new(), None, |fds| {
            assert_eq!(fds.len(), subscribed);
            assert!(
                fds.iter().all(|fd| fd.revents().is_empty()),
                "component fired"
            );
            observe(&owner, waits, fds);
        })
        .unwrap();
    assert!(started.elapsed() < PROMPT, "the ring did not end the wait");
    ring.join().unwrap();
    assert_eq!(owner.wait_causes().count(RING | SERVICE), 1);
    // A quiet observation is still an observation: no probe follows it.
    assert!(h.owner.turn_connected().iter().all(Option::is_none));
    assert_eq!(turns(&h), (base.turns_observed + 1, base.turns_blind));
    let readiness = h.owner.fd_readiness();
    assert_eq!(
        (readiness.waits - base.waits, readiness.any - base.any),
        (1, 0)
    );
    drop(peer);
    h.owner.close(key).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn a_ready_relay_behind_the_component_prefix_is_other_readiness() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(0).unwrap();
    let peer = h.connect(key);
    let owner = OwnerWake::new().unwrap();
    settle(&mut h);
    let base = h.owner.fd_readiness();
    owner.begin_pass().unwrap();
    let (relay, mut writer) = relay();
    writer.write_all(b"relay").unwrap();
    let mut fds = smallvec::SmallVec::<[PollFd<'_>; 64]>::new();
    let waits = subscribe(Some(&h.owner), true, &mut fds);
    let relay_index = fds.len();
    fds.push(PollFd::new(&relay, PollFlags::IN));
    owner
        .wait_for_service_observed(WAIT, fds, Vec::new(), None, |fds| {
            assert!(
                !fds[relay_index].revents().is_empty(),
                "ready relay did not fire"
            );
            observe(&owner, waits, fds);
        })
        .unwrap();
    // The relay's readiness is outside the component slice: another fd, not a wire.
    assert_eq!(owner.wait_causes().count(FD | SERVICE), 1);
    let readiness = h.owner.fd_readiness();
    assert_eq!(
        (
            readiness.waits - base.waits,
            readiness.any - base.any,
            readiness.wire - base.wire
        ),
        (1, 0, 0)
    );
    drop(peer);
    h.owner.close(key).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn without_a_runtime_or_components_nothing_is_subscribed() {
    let h = Harness::new();
    let mut fds = smallvec::SmallVec::<[PollFd<'_>; 64]>::new();
    assert!(subscribe(Some(&h.owner), false, &mut fds).is_none());
    assert!(subscribe(None, true, &mut fds).is_none());
    assert!(fds.is_empty());
    let owner = OwnerWake::new().unwrap();
    observe(&owner, None, &[]);
    assert_eq!(owner.wait_causes().waits(), 0);
}
