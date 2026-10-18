//! Independent client of Sophia's device-free, supplied-admission lock export.
use sophia_9p_client::pipeline::{IoInterest, Pipeline, PipelineLimits, Reply};
use sophia_image_producer::{
    ChannelState, OfferState, ProducerMetadata, ResourceStatusKind, create_isb, finish_isb,
};
use sophia_lock_client::{journal::Event, protocol::lock_files::*, *};
use std::{
    io::Read,
    os::fd::BorrowedFd,
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
            wake.unwrap_or(10000).min(10000).saturating_sub(now).min(10),
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
                Err(SessionError::Submission(submission::Error::Busy)) => self.turn(s),
                Err(e) => panic!("admission: {e:?}"),
            }
        }
    }
}
fn main() {
    let path = std::env::args_os().nth(1).expect("socket path");
    if let Some(path) = std::env::var_os("SOPHIA_TEST_READY_FILE") {
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read(&path).ok().as_deref() != Some(b"G") {
            assert!(Instant::now() < deadline, "admission gate deadline");
            std::thread::sleep(Duration::from_millis(1));
        }
    } else {
        let mut gate = [0];
        std::io::stdin().read_exact(&mut gate).unwrap();
        assert_eq!(gate, [b'G']);
    }
    let clock = Clock(Instant::now());
    let path = std::path::PathBuf::from(path);
    let mut wire = Pipeline::connect(
        &path,
        PipelineLimits {
            msize: 8192,
            max_outstanding: 8,
            ..PipelineLimits::default()
        },
        Duration::from_secs(5),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (tag, afid) = wire.auth(&[], &[]).unwrap();
    assert!(matches!(wire.wait(tag, deadline).unwrap(), Reply::Auth(_)));
    let (tag, root) = wire.attach_with_afid(afid, &[], &[]).unwrap();
    assert!(matches!(
        wire.wait(tag, deadline).unwrap(),
        Reply::Attach(_)
    ));
    let tag = wire.clunk(afid).unwrap();
    assert!(matches!(wire.wait(tag, deadline).unwrap(), Reply::Clunk));
    let mut c = Connecting::attached(
        wire,
        root,
        LockNegotiate {
            minimum_revision: 2,
            maximum_revision: 2,
            requested_capabilities: 3 | LOCK_FILE_CAPABILITY_SEALED_MEMORY,
            chords: vec![LockChordRequest {
                keysym: 0x62,
                modifiers: 0b0100,
            }],
        },
        clock.now(),
        10000,
    )
    .unwrap();
    c.enable_images(LockImageConfig {
        endpoint_dir: path.parent().unwrap().to_owned(),
        startup_deadline_ms: 10000,
        transfer_timeout_ms: 5000,
        formats_capacity: 65536,
    })
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
    let resource = LockResourceId {
        id: 1,
        generation: 1,
    };
    while s.image().unwrap().producer().channel() != ChannelState::Ready {
        clock.turn(&mut s);
    }
    let source = create_isb(2, 2).unwrap();
    source.write_pixels(0, &[0x7f; 16]).unwrap();
    finish_isb(&source).unwrap();
    let metadata = ProducerMetadata {
        offer_id: 0,
        resource: resource.id,
        resource_generation: resource.generation,
        admission_epoch: 3,
        output_id: 1,
        allocation_id: 1,
        topology_generation: generation,
        source_kind: 1,
        source_format: 1,
        width: 2,
        height: 2,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 16,
        modifier: 0,
    };
    let mut fd = source.into_fd();
    let offer = loop {
        match s.image_begin(metadata, fd, clock.now()) {
            Ok(v) => break v.offer,
            Err(e) if e.error == ImageError::Busy => {
                fd = e.source.unwrap();
                clock.turn(&mut s);
            }
            Err(e) => panic!("image begin: {e:?}"),
        }
    };
    let mut accepted = false;
    loop {
        if let Some((_, event)) = s.event() {
            assert!(
                matches!(event, Event::ResourceStatus(v) if v.status == LockResourceState::Accepted)
            );
            accepted = true;
            s.consume().unwrap();
        }
        let info = s.image().unwrap().producer().offer(offer).unwrap();
        if accepted && info.state == OfferState::Settled {
            assert_eq!(info.status, Some(ResourceStatusKind::Accepted));
            assert_eq!(info.disposition, 1);
            break;
        }
        clock.turn(&mut s);
    }
    s.image_forget(offer).unwrap();
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
        resource,
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
