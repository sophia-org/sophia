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
fn admission_reserves_all_worker_credits_before_any_source_arrives() {
    let mut worker = SealedValidationWorker::new(4, 8).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let ids: Vec<_> = (0..4)
        .map(|_| worker.reserve(8, deadline).unwrap())
        .collect();
    assert_eq!(worker.pending(), 4);
    assert_eq!(
        worker.reserve(8, deadline),
        Err(SealedSubmitFailure::Capacity)
    );
    assert!(worker.take_completed().unwrap().is_none());
    assert!(worker.release_reserved(ids[1]));
    let replacement = worker.reserve(8, deadline).unwrap();
    assert!(replacement > ids[3]);
    let stale_fd = source(&[0; 8]);
    let raw = stale_fd.as_raw_fd();
    let (error, returned) = worker.submit_reserved(ids[1], stale_fd).unwrap_err();
    assert_eq!(error, SealedSubmitFailure::Invalid);
    assert_eq!(returned.as_raw_fd(), raw);
    drop(returned);
    for id in [ids[3], replacement, ids[0], ids[2]] {
        worker.submit_reserved(id, source(&[0; 8])).unwrap();
        assert!(
            !worker.release_reserved(id),
            "submitted custody cannot be released early"
        );
    }
    for id in [ids[3], replacement, ids[0], ids[2]] {
        let done = completion(&mut worker);
        assert_eq!(done.job, id);
        assert_eq!(done.result.unwrap().bytes(), &[0; 8]);
    }
    assert_eq!(worker.pending(), 0);
}

#[test]
fn reserved_source_cannot_renew_the_admission_deadline_or_reenter_after_cancel() {
    let mut worker = SealedValidationWorker::new(2, 8).unwrap();
    let expired = worker
        .reserve(8, Instant::now() - Duration::from_millis(1))
        .unwrap();
    let cancelled = worker
        .reserve(8, Instant::now() + Duration::from_secs(3))
        .unwrap();
    assert!(worker.cancel(cancelled));
    worker.submit_reserved(expired, source(&[0; 8])).unwrap();
    worker.submit_reserved(cancelled, source(&[0; 8])).unwrap();
    let duplicate = source(&[0; 8]);
    assert_eq!(
        worker.submit_reserved(cancelled, duplicate).unwrap_err().0,
        SealedSubmitFailure::Invalid
    );
    assert_eq!(
        completion(&mut worker).result.err(),
        Some(SealedValidationFailure::Timeout)
    );
    assert_eq!(
        completion(&mut worker).result.err(),
        Some(SealedValidationFailure::Cancelled)
    );
    assert_eq!(worker.pending(), 0);
}

#[test]
fn worker_completion_reaches_its_disconnected_registry_owner() {
    use sophia_protocol::*;
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry.admit(ContentLimits::prototype(grant)).unwrap();
    registry
        .resources_mut(grant)
        .unwrap()
        .begin_sealed(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 2,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 8,
            },
            0,
        )
        .unwrap();
    let mut worker = SealedValidationWorker::new(1, 8).unwrap();
    let id = worker
        .reserve(8, Instant::now() + Duration::from_secs(3))
        .unwrap();
    worker.submit_reserved(id, source(&[0; 8])).unwrap();
    assert!(registry.disconnect(grant));
    assert_eq!(registry.retired_backing_bytes(), 8);
    let done = completion(&mut worker);
    assert_eq!(done.job, id);
    let mut pixels = Some(done.result.map_err(|_| ContentStoreError::Malformed));
    assert_eq!(
        registry.complete_sealed(grant, resource, &mut pixels, 1),
        Err(ContentStoreError::Revoked)
    );
    assert!(pixels.is_none());
    assert_eq!(registry.accounting().retired_epochs, 0);
    assert_eq!(registry.retired_backing_bytes(), 0);
    assert_eq!(registry.accounting().response_records, 0);
}

#[test]
fn published_validation_budget_can_split_pixels_without_skipping_components() {
    use sophia_linux_peer::SealedMapping;
    let mut bytes = vec![0; SEALED_PIXEL_VALIDATION_BYTES + 4];
    bytes[SEALED_PIXEL_VALIDATION_BYTES] = 1;
    let mut capped =
        SealedPixelValidation::new(SealedMapping::map(source(&bytes), bytes.len()).unwrap())
            .unwrap();
    assert!(!capped.step_with_budget(usize::MAX).unwrap());
    assert!(capped.step_with_budget(usize::MAX).is_err());
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
