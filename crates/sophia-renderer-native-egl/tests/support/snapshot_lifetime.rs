use super::*;

#[test]
fn pending_uses_block_recycling_until_the_last_batch_finishes() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    assert!(generation.can_recycle());
    let first = generation.acquire_use();
    let second = generation.acquire_use();
    assert_eq!(generation.pending_uses.get(), 2);
    assert!(!generation.can_recycle());
    drop(second);
    assert_eq!(generation.pending_uses.get(), 1);
    assert!(!generation.can_recycle());
    drop(first);
    assert_eq!(generation.pending_uses.get(), 0);
    assert!(generation.can_recycle());
}

#[test]
fn raw_export_permanently_prevents_recycling_after_uses_finish() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    let in_flight = generation.acquire_use();
    generation.mark_exported();
    generation.mark_exported();
    drop(in_flight);
    assert!(generation.exported());
    assert!(generation.reuse_allowed());
    assert_eq!(generation.pending_uses.get(), 0);
    assert!(!generation.can_recycle());
    let later_read = generation.acquire_use();
    drop(later_read);
    assert!(!generation.can_recycle());
}

#[test]
fn uncertain_completion_permanently_prevents_recycling() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    let in_flight = generation.acquire_use();
    in_flight.generation().abandon_reuse();
    drop(in_flight);
    assert!(!generation.exported());
    assert!(!generation.reuse_allowed());
    assert_eq!(generation.pending_uses.get(), 0);
    assert!(!generation.can_recycle());
    let later_read = generation.acquire_use();
    drop(later_read);
    assert!(!generation.can_recycle());
}

#[test]
fn a_late_release_changes_only_its_original_content_generation() {
    let old = NativeSnapshotGeneration::new(7, 1);
    let old_use = old.acquire_use();
    let next = NativeSnapshotGeneration::new(7, 2);
    let next_use = next.acquire_use();
    assert_eq!(old_use.generation().allocation_id, next.allocation_id);
    assert_ne!(old_use.generation().generation(), next.generation());
    old.abandon_reuse();
    drop(old_use);
    assert_eq!(old.pending_uses.get(), 0);
    assert!(!old.can_recycle());
    assert_eq!(next.pending_uses.get(), 1);
    assert!(!next.can_recycle());
    assert!(next.reuse_allowed());
    drop(next_use);
    assert!(next.can_recycle());
}

#[test]
fn metadata_references_do_not_count_as_active_pixel_readers() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    let metadata = generation.clone();
    let in_flight = generation.acquire_use();
    drop(in_flight);
    assert_eq!(std::rc::Rc::strong_count(&generation), 2);
    assert_eq!(metadata.pending_uses.get(), 0);
    assert!(generation.can_recycle());
}

#[test]
fn gpu_completion_keeps_discarded_image_storage_alive() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    let allocation = std::rc::Rc::new(());
    let weak = std::rc::Rc::downgrade(&allocation);
    let gpu_use = NativeSnapshotGpuUse {
        allocation: allocation.clone(),
        use_guard: generation.acquire_use(),
    };
    drop(allocation);
    assert!(
        weak.upgrade().is_some(),
        "GPU work owns the discarded image storage"
    );
    assert!(!generation.can_recycle());
    drop(gpu_use);
    assert!(weak.upgrade().is_none());
    assert!(generation.can_recycle());
}

#[test]
fn uncertain_gpu_completion_keeps_one_allocation_owner_until_final_teardown() {
    let generation = NativeSnapshotGeneration::new(7, 1);
    let allocation = std::rc::Rc::new(());
    let weak = std::rc::Rc::downgrade(&allocation);
    let uses = (0..2)
        .map(|_| NativeSnapshotGpuUse {
            allocation: allocation.clone(),
            use_guard: generation.acquire_use(),
        })
        .collect();
    let mut quarantine = Vec::new();
    quarantine_snapshot_uses(uses, &mut quarantine);
    drop(allocation);
    assert_eq!(generation.pending_uses.get(), 0);
    assert!(!generation.can_recycle());
    assert_eq!(
        quarantine.len(),
        1,
        "multiple uncertain reads share one storage charge"
    );
    assert!(weak.upgrade().is_some());
    quarantine.clear();
    assert!(weak.upgrade().is_none());
}
