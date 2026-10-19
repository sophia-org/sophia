#![cfg(feature = "comparator-progress")]
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use sophia_9p::progress::wait_sources::{Ledger, Role};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

fn observe(ledger: &mut Ledger, sockets: &[&UnixStream], native: usize) {
    let mut fds: Vec<_> = sockets
        .iter()
        .map(|s| PollFd::new(*s, PollFlags::IN))
        .collect();
    poll(
        &mut fds,
        Some(&Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .unwrap();
    ledger.record(&fds, native, fds.len() - 1);
}
fn bit(role: Role) -> usize {
    1 << role as usize
}

#[test]
fn combinations_reconcile_without_double_counting_ready_descriptors() {
    let pairs: Vec<_> = (0..5).map(|_| UnixStream::pair().unwrap()).collect();
    let roles = [Role::WireSocket, Role::Worker, Role::Worker];
    let mut l = Ledger::default();
    l.begin();
    for (pair, role) in pairs.iter().zip(roles) {
        l.register(pair.0.as_raw_fd(), role, false);
    }
    for pair in &pairs {
        (&pair.1).write_all(b"x").unwrap();
    }
    let sockets: Vec<_> = pairs.iter().map(|p| &p.0).collect();
    observe(&mut l, &sockets, 3);
    let mask = bit(Role::WireSocket) | bit(Role::Worker) | bit(Role::Native) | bit(Role::Ring);
    assert_eq!(l.masks[mask], 1);
    assert_eq!(l.input[Role::Worker as usize], 1);
    assert_eq!(l.masks.iter().sum::<u64>(), l.waits);
    assert_eq!(l.input[Role::Native as usize], 1);
    assert_eq!(l.input[Role::Ring as usize], 1);
}
#[test]
fn timeout_and_hup_remain_distinct() {
    let (a, b) = UnixStream::pair().unwrap();
    let (ring, _writer) = UnixStream::pair().unwrap();
    let mut l = Ledger::default();
    l.begin();
    l.register(a.as_raw_fd(), Role::ImageSocket, false);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.masks[0], 1);
    drop(b);
    l.begin();
    l.register(a.as_raw_fd(), Role::ImageSocket, false);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.masks[bit(Role::ImageSocket)], 1);
    assert_eq!(l.fault[Role::ImageSocket as usize], 1);
}
#[test]
fn registration_expires_and_replacement_must_be_classified_afresh() {
    let (a, mut b) = UnixStream::pair().unwrap();
    let (ring, _writer) = UnixStream::pair().unwrap();
    b.write_all(b"x").unwrap();
    let mut l = Ledger::default();
    l.begin();
    l.register(a.as_raw_fd(), Role::ImageListener, false);
    observe(&mut l, &[&a, &ring], 1);
    // Same numeric descriptor cannot carry its old role into a new wait.
    l.begin();
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.masks[bit(Role::Unknown)], 1);
    l.begin();
    l.register(a.as_raw_fd(), Role::Worker, false);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.masks[bit(Role::Worker)], 1);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.missing_scope, 1);
}
#[test]
fn conflicts_and_overflow_are_visible_and_fallback_cannot_hide_them() {
    let (a, mut b) = UnixStream::pair().unwrap();
    let (ring, _writer) = UnixStream::pair().unwrap();
    b.write_all(b"x").unwrap();
    let mut l = Ledger::default();
    l.begin();
    l.register(a.as_raw_fd(), Role::Worker, false);
    l.register(a.as_raw_fd(), Role::TransportOther, true);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.masks[bit(Role::Worker)], 1);
    l.begin();
    l.register(a.as_raw_fd(), Role::Worker, false);
    l.register(a.as_raw_fd(), Role::Relay, false);
    l.register(a.as_raw_fd(), Role::TransportOther, true);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.conflicts, 1);
    assert_eq!(l.masks[bit(Role::Unknown)], 1);
    l.begin();
    for fd in -256..0 {
        l.register(fd, Role::Relay, false);
    }
    l.register(a.as_raw_fd(), Role::Worker, false);
    observe(&mut l, &[&a, &ring], 1);
    assert_eq!(l.overflow, 1);
    assert_eq!(l.masks[bit(Role::Unknown)], 2);
}
#[test]
fn output_readiness_has_its_own_marginal() {
    let (a, _b) = UnixStream::pair().unwrap();
    let (ring, _writer) = UnixStream::pair().unwrap();
    let mut l = Ledger::default();
    l.begin();
    l.register(a.as_raw_fd(), Role::WireSocket, false);
    let mut fds = [
        PollFd::new(&a, PollFlags::OUT),
        PollFd::new(&ring, PollFlags::IN),
    ];
    poll(
        &mut fds,
        Some(&Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .unwrap();
    l.record(&fds, 1, 1);
    assert_eq!(l.output[Role::WireSocket as usize], 1);
    assert_eq!(l.input[Role::WireSocket as usize], 0);
    assert!(
        l.to_string()
            .contains(&format!("mask_{}=1", bit(Role::WireSocket)))
    );
}

#[test]
fn worst_case_record_fits_capture_line_bound() {
    let mut l = Ledger::default();
    l.masks.fill(u64::MAX);
    l.input.fill(u64::MAX);
    l.output.fill(u64::MAX);
    l.fault.fill(u64::MAX);
    l.waits = u64::MAX;
    l.overflow = u64::MAX;
    l.conflicts = u64::MAX;
    l.missing_scope = u64::MAX;
    assert!(l.to_string().len() + 200 < 65_536);
}
