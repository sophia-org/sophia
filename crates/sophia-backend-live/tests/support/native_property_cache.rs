use super::*;
use std::cell::Cell;

use crate::drm::framebuffer_test_device as framebuffer;

struct Device {
    calls: Cell<usize>,
    missing: Cell<bool>,
    failed: Cell<bool>,
    value: Cell<u64>,
    base: u32,
    commits: Cell<usize>,
    out_fences: Cell<usize>,
}

impl Device {
    fn new(base: u32) -> Self {
        Self {
            calls: Cell::new(0),
            missing: Cell::new(false),
            failed: Cell::new(false),
            value: Cell::new(1),
            base,
            commits: Cell::new(0),
            out_fences: Cell::new(0),
        }
    }
    fn properties(&self, names: &[&str]) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.calls.set(self.calls.get() + 1);
        if self.failed.get() {
            return Err(io::Error::from_raw_os_error(5));
        }
        Ok(LibdrmNativePropertyHandleSet::new(
            names
                .iter()
                .enumerate()
                .map(|(i, name)| (*name, drm::control::from_u32(self.base + i as u32).unwrap())),
        ))
    }
}

impl LibdrmNativePropertyLookupDevice for Device {
    fn connector_property_handles(
        &self,
        _: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.properties(&["CRTC_ID"])
    }
    fn crtc_property_handles(
        &self,
        _: drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.properties(&["MODE_ID", "ACTIVE", "OUT_FENCE_PTR"])
    }
    fn plane_property_handles(
        &self,
        _: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        self.properties(if self.missing.get() {
            &[]
        } else {
            &[
                "FB_ID", "CRTC_ID", "SRC_X", "SRC_Y", "SRC_W", "SRC_H", "CRTC_X", "CRTC_Y",
                "CRTC_W", "CRTC_H",
            ]
        })
    }
    fn connector_property_value(
        &self,
        _: drm::control::connector::Handle,
        _: drm::control::property::Handle,
    ) -> io::Result<Option<u64>> {
        Ok(Some(self.value.get()))
    }
}

impl LibdrmNativeAtomicCommitDevice for Device {
    fn submit_atomic_commit(
        &self,
        _: drm::control::AtomicCommitFlags,
        _: drm::control::atomic::AtomicModeReq,
    ) -> io::Result<()> {
        self.commits.set(self.commits.get() + 1);
        if self.failed.get() {
            Err(io::Error::from_raw_os_error(5))
        } else {
            Ok(())
        }
    }
    fn submit_atomic_commit_with_out_fence(
        &self,
        _: drm::control::AtomicCommitFlags,
        _: drm::control::atomic::AtomicModeReq,
        _: drm::control::crtc::Handle,
        _: drm::control::property::Handle,
    ) -> io::Result<Option<OwnedFd>> {
        self.out_fences.set(self.out_fences.get() + 1);
        if self.failed.get() {
            Err(io::Error::from_raw_os_error(5))
        } else {
            Ok(None)
        }
    }
}

fn discover<D: LibdrmNativePropertyLookupDevice>(
    d: &D,
    ids: [u32; 3],
) -> LibdrmNativePrimaryPlanePropertyDiscoveryResult {
    crate::discover_native_primary_plane_property_handles(
        d,
        drm::control::from_u32(ids[0]).unwrap(),
        drm::control::from_u32(ids[1]).unwrap(),
        drm::control::from_u32(ids[2]).unwrap(),
    )
}

#[test]
fn same_target_reuses_ids_but_values_and_commits_stay_live() {
    let device = Device::new(100);
    let cache = NativePrimaryPropertyCache::default();
    let wrapped = TargetPropertyDevice::new(&device, &cache, 0, 1);
    let expected = discover(&device, [1, 2, 3]).properties.unwrap();
    device.calls.set(0);
    for _ in 0..1000 {
        assert_eq!(discover(&wrapped, [1, 2, 3]).properties, Some(expected));
    }
    assert_eq!(device.calls.get(), 3);
    let connector = drm::control::from_u32(1).unwrap();
    let property = drm::control::from_u32(100).unwrap();
    for value in [1, 29, 0] {
        device.value.set(value);
        assert_eq!(
            wrapped
                .connector_property_value(connector, property)
                .unwrap(),
            Some(value)
        );
    }
    wrapped
        .submit_atomic_commit(
            drm::control::AtomicCommitFlags::TEST_ONLY,
            Default::default(),
        )
        .unwrap();
    wrapped
        .submit_atomic_commit_with_out_fence(
            drm::control::AtomicCommitFlags::NONBLOCK,
            Default::default(),
            drm::control::from_u32(2).unwrap(),
            property,
        )
        .unwrap();
    assert_eq!((device.commits.get(), device.out_fences.get()), (1, 1));
}

#[test]
fn generation_device_and_each_object_identity_require_fresh_discovery() {
    let device = Device::new(100);
    let cache = NativePrimaryPropertyCache::default();
    let cases = [
        (0, 1, [1, 2, 3]),
        (0, 2, [1, 2, 3]),
        (1, 2, [1, 2, 3]),
        (1, 2, [4, 2, 3]),
        (1, 2, [4, 5, 3]),
        (1, 2, [4, 5, 6]),
        (0, 1, [1, 2, 3]),
    ];
    for (i, (group, generation, ids)) in cases.into_iter().enumerate() {
        let wrapped = TargetPropertyDevice::new(&device, &cache, group, generation);
        assert!(discover(&wrapped, ids).properties.is_some());
        assert!(discover(&wrapped, ids).properties.is_some());
        assert_eq!(device.calls.get(), 3 * (i + 1));
    }
    cache.invalidate();
    assert!(
        discover(&TargetPropertyDevice::new(&device, &cache, 0, 1), [1, 2, 3])
            .properties
            .is_some()
    );
    assert_eq!(device.calls.get(), 24);
}

#[test]
fn failed_and_missing_discoveries_retry_and_cannot_restore_an_old_entry() {
    let device = Device::new(100);
    let cache = NativePrimaryPropertyCache::default();
    assert!(
        discover(&TargetPropertyDevice::new(&device, &cache, 0, 1), [1, 2, 3])
            .properties
            .is_some()
    );
    device.failed.set(true);
    let replacement = TargetPropertyDevice::new(&device, &cache, 0, 2);
    for _ in 0..2 {
        assert_eq!(
            discover(&replacement, [1, 2, 3]).status,
            LibdrmNativePrimaryPlanePropertyDiscoveryStatus::ReadFailed
        );
    }
    assert_eq!(device.calls.get(), 5);
    device.failed.set(false);
    device.missing.set(true);
    for _ in 0..2 {
        assert_eq!(
            discover(&replacement, [1, 2, 3]).status,
            LibdrmNativePrimaryPlanePropertyDiscoveryStatus::MissingPlaneProperty
        );
    }
    assert_eq!(device.calls.get(), 11);
    device.missing.set(false);
    assert!(
        discover(&TargetPropertyDevice::new(&device, &cache, 0, 1), [1, 2, 3])
            .properties
            .is_some()
    );
    assert_eq!(device.calls.get(), 14);
}

#[test]
fn native_owner_replacement_and_two_heads_keep_separate_entries() {
    let first = Device::new(100);
    let second = Device::new(200);
    let a = NativePrimaryPropertyCache::default();
    let b = NativePrimaryPropertyCache::default();
    let pa = discover(&TargetPropertyDevice::new(&first, &a, 0, 1), [1, 2, 3]).properties;
    let pb = discover(&TargetPropertyDevice::new(&second, &b, 0, 1), [1, 2, 3]).properties;
    assert_ne!(pa, pb);
    let replacement = NativePrimaryPropertyCache::default();
    assert_eq!(
        discover(
            &TargetPropertyDevice::new(&second, &replacement, 0, 1),
            [1, 2, 3]
        )
        .properties,
        pb
    );
    assert_eq!((first.calls.get(), second.calls.get()), (3, 6));
    // Replacing a card in the same native group must not reuse its IDs,
    // even if a caller retains the head and all KMS numeric IDs are equal.
    assert_eq!(
        discover(&TargetPropertyDevice::new(&second, &a, 2, 1), [1, 2, 3]).properties,
        pb
    );
    assert_eq!(second.calls.get(), 9);
}

#[test]
fn atomic_failure_is_returned_and_invalidates_discovery() {
    let device = Device::new(100);
    let cache = NativePrimaryPropertyCache::default();
    let wrapped = TargetPropertyDevice::new(&device, &cache, 0, 1);
    assert!(discover(&wrapped, [1, 2, 3]).properties.is_some());
    device.failed.set(true);
    assert_eq!(
        wrapped
            .submit_atomic_commit(
                drm::control::AtomicCommitFlags::TEST_ONLY,
                Default::default()
            )
            .unwrap_err()
            .raw_os_error(),
        Some(5)
    );
    device.failed.set(false);
    assert!(discover(&wrapped, [1, 2, 3]).properties.is_some());
    assert_eq!(device.calls.get(), 6);
}

#[test]
fn cached_preparation_preserves_framebuffer_failure_and_cleanup() {
    let device = framebuffer::Device::new();
    let cache = NativePrimaryPropertyCache::default();
    let wrapped = TargetPropertyDevice::new(&device, &cache, 0, 1);
    let selection = LibdrmNativePrimaryPlaneSelectionResult {
        status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
        selection: Some(LibdrmNativePrimaryPlaneSelection::new(
            drm::control::from_u32(1).unwrap(),
            drm::control::from_u32(2).unwrap(),
            drm::control::from_u32(3).unwrap(),
            framebuffer::SIZE,
            None,
        )),
    };
    // A warm hit must not bypass actual resource preparation or its failures.
    assert!(discover(&wrapped, [1, 2, 3]).properties.is_some());
    let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    let result = crate::prepare_native_primary_plane_scanout_from_selection_and_renderer_dma_bufs_with_policy(
        &wrapped, selection, framebuffer::descriptor(framebuffer::DCC),
        [Some(a.into()), Some(b.into()), None, None],
        LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip());
    assert!(result.prepared.is_none());
    assert_eq!(device.imports.get(), 2);
    assert_eq!(device.addfb_calls.get(), 1);
    assert_eq!(*device.closed.borrow(), vec![200]);
    assert!(cache.entry.get().is_none());
}

#[test]
fn warm_preparation_still_tests_commits_and_cleans_up_a_rejected_frame() {
    for errno in [11, 22] {
        let device = framebuffer::Device::new();
        device.errno.set(None);
        let cache = NativePrimaryPropertyCache::default();
        let wrapped = TargetPropertyDevice::new(&device, &cache, 0, 1);
        let selection = LibdrmNativePrimaryPlaneSelectionResult {
            status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
            selection: Some(LibdrmNativePrimaryPlaneSelection::new(
                drm::control::from_u32(1).unwrap(),
                drm::control::from_u32(2).unwrap(),
                drm::control::from_u32(3).unwrap(),
                framebuffer::SIZE,
                None,
            )),
        };
        // Successful first preparation establishes the entry; cancellation
        // must release resources without treating valid metadata as stale.
        let prepare = || {
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            crate::prepare_native_primary_plane_scanout_from_selection_and_renderer_dma_bufs_with_policy(
                &wrapped, selection, framebuffer::descriptor(framebuffer::DCC),
                [Some(a.into()), Some(b.into()), None, None],
                LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip())
                .prepared.expect("framebuffer accepted")
        };
        let first = prepare();
        assert!(
            crate::cancel_prepared_native_primary_plane_scanout(&wrapped, first)
                .cleanup
                .is_none()
        );
        assert!(cache.entry.get().is_some());
        let second = prepare();
        device.test_errno.set(Some(errno));
        let result = crate::submit_prepared_native_primary_plane_scanout(&wrapped, second);
        assert!(result.submission.is_none());
        assert!(result.cleanup.is_none());
        assert_eq!(
            result.submit,
            Some(if errno == 11 {
                LibdrmNativeAtomicCommitSubmitStatus::WouldBlock
            } else {
                LibdrmNativeAtomicCommitSubmitStatus::Rejected
            })
        );
        assert_eq!(device.tests.borrow().len(), 1);
        assert_eq!(*device.destroyed.borrow(), vec![100, 100]);
        assert_eq!(*device.closed.borrow(), vec![200, 201]);
        assert!(cache.entry.get().is_none());
    }
}
