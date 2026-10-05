use super::*;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};

fn allocation(bytes: u64) -> OwnedFd {
    let fd = rustix::fs::memfd_create(c"capture-source", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
    rustix::fs::ftruncate(&fd, bytes).unwrap();
    fd
}

fn frame(fd: &OwnedFd) -> NativeMultiPlaneDmaBufFrame<'_> {
    NativeMultiPlaneDmaBufFrame {
        width: 16,
        height: 16,
        format: 0x3432_5258,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(NativeDmaBufPlane {
                fd: fd.as_fd(),
                offset: 0,
                stride: 64,
            }),
            None,
            None,
            None,
        ],
    }
}

#[test]
fn capture_source_identity_follows_the_allocation_not_fd_numbers() {
    let mut first = allocation(4096);
    let duplicate = first.try_clone().unwrap();
    let original = NativeCaptureSourceFingerprint::from_frame(frame(&first)).unwrap();
    assert_eq!(
        original,
        NativeCaptureSourceFingerprint::from_frame(frame(&duplicate)).unwrap()
    );
    let replacement = allocation(4096);
    let fd_number = first.as_raw_fd();
    rustix::io::dup2(&replacement, &mut first).unwrap();
    assert_eq!(first.as_raw_fd(), fd_number);
    assert_ne!(
        original,
        NativeCaptureSourceFingerprint::from_frame(frame(&first)).unwrap()
    );
    assert_eq!(
        original,
        NativeCaptureSourceFingerprint::from_frame(frame(&duplicate)).unwrap()
    );
}

#[test]
fn capture_source_identity_includes_every_import_layout_input() {
    let fd = allocation(8192);
    let base = frame(&fd);
    let original = NativeCaptureSourceFingerprint::from_frame(base).unwrap();
    let mut variants = [base; 7];
    variants[0].width += 1;
    variants[1].height += 1;
    variants[2].format = 0x3432_5241;
    variants[3].modifier = 1;
    variants[4].planes[0].as_mut().unwrap().offset += 4;
    variants[5].planes[0].as_mut().unwrap().stride += 4;
    variants[6].plane_count = 2;
    variants[6].planes[1] = base.planes[0];
    for changed in variants {
        assert_ne!(
            original,
            NativeCaptureSourceFingerprint::from_frame(changed).unwrap()
        );
    }
}

#[test]
fn capture_source_charges_actual_deduplicated_plane_allocations() {
    let first = allocation(16_384);
    let duplicate = first.try_clone().unwrap();
    let other = allocation(32_768);
    let mut shared = frame(&first);
    shared.plane_count = 2;
    shared.planes[1] = Some(NativeDmaBufPlane {
        fd: duplicate.as_fd(),
        offset: 1024,
        stride: 64,
    });
    assert_eq!(
        NativeCaptureSourceFingerprint::from_frame(shared)
            .unwrap()
            .allocation_bytes(shared),
        Some(16_384)
    );
    shared.planes[1].as_mut().unwrap().fd = other.as_fd();
    assert_eq!(
        NativeCaptureSourceFingerprint::from_frame(shared)
            .unwrap()
            .allocation_bytes(shared),
        Some(49_152)
    );
}

#[test]
fn capture_source_unknown_or_empty_allocation_is_not_retainable() {
    let empty = allocation(0);
    let empty_frame = frame(&empty);
    assert_eq!(
        NativeCaptureSourceFingerprint::from_frame(empty_frame)
            .unwrap()
            .allocation_bytes(empty_frame),
        None
    );
    let (read, _write) = std::os::unix::net::UnixStream::pair().unwrap();
    let read: OwnedFd = read.into();
    let pipe_frame = frame(&read);
    assert_eq!(
        NativeCaptureSourceFingerprint::from_frame(pipe_frame)
            .unwrap()
            .allocation_bytes(pipe_frame),
        None
    );
}

#[test]
fn capture_source_budget_is_shared_across_execution_contexts() {
    let budget = std::rc::Rc::new(NativeCaptureSourceBudget::default());
    let first = NativeCaptureSourceCache::with_budget(budget.clone());
    let second = NativeCaptureSourceCache::with_budget(budget.clone());
    let charge = first.budget.reserve(CAPTURE_SOURCE_BYTE_LIMIT - 1).unwrap();
    assert!(second.budget.reserve(2).is_none());
    let last = second.budget.reserve(1).unwrap();
    assert_eq!(budget.usage(), (2, CAPTURE_SOURCE_BYTE_LIMIT));
    drop(charge);
    assert_eq!(budget.usage(), (1, 1));
    assert!(first.budget.reserve(CAPTURE_SOURCE_BYTE_LIMIT).is_none());
    drop(last);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn capture_source_budget_bounds_entries_even_for_small_allocations() {
    let budget = std::rc::Rc::new(NativeCaptureSourceBudget::default());
    let charges = (0..CAPTURE_SOURCE_LIMIT)
        .map(|_| budget.reserve(1).unwrap())
        .collect::<Vec<_>>();
    assert!(budget.reserve(1).is_none());
    assert!(budget.reserve(0).is_none());
    assert_eq!(
        budget.usage(),
        (CAPTURE_SOURCE_LIMIT, CAPTURE_SOURCE_LIMIT as u64)
    );
    drop(charges);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn capture_source_rejects_incomplete_plane_layouts_before_fingerprinting() {
    let fd = allocation(4096);
    let mut invalid = frame(&fd);
    invalid.plane_count = 2;
    assert_eq!(
        NativeCaptureSourceFingerprint::from_frame(invalid).unwrap_err(),
        NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor
    );
    invalid.plane_count = 0;
    assert!(NativeCaptureSourceFingerprint::from_frame(invalid).is_err());
}

fn quarantine_record(
    budget: &std::rc::Rc<NativeCaptureSourceBudget>,
) -> NativeCaptureSourceQuarantine {
    NativeCaptureSourceQuarantine::new(
        budget,
        // Opaque sentinel for pure accounting tests; never passed to EGL.
        unsafe { khronos_egl::Image::from_ptr(std::ptr::dangling_mut()) },
        vec![allocation(4096)],
        budget.reserve(4096),
    )
}

#[test]
fn failed_source_cleanup_keeps_storage_charged_until_success() {
    let budget = std::rc::Rc::new(NativeCaptureSourceBudget::default());
    let mut cache = NativeCaptureSourceCache::with_budget(budget.clone());
    cache.quarantine.push(quarantine_record(&budget));
    let mut attempts = 0;
    cache.retry_quarantine_with(|_| {
        attempts += 1;
        false
    });
    assert_eq!(attempts, 1);
    assert_eq!(budget.usage(), (1, 4096));
    assert!(budget.poisoned());
    assert!(
        budget.reserve(1).is_none(),
        "another context must not acquire while cleanup is uncertain"
    );
    assert_eq!(cache.resident_bytes(), 4096);
    assert_eq!(cache.quarantine.len(), 1);
    assert_eq!(
        rustix::fs::fstat(&cache.quarantine[0]._planes[0])
            .unwrap()
            .st_size,
        4096
    );
    cache.retry_quarantine_with(|_| true);
    assert!(cache.quarantine.is_empty());
    assert_eq!(budget.usage(), (0, 0));
    assert!(!budget.poisoned());
}

#[test]
fn failed_source_cleanup_survives_execution_until_display_teardown() {
    let budget = std::rc::Rc::new(NativeCaptureSourceBudget::default());
    let weak = std::rc::Rc::downgrade(&budget);
    let mut cache = NativeCaptureSourceCache::with_budget(budget.clone());
    cache.quarantine.push(quarantine_record(&budget));
    drop(cache);
    assert_eq!(budget.usage(), (1, 4096));
    assert!(budget.poisoned());
    assert_eq!(budget.graveyard.borrow().len(), 1);
    assert_eq!(
        rustix::fs::fstat(&budget.graveyard.borrow()[0]._planes[0])
            .unwrap()
            .st_size,
        4096
    );
    drop(budget);
    assert!(
        weak.upgrade().is_none(),
        "display graveyard must not form an Rc cycle"
    );
}

#[test]
fn failed_transient_source_cleanup_blocks_the_renderer_budget() {
    let budget = std::rc::Rc::new(NativeCaptureSourceBudget::default());
    let charge = budget.reserve(4096).unwrap();
    let mut cache = NativeCaptureSourceCache::with_budget(budget.clone());
    cache.quarantine.push(NativeCaptureSourceQuarantine::new(
        &budget,
        unsafe { khronos_egl::Image::from_ptr(std::ptr::dangling_mut()) },
        vec![allocation(4096)],
        None,
    ));
    assert_eq!(budget.usage(), (2, u64::MAX));
    assert!(budget.poisoned());
    assert!(budget.reserve(1).is_none());
    drop(charge);
    assert_eq!(budget.usage(), (1, u64::MAX));
    cache.retry_quarantine_with(|_| true);
    assert_eq!(budget.usage(), (0, 0));
    assert!(!budget.poisoned());
}
