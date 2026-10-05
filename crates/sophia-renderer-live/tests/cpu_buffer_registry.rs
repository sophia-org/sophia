use sophia_protocol::{Rect, Size};
use sophia_renderer_live::{
    LiveCpuBufferPatch, LiveCpuBufferPatchBatch, LiveCpuBufferPatchRegion, LiveCpuBufferRegistry,
    LiveCpuBufferRegistryError, LiveCpuBufferSource, LiveCpuBufferUpdate,
};
use std::sync::Arc;

fn buffer(handle: u64, generation: u64) -> LiveCpuBufferSource {
    LiveCpuBufferSource {
        handle,
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: u32::from_le_bytes(*b"XR24"),
        generation,
        bytes: Arc::new(vec![0; 16]),
    }
}

#[test]
fn replacement_and_patch_preserve_generation_order() {
    let mut registry = LiveCpuBufferRegistry::new();
    assert!(
        registry
            .apply(LiveCpuBufferUpdate::Replace(buffer(7, 2)))
            .unwrap()
    );
    assert!(
        registry
            .apply(LiveCpuBufferUpdate::Patch(LiveCpuBufferPatch {
                handle: 7,
                size: Size {
                    width: 2,
                    height: 2
                },
                stride: 8,
                format: u32::from_le_bytes(*b"XR24"),
                generation: 3,
                rect: Rect {
                    x: 1,
                    y: 0,
                    width: 1,
                    height: 2
                },
                bytes: vec![1, 2, 3, 4, 5, 6, 7, 8].into(),
            }))
            .unwrap()
    );
    let stored = registry.get(7).unwrap();
    assert_eq!(stored.generation, 3);
    assert_eq!(&stored.bytes[4..8], &[1, 2, 3, 4]);
    assert_eq!(&stored.bytes[12..16], &[5, 6, 7, 8]);

    assert!(
        !registry
            .apply(LiveCpuBufferUpdate::Replace(buffer(7, 1)))
            .unwrap()
    );
    assert_eq!(registry.get(7).unwrap().generation, 3);
}

#[test]
fn malformed_replacement_fails_closed() {
    let mut registry = LiveCpuBufferRegistry::new();
    let mut malformed = buffer(4, 1);
    Arc::make_mut(&mut malformed.bytes).pop();
    assert_eq!(
        registry.apply(LiveCpuBufferUpdate::Replace(malformed)),
        Err(LiveCpuBufferRegistryError::InvalidBufferMetadata)
    );
    assert!(registry.get(4).is_none());
}

#[test]
fn malformed_patch_fails_closed_without_mutating_base() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(9, 1)))
        .unwrap();
    let before = registry.get(9).unwrap().clone();
    let error = registry
        .apply(LiveCpuBufferUpdate::Patch(LiveCpuBufferPatch {
            handle: 9,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation: 2,
            rect: Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            },
            bytes: vec![1; 8].into(),
        }))
        .unwrap_err();
    assert_eq!(error, LiveCpuBufferRegistryError::InvalidPatchBounds);
    assert_eq!(registry.get(9), Some(&before));
}

#[test]
fn patch_batch_applies_atomically_at_one_generation() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(11, 1)))
        .unwrap();
    let update = LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
        handle: 11,
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: u32::from_le_bytes(*b"XR24"),
        generation: 2,
        patches: vec![
            LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                bytes: vec![1, 2, 3, 4].into(),
            },
            LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 1,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                bytes: vec![5, 6, 7, 8].into(),
            },
        ],
    });
    assert!(registry.apply(update).unwrap());
    let stored = registry.get(11).unwrap();
    assert_eq!(stored.generation, 2);
    assert_eq!(&stored.bytes[0..4], &[1, 2, 3, 4]);
    assert_eq!(&stored.bytes[12..16], &[5, 6, 7, 8]);
}

#[test]
fn malformed_patch_batch_does_not_apply_valid_prefix() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(12, 1)))
        .unwrap();
    let before = registry.get(12).unwrap().clone();
    let error = registry
        .apply(LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
            handle: 12,
            size: before.size,
            stride: before.stride,
            format: before.format,
            generation: 2,
            patches: vec![
                LiveCpuBufferPatchRegion {
                    rect: Rect {
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1,
                    },
                    bytes: vec![9; 4].into(),
                },
                LiveCpuBufferPatchRegion {
                    rect: Rect {
                        x: 2,
                        y: 0,
                        width: 1,
                        height: 1,
                    },
                    bytes: vec![7; 4].into(),
                },
            ],
        }))
        .unwrap_err();
    assert_eq!(error, LiveCpuBufferRegistryError::InvalidPatchBounds);
    assert_eq!(registry.get(12), Some(&before));
}

#[test]
fn retention_drops_unreferenced_renderer_resources() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(1, 1)))
        .unwrap();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(2, 1)))
        .unwrap();
    registry.retain_handles(|handle| handle == 2);
    assert!(registry.get(1).is_none());
    assert!(registry.get(2).is_some());
}

/// A lease keeps the pixels it was handed when a patch arrives.
///
/// The counterexample the model produces when the split is removed: a
/// presentation is planned against a buffer, the client patches the same
/// handle, and the presentation composes pixels from a generation it was never
/// planned against. The bytes are shared, so nothing about the buffer's
/// identity would have said the frame changed underneath it.
///
/// `NC1` in `validation/specula/stable-x-backing-lease-modeling-brief.md`,
/// which violates `LeasedContentStable`.
#[test]
fn cow_split_preserves_leased_bytes() {
    let mut registry = LiveCpuBufferRegistry::new();
    let mut base = buffer(11, 1);
    Arc::make_mut(&mut base.bytes).fill(0xaa);
    registry
        .apply(LiveCpuBufferUpdate::Replace(base))
        .expect("the base must be admitted");

    // A presentation takes the bytes it was planned against.
    let leased = Arc::clone(&registry.get(11).unwrap().bytes);
    assert_eq!(leased.as_slice(), [0xaa; 16]);

    registry
        .apply(LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
            handle: 11,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation: 2,
            patches: vec![LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                bytes: vec![0xbb; 8].into(),
            }],
        }))
        .expect("the patch must be admitted");

    let current = registry.get(11).unwrap();
    assert_eq!(
        current.bytes[..8],
        [0xbb; 8],
        "the registry must hold the patched row"
    );
    assert_eq!(
        leased.as_slice(),
        [0xaa; 16],
        "a presentation still holding the buffer must read what it was handed"
    );
    assert!(
        !Arc::ptr_eq(&leased, &current.bytes),
        "the patch must land on a copy while the lease holds the original"
    );
}

/// Once nothing else reads the bytes, a patch stops copying them.
///
/// This is the other half of the same guard, and the half that makes the
/// optimization worth anything: a stable toplevel whose presentations retire
/// before the next update patches one allocation forever. Checking allocation
/// identity rather than a counter is what makes it a statement about copying.
///
/// `NC4` in the brief, which violates `LeasedAllocationsLive` when retirement
/// stops tracking who still holds an allocation.
#[test]
fn patch_after_lease_release_mutates_in_place() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(12, 1)))
        .expect("the base must be admitted");

    let patch = |generation: u64, fill: u8| {
        LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
            handle: 12,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation,
            patches: vec![LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                bytes: vec![fill; 8].into(),
            }],
        })
    };

    // Hold it, patch it, and let go: the split is expected here.
    let leased = Arc::clone(&registry.get(12).unwrap().bytes);
    registry.apply(patch(2, 0x11)).expect("first patch");
    let after_split = Arc::as_ptr(&registry.get(12).unwrap().bytes);
    drop(leased);

    // With no holder left, every later patch writes into the same allocation.
    for (index, generation) in (3..8).enumerate() {
        registry
            .apply(patch(generation, 0x20 + u8::try_from(index).unwrap()))
            .expect("later patches");
        assert_eq!(
            Arc::as_ptr(&registry.get(12).unwrap().bytes),
            after_split,
            "patch at generation {generation} must reuse the allocation, not copy it"
        );
    }
}

/// A refused patch leaves the base alone, and does not copy it either.
///
/// The split must happen after validation. Splitting first would leave the
/// registry pointing at a fresh allocation holding exactly the old pixels --
/// correct, and a copy made for a write that never happened.
#[test]
fn a_refused_patch_neither_mutates_nor_copies_the_base() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(13, 1)))
        .expect("the base must be admitted");
    let before = Arc::clone(&registry.get(13).unwrap().bytes);

    assert_eq!(
        registry.apply(LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
            handle: 13,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation: 2,
            patches: vec![LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                // One byte short of the rectangle it claims.
                bytes: vec![0xcc; 7].into(),
            }],
        })),
        Err(LiveCpuBufferRegistryError::InvalidPatchBytes)
    );

    let current = &registry.get(13).unwrap().bytes;
    assert_eq!(
        current.as_slice(),
        [0; 16],
        "a refused patch must not write"
    );
    assert!(
        Arc::ptr_eq(&before, current),
        "a refused patch must not copy the base either"
    );
}

/// Dropping a handle does not invalidate a presentation still holding it.
///
/// A resize retires a handle and starts a new one, and residency
/// reconciliation later drops the old entry. A frame queued against the old
/// size was planned against those exact bytes and composes from them after the
/// registry has forgotten the handle. Sharing the allocation is what makes the
/// two facts compatible; owning it in the registry alone would make eviction a
/// use-after-free in the safe-code sense of composing from a resurrected buffer.
///
/// `NC6` in `validation/specula/stable-x-backing-lease-modeling-brief.md`,
/// which violates `LeasedAllocationsLive`.
#[test]
fn an_evicted_handle_keeps_its_bytes_for_whoever_still_holds_them() {
    let mut registry = LiveCpuBufferRegistry::new();
    let mut old_epoch = buffer(14, 1);
    Arc::make_mut(&mut old_epoch.bytes).fill(0x7e);
    registry
        .apply(LiveCpuBufferUpdate::Replace(old_epoch))
        .expect("the pre-resize buffer must be admitted");

    // A Present is planned against this handle at this size.
    let queued = Arc::clone(&registry.get(14).unwrap().bytes);

    // The resize starts a new handle, and reconciliation drops the old one.
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(15, 1)))
        .expect("the post-resize buffer must be admitted");
    registry.retain_handles(|handle| handle == 15);

    assert!(
        registry.get(14).is_none(),
        "the pre-resize handle must be gone from the registry"
    );
    assert_eq!(
        queued.as_slice(),
        [0x7e; 16],
        "a queued Present must still compose the bytes it was planned against"
    );
}

/// A warmed registry stops allocating.
///
/// The milestone's exit criterion is "no steady-state allocation growth", and
/// on a physical run that is measured from sampled gauges. This is the same
/// property where it can be made exact: after the first pass, repeated patch
/// cycles against a settled buffer must neither grow the registry nor move the
/// allocation.
///
/// Counting allocations rather than sampling resident size is what makes it a
/// regression instead of a flake. RSS drifts under identical work because the
/// allocator returns freed pages to its own arenas; allocation identity does
/// not drift.
#[test]
fn a_warmed_registry_neither_grows_nor_reallocates() {
    let mut registry = LiveCpuBufferRegistry::new();
    registry
        .apply(LiveCpuBufferUpdate::Replace(buffer(21, 1)))
        .expect("the base must be admitted");

    let patch = |generation: u64, fill: u8| {
        LiveCpuBufferUpdate::PatchBatch(LiveCpuBufferPatchBatch {
            handle: 21,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation,
            patches: vec![LiveCpuBufferPatchRegion {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                bytes: vec![fill; 8].into(),
            }],
        })
    };

    // One warmup pass, then the settled series the assertions describe.
    registry.apply(patch(2, 1)).expect("warmup patch");
    let settled_allocation = Arc::as_ptr(&registry.get(21).unwrap().bytes);
    let settled_len = registry.len();
    let settled_bytes = registry.total_bytes();
    let settled_splits = registry.cow_splits();

    for generation in 3..64 {
        registry
            .apply(patch(generation, u8::try_from(generation % 251).unwrap()))
            .expect("settled patch");
    }

    assert_eq!(
        registry.len(),
        settled_len,
        "a warmed registry must not accumulate buffers"
    );
    assert_eq!(
        registry.total_bytes(),
        settled_bytes,
        "a warmed registry must not accumulate bytes"
    );
    assert_eq!(
        Arc::as_ptr(&registry.get(21).unwrap().bytes),
        settled_allocation,
        "a warmed registry must not reallocate the buffer it keeps patching"
    );
    assert_eq!(
        registry.cow_splits(),
        settled_splits,
        "no presentation held these bytes, so nothing should have copied them"
    );

    // And the peaks agree with the live figures, which is what makes the
    // sampled bound a statement about the whole run rather than its end.
    assert_eq!(registry.peak_buffers(), settled_len);
    assert_eq!(registry.peak_bytes(), settled_bytes);
}
