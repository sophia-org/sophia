//! Real custody and three-slot pools; only framebuffer destruction is supplied.
//! No DRM device, renderer, or physical presentation is exercised.
use super::*;
use crate::{LiveRendererFrameSlotAcquire, LiveRendererFrameSlotPool, LiveRendererFrameSlotToken};
use std::{
    cell::{Cell, RefCell},
    num::NonZeroU32,
    rc::Rc,
};

struct Slot {
    pool: Rc<RefCell<LiveRendererFrameSlotPool>>,
    token: LiveRendererFrameSlotToken,
}

impl Drop for Slot {
    fn drop(&mut self) {
        assert_eq!(
            self.pool.borrow_mut().release(self.token),
            crate::LiveRendererFrameSlotRelease::Released
        );
    }
}

fn slot(pool: &Rc<RefCell<LiveRendererFrameSlotPool>>) -> Slot {
    let LiveRendererFrameSlotAcquire::Acquired(token) = pool.borrow_mut().try_acquire() else {
        panic!("topology preparation exhausted the three-slot pool");
    };
    Slot {
        pool: pool.clone(),
        token,
    }
}

fn submission(
    pool: &Rc<RefCell<LiveRendererFrameSlotPool>>,
    fb: u32,
) -> BoxedRenderedPrimaryPlaneScanoutSubmission {
    LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: Box::new(slot(pool)),
        correlation: None,
        primary_plane: LibdrmNativePrimaryPlaneScanoutSubmission {
            resources: LibdrmNativePrimaryPlaneResourceBundle::new(
                NonZeroU32::new(fb).unwrap().into(),
                None,
                Size {
                    width: 640,
                    height: 480,
                },
            ),
            completion_fence: None,
        },
        submitted_after_page_flip_serial: Some(3),
        layout_witness: None,
    }
}

#[derive(Default)]
struct Device {
    fail: Cell<bool>,
    destroyed: RefCell<Vec<u32>>,
}
impl LibdrmNativePrimaryPlaneResourceDevice for Device {
    fn create_mode_blob_for_selection(
        &self,
        _: LibdrmNativePrimaryPlaneSelection,
    ) -> io::Result<u64> {
        unreachable!()
    }
    fn create_mode_blob(&self, _: ::drm::control::Mode) -> io::Result<u64> {
        unreachable!()
    }
    fn add_scanout_framebuffer_with_modifiers<B: ::drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_scanout_framebuffer_without_modifiers<B: ::drm::buffer::PlanarBuffer + ?Sized>(
        &self,
        _: &B,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn add_legacy_scanout_framebuffer<B: ::drm::buffer::Buffer + ?Sized>(
        &self,
        _: &B,
        _: u32,
        _: u32,
    ) -> io::Result<::drm::control::framebuffer::Handle> {
        unreachable!()
    }
    fn destroy_scanout_framebuffer(
        &self,
        fb: ::drm::control::framebuffer::Handle,
    ) -> io::Result<()> {
        if self.fail.get() {
            return Err(io::Error::from_raw_os_error(16));
        }
        self.destroyed.borrow_mut().push(fb.into());
        Ok(())
    }
    fn destroy_mode_blob(&self, _: u64) -> io::Result<()> {
        unreachable!()
    }
}

fn callback(serial: u64) -> LivePageFlipCallbackReport {
    LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(serial),
        },
    }
}

#[test]
fn topology_handoff_leaves_room_for_candidate_and_rollback_after_ordinary_presentation() {
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut head = PersistentScanoutCustody::default();
    let mut runtime = PersistentScanoutCustody::default();
    head.adopt_displayed(submission(&pool, 10)).unwrap();
    assert!(head.transfer_displayed_to(&mut runtime));
    assert!(head.displayed().is_none());
    assert!(device.destroyed.borrow().is_empty());
    runtime.accept_submission(submission(&pool, 11)).unwrap();
    assert!(matches!(
        runtime.present(&device, &callback(3), None),
        PersistentFlipOutcome::Waiting
    ));
    assert!(
        device.destroyed.borrow().is_empty(),
        "adoption and an old callback do not retire the on-plane owner"
    );
    assert!(matches!(
        runtime.present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert_eq!(*device.destroyed.borrow(), [10]);
    let candidate = slot(&pool);
    let rollback = slot(&pool);
    assert_eq!(
        pool.borrow_mut().try_acquire(),
        LiveRendererFrameSlotAcquire::Deferred
    );
    drop((candidate, rollback));
    assert!(
        runtime
            .retire_replaced_displayed(&device)
            .unwrap()
            .is_none()
    );
    assert_eq!(*device.destroyed.borrow(), [10, 11]);
}

#[test]
fn topology_rebind_retires_old_runtime_through_drm_and_preserves_failed_cleanup() {
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut previous = PersistentScanoutCustody::default();
    let mut head = PersistentScanoutCustody::default();
    let mut next = PersistentScanoutCustody::default();
    previous.adopt_displayed(submission(&pool, 20)).unwrap();
    head.adopt_displayed(submission(&pool, 21)).unwrap();
    // Represents a completed blocking topology commit. Failed rmfb still owns
    // its renderer slot after the old runtime has been discarded.
    device.fail.set(true);
    let cleanup = previous
        .retire_replaced_displayed(&device)
        .unwrap()
        .unwrap();
    assert!(previous.displayed().is_none());
    drop(previous);
    assert!(head.transfer_displayed_to(&mut next));
    let third = slot(&pool);
    assert_eq!(
        pool.borrow_mut().try_acquire(),
        LiveRendererFrameSlotAcquire::Deferred
    );
    device.fail.set(false);
    assert!(
        crate::retry_rendered_primary_plane_scanout_cleanup(&device, cleanup)
            .cleanup
            .is_none()
    );
    assert_eq!(*device.destroyed.borrow(), [20]);
    let available = slot(&pool);
    drop((third, available));
    assert!(next.retire_replaced_displayed(&device).unwrap().is_none());
    assert_eq!(*device.destroyed.borrow(), [20, 21]);
}

#[test]
fn topology_adoption_refusal_and_in_flight_rebind_leave_owners_untouched() {
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut head = PersistentScanoutCustody::default();
    let mut runtime = PersistentScanoutCustody::default();
    head.adopt_displayed(submission(&pool, 30)).unwrap();
    runtime.accept_submission(submission(&pool, 31)).unwrap();
    assert!(!head.transfer_displayed_to(&mut runtime));
    assert!(head.displayed().is_some());
    assert!(runtime.submitted().is_some());
    assert!(runtime.retire_replaced_displayed(&device).is_err());
    assert!(device.destroyed.borrow().is_empty());
    assert!(matches!(
        runtime.present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert!(
        !head.transfer_displayed_to(&mut runtime),
        "occupied destination must not drop either owner"
    );
    assert!(head.retire_replaced_displayed(&device).unwrap().is_none());
    assert!(
        runtime
            .retire_replaced_displayed(&device)
            .unwrap()
            .is_none()
    );
    assert_eq!(*device.destroyed.borrow(), [30, 31]);
}

#[test]
fn topology_transfer_keeps_predecessor_cleanup_in_the_source_ledger() {
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut head = PersistentScanoutCustody::default();
    let mut runtime = PersistentScanoutCustody::default();
    head.adopt_displayed(submission(&pool, 40)).unwrap();
    device.fail.set(true);
    assert!(!head.retire_displayed(&device).unwrap().unwrap().released);
    head.adopt_displayed(submission(&pool, 41)).unwrap();
    assert!(head.transfer_displayed_to(&mut runtime));
    assert!(head.cleanup_pending());
    assert!(head.displayed().is_none());
    assert!(runtime.displayed().is_some());
    device.fail.set(false);
    assert!(head.retry_cleanup(&device).unwrap().released);
    assert_eq!(*device.destroyed.borrow(), [40]);
    let candidate = slot(&pool);
    let rollback = slot(&pool);
    drop((candidate, rollback));
    assert!(
        runtime
            .retire_replaced_displayed(&device)
            .unwrap()
            .is_none()
    );
    assert_eq!(*device.destroyed.borrow(), [40, 41]);
}

#[test]
fn retiring_a_signalled_fence_removes_it_before_displayed_custody() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut custody = PersistentScanoutCustody::default();
    let (fence, mut signal) = UnixStream::pair().unwrap();
    signal.write_all(b"done").unwrap();
    let mut frame = submission(&pool, 20);
    frame.primary_plane.completion_fence = Some(fence.into());
    custody.accept_submission(frame).unwrap();
    assert!(custody.submitted().unwrap().completion_fence().is_some());
    assert!(matches!(
        custody.present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert!(custody.submitted().is_none());
    assert!(
        custody.displayed().unwrap().completion_fence().is_none(),
        "a signalled sync_file stays readable until closed; never carry it into an idle wait"
    );
    assert!(!custody.cleanup_pending());
}

#[test]
fn a_signalled_fence_with_a_refused_callback_uses_bounded_service() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let pool = Rc::new(RefCell::new(LiveRendererFrameSlotPool::new()));
    let device = Device::default();
    let mut custody = PersistentScanoutCustody::default();
    let (fence, mut signal) = UnixStream::pair().unwrap();
    signal.write_all(b"done").unwrap();
    let mut frame = submission(&pool, 21);
    frame.primary_plane.completion_fence = Some(fence.into());
    custody.accept_submission(frame).unwrap();
    // Serial 3 is at the submission baseline, so the callback cannot retire it.
    assert!(matches!(
        custody.present(&device, &callback(3), None),
        PersistentFlipOutcome::Waiting
    ));
    let submitted = custody.submitted().unwrap();
    let observed = submitted.completion_fence_status().unwrap();
    assert_eq!(observed, LibdrmNativeCompletionFenceStatus::Signaled);
    let mut wait = crate::LiveNativeCompletionWait::default();
    wait.observe_fence(submitted.completion_fence().unwrap(), observed);
    assert!(
        wait.descriptors.is_empty(),
        "a refused signalled fence must not spin poll"
    );
    assert!(wait.short_service);
    drop(wait);
    assert!(custody.displayed().is_none());
    assert!(matches!(
        custody.present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert!(custody.displayed().unwrap().completion_fence().is_none());
}

#[test]
fn another_heads_progress_cannot_signal_or_release_a_pending_fence() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let device = Device::default();
    let pools = [
        Rc::new(RefCell::new(LiveRendererFrameSlotPool::new())),
        Rc::new(RefCell::new(LiveRendererFrameSlotPool::new())),
    ];
    let mut heads: [PersistentScanoutCustody; 2] = Default::default();
    let mut signals = Vec::new();
    for (index, head) in heads.iter_mut().enumerate() {
        let (fence, signal) = UnixStream::pair().unwrap();
        let mut frame = submission(&pools[index], 30 + index as u32);
        frame.primary_plane.completion_fence = Some(fence.into());
        head.accept_submission(frame).unwrap();
        signals.push(signal);
    }
    signals[1].write_all(b"ready").unwrap();
    assert_eq!(
        heads[1]
            .submitted()
            .unwrap()
            .completion_fence_status()
            .unwrap(),
        LibdrmNativeCompletionFenceStatus::Signaled
    );
    assert!(matches!(
        heads[1].present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert!(
        heads[1]
            .retire_displayed(&device)
            .unwrap()
            .unwrap()
            .released
    );
    assert_eq!(*device.destroyed.borrow(), [31]);

    // Repeated inspection and other-head cleanup leave the delayed head's
    // descriptor and submission intact. Readiness is not a retirement proof.
    for _ in 0..3 {
        let submitted = heads[0].submitted().unwrap();
        let status = submitted.completion_fence_status().unwrap();
        assert_eq!(status, LibdrmNativeCompletionFenceStatus::Pending);
        let mut wait = crate::LiveNativeCompletionWait::default();
        wait.observe_fence(submitted.completion_fence().unwrap(), status);
        assert_eq!(wait.descriptors.len(), 1);
        assert!(!wait.short_service);
        assert!(heads[0].displayed().is_none());
        assert_eq!(*device.destroyed.borrow(), [31]);
    }
    // The same retained fd sees a later signal, without cancellation, detach,
    // replacement or framebuffer destruction making it ready first.
    signals[0].write_all(b"late").unwrap();
    assert_eq!(
        heads[0]
            .submitted()
            .unwrap()
            .completion_fence_status()
            .unwrap(),
        LibdrmNativeCompletionFenceStatus::Signaled
    );
    assert!(matches!(
        heads[0].present(&device, &callback(4), None),
        PersistentFlipOutcome::Presented { .. }
    ));
    assert!(heads[0].submitted().is_none());
    assert!(heads[0].displayed().unwrap().completion_fence().is_none());
    assert!(
        heads[0]
            .retire_displayed(&device)
            .unwrap()
            .unwrap()
            .released
    );
    assert_eq!(*device.destroyed.borrow(), [31, 30]);
}
