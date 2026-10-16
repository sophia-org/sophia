//! Real Unix readiness without a live authentication provider or device.
use rustix::event::{PollFd, Timespec, poll};
use sophia_9p::unix::Observation;
use sophia_factotum::launch::{
    queue,
    relay::{Relay, RelayServer},
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn observe(relay: &RelayServer) -> Observation {
    let subscription = relay.subscription().unwrap();
    let mut fds: Vec<PollFd<'_>> = Vec::new();
    relay.visit_poll_fds(|fd| fds.push(fd));
    poll(&mut fds, Some(&Timespec::default())).unwrap();
    subscription.observe(&fds).unwrap()
}
fn pair() -> (RelayServer, UnixStream) {
    let (local, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (ingress, _owner) = queue::channel();
    (Relay::over(local, 1, ingress).unwrap(), peer)
}

#[test]
fn ten_thousand_idle_observations_do_not_turn_or_flush_the_core() {
    let (mut relay, _peer) = pair();
    let original = observe(&relay);
    for _ in 0..10_000 {
        let idle = observe(&relay);
        assert!(!idle.is_ready());
        assert!(relay.turn_observed(idle, Instant::now()).unwrap());
    }
    // Either core operation would invalidate this exact service generation.
    assert!(relay.observation_is_current(&original));
}

#[test]
fn late_input_is_consumed_by_the_next_observation() {
    let (mut relay, mut peer) = pair();
    let idle = observe(&relay);
    let mut version = vec![21, 0, 0, 0, 100, 255, 255];
    version.extend(65536_u32.to_le_bytes());
    version.extend([8, 0]);
    version.extend(b"9P2000.L");
    peer.write_all(&version).unwrap();
    assert!(relay.turn_observed(idle, Instant::now()).unwrap());
    let ready = observe(&relay);
    assert!(ready.is_ready());
    assert!(relay.turn_observed(ready, Instant::now()).unwrap());
    let mut reply = [0; 21];
    peer.read_exact(&mut reply).unwrap();
    assert_eq!(reply[4], 101);
}

#[test]
fn activation_wakes_and_revocation_cannot_hide_behind_idle_readiness() {
    let (mut relay, _peer) = pair();
    let idle = observe(&relay);
    relay.export_mut().activate(1).unwrap();
    assert!(relay.turn_observed(idle, Instant::now()).unwrap());
    let ready = observe(&relay);
    assert!(ready.is_ready());
    assert!(relay.turn_observed(ready, Instant::now()).unwrap());
    let idle = observe(&relay);
    relay.export_mut().revoke();
    assert!(!relay.turn_observed(idle, Instant::now()).unwrap());
}

#[test]
fn foreign_observation_and_peer_close_use_current_connection() {
    let (mut relay, peer) = pair();
    let (other, _other_peer) = pair();
    let foreign = observe(&other);
    assert!(!relay.observation_is_current(&foreign));
    drop(peer);
    assert!(!relay.turn_observed(foreign, Instant::now()).unwrap());
}
