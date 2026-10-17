use sophia_runtime::*;
use std::io::Write;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

fn source(bytes: &[u8]) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "validation-worker",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}
fn ready(worker: &SealedValidationWorker) {
    use rustix::event::{PollFd, PollFlags, Timespec};
    let mut fds = [PollFd::from_borrowed_fd(worker.readiness(), PollFlags::IN)];
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
fn completion(worker: &mut SealedValidationWorker) -> SealedValidationCompletion {
    let end = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(result) = worker.take_completed().unwrap() {
            return result;
        }
        assert!(Instant::now() < end, "validation did not finish");
        ready(worker);
    }
}

#[test]
fn finished_but_uncollected_work_still_consumes_its_credit() {
    let mut worker = SealedValidationWorker::new(1, 65540).unwrap();
    let bytes = vec![0; 65540];
    let deadline = Instant::now() + Duration::from_secs(3);
    let job = worker
        .submit(source(&bytes), bytes.len(), deadline)
        .unwrap();
    ready(&worker);
    assert_eq!(worker.pending(), 1);
    let extra = source(&[0; 4]);
    let fd = extra.as_raw_fd();
    let (error, returned) = worker.submit(extra, 4, deadline).unwrap_err();
    assert_eq!(error, SealedSubmitFailure::Capacity);
    assert_eq!(returned.as_raw_fd(), fd);
    drop(returned);
    let done = completion(&mut worker);
    assert_eq!(done.job, job);
    assert_eq!(done.result.unwrap().bytes(), bytes);
    assert_eq!(worker.pending(), 0);
    assert!(worker.take_completed().unwrap().is_none());
    let next = worker.submit(source(&[0; 4]), 4, deadline).unwrap();
    assert!(next > job);
    assert_eq!(completion(&mut worker).job, next);
}

#[test]
fn invalid_sources_and_expired_deadlines_return_one_terminal_completion() {
    let mut worker = SealedValidationWorker::new(4, 8).unwrap();
    let future = Instant::now() + Duration::from_secs(3);
    let past = Instant::now() - Duration::from_millis(1);
    let expired = worker.submit(source(&[0; 8]), 8, past).unwrap();
    let invalid = worker
        .submit(std::fs::File::open("/dev/null").unwrap().into(), 8, future)
        .unwrap();
    let bad_pixel = worker
        .submit(source(&[1, 0, 0, 0, 0, 0, 0, 0]), 8, future)
        .unwrap();
    let wrong_size = worker.submit(source(&[0; 4]), 8, future).unwrap();
    for id in [expired, invalid, bad_pixel, wrong_size] {
        let result = completion(&mut worker);
        assert_eq!(result.job, id);
        assert_eq!(
            result.result.err(),
            Some(if id == expired {
                SealedValidationFailure::Timeout
            } else {
                SealedValidationFailure::Malformed
            })
        );
    }
    assert_eq!(worker.pending(), 0);
    assert!(worker.take_completed().unwrap().is_none());
}

#[test]
fn cancellation_never_removes_the_completion_obligation() {
    let mut worker = SealedValidationWorker::new(1, 4 << 20).unwrap();
    let job = worker
        .submit(
            source(&vec![0; 4 << 20]),
            4 << 20,
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
    assert!(worker.cancel(job));
    assert_eq!(worker.pending(), 1);
    let done = completion(&mut worker);
    assert_eq!(done.job, job);
    // Completion can win the cancellation race. The resource owner must
    // still reject that result if its reservation was already cancelled.
    if let Err(error) = done.result {
        assert_eq!(error, SealedValidationFailure::Cancelled);
    }
    assert!(!worker.cancel(job));
    assert!(worker.take_completed().unwrap().is_none());
}

#[test]
fn published_validation_budget_can_split_pixels_without_skipping_components() {
    use sophia_linux_peer::SealedMapping;
    for budget in [1, 2, 3, 5, 7] {
        let mut scan =
            SealedPixelValidation::new(SealedMapping::map(source(&[0; 8]), 8).unwrap()).unwrap();
        for unit in 1..=8usize.div_ceil(budget) {
            assert_eq!(scan.step_with_budget(budget).unwrap(), unit * budget >= 8);
        }
        assert_eq!(scan.finish().unwrap().bytes(), &[0; 8]);
        // Every colour component, including those crossing a unit boundary,
        // must be compared with its own pixel's alpha.
        for component in [0, 1, 2, 4, 5, 6] {
            let mut bytes = [0; 8];
            bytes[component] = 1;
            let mut scan =
                SealedPixelValidation::new(SealedMapping::map(source(&bytes), 8).unwrap()).unwrap();
            while let Ok(complete) = scan.step_with_budget(budget) {
                assert!(!complete, "invalid component was skipped");
            }
            assert!(scan.finish().is_err());
        }
    }
    assert!(SealedValidationWorker::with_validation_budget(1, 8, 0).is_err());
    let mut worker = SealedValidationWorker::with_validation_budget(1, 8, 3).unwrap();
    worker
        .submit(source(&[0; 8]), 8, Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert_eq!(completion(&mut worker).result.unwrap().bytes(), &[0; 8]);
}
