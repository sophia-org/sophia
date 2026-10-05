// One immutable content generation of renderer-owned snapshot storage.
// Allocation/import keepalives are separate: a cached texture alone is not an
// active reader and must not prevent retired pixels from being overwritten.
#[derive(Debug)]
struct NativeSnapshotGeneration {
    allocation_id: u64,
    generation: u64,
    exported: std::cell::Cell<bool>,
    recyclable: std::cell::Cell<bool>,
    pending_uses: std::cell::Cell<usize>,
}

impl NativeSnapshotGeneration {
    fn new(allocation_id: u64, generation: u64) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Self {
            allocation_id,
            generation,
            exported: std::cell::Cell::new(false),
            recyclable: std::cell::Cell::new(true),
            pending_uses: std::cell::Cell::new(0),
        })
    }

    fn generation(&self) -> u64 {
        debug_assert_ne!(
            self.allocation_id, 0,
            "snapshot allocation identity is invalid"
        );
        self.generation
    }

    fn exported(&self) -> bool {
        self.exported.get()
    }

    fn reuse_allowed(&self) -> bool {
        self.recyclable.get()
    }

    /// Raw descriptor owners cannot be counted after a snapshot is exported.
    /// This generation must therefore never return its storage to the pool.
    fn mark_exported(&self) {
        self.exported.set(true);
    }

    /// A failed or unavailable completion proof permanently disables reuse.
    fn abandon_reuse(&self) {
        self.recyclable.set(false);
    }

    /// The caller must first retire the image and drain its logical readers.
    /// This checks only export status and the outstanding renderer/GPU uses.
    fn can_recycle(&self) -> bool {
        !self.exported.get() && self.recyclable.get() && self.pending_uses.get() == 0
    }

    fn acquire_use(self: &std::rc::Rc<Self>) -> NativeSnapshotUse {
        let Some(pending) = self.pending_uses.get().checked_add(1) else {
            self.abandon_reuse();
            panic!("snapshot generation use count exhausted");
        };
        self.pending_uses.set(pending);
        NativeSnapshotUse {
            generation: self.clone(),
        }
    }
}

/// Retained by the renderer's completion record until the GPU use finishes.
/// On an uncertain completion, poison the generation before dropping this.
/// A guard always addresses its original generation, never a reused slot.
#[derive(Debug)]
struct NativeSnapshotUse {
    generation: std::rc::Rc<NativeSnapshotGeneration>,
}

impl NativeSnapshotUse {
    fn generation(&self) -> &std::rc::Rc<NativeSnapshotGeneration> {
        &self.generation
    }
}

impl Drop for NativeSnapshotUse {
    fn drop(&mut self) {
        let Some(pending) = self.generation.pending_uses.get().checked_sub(1) else {
            // A broken accounting invariant must never authorize reuse, even
            // if this drop is running during error cleanup.
            self.generation.abandon_reuse();
            return;
        };
        self.generation.pending_uses.set(pending);
    }
}

/// GPU commands retain both their content generation and charged storage.
/// The separate storage owner prevents a discarded image from disappearing
/// from the budget before its submitted GPU access has finished.
struct NativeSnapshotGpuUse<A> {
    allocation: std::rc::Rc<A>,
    use_guard: NativeSnapshotUse,
}

impl<A> NativeSnapshotGpuUse<A> {
    fn generation(&self) -> &std::rc::Rc<NativeSnapshotGeneration> {
        self.use_guard.generation()
    }
}

/// Unprovable completion keeps the allocation charged until final teardown.
/// Multiple failed batches reading one allocation add only one retained owner.
fn quarantine_snapshot_uses<A>(
    uses: Vec<NativeSnapshotGpuUse<A>>,
    quarantine: &mut Vec<std::rc::Rc<A>>,
) {
    for gpu_use in uses {
        gpu_use.generation().abandon_reuse();
        if !quarantine
            .iter()
            .any(|held| std::rc::Rc::ptr_eq(held, &gpu_use.allocation))
        {
            quarantine.push(gpu_use.allocation.clone());
        }
    }
}
