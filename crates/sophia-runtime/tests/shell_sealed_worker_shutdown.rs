//! Shared worker shutdown with real sealed descriptors. This separate binary
//! keeps the FD baseline independent of parallel runtime tests. It proves
//! submitted/queued and completed custody cleanup, not a particular scan offset.
use sophia_runtime::SealedValidationWorker;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

fn source(bytes: u64) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, ftruncate, memfd_create};
    let fd = memfd_create(
        "worker-shutdown",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .unwrap();
    ftruncate(&fd, bytes).unwrap();
    fcntl_add_seals(
        &fd,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    fd
}

fn descriptors() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

fn await_descriptors(expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while descriptors() != expected {
        assert!(Instant::now() < deadline, "worker retained descriptors");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn drop_without_joining(worker: SealedValidationWorker) {
    let (done, receiver) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    // A join-on-validation regression must fail this control rather than
    // making the test wait for the deliberately distant job deadlines.
    receiver.recv_timeout(Duration::from_secs(3)).unwrap();
    owner.join().unwrap();
}

#[test]
fn shutdown_reclaims_submitted_sources_and_preserves_delivered_pixels() {
    let baseline = descriptors();
    for _ in 0..4 {
        let bytes = 4 << 20;
        let mut worker = SealedValidationWorker::with_validation_budget(4, bytes, 1).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        for _ in 0..3 {
            worker
                .submit(source(bytes as u64), bytes, deadline)
                .unwrap();
        }
        worker.reserve(bytes, deadline).unwrap();
        assert_eq!(worker.pending(), 4);
        drop_without_joining(worker);
        await_descriptors(baseline);
    }

    // A completed result can still be queued: shutdown must drop that mapping
    // too, even when the owner never takes its completion.
    let mut uncollected = SealedValidationWorker::new(1, 8).unwrap();
    uncollected
        .submit(source(8), 8, Instant::now() + Duration::from_secs(3))
        .unwrap();
    {
        use rustix::event::{PollFd, PollFlags, Timespec};
        let mut fds = [PollFd::from_borrowed_fd(
            uncollected.readiness(),
            PollFlags::IN,
        )];
        assert_eq!(
            rustix::event::poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: 3,
                    tv_nsec: 0
                })
            )
            .unwrap(),
            1
        );
    }
    assert_eq!(uncollected.pending(), 1);
    drop_without_joining(uncollected);
    await_descriptors(baseline);

    let mut worker = SealedValidationWorker::new(1, 8).unwrap();
    worker
        .submit(source(8), 8, Instant::now() + Duration::from_secs(3))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let pixels = loop {
        if let Some(completion) = worker.take_completed().unwrap() {
            break completion.result.unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(worker.pending(), 0);
    drop_without_joining(worker);
    await_descriptors(baseline + 1);
    assert_eq!(pixels.bytes(), &[0; 8]);
    drop(pixels);
    await_descriptors(baseline);
}
