use super::*;
#[path = "mod.rs"]
mod support;
use support::StaticExport;

fn server() -> (Server<StaticExport>, UnixStream) {
    let (local, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut server = Server::new(
        StaticExport::new(),
        Limits::new(65536, 512, 16, 32, 131072, 2).unwrap(),
    )
    .unwrap();
    server.adopt(local).map_err(|e| e.error).unwrap();
    (server, peer)
}

fn observe(server: &Server<StaticExport>) -> Observation {
    let subscription = server.subscription().unwrap();
    let mut fds = server.poll_fds();
    poll(&mut fds, Some(&Timespec::default())).unwrap();
    subscription.observe(&fds).unwrap()
}

#[test]
fn observed_turn_avoids_poll_and_late_input_stays_ready() {
    let (mut server, mut peer) = server();
    let observed = observe(&server);
    peer.write_all(&support::frame(
        100,
        support::NOTAG,
        support::Body::default().u32(65536).string(b"9P2000.L"),
    ))
    .unwrap();
    assert!(server.turn_observed(observed).unwrap());
    assert_eq!(server.polls.get(), 0);
    let ready = observe(&server);
    assert!(ready.flags[1].contains(PollFlags::IN));
    assert!(server.turn_observed(ready).unwrap());
    assert_eq!(server.polls.get(), 0);
    let mut reply = [0; 7];
    peer.read_exact(&mut reply).unwrap();
    assert_eq!(reply[4], 101);
}

#[test]
fn the_wake_reader_is_visited_first_and_once() {
    let (server, _peer) = server();
    let reader = server.reader.as_raw_fd();
    let mut visited = Vec::new();
    server.visit_poll_fds(|fd| visited.push(fd.as_fd().as_raw_fd()));
    assert!(visited.len() > 1, "{visited:?}");
    assert_eq!(visited[0], reader);
    assert_eq!(visited.iter().filter(|&&fd| fd == reader).count(), 1);
}

#[test]
fn intervening_flush_or_turn_forces_real_poll() {
    let (mut server, _peer) = server();
    let stale = observe(&server);
    server.flush().unwrap();
    server.turn_observed(stale).unwrap();
    assert_eq!(server.polls.get(), 1);
    let stale = observe(&server);
    server.turn(Some(Duration::ZERO)).unwrap();
    server.turn_observed(stale).unwrap();
    assert_eq!(server.polls.get(), 3);
}

#[test]
fn wrong_server_and_changed_interests_fall_back() {
    let (mut first, _peer) = server();
    let (second, _other) = server();
    first.turn_observed(observe(&second)).unwrap();
    assert_eq!(first.polls.get(), 1);
    let mut changed = observe(&first);
    changed.subscription.interests[1] = PollFlags::OUT;
    first.turn_observed(changed).unwrap();
    assert_eq!(first.polls.get(), 2);
}

#[test]
fn late_stop_is_level_ready_on_next_pass() {
    let (mut server, _peer) = server();
    let observed = observe(&server);
    server.wake().stop();
    assert!(server.turn_observed(observed).unwrap());
    let observed = observe(&server);
    assert!(observed.flags[0].contains(PollFlags::IN));
    assert!(!server.turn_observed(observed).unwrap());
    assert_eq!(server.connection_count(), 0);
    assert_eq!(server.polls.get(), 0);
}

#[test]
fn wrong_descriptors_and_multiple_sockets_are_not_eligible() {
    let (mut first, _peer) = server();
    let (second, _other) = server();
    assert!(
        first
            .subscription()
            .unwrap()
            .observe(&second.poll_fds())
            .is_none()
    );
    let old = observe(&first);
    let (local, _third) = UnixStream::pair().unwrap();
    first.adopt(local).map_err(|e| e.error).unwrap();
    assert!(first.subscription().is_none());
    first.turn_observed(old).unwrap();
    assert_eq!(first.polls.get(), 1);
}

#[test]
fn connection_identity_and_generation_exhaustion_fail_closed() {
    let (mut server, _peer) = server();
    let mut stale = observe(&server);
    stale.subscription.connection = ConnectionId(0);
    server.turn_observed(stale).unwrap();
    assert_eq!(server.polls.get(), 1);
    server.generation = Some(u64::MAX);
    let last = observe(&server);
    server.flush().unwrap();
    assert!(server.subscription().is_none());
    server.turn_observed(last).unwrap();
    assert_eq!(server.polls.get(), 2);
}

#[test]
fn descriptor_numbers_cannot_substitute_for_server_identity() {
    let (first, peer) = server();
    let mut stale = observe(&first);
    drop((first, peer));
    let (mut replacement, _peer) = server();
    let current = replacement.subscription().unwrap();
    // Model exact descriptor reuse, even when parallel tests allocate fds.
    stale.subscription.descriptors = current.descriptors;
    stale.subscription.generation = current.generation;
    stale.subscription.connection = current.connection;
    replacement.turn_observed(stale).unwrap();
    assert_eq!(replacement.polls.get(), 1);
}

fn tversion() -> Vec<u8> {
    support::frame(
        100,
        support::NOTAG,
        support::Body::default().u32(65536).string(b"9P2000.L"),
    )
}

/// The wake pipe holds nothing: every wakeup written so far was drained.
fn wake_pipe_is_empty(server: &Server<StaticExport>) -> bool {
    rustix::io::read(&server.reader, &mut [0; 8]) == Err(rustix::io::Errno::AGAIN)
}

/// An interrupted wait returns at once with nothing ready and nothing closed;
/// the socket and the wake pipe then make ordinary progress.
fn interrupted_wait_then_progress(mut server: Server<StaticExport>, mut peer: UnixStream) {
    let connections = server.connection_count();
    server.interrupts.set(1);
    let started = std::time::Instant::now();
    assert!(server.turn(None).unwrap());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(server.interrupts.get(), 0);
    assert_eq!(server.polls.get(), 1);
    assert_eq!(server.connection_count(), connections);
    peer.write_all(&tversion()).unwrap();
    assert!(server.turn(Some(Duration::from_secs(2))).unwrap());
    let mut reply = [0; 7];
    peer.read_exact(&mut reply).unwrap();
    assert_eq!(reply[4], 101);
    server.wake().wake();
    let mut pending = [PollFd::new(&server.reader, PollFlags::IN)];
    poll(&mut pending, Some(&Timespec::default())).unwrap();
    assert!(pending[0].revents().contains(PollFlags::IN));
    assert!(server.turn(Some(Duration::ZERO)).unwrap());
    assert!(wake_pipe_is_empty(&server));
    assert_eq!(server.polls.get(), 3);
}

#[test]
fn an_interrupted_single_socket_wait_yields_nothing_and_progress_follows() {
    let (server, peer) = server();
    interrupted_wait_then_progress(server, peer);
}

#[test]
fn an_interrupted_multi_socket_wait_yields_nothing_and_progress_follows() {
    let (mut server, peer) = server();
    let (local, mut other) = UnixStream::pair().unwrap();
    other
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    server.adopt(local).map_err(|e| e.error).unwrap();
    assert_eq!(server.connection_count(), 2);
    other.write_all(&tversion()).unwrap();
    interrupted_wait_then_progress(server, peer);
    let mut reply = [0; 7];
    other.read_exact(&mut reply).unwrap();
    assert_eq!(reply[4], 101);
}

#[test]
fn a_wake_retries_interrupted_writes_and_still_coalesces_when_full() {
    let (server, _peer) = server();
    let wake = server.wake();
    wake.inner.interrupts.store(2, Ordering::SeqCst);
    wake.wake();
    assert_eq!(wake.inner.interrupts.load(Ordering::SeqCst), 0);
    assert_eq!(rustix::io::read(&server.reader, &mut [0; 8]), Ok(1));
    assert!(wake_pipe_is_empty(&server));
    while rustix::io::write(&wake.inner.writer, &[1; 4096]).is_ok() {}
    wake.inner.interrupts.store(1, Ordering::SeqCst);
    wake.wake();
    assert_eq!(wake.inner.interrupts.load(Ordering::SeqCst), 0);
    server.drain_wake().unwrap();
    assert!(wake_pipe_is_empty(&server));
}
