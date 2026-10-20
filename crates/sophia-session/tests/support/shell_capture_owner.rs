//! Real mailbox + Session correlation, with shutdown before GPU submission.
use super::*;
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
        }
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
        (
            self.owner
                .submit(binding, r, self.now + Duration::from_secs(10))
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
        let (error, r) = f.owner.submit(duplicate, r, f.now).unwrap_err();
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
    let (error, r) = f.owner.submit(binding(1, 5), r, f.now).unwrap_err();
    assert_eq!(error, SubmitFailure::Capacity);
    assert_eq!(r.source.fd.as_raw_fd(), fd);
    drop(r);
    assert_eq!(f.close().completed.len(), 4);
    assert_eq!(f.budget.usage(), (0, 0));
}

#[test]
fn mismatched_source_metadata_or_admission_never_enters_mailbox() {
    for field in 0..10 {
        let mut f = Fixture::new();
        let mut b = binding(1, 1);
        let r = f.request();
        let fd = r.source.fd.as_raw_fd();
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
            _ => b.plane.fd_index = 1,
        }
        let (error, r) = f.owner.submit(b, r, f.now).unwrap_err();
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
    let (error, r) = f.owner.submit(binding(1, 1), r, f.now).unwrap_err();
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
    assert_eq!(
        f.owner.submit(binding(2, 1), r, f.now).unwrap_err().0,
        SubmitFailure::Closed
    );
}
