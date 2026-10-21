//! ISB image admission and worker custody against the real allocation owner.
use super::*;
#[cfg(feature = "experimental-shell-capture")]
#[path = "shell_capture_admission.rs"]
mod capture_tests;

fn image_registry() -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    add_image_epoch(&mut registry, grant());
    registry
}

fn add_image_epoch(registry: &mut ContentEpochRegistry, epoch: ContentGrant) {
    let mut limits = ContentLimits::prototype(epoch);
    limits.max_staging_bytes = 8 << 20;
    limits.max_resident_bytes = 8 << 20;
    limits.max_retiring_bytes = 8 << 20;
    registry.admit(limits).unwrap();
    let allocations = registry.allocations_mut(epoch).unwrap();
    allocations
        .publish_outputs(tx(1), 1, vec![facts(100)])
        .unwrap();
    let mut request = panel_request(1, 1, ContentAllocationId::default());
    request.grant = epoch;
    allocations.request(tx(2), request, &[], 0).unwrap();
    allocations
        .grant(
            1,
            panel(ContentAllocationId {
                id: 1,
                generation: 1,
            }),
            &[],
        )
        .unwrap();
    while allocations.take_event().is_some() {}
}

#[test]
fn expiry_continues_after_another_epochs_clock_error() {
    use std::time::{Duration, Instant};
    for epoch_only in [false, true] {
        let mut registry = image_registry();
        let other = ContentGrant {
            connection_epoch: 2,
            content_grant_epoch: 2,
        };
        add_image_epoch(&mut registry, other);
        let mut worker = IsbValidationWorker::new(2, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        jobs.reserve(
            &mut registry,
            &mut worker,
            grant(),
            image_metadata(),
            10,
            deadline,
        )
        .unwrap();
        let mut other_metadata = image_metadata();
        other_metadata.admission_epoch = other.content_grant_epoch;
        jobs.reserve(
            &mut registry,
            &mut worker,
            other,
            other_metadata,
            0,
            deadline,
        )
        .unwrap();
        if epoch_only {
            jobs.withdraw_epoch(&mut registry, &mut worker, other, 5)
                .unwrap();
        } else {
            assert!(
                jobs.expire(&mut registry, &mut worker, deadline, 5)
                    .is_err()
            );
        }
        assert_eq!(worker.pending(), 1);
        let settled = jobs.reconcile(&mut registry, 5).unwrap();
        assert_eq!(settled.admission.grant, other);
        assert_eq!(jobs.pending(), 1);
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 16000);
        jobs.withdraw_epoch(&mut registry, &mut worker, grant(), 10)
            .unwrap();
        assert_eq!(
            jobs.reconcile(&mut registry, 10).unwrap().admission.grant,
            grant()
        );
    }
}

#[test]
fn delivered_success_has_no_remaining_validation_deadline() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = IsbImageJobs::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    let job = jobs
        .reserve(
            &mut registry,
            &mut worker,
            grant(),
            image_metadata(),
            0,
            deadline,
        )
        .unwrap();
    jobs.submit(&mut worker, job, image_source()).unwrap();
    let completion = loop {
        if let Some(completion) = worker.take_completed().unwrap() {
            break completion;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(completion.result.is_ok());
    assert!(jobs.receive(completion).is_ok());
    assert_eq!(jobs.deadline(), None);
    jobs.expire(&mut registry, &mut worker, deadline, 1)
        .unwrap();
    assert!(jobs.reconcile(&mut registry, 1).unwrap().outcome.is_ok());
}

#[test]
fn resource_cancel_reaches_unsent_and_submitted_worker_custody() {
    use std::time::{Duration, Instant};
    for submitted in [false, true] {
        let mut registry = image_registry();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let metadata = image_metadata();
        let deadline = Instant::now() + Duration::from_secs(3);
        let job = jobs
            .reserve(&mut registry, &mut worker, grant(), metadata, 0, deadline)
            .unwrap();
        if submitted {
            jobs.submit(&mut worker, job, image_source()).unwrap();
        }
        registry
            .resources_mut(grant())
            .unwrap()
            .cancel(
                tx(metadata.offer_id),
                &ContentResourceCancel {
                    grant: grant(),
                    resource: ContentResourceId {
                        id: metadata.resource,
                        generation: metadata.resource_generation,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            worker.pending(),
            1,
            "store cancellation alone does not return worker custody"
        );
        jobs.observe_store_stops(&mut registry, &mut worker, 1)
            .unwrap();
        jobs.observe_store_stops(&mut registry, &mut worker, 1)
            .unwrap();
        if submitted {
            assert_eq!(worker.pending(), 1);
            assert!(jobs.reconcile(&mut registry, 1).is_none());
            let completion = loop {
                if let Some(completion) = worker.take_completed().unwrap() {
                    break completion;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            };
            assert!(jobs.receive(completion).is_ok());
        } else {
            assert_eq!(worker.pending(), 0);
            assert!(jobs.submit(&mut worker, job, image_source()).is_err());
        }
        let settlement = jobs.reconcile(&mut registry, 1).unwrap();
        assert_eq!(settlement.stopped, Some(ContentReason::Cancelled));
        assert!(settlement.outcome.is_err());
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
        assert!(jobs.reconcile(&mut registry, 1).is_none());
    }
}

#[test]
fn unsent_withdrawal_with_wrong_worker_preserves_a_retryable_credit() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut wrong = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = IsbImageJobs::default();
    let job = jobs
        .reserve(
            &mut registry,
            &mut worker,
            grant(),
            image_metadata(),
            0,
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(
        jobs.withdraw(
            &mut registry,
            &mut wrong,
            job,
            IsbImageWithdrawal::Cancelled,
            1
        ),
        Err(ContentStoreError::Incomplete)
    );
    assert_eq!(worker.pending(), 1);
    assert!(jobs.reconcile(&mut registry, 1).is_none());
    jobs.withdraw(
        &mut registry,
        &mut worker,
        job,
        IsbImageWithdrawal::ChannelLost,
        1,
    )
    .unwrap();
    assert_eq!(worker.pending(), 0);
    assert_eq!(
        jobs.reconcile(&mut registry, 1).unwrap().stopped,
        Some(ContentReason::Cancelled)
    );
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
}

fn image_metadata() -> sophia_protocol::shell_files::ShellImageMetadata {
    sophia_protocol::shell_files::ShellImageMetadata {
        offer_id: 1,
        resource: 1,
        resource_generation: 1,
        admission_epoch: 1,
        output_id: 2,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 200,
        height: 20,
        stride: 800,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 16000,
        modifier: 0,
    }
}

#[test]
fn sealed_admission_checks_live_allocation_and_rolls_back_worker_credit_on_refusal() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(2, 16000).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    for field in 0..5 {
        let mut metadata = image_metadata();
        match field {
            0 => metadata.admission_epoch += 1,
            1 => metadata.topology_generation += 1,
            2 => metadata.output_id += 1,
            3 => metadata.allocation_id += 1,
            _ => metadata.stride += 1,
        }
        assert!(
            registry
                .reserve_isb_image(grant(), metadata, &mut worker, 0, deadline)
                .is_err()
        );
        assert_eq!(worker.pending(), 0);
        assert_eq!(
            registry.resources(grant()).unwrap().usage(),
            ContentMemoryUsage::default()
        );
    }
    let admitted = registry
        .reserve_isb_image(grant(), image_metadata(), &mut worker, 0, deadline)
        .unwrap();
    assert_eq!(admitted.metadata, image_metadata());
    assert_eq!(admitted.deadline, deadline);
    assert_eq!(worker.pending(), 1);
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 16000);
    assert!(
        registry
            .resources(grant())
            .unwrap()
            .pending_event()
            .is_none()
    );
    assert!(
        registry
            .reserve_isb_image(grant(), image_metadata(), &mut worker, 0, deadline)
            .is_err()
    );
    assert_eq!(
        worker.pending(),
        1,
        "failed resource reservation returns worker credit"
    );
    assert!(worker.release_reserved(admitted.worker_job));
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let mut result = Some(Err(ContentStoreError::Malformed));
    assert_eq!(
        registry.complete_isb(grant(), resource, &mut result, 1),
        Err(ContentStoreError::Malformed)
    );
    assert!(result.is_none());
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
}

#[test]
fn exhausted_worker_does_not_consume_resource_generation() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let occupied = worker.reserve(4, deadline).unwrap();
    assert_eq!(
        registry
            .reserve_isb_image(grant(), image_metadata(), &mut worker, 0, deadline)
            .unwrap_err(),
        IsbImageAdmissionError::Worker(IsbSubmitFailure::Capacity)
    );
    worker.release_reserved(occupied);
    let admitted = registry
        .reserve_isb_image(grant(), image_metadata(), &mut worker, 0, deadline)
        .unwrap();
    assert!(registry.disconnect(grant()));
    worker.release_reserved(admitted.worker_job);
    let mut result = Some(Err(ContentStoreError::Revoked));
    assert_eq!(
        registry.complete_isb(
            grant(),
            ContentResourceId {
                id: 1,
                generation: 1
            },
            &mut result,
            1
        ),
        Err(ContentStoreError::Revoked)
    );
    assert_eq!(registry.accounting().retired_epochs, 0);
}

#[test]
fn owner_job_table_keeps_completions_across_clock_retry_and_disconnect() {
    use std::time::{Duration, Instant};
    for disconnect in [false, true] {
        let mut registry = image_registry();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        let job = jobs
            .reserve(
                &mut registry,
                &mut worker,
                grant(),
                image_metadata(),
                10,
                deadline,
            )
            .unwrap();
        // An ordinary file fails validation on the real worker, rather than
        // inventing a completion or testing only a successful store handoff.
        jobs.submit(
            &mut worker,
            job,
            std::fs::File::open("/dev/null").unwrap().into(),
        )
        .unwrap();
        if disconnect {
            assert!(registry.disconnect(grant()));
        }
        let completion = loop {
            if let Some(completion) = worker.take_completed().unwrap() {
                break completion;
            }
            assert!(Instant::now() < deadline, "worker completion deadline");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(jobs.receive(completion).is_ok());
        assert_eq!(worker.pending(), 0);
        assert!(jobs.reconcile(&mut registry, 9).is_none());
        assert_eq!(jobs.pending(), 1, "clock failure retains owner custody");
        let settlement = jobs.reconcile(&mut registry, 11).unwrap();
        assert_eq!(settlement.admission.worker_job, job);
        assert_eq!(
            settlement.validation_failure,
            Some(IsbValidationFailure::Malformed)
        );
        assert_eq!(
            settlement.outcome,
            Err(if disconnect {
                ContentStoreError::Revoked
            } else {
                ContentStoreError::Malformed
            })
        );
        assert_eq!(jobs.pending(), 0);
        assert!(jobs.reconcile(&mut registry, 11).is_none());
        assert_eq!(registry.accounting().retired_epochs, 0);
        let duplicate = IsbValidationCompletion {
            job,
            result: Err(IsbValidationFailure::Cancelled),
        };
        let returned = jobs.receive(duplicate).err().unwrap();
        assert_eq!(returned.job, job);
        assert!(matches!(
            returned.result,
            Err(IsbValidationFailure::Cancelled)
        ));
    }
}

#[test]
fn completed_worker_jobs_still_hold_owner_table_capacity() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = IsbImageJobs::default();
    let deadline = Instant::now() + Duration::from_secs(3);
    for id in 1..=4 {
        let mut metadata = image_metadata();
        metadata.offer_id = id;
        metadata.resource = id;
        let job = jobs
            .reserve(&mut registry, &mut worker, grant(), metadata, 0, deadline)
            .unwrap();
        jobs.submit(
            &mut worker,
            job,
            std::fs::File::open("/dev/null").unwrap().into(),
        )
        .unwrap();
        loop {
            if let Some(completion) = worker.take_completed().unwrap() {
                assert!(jobs.receive(completion).is_ok());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(worker.pending(), 0);
        assert_eq!(jobs.pending(), id as usize);
    }
    let mut metadata = image_metadata();
    metadata.offer_id = 5;
    metadata.resource = 5;
    assert_eq!(
        jobs.reserve(&mut registry, &mut worker, grant(), metadata, 0, deadline),
        Err(IsbImageAdmissionError::Worker(IsbSubmitFailure::Capacity))
    );
    assert_eq!(worker.pending(), 0);
    assert!(registry.disconnect(grant()));
    for remaining in (0..4).rev() {
        assert!(jobs.reconcile(&mut registry, 1).is_some());
        assert_eq!(jobs.pending(), remaining);
    }
    assert_eq!(registry.accounting().retired_epochs, 0);
}

fn image_source() -> std::os::fd::OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let file = std::fs::File::from(
        memfd_create(
            c"image-owner-test",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.set_len(16000).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

#[test]
fn withdrawing_an_unsent_offer_releases_worker_credit_but_waits_for_store_handoff() {
    use std::time::{Duration, Instant};
    for (cause, reason) in [
        (IsbImageWithdrawal::Cancelled, ContentReason::Cancelled),
        (IsbImageWithdrawal::Expired, ContentReason::Timeout),
        (IsbImageWithdrawal::ChannelLost, ContentReason::Cancelled),
        (
            IsbImageWithdrawal::AllocationLost,
            ContentReason::AllocationLost,
        ),
    ] {
        let mut registry = image_registry();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let job = jobs
            .reserve(
                &mut registry,
                &mut worker,
                grant(),
                image_metadata(),
                10,
                Instant::now() + Duration::from_secs(3),
            )
            .unwrap();
        assert_eq!(
            jobs.withdraw(&mut registry, &mut worker, job, cause, 9),
            Err(ContentStoreError::ClockRegression)
        );
        assert_eq!(worker.pending(), 1);
        jobs.withdraw(&mut registry, &mut worker, job, cause, 10)
            .unwrap();
        assert_eq!(worker.pending(), 0);
        assert_eq!(jobs.pending(), 1);
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 16000);
        let (error, fd) = jobs.submit(&mut worker, job, image_source()).unwrap_err();
        assert_eq!(error, IsbSubmitFailure::Invalid);
        drop(fd);
        // A later cancellation cannot replace the original expiry/cause.
        jobs.withdraw(
            &mut registry,
            &mut worker,
            job,
            IsbImageWithdrawal::Cancelled,
            10,
        )
        .unwrap();
        let settlement = jobs.reconcile(&mut registry, 10).unwrap();
        assert_eq!(settlement.stopped, Some(reason));
        assert_eq!(settlement.validation_failure, None);
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
        let event = registry
            .resources_mut(grant())
            .unwrap()
            .take_event()
            .unwrap();
        assert!(
            matches!(event.record, ShellContentRecord::ResourceStatus(status)
            if status.reason == reason as u16 && status.status == if reason == ContentReason::Cancelled { 4 } else { 3 })
        );
        assert!(
            registry
                .resources_mut(grant())
                .unwrap()
                .take_event()
                .is_none()
        );
        assert!(jobs.reconcile(&mut registry, 10).is_none());
    }
}

#[test]
fn cancellation_waits_for_submitted_validation_and_overrides_an_uncommitted_success() {
    use std::time::{Duration, Instant};
    for already_completed in [false, true] {
        let mut registry = image_registry();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        let job = jobs
            .reserve(
                &mut registry,
                &mut worker,
                grant(),
                image_metadata(),
                0,
                deadline,
            )
            .unwrap();
        jobs.submit(&mut worker, job, image_source()).unwrap();
        if !already_completed {
            jobs.withdraw(
                &mut registry,
                &mut worker,
                job,
                IsbImageWithdrawal::Cancelled,
                1,
            )
            .unwrap();
            assert_eq!(worker.pending(), 1);
            assert!(jobs.reconcile(&mut registry, 1).is_none());
        }
        let completion = loop {
            if let Some(value) = worker.take_completed().unwrap() {
                break value;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        if already_completed {
            assert!(completion.result.is_ok());
        }
        assert!(jobs.receive(completion).is_ok());
        jobs.withdraw(
            &mut registry,
            &mut worker,
            job,
            IsbImageWithdrawal::Cancelled,
            1,
        )
        .unwrap();
        let settlement = jobs.reconcile(&mut registry, 1).unwrap();
        assert_eq!(settlement.stopped, Some(ContentReason::Cancelled));
        assert_eq!(registry.resources(grant()).unwrap().usage().resident, 0);
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
    }
}

#[test]
fn deadlines_and_epoch_loss_retire_unsent_jobs_without_waiting_for_a_source() {
    use std::time::{Duration, Instant};
    for disconnect in [false, true] {
        let mut registry = image_registry();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let mut jobs = IsbImageJobs::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        jobs.reserve(
            &mut registry,
            &mut worker,
            grant(),
            image_metadata(),
            0,
            deadline,
        )
        .unwrap();
        assert_eq!(jobs.deadline(), Some(deadline));
        jobs.expire(
            &mut registry,
            &mut worker,
            deadline - Duration::from_millis(1),
            1,
        )
        .unwrap();
        assert_eq!(worker.pending(), 1);
        if disconnect {
            assert!(registry.disconnect(grant()));
            jobs.withdraw_epoch(&mut registry, &mut worker, grant(), 2)
                .unwrap();
        } else {
            jobs.expire(&mut registry, &mut worker, deadline, 2)
                .unwrap();
        }
        assert_eq!(jobs.deadline(), None);
        assert_eq!(worker.pending(), 0);
        let settlement = jobs.reconcile(&mut registry, 2).unwrap();
        assert_eq!(
            settlement.stopped,
            Some(if disconnect {
                ContentReason::Revoked
            } else {
                ContentReason::Timeout
            })
        );
        assert_eq!(registry.accounting().retired_epochs, 0);
        assert_eq!(jobs.pending(), 0);
    }
}

#[test]
fn worker_timeout_is_not_reported_as_malformed_source() {
    use std::time::{Duration, Instant};
    let mut registry = image_registry();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = IsbImageJobs::default();
    let job = jobs
        .reserve(
            &mut registry,
            &mut worker,
            grant(),
            image_metadata(),
            0,
            Instant::now(),
        )
        .unwrap();
    jobs.submit(&mut worker, job, image_source()).unwrap();
    let bound = Instant::now() + Duration::from_secs(3);
    let completion = loop {
        if let Some(completion) = worker.take_completed().unwrap() {
            break completion;
        }
        assert!(Instant::now() < bound);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(matches!(
        completion.result,
        Err(IsbValidationFailure::Timeout)
    ));
    assert!(jobs.receive(completion).is_ok());
    let settlement = jobs.reconcile(&mut registry, 0).unwrap();
    assert_eq!(settlement.stopped, Some(ContentReason::Timeout));
    assert!(
        matches!(registry.resources_mut(grant()).unwrap().take_event().unwrap().record,
        ShellContentRecord::ResourceStatus(status) if status.reason == ContentReason::Timeout as u16)
    );
}
