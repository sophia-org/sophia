//! Real mailbox + Session correlation, with shutdown before GPU submission.
use super::*;
use sophia_protocol::*;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureExecutor,
    LiveShellCaptureOutcome, LiveShellCaptureSource, live_shell_capture_channel,
};
use std::{
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::Arc,
    time::Duration,
};

struct Fixture {
    owner: Owner,
    executor: Option<LiveShellCaptureExecutor>,
    epoch: Arc<LiveRendererSnapshotEpoch>,
    budget: Arc<LiveRendererSnapshotBudget>,
    now: Instant,
    registries: Vec<ContentEpochRegistry>,
}
impl Fixture {
    fn new() -> Self {
        let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
        let (inbox, executor) = live_shell_capture_channel(epoch.clone()).unwrap();
        Self {
            owner: Owner::new(inbox),
            executor: Some(executor),
            epoch,
            budget: LiveRendererSnapshotBudget::new(8, 512),
            now: Instant::now(),
            registries: Vec::new(),
        }
    }
    fn reserve(&mut self, binding: Binding) -> ContentCaptureReservation {
        let mut registry = allocation_registry(binding.grant);
        for generation in 1..binding.metadata.resource_generation {
            let mut prior = binding.metadata;
            prior.resource_generation = generation;
            let mut r = Some(
                registry
                    .reserve_capture_image(binding.grant, &prior, 0)
                    .unwrap(),
            );
            registry
                .fail_capture(&mut r, ContentCaptureFailure::Renderer, 0)
                .unwrap();
        }
        while registry
            .resources_mut(binding.grant)
            .unwrap()
            .take_event()
            .is_some()
        {}
        let reservation = registry
            .reserve_capture_image(binding.grant, &binding.metadata, 0)
            .unwrap();
        self.registries.push(registry);
        reservation
    }
    fn request(&self) -> LiveShellCaptureRequest {
        let (fd, _) = UnixStream::pair().unwrap();
        LiveShellCaptureRequest {
            source: LiveShellCaptureSource {
                fd: fd.into(),
                width: 4,
                height: 4,
                stride: 16,
                modifier: 0,
                offset: 0,
            },
            image_id: sophia_renderer_live::LiveRendererImageId::from_raw(1),
            budget: self.budget.clone(),
            reservation: self.budget.reserve_capture(64).unwrap(),
            epoch: self.epoch.clone(),
        }
    }
    fn submit(&mut self, binding: Binding) -> (LiveShellCaptureJob, i32) {
        let r = self.request();
        let fd = r.source.fd.as_raw_fd();
        let reservation = self.reserve(binding);
        (
            self.owner
                .submit(binding, r, reservation, self.now + Duration::from_secs(10))
                .unwrap(),
            fd,
        )
    }
    fn close(&mut self) -> Settlements {
        drop(self.executor.take());
        self.owner.take_completed(self.now).unwrap()
    }
}
fn binding(epoch: u64, resource: u64) -> Binding {
    Binding {
        plane: ShellImagePlane {
            offset: 0,
            stride: 16,
            fd_index: 0,
        },
        slot: 0,
        grant: ContentGrant {
            connection_epoch: epoch,
            content_grant_epoch: epoch,
        },
        metadata: ShellImageMetadata {
            offer_id: resource,
            resource,
            resource_generation: 1,
            admission_epoch: epoch,
            output_id: 1,
            allocation_id: 1,
            topology_generation: 1,
            source_kind: SHELL_IMAGE_SOURCE_DMA_BUF,
            source_format: sophia_protocol::DRM_FORMAT_XRGB8888,
            width: 4,
            height: 4,
            stride: 0,
            plane_count: 1,
            descriptor_count: 1,
            declared_bytes: 64,
            modifier: 0,
        },
    }
}

#[test]
fn reconnect_does_not_rebind_old_receipt_or_withdraw_new_epoch() {
    let mut f = Fixture::new();
    let old = binding(1, 1);
    let new = binding(2, 1);
    let (old_job, old_fd) = f.submit(old);
    let (new_job, new_fd) = f.submit(new);
    f.owner.withdraw(old.slot, old.grant);
    assert_eq!(f.owner.pending.iter().flatten().count(), 2);
    let batch = f.close();
    assert!(!batch.fault);
    assert_eq!(batch.completed.len(), 2);
    for s in batch.completed {
        let (wanted, fd, withdrawn) = if s.receipt.job() == old_job {
            (old, old_fd, true)
        } else {
            assert_eq!(s.receipt.job(), new_job);
            (new, new_fd, false)
        };
        assert_eq!(s.binding, wanted);
        assert_eq!(s.withdrawn, withdrawn);
        let LiveShellCaptureOutcome::Returned { request, .. } = s.receipt.into_outcome() else {
            panic!("pre-use rollback")
        };
        assert_eq!(request.source.fd.as_raw_fd(), fd);
    }
    assert_eq!(f.budget.usage(), (0, 0));
    assert!(f.owner.take_completed(f.now).unwrap().completed.is_empty());
}

#[test]
fn cancel_matches_slot_grant_and_resource_generation() {
    let mut f = Fixture::new();
    let a = binding(1, 1);
    let mut b = a;
    b.metadata.resource_generation = 2;
    b.metadata.offer_id = 2;
    let mut c = a;
    c.slot = 1;
    f.submit(a);
    f.submit(b);
    f.submit(c);
    f.owner.cancel_resource(a.slot, a.grant, a.resource());
    let batch = f.close();
    assert_eq!(batch.completed.len(), 3);
    for s in batch.completed {
        assert_eq!(s.withdrawn, s.binding == a);
    }
}

#[test]
fn duplicate_offer_or_resource_and_capacity_return_original_custody() {
    let mut f = Fixture::new();
    f.submit(binding(1, 1));
    for same_offer in [true, false] {
        let mut duplicate = binding(1, 1);
        if same_offer {
            duplicate.metadata.resource = 2;
        } else {
            duplicate.metadata.offer_id = 2;
        }
        let r = f.request();
        let fd = r.source.fd.as_raw_fd();
        let reservation = f.reserve(duplicate);
        let Rejected {
            reason: error,
            request: r,
            reservation,
        } = f
            .owner
            .submit(duplicate, r, reservation, f.now)
            .unwrap_err();
        assert_eq!(*reservation.metadata(), duplicate.metadata);
        assert_eq!(error, SubmitFailure::Duplicate);
        assert_eq!(r.source.fd.as_raw_fd(), fd);
        assert_eq!(f.budget.usage(), (2, 128));
        drop(r);
    }
    for id in 2..=4 {
        f.submit(binding(1, id));
    }
    let r = f.request();
    let fd = r.source.fd.as_raw_fd();
    let reservation = f.reserve(binding(1, 5));
    let Rejected {
        reason: error,
        request: r,
        reservation: _,
    } = f
        .owner
        .submit(binding(1, 5), r, reservation, f.now)
        .unwrap_err();
    assert_eq!(error, SubmitFailure::Capacity);
    assert_eq!(r.source.fd.as_raw_fd(), fd);
    drop(r);
    assert_eq!(f.close().completed.len(), 4);
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn mismatched_source_metadata_or_admission_never_enters_mailbox() {
    for field in 0..16 {
        let mut f = Fixture::new();
        let mut b = binding(1, 1);
        let r = f.request();
        let fd = r.source.fd.as_raw_fd();
        let reservation = f.reserve(b);
        match field {
            0 => b.metadata.width = 5,
            1 => b.metadata.stride = 32,
            2 => b.metadata.modifier = 42,
            3 => b.metadata.admission_epoch = 2,
            4 => b.grant.connection_epoch = 0,
            5 => b.metadata.source_kind = 1,
            6 => b.metadata.plane_count = 2,
            7 => b.plane.offset = 4,
            8 => b.plane.stride = 32,
            9 => b.plane.fd_index = 1,
            10 => b.metadata.output_id += 1,
            11 => b.metadata.allocation_id += 1,
            12 => b.metadata.topology_generation += 1,
            13 => b.metadata.resource_generation += 1,
            14 => b.metadata.offer_id += 1,
            _ => b.grant.connection_epoch += 1,
        }
        let Rejected {
            reason: error,
            request: r,
            reservation: _,
        } = f.owner.submit(b, r, reservation, f.now).unwrap_err();
        assert_eq!(error, SubmitFailure::Binding);
        assert_eq!(r.source.fd.as_raw_fd(), fd);
        assert!(f.owner.pending.iter().all(Option::is_none));
    }
}

#[test]
fn closed_executor_returns_reservation_without_creating_binding() {
    let mut f = Fixture::new();
    drop(f.executor.take());
    let r = f.request();
    let fd = r.source.fd.as_raw_fd();
    let reservation = f.reserve(binding(1, 1));
    let Rejected {
        reason: error,
        request: r,
        reservation: _,
    } = f
        .owner
        .submit(binding(1, 1), r, reservation, f.now)
        .unwrap_err();
    assert_eq!(
        error,
        SubmitFailure::Renderer(LiveShellCaptureSubmitFailure::Closed)
    );
    assert_eq!(r.source.fd.as_raw_fd(), fd);
    assert!(f.owner.pending.iter().all(Option::is_none));
}

#[test]
fn unmatched_receipt_is_retained_and_closes_submission() {
    let mut f = Fixture::new();
    f.submit(binding(1, 1));
    // Fault injection in this private owner test, not a public receipt factory.
    f.owner.pending[0] = None;
    let batch = f.close();
    assert!(batch.fault);
    assert!(batch.completed.is_empty());
    assert_eq!(f.owner.orphaned.len(), 1);
    assert_eq!(f.budget.usage(), (1, 64));
    let r = f.request();
    let reservation = f.reserve(binding(2, 1));
    assert_eq!(
        f.owner
            .submit(binding(2, 1), r, reservation, f.now)
            .unwrap_err()
            .reason,
        SubmitFailure::Closed
    );
}

// Each test can deliberately supply independent exact owners with reused IDs;
// receipt settlement still has to route to the reservation's actual owner.
#[path = "../../../sophia-runtime/tests/support/captured_resource_fixture.rs"]
mod captured_fixture;
use captured_fixture::allocation_registry;

#[path = "shell_capture_acceptance.rs"]
mod acceptance;

#[test]
fn failed_settlement_preserves_both_reservations_until_the_right_owner_clock() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    let (job, fd) = f.submit(b);
    f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .expire(10)
        .unwrap();
    let mut s = f.close().completed.pop().unwrap();
    assert_eq!(s.receipt.job(), job);
    assert_eq!(
        s.fail_unpublished(&mut f.registries[0], 9),
        Err(ContentStoreError::ClockRegression)
    );
    assert!(s.reservation.is_some());
    assert_eq!(
        f.registries[0].resources(b.grant).unwrap().usage().backing,
        64
    );
    assert_eq!(f.budget.usage(), (1, 64));
    s.fail_unpublished(&mut f.registries[0], 10).unwrap();
    assert!(s.reservation.is_none());
    assert_eq!(
        f.registries[0].resources(b.grant).unwrap().usage(),
        sophia_runtime::ContentMemoryUsage::default()
    );
    assert_eq!(f.budget.usage(), (1, 64)); // Physical reservation still in returned request.
    let ShellContentRecord::ResourceStatus(status) = f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .take_event()
        .unwrap()
        .record
    else {
        panic!("failure only")
    };
    assert_eq!(status.reason, ContentReason::RendererFailed as u16);
    assert_eq!(
        s.fail_unpublished(&mut f.registries[0], 10),
        Err(ContentStoreError::Stale)
    );
    let LiveShellCaptureOutcome::Returned { request, .. } = s.receipt.into_outcome() else {
        panic!("pre-use return")
    };
    assert_eq!(request.source.fd.as_raw_fd(), fd);
    drop(request);
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn resource_cancel_is_observed_but_credit_waits_for_the_real_receipt() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    f.submit(b);
    f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .cancel(
            TransactionId::from_raw(7),
            &ContentResourceCancel {
                grant: b.grant,
                resource: b.resource(),
            },
        )
        .unwrap();
    f.owner.observe_resources(&mut f.registries[0], 0).unwrap();
    assert!(f.owner.pending[0].as_ref().unwrap().withdrawn);
    assert_eq!(
        f.registries[0].resources(b.grant).unwrap().usage().backing,
        64
    );
    assert!(f.owner.take_completed(f.now).unwrap().completed.is_empty());
    let mut s = f.close().completed.pop().unwrap();
    s.fail_unpublished(&mut f.registries[0], 0).unwrap();
    let event = f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .take_event()
        .unwrap();
    assert_eq!(event.transaction, TransactionId::from_raw(7));
    let ShellContentRecord::ResourceStatus(status) = event.record else {
        panic!("failure only")
    };
    assert_eq!(status.reason, ContentReason::Cancelled as u16);
}

#[test]
fn disconnected_capture_settles_retired_registry_without_authorizing_successor() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    f.submit(b);
    assert!(f.registries[0].disconnect(b.grant));
    f.owner.observe_resources(&mut f.registries[0], 0).unwrap();
    assert!(f.registries[0].retired_bytes() > 0);
    let mut s = f.close().completed.pop().unwrap();
    let mut wrong = allocation_registry(b.grant);
    let _other = wrong
        .reserve_capture_image(b.grant, &b.metadata, 0)
        .unwrap();
    assert_eq!(
        s.fail_unpublished(&mut wrong, 0),
        Err(ContentStoreError::Stale)
    );
    assert!(s.reservation.is_some());
    s.fail_unpublished(&mut f.registries[0], 0).unwrap();
    assert_eq!(f.registries[0].retired_bytes(), 0);
    assert_eq!(wrong.resources(b.grant).unwrap().usage().backing, 64);
}

#[test]
fn uncertain_disposition_never_enters_credit_reconciliation_even_after_timeout() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    let mut reservation = Some(f.reserve(b));
    f.registries[0]
        .resources_mut(b.grant)
        .unwrap()
        .expire(100_000)
        .unwrap();
    let mut called = false;
    assert_eq!(
        reconcile_proven_reads(&LiveShellCaptureOutcome::Uncertain, || {
            called = true;
            f.registries[0].fail_capture(&mut reservation, ContentCaptureFailure::Timeout, 100_000)
        }),
        Err(ContentStoreError::Incomplete)
    );
    assert!(!called);
    assert!(reservation.is_some());
    assert_eq!(
        f.registries[0].resources(b.grant).unwrap().usage().backing,
        64
    );
    assert!(
        f.registries[0]
            .resources_mut(b.grant)
            .unwrap()
            .take_event()
            .is_none()
    );
}
