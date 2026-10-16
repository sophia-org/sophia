#![cfg(feature = "comparator-progress")]
mod support;
use sophia_9p::progress::{self, Counter};
use support::*;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn counters_do_not_allocate_and_count_actual_reply_and_retry_work() {
    let before = ALLOCATOR.stats();
    let start = progress::snapshot();
    for _ in 0..1000 {
        progress::add(Counter::OwnerPass, 1);
    }
    let end = progress::snapshot();
    let after = ALLOCATOR.stats();
    assert_eq!(
        end[Counter::OwnerPass as usize] - start[Counter::OwnerPass as usize],
        1000
    );
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.reallocations, before.reallocations);

    let mut h = Harness::attached();
    h.open(1, &[b"events"], 0);
    let start = progress::snapshot();
    assert!(h.send(&tread(9, 1, 0, 64)).unwrap().is_empty());
    h.connection.retry_waiting(&mut h.export).unwrap();
    let pending = progress::snapshot();
    assert_eq!(
        pending[Counter::RepliesEncoded as usize],
        start[Counter::RepliesEncoded as usize]
    );
    assert_eq!(
        pending[Counter::PendingReadAttempt as usize] - start[Counter::PendingReadAttempt as usize],
        1
    );
    h.export.push_event(b"ready");
    h.connection.retry_waiting(&mut h.export).unwrap();
    assert_eq!(h.take()[0].data(), b"ready");
    let end = progress::snapshot();
    assert_eq!(
        end[Counter::RepliesEncoded as usize] - start[Counter::RepliesEncoded as usize],
        1
    );
    assert_eq!(
        end[Counter::PendingReadAttempt as usize] - start[Counter::PendingReadAttempt as usize],
        2
    );

    let (local, mut peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let mut server =
        sophia_9p::unix::Server::new(StaticExport::new(), sophia_9p::Limits::default()).unwrap();
    server.adopt(local).map_err(|e| e.error).unwrap();
    let observe = |server: &sophia_9p::unix::Server<StaticExport>| {
        let subscription = server.subscription().unwrap();
        let mut fds = server.poll_fds();
        rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap();
        subscription.observe(&fds).unwrap()
    };
    use std::io::Write;
    let request = tversion(NOTAG, 8192, b"9P2000.L");
    peer.write_all(&request).unwrap();
    let start = progress::snapshot();
    server.turn_observed(observe(&server)).unwrap();
    let stale = observe(&server);
    server.flush().unwrap();
    server.turn_observed(stale).unwrap();
    let end = progress::snapshot();
    for (counter, expected) in [
        (Counter::HandoffAccepted, 1),
        (Counter::HandoffFallback, 1),
        (Counter::ServerPollTurn, 1),
        (Counter::ServerFlush, 1),
        (Counter::ReadBytes, request.len() as u64),
        (Counter::WrittenBytes, 21),
    ] {
        assert_eq!(end[counter as usize] - start[counter as usize], expected);
    }
}
