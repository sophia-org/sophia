//! Independent client of Sophia's device-free, supplied-admission lock export.
use sophia_9p_client::pipeline::{IoInterest, PipelineLimits};
use sophia_lock_client::{journal::Event, protocol::lock_files::*, *};
use std::{
    io::Read,
    os::{fd::BorrowedFd, unix::net::UnixStream},
    time::{Duration, Instant},
};

struct Clock(Instant);
impl Clock {
    fn now(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn wait(&self, fd: BorrowedFd<'_>, interest: IoInterest, wake: Option<u64>) {
        let now = self.now();
        assert!(now < 10000, "fixture deadline");
        if interest.immediate {
            return;
        }
        let mut flags = rustix::event::PollFlags::empty();
        if interest.readable {
            flags |= rustix::event::PollFlags::IN;
        }
        if interest.writable {
            flags |= rustix::event::PollFlags::OUT;
        }
        let timeout = rustix::event::Timespec::try_from(Duration::from_millis(
            wake.unwrap_or(10000).min(10000).saturating_sub(now),
        ))
        .unwrap();
        let mut fds = [rustix::event::PollFd::new(&fd, flags)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => (),
            Err(e) => panic!("poll {e}"),
        }
    }
    fn turn(&self, s: &mut Session) {
        self.wait(s.fd().unwrap(), s.io_interest().unwrap(), s.wake_deadline());
        s.dispatch(self.now(), 65536).unwrap();
    }
    fn event(&self, s: &mut Session) -> Event {
        loop {
            if let Some((_, e)) = s.event() {
                return *e;
            }
            self.turn(s);
        }
    }
    fn admit(&self, s: &mut Session, mut f: impl FnMut(&mut Session) -> Result<(), SessionError>) {
        loop {
            match f(s) {
                Ok(()) => return,
                Err(
                    SessionError::Submission(submission::Error::Busy)
                    | SessionError::Upload(upload::Error::Busy),
                ) => self.turn(s),
                Err(e) => panic!("admission: {e:?}"),
            }
        }
    }
}
fn main() {
    let path = std::env::args_os().nth(1).expect("socket path");
    let mut gate = [0];
    std::io::stdin().read_exact(&mut gate).unwrap();
    assert_eq!(gate, [b'G']);
    let clock = Clock(Instant::now());
    let mut c = Connecting::direct(
        UnixStream::connect(path).unwrap(),
        PipelineLimits {
            msize: 8192,
            max_outstanding: 8,
            ..PipelineLimits::default()
        },
        LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: 3,
            chords: vec![LockChordRequest {
                keysym: 0x62,
                modifiers: 0b0100,
            }],
        },
        clock.now(),
        10000,
    )
    .unwrap();
    while !c.ready() {
        clock.wait(c.fd().unwrap(), c.io_interest().unwrap(), c.wake_deadline());
        c.dispatch(clock.now(), 65536).unwrap();
    }
    let mut s = c.take_session().unwrap();
    assert!(matches!(clock.event(&mut s), Event::ObjectPublished(_)));
    let (generation, lock) = s.lock().unwrap();
    assert_eq!(lock.phase, LockPhase::Locked);
    assert_eq!(lock.lock_epoch, 3);
    assert_eq!(lock.allocations.len(), 1);
    assert_eq!(lock.allocations[0].pixel_width, 2);
    assert_eq!(s.limits().upload_slots, 1);
    assert_eq!(s.welcome().unwrap().granted_chords, 1);
    s.consume().unwrap();
    let begin = LockResourceBegin {
        transaction: 1,
        resource: LockResourceId {
            id: 1,
            generation: 1,
        },
        width_px: 2,
        height_px: 2,
        slot: 0,
    };
    clock.admit(&mut s, |s| s.upload_begin(begin));
    assert!(
        matches!(clock.event(&mut s),Event::ResourceStatus(v) if v.status==LockResourceState::Admitted)
    );
    s.consume().unwrap();
    for size in [6, 10] {
        while !s.upload_ready() {
            clock.turn(&mut s);
        }
        s.upload_chunk(vec![0x7f; size]).unwrap();
        while !s.upload_ready() {
            clock.turn(&mut s);
        }
        assert_eq!(s.take_upload_chunk().unwrap(), vec![0x7f; size]);
    }
    clock.admit(&mut s, |s| s.upload_end());
    assert!(
        matches!(clock.event(&mut s),Event::ResourceStatus(v) if v.status==LockResourceState::Accepted)
    );
    s.consume().unwrap();
    while s.upload_pending() {
        clock.turn(&mut s);
    }
    let demand = Candidate::FrameDemand(LockFrameDemand {
        transaction: 3,
        lock_epoch: 3,
        allocation_id: 1,
        allocation_generation: 1,
        demand_id: 1,
    });
    clock.admit(&mut s, |s| s.submit(&demand).map(|_| ()));
    let Event::FramePermit(permit) = clock.event(&mut s) else {
        panic!("expected permit")
    };
    s.consume().unwrap();
    let candidate = Candidate::Present(LockCandidate {
        transaction: 4,
        lock_epoch: 3,
        output_id: 1,
        output_generation: 1,
        allocation_id: 1,
        allocation_generation: 1,
        candidate_generation: 1,
        pacing_permit: permit.pacing_permit,
        resource: begin.resource,
    });
    clock.admit(&mut s, |s| s.submit(&candidate).map(|_| ()));
    assert!(
        matches!(clock.event(&mut s),Event::CandidateOutcome(v) if v.status==LockCandidateStatus::Presented)
    );
    s.consume().unwrap();
    assert!(
        matches!(clock.event(&mut s),Event::Entry(v) if v.entry==LockEntryKind::Insert && !v.empty_after)
    );
    s.consume().unwrap();
    assert!(matches!(clock.event(&mut s),Event::Chord(v) if v.chord==0));
    s.consume().unwrap();
    assert!(matches!(clock.event(&mut s), Event::ObjectPublished(_)));
    let (new_generation, lock) = s.lock().unwrap();
    assert!(new_generation > generation);
    assert_eq!(lock.phase, LockPhase::Unlocking);
    assert!(lock.allocations.is_empty());
    s.consume().unwrap();
    while s.obligations().acked < s.obligations().consumed {
        clock.turn(&mut s);
    }
    s.close();
    println!("lock_client_rust_sdk status=done");
}
