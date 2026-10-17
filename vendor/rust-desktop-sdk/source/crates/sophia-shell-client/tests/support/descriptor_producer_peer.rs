//! Test-side producer policy. No public SDK or authenticated 9P transport.
use super::*;
use rustix::event::{PollFd, PollFlags, Timespec, poll};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Custody {
    Unsent,
    AwaitingAdmission,
    Admitted,
    ExpiredUnsent,
    Sent,
    Unknown,
    Released,
}
struct Producer {
    epoch: u64,
    generation: u64,
    ready: bool,
    disabled: bool,
    id: u64,
    next: Option<u64>,
    custody: Custody,
    writable: bool,
    releases: usize,
    now: u64,
    deadline: Option<u64>,
}
impl Producer {
    fn new() -> Self {
        Self {
            epoch: 1,
            generation: 2,
            ready: false,
            disabled: false,
            id: 0,
            next: Some(1),
            custody: Custody::Unsent,
            writable: false,
            releases: 0,
            now: 0,
            deadline: None,
        }
    }
    fn ready(&mut self, epoch: u64, generation: u64) {
        if (epoch, generation) == (self.epoch, self.generation) && !self.disabled {
            self.ready = true;
        }
    }
    fn begin(&mut self) -> Option<u64> {
        if !self.ready
            || self.disabled
            || !matches!(self.custody, Custody::Unsent | Custody::Released)
        {
            return None;
        }
        self.id = self.next?;
        self.next = self.id.checked_add(1);
        self.custody = Custody::AwaitingAdmission;
        Some(self.id)
    }
    fn admitted(&mut self, epoch: u64, generation: u64, id: u64) {
        if (epoch, generation, id) == (self.epoch, self.generation, self.id)
            && self.custody == Custody::AwaitingAdmission
        {
            self.custody = Custody::Admitted;
            self.deadline = Some(self.now.checked_add(2000).unwrap());
        }
    }
    fn timeout(&self) -> Option<u64> {
        self.deadline.map(|at| at.saturating_sub(self.now))
    }
    fn advance(&mut self, now: u64) {
        assert!(now >= self.now);
        self.now = now;
        if self.deadline.is_some_and(|at| now >= at) {
            self.deadline = None;
            self.writable = false;
            if self.custody == Custody::Admitted {
                // Stop sending; only a journal release settles admitted custody.
                self.custody = Custody::ExpiredUnsent;
            }
        }
    }
    fn pump(&mut self, tx: &OwnedFd, fd: &OwnedFd, role: u16, writable: bool) -> bool {
        if self.disabled || self.custody != Custody::Admitted || (self.writable && !writable) {
            return false;
        }
        match try_send(tx, &packet(role, self.id), fd) {
            Ok(200) => {
                self.custody = Custody::Sent;
                self.writable = false;
                self.deadline = None;
                true
            }
            Err(io::Errno::AGAIN) => {
                self.writable = true;
                false
            }
            // Even an unexpected short send cannot authorize a replay.
            _ => {
                self.custody = Custody::Unknown;
                self.writable = false;
                self.deadline = None;
                false
            }
        }
    }
    fn closed(&mut self, epoch: u64, generation: u64) {
        if (epoch, generation) != (self.epoch, self.generation) {
            return;
        }
        self.ready = false;
        self.disabled = true;
        self.writable = false;
        self.deadline = None;
        // A channel event is not a release, even for an unsent admitted offer.
    }
    fn fallback(&self, caller_permits: bool) -> bool {
        self.disabled && caller_permits
    }
    fn eof(&mut self) {
        self.closed(self.epoch, self.generation);
        if matches!(
            self.custody,
            Custody::Sent | Custody::Admitted | Custody::ExpiredUnsent | Custody::AwaitingAdmission
        ) {
            self.custody = Custody::Unknown;
        }
    }
    fn release(&mut self, b: &[u8]) {
        if b.len() != 64 {
            return;
        }
        let u16at = |at| u16::from_le_bytes(b[at..at + 2].try_into().unwrap());
        let u64at = |at| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
        if u32::from_le_bytes(b[..4].try_into().unwrap()) != 64
            || u16at(4) != 1
            || u16at(6) != 0x7002
            || u64at(8) != self.epoch
            || u64at(16) != 0
            || u64at(24) == 0
            || u64at(32) != self.id
            || u64at(40) != 4
            || u64at(48) != 5
            || !(1..=5).contains(&u32::from_le_bytes(b[56..60].try_into().unwrap()))
            || b[60..].iter().any(|v| *v != 0)
        {
            return;
        }
        if matches!(
            self.custody,
            Custody::Admitted | Custody::ExpiredUnsent | Custody::Sent | Custody::Unknown
        ) {
            self.custody = Custody::Released;
            self.deadline = None;
            self.writable = false;
            self.releases += 1;
        }
    }
}
fn release_record(id: u64, disposition: u32) -> [u8; 64] {
    let mut b = [0; 64];
    b[..4].copy_from_slice(&64u32.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&0x7002u16.to_le_bytes());
    for (at, value) in [(8, 1u64), (24, 2), (32, id), (40, 4), (48, 5)] {
        b[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    b[56..60].copy_from_slice(&disposition.to_le_bytes());
    b
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Discovery {
    Found,
    Missing,
    Denied,
    Stale,
    Malformed,
}
fn bytes_allowed(result: Discovery, caller_permits: bool) -> bool {
    result == Discovery::Missing && caller_permits
}
struct SnapshotReader {
    last_pass: Option<u64>,
    deadline: u64,
    reads: usize,
    hello: bool,
    failed: bool,
}
impl SnapshotReader {
    fn drive(&mut self, pass: u64, now: u64, generations: [u64; 3]) {
        if self.failed || self.hello {
            return;
        }
        if now >= self.deadline {
            self.failed = true;
            return;
        }
        if self.last_pass == Some(pass) {
            return;
        }
        self.last_pass = Some(pass);
        self.reads += 1;
        self.hello = generations[0] != 0 && generations.iter().all(|g| *g == generations[0]);
    }
}

fn hello(role: u16, epoch: u64, generation: u64) -> [u8; 40] {
    let mut b = [0; 40];
    b[..4].copy_from_slice(&40u32.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&1u16.to_le_bytes());
    b[8..10].copy_from_slice(&role.to_le_bytes());
    b[16..24].copy_from_slice(&epoch.to_le_bytes());
    b[24..32].copy_from_slice(&generation.to_le_bytes());
    b
}
fn join(owner: &mut Owner, bytes: &[u8], expected_assignee: bool) -> bool {
    // Authentication is an explicit fixture input, never inferred from a pathname.
    if !expected_assignee
        || owner.ready
        || owner.disabled
        || !owner.live_journal
        || bytes != hello(owner.role, 1, 2)
    {
        return false;
    }
    owner.ready = true;
    true
}
fn discovery_and_readiness() {
    for result in [
        Discovery::Found,
        Discovery::Missing,
        Discovery::Denied,
        Discovery::Stale,
        Discovery::Malformed,
    ] {
        assert!(!bytes_allowed(result, false));
        assert_eq!(bytes_allowed(result, true), result == Discovery::Missing);
    }
    let mut reader = SnapshotReader {
        last_pass: None,
        deadline: 10,
        reads: 0,
        hello: false,
        failed: false,
    };
    for pass in 1..=2 {
        reader.drive(pass, pass, [2, 3, 2]);
        reader.drive(pass, pass, [3; 3]);
        assert!(!reader.hello);
        assert_eq!(reader.reads, pass as usize);
    }
    reader.drive(3, 3, [3; 3]);
    assert!(reader.hello);
    let mut reader = SnapshotReader {
        last_pass: None,
        deadline: 3,
        reads: 0,
        hello: false,
        failed: false,
    };
    for pass in 1..=4 {
        reader.drive(pass, pass, [2, 3, 2]);
    }
    assert!(reader.failed && !reader.hello);
    assert_eq!(reader.reads, 2);
    // The opened snapshot owns its bytes across publication replacement.
    let mut published = vec![1; 100];
    let opened = published.clone();
    let prefix = opened[..50].to_vec();
    published.fill(2);
    assert!(
        prefix
            .into_iter()
            .chain(opened[50..].iter().copied())
            .all(|b| b == 1)
    );

    for role in [1, 2] {
        let mut owner = Owner::new(role);
        assert!(!join(&mut owner, &hello(role, 1, 2), false));
        assert!(!join(&mut owner, &hello(role, 9, 2), true));
        assert!(join(&mut owner, &hello(role, 1, 2), true));
        assert!(!join(&mut owner, &hello(role, 1, 2), true));
        assert!(!join(&mut owner, &hello(role, 1, 3), true));
        assert!(owner.ready);
        assert!(owner.events.is_empty());
        assert!(!owner.disabled);
        assert_eq!(owner.high, 0);
        let mut p = Producer::new();
        assert_eq!(p.begin(), None);
        p.ready(1, 1);
        assert_eq!(p.begin(), None);
        p.ready(1, 2);
        assert_eq!(p.begin(), Some(1));
        p.closed(1, 1);
        assert!(p.ready);
        p.closed(1, 2);
        assert!(!p.ready);
        assert!(!p.fallback(false));
        assert!(p.fallback(true));
        p.ready(1, 2);
        assert!(!p.ready); // Disabled for this epoch.
        p.eof();
        assert_eq!(p.custody, Custody::Unknown);
        assert_eq!(p.releases, 0);
    }
}

fn send_and_release() {
    for role in [1, 2] {
        let (tx, rx) = sockets();
        let fd = source();
        let mut p = Producer::new();
        p.ready(1, 2);
        assert_eq!(p.begin(), Some(1));
        assert!(!p.pump(&tx, &fd, role, true));
        let mut empty = [0; 200];
        assert_eq!(
            recv(&rx, &mut empty, RecvFlags::DONTWAIT),
            Err(io::Errno::AGAIN)
        );
        p.admitted(1, 1, 1);
        assert!(!p.pump(&tx, &fd, role, true));
        p.admitted(1, 2, 1);
        // Fill transport with descriptor-free packets, so only socket capacity
        // (not the process-wide in-flight rights limit) controls this EAGAIN.
        sockopt::set_socket_send_buffer_size(&tx, 4096).unwrap();
        let mut filled = 0;
        loop {
            match rustix::net::send(&tx, &[0; 200], SendFlags::NOSIGNAL) {
                Ok(200) => {
                    filled += 1;
                    assert!(filled < 1024);
                }
                Err(io::Errno::AGAIN) => break,
                other => panic!("unexpected filler result: {other:?}"),
            }
        }
        assert!(!p.pump(&tx, &fd, role, true));
        assert!(p.writable);
        assert_eq!(p.custody, Custody::Admitted);
        assert!(!p.pump(&tx, &fd, role, false));
        let mut stalled = Producer::new();
        stalled.ready(1, 2);
        stalled.advance(100);
        assert_eq!(stalled.begin(), Some(1));
        stalled.admitted(1, 2, 1);
        assert!(!stalled.pump(&tx, &fd, role, true));
        assert!(stalled.writable);
        assert_eq!(stalled.timeout(), Some(2000));
        stalled.advance(2099);
        assert_eq!(stalled.timeout(), Some(1));
        stalled.advance(2100);
        assert_eq!(stalled.custody, Custody::ExpiredUnsent);
        assert!(!stalled.writable);
        assert_eq!(stalled.timeout(), None);
        assert_eq!(stalled.releases, 0);
        for _ in 0..filled {
            assert_eq!(recv(&rx, &mut empty, RecvFlags::DONTWAIT).unwrap().0, 200);
        }
        let mut pollfds = [PollFd::new(&tx, PollFlags::OUT)];
        assert_eq!(
            poll(
                &mut pollfds,
                Some(&Timespec {
                    tv_sec: 0,
                    tv_nsec: 0
                })
            )
            .unwrap(),
            1
        );
        assert!(p.pump(
            &tx,
            &fd,
            role,
            pollfds[0].revents().contains(PollFlags::OUT)
        ));
        assert_eq!(p.custody, Custody::Sent);
        assert!(!stalled.pump(&tx, &fd, role, true));
        stalled.release(&release_record(1, 3));
        assert_eq!(stalled.custody, Custody::Released);
        assert!(!p.pump(&tx, &fd, role, true));
        let mut owner = Owner::new(role);
        owner.ready = true;
        assert!(owner.admit(1));
        owner.receive(&rx);
        owner.start_worker(1);
        owner.worker_returns(1);
        assert_eq!(p.custody, Custody::Sent); // Owner success is not producer observation.
        let release = release_record(1, 1);
        p.release(&release_record(2, 1));
        assert_eq!(p.releases, 0);
        p.release(&release);
        p.release(&release);
        assert_eq!(p.releases, 1);
        assert_eq!(p.custody, Custody::Released);
        assert_eq!(io::pwrite(&fd, b"X", 0), Err(io::Errno::PERM));
        assert_eq!(p.begin(), Some(2));
        p.admitted(1, 2, 2);
        assert!(p.pump(&tx, &fd, role, true));
        p.custody = Custody::Unknown; // Test seam: transport result was lost to the caller.
        assert!(!p.pump(&tx, &fd, role, true));
        assert!(owner.admit(2));
        owner.receive(&rx);
        owner.cancel(2);
        p.release(&release_record(2, 4));
        assert_eq!(p.custody, Custody::Released);
        p.next = Some(u64::MAX);
        assert_eq!(p.begin(), Some(u64::MAX));
        p.admitted(1, 2, u64::MAX);
        p.release(&release_record(u64::MAX, 3));
        assert_eq!(p.begin(), None);

        let (tx, rx) = sockets();
        drop(rx); // Exercise the send error branch, not just injected uncertainty.
        let mut p = Producer::new();
        p.ready(1, 2);
        assert_eq!(p.begin(), Some(1));
        p.admitted(1, 2, 1);
        assert!(!p.pump(&tx, &fd, role, true));
        assert_eq!(p.custody, Custody::Unknown);
        assert!(!p.pump(&tx, &fd, role, true));
        p.eof();
        assert_eq!(p.releases, 0);
    }
}

pub(super) fn exercise() {
    discovery_and_readiness();
    send_and_release();
}
