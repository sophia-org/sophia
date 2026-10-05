// Snapshot allocation reuse stays on the renderer thread. Raw exports detach
// their allocation permanently; active GPU uses belong to content generations,
// while output texture caches keep only the underlying allocation alive.
const NATIVE_CAPTURE_IDLE_CAPACITY: usize = 8;
const NATIVE_CAPTURE_IDLE_BYTES: u64 = 64 * 1024 * 1024;

enum NativeCaptureDrawFailure {
    BeforeDestinationWork(NativeGbmScanoutBufferExportDetail),
    DestinationMayHaveWork(NativeGbmScanoutBufferExportDetail),
}

// Only submitted work needs an allocation owner beyond this attempt. A source
// import refusal happens before the destination clear and must release its
// unused storage, otherwise repeated successful transfers exhaust the pool.
fn finish_native_capture_attempt<A, R>(
    allocation: std::rc::Rc<A>,
    result: Result<R, NativeCaptureDrawFailure>,
    uncertain: &mut Vec<std::rc::Rc<A>>,
) -> Result<R, NativeGbmScanoutBufferExportDetail> {
    match result {
        Ok(captured) => Ok(captured),
        Err(NativeCaptureDrawFailure::BeforeDestinationWork(error)) => Err(error),
        Err(NativeCaptureDrawFailure::DestinationMayHaveWork(error)) => {
            uncertain.push(allocation);
            Err(error)
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeCaptureReuseStats {
    pub allocations: usize,
    pub reuses: usize,
    pub misses: usize,
    pub fallbacks: usize,
    pub exported_exclusions: usize,
    pub peak_bytes: u64,
    pub context_creations: usize,
    pub config_selections: usize,
    pub context_reuses: usize,
    pub live_bytes: u64,
    pub live_count: usize,
    pub idle_bytes: u64,
    pub idle_count: usize,
    pub draining_count: usize,
    pub source_imports: usize,
    pub source_hits: usize,
    pub source_rebinds: usize,
    pub source_bytes: u64,
    pub source_entries: usize,
    pub sampling: crate::NativeCompositionSamplingStats,
    pub capture_cpu: std::time::Duration,
    pub capture_elapsed: std::time::Duration,
    pub setup_cpu: std::time::Duration,
    pub setup_elapsed: std::time::Duration,
    pub copy_cpu: std::time::Duration,
    pub copy_elapsed: std::time::Duration,
    pub cleanup_cpu: std::time::Duration,
    pub cleanup_elapsed: std::time::Duration,
    pub reclaim_cpu: std::time::Duration,
    pub reclaim_elapsed: std::time::Duration,
}

#[derive(Default)]
struct NativeCaptureAllocationAccounting {
    count: std::cell::Cell<usize>,
    bytes: std::cell::Cell<u64>,
    peak_bytes: std::cell::Cell<u64>,
    cleanup_failed: std::cell::Cell<bool>,
    // Failed EGL destruction cannot be discharged by destroying a context.
    // Root keeps this accounting owner through eglTerminate, including clear().
    graveyard: std::cell::RefCell<Vec<NativeCaptureAbandonedAllocation>>,
    timing_enabled: std::cell::Cell<bool>,
    timing: NativeCaptureTimingTotals,
}

// Storage charges have no GL context ownership. A failed output import can
// retain this charge and the BO through eglTerminate without retaining an
// execution context, and Weak prevents the display graveyard owning itself.
struct NativeCaptureAllocationCharge {
    accounting: std::rc::Weak<NativeCaptureAllocationAccounting>,
    bytes: u64,
}

impl NativeCaptureAllocationCharge {
    fn new(
        accounting: &std::rc::Rc<NativeCaptureAllocationAccounting>,
        bytes: u64,
    ) -> std::rc::Rc<Self> {
        accounting.bytes.set(accounting.bytes.get() + bytes);
        accounting.count.set(accounting.count.get() + 1);
        accounting
            .peak_bytes
            .set(accounting.peak_bytes.get().max(accounting.bytes.get()));
        std::rc::Rc::new(Self {
            accounting: std::rc::Rc::downgrade(accounting),
            bytes,
        })
    }
}

impl Drop for NativeCaptureAllocationCharge {
    fn drop(&mut self) {
        if let Some(accounting) = self.accounting.upgrade() {
            accounting.bytes.set(accounting.bytes.get() - self.bytes);
            accounting.count.set(accounting.count.get() - 1);
        }
    }
}

struct NativePooledCapture {
    allocation: std::rc::Rc<NativeCaptureAllocation>,
    generation: std::rc::Rc<NativeSnapshotGeneration>,
    copy_completion: Option<khronos_egl::Sync>,
    copy_use: Option<NativeSnapshotUse>,
}

impl NativePooledCapture {
    fn take_copy_completion(&mut self) -> Option<(khronos_egl::Sync, NativeSnapshotUse)> {
        let sync = self.copy_completion.take()?;
        Some((
            sync,
            self.copy_use
                .take()
                .expect("capture fence owns its GPU use"),
        ))
    }
}

impl Drop for NativePooledCapture {
    fn drop(&mut self) {
        // The caller normally transfers the fence/use into its completion list.
        // A discarded capture must never accidentally become reusable.
        if let Some(sync) = self.copy_completion.take() {
            self.generation.abandon_reuse();
            let execution = &self.allocation.execution;
            let _ = unsafe { execution.egl.destroy_sync(execution.display, sync) };
        }
    }
}

struct NativeCaptureReuse {
    execution: [Option<std::rc::Rc<NativeCaptureExecution>>; 2],
    idle: Vec<std::rc::Rc<NativeCaptureAllocation>>,
    retired: Vec<NativePooledCapture>,
    // Failed draws may have submitted GPU work without a trustworthy fence.
    // These never return to idle and survive ordinary image-store clearing.
    uncertain: Vec<std::rc::Rc<NativeCaptureAllocation>>,
    accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
    source_budget: std::rc::Rc<NativeCaptureSourceBudget>,
    known_sizes: Vec<(NativeCaptureAllocationKey, u64)>,
    next_allocation_id: u64,
    enabled: bool,
    source_reuse_enabled: bool,
    stats: NativeCaptureReuseStats,
}

impl Default for NativeCaptureReuse {
    fn default() -> Self {
        Self {
            execution: [None, None],
            idle: Vec::new(),
            retired: Vec::new(),
            uncertain: Vec::new(),
            accounting: Default::default(),
            source_budget: Default::default(),
            known_sizes: Vec::new(),
            next_allocation_id: 1,
            enabled: true,
            source_reuse_enabled: true,
            stats: Default::default(),
        }
    }
}

impl NativeCaptureReuse {
    fn set_render_timing_enabled(&mut self, enabled: bool) {
        self.accounting.timing_enabled.set(enabled);
    }

    fn set_source_reuse_enabled(&mut self, enabled: bool) {
        self.source_reuse_enabled = enabled;
    }

    fn retained_bytes(&self) -> u64 {
        self.accounting.bytes.get()
    }

    fn retained_count(&self) -> usize {
        self.accounting.count.get()
    }

    fn source_bytes(&self) -> u64 {
        self.source_budget.usage().1
    }

    fn source_cleanup_failed(&self) -> bool {
        self.source_budget.poisoned() || self.accounting.cleanup_failed.get()
    }

    fn source_entries(&self) -> usize {
        self.source_budget.usage().0
    }

    fn stats(&self) -> NativeCaptureReuseStats {
        let mut stats = NativeCaptureReuseStats {
            peak_bytes: self.accounting.peak_bytes.get(),
            live_bytes: self.retained_bytes(),
            live_count: self.retained_count(),
            idle_bytes: self.idle.iter().map(|entry| entry.bytes).sum(),
            idle_count: self.idle.len(),
            draining_count: self.retired.len(),
            source_bytes: self.source_bytes(),
            source_entries: self.source_entries(),
            capture_cpu: self.accounting.timing.cpu[0].get(),
            capture_elapsed: self.accounting.timing.wall[0].get(),
            setup_cpu: self.accounting.timing.cpu[1].get(),
            setup_elapsed: self.accounting.timing.wall[1].get(),
            copy_cpu: self.accounting.timing.cpu[2].get(),
            copy_elapsed: self.accounting.timing.wall[2].get(),
            cleanup_cpu: self.accounting.timing.cpu[3].get(),
            cleanup_elapsed: self.accounting.timing.wall[3].get(),
            reclaim_cpu: self.accounting.timing.cpu[4].get(),
            reclaim_elapsed: self.accounting.timing.wall[4].get(),
            ..self.stats
        };
        for execution in self.execution.iter().flatten() {
            if let Some(pipeline) = execution.pipeline.borrow().as_ref() {
                stats.sampling = stats.sampling.saturating_add(pipeline.sampling_stats());
            }
            let source = execution.sources.borrow().stats();
            stats.source_imports = stats.source_imports.saturating_add(source.imports);
            stats.source_hits = stats.source_hits.saturating_add(source.hits);
            stats.source_rebinds = stats.source_rebinds.saturating_add(source.rebinds);
        }
        stats
    }

    fn retire(&mut self, captured: NativePooledCapture) {
        self.retired.push(captured);
        self.reap();
    }

    fn reap(&mut self) {
        let mut waiting = Vec::with_capacity(self.retired.len());
        for mut captured in self.retired.drain(..) {
            // Also support callers that retire a capture before transferring
            // its copy fence. Polling is nonblocking and never done on owner.
            if let Some(sync) = captured.copy_completion {
                let execution = &captured.allocation.execution;
                match unsafe {
                    execution
                        .egl
                        .client_wait_sync(execution.display, sync, 0, 0)
                } {
                    Ok(khronos_egl::CONDITION_SATISFIED) => {
                        if unsafe { execution.egl.destroy_sync(execution.display, sync) }.is_err() {
                            captured.generation.abandon_reuse();
                        }
                        captured.copy_completion = None;
                        captured.copy_use = None;
                    }
                    Ok(khronos_egl::TIMEOUT_EXPIRED) => {
                        // Exporting forbids overwrite, but does not complete
                        // the GPU copy or release its allocation charge.
                        waiting.push(captured);
                        continue;
                    }
                    _ => {
                        captured.generation.abandon_reuse();
                        self.uncertain.push(captured.allocation.clone());
                        continue;
                    }
                }
            }
            if captured.generation.exported() || !captured.generation.reuse_allowed() {
                self.stats.exported_exclusions += usize::from(captured.generation.exported());
                continue;
            }
            if captured.generation.can_recycle() && self.enabled {
                self.idle.push(captured.allocation.clone());
            } else if self.enabled {
                waiting.push(captured);
            }
        }
        self.retired = waiting;
        self.trim_idle(NATIVE_CAPTURE_IDLE_CAPACITY, NATIVE_CAPTURE_IDLE_BYTES);
    }

    fn trim_idle(&mut self, count: usize, bytes: u64) {
        let mut idle_bytes: u64 = self.idle.iter().map(|entry| entry.bytes).sum();
        while self.idle.len() > count || idle_bytes > bytes {
            let entry = self.idle.remove(0);
            idle_bytes = idle_bytes.saturating_sub(entry.bytes);
            drop(entry);
        }
    }

    /// The cap includes source imports and all pooled allocations, including
    /// allocations retained by an output import cache after leaving this list.
    fn trim_to(&mut self, bytes: u64) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        self.reap();
        self.trim_sources(bytes.saturating_sub(self.retained_bytes()))?;
        while !self.idle.is_empty()
            && self.retained_bytes().saturating_add(self.source_bytes()) > bytes
        {
            self.idle.remove(0);
        }
        if self.source_cleanup_failed() {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        Ok(())
    }

    fn trim_sources(&mut self, max_bytes: u64) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        let mut remaining = max_bytes;
        for execution in self.execution.iter().flatten() {
            execution.make_current()?;
            let pipeline = execution.pipeline.borrow();
            let pipeline = pipeline.as_ref().expect("live capture execution pipeline");
            let result = execution.sources.borrow_mut().trim_to(
                &execution.egl,
                execution.display,
                pipeline,
                remaining,
            );
            let _ = execution
                .egl
                .make_current(execution.display, None, None, None);
            result?;
            remaining = remaining.saturating_sub(execution.sources.borrow().resident_bytes());
        }
        Ok(())
    }

    /// `available_bytes` and `allocation_limit` are TOTAL caps for this pool,
    /// not additional allowances. Root subtracts nonpool images/bridge storage.
    fn capture<T: AsFd>(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        gbm_device: &gbm::Device<T>,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
        available_bytes: u64,
        allocation_limit: usize,
    ) -> Result<Option<NativePooledCapture>, NativeGbmScanoutBufferExportDetail> {
        use NativeGbmScanoutBufferExportDetail as E;
        let _whole =
            NativeCaptureTimingSpan::start(&self.accounting, NativeCaptureTimingKind::Capture);
        let setup =
            NativeCaptureTimingSpan::start(&self.accounting, NativeCaptureTimingKind::Setup);
        if self.source_cleanup_failed() {
            return Err(E::EglImageDestroyFailed);
        }
        if !self.enabled {
            return Ok(None);
        }
        if !frame.is_valid()
            || i32::try_from(frame.width).is_err()
            || i32::try_from(frame.height).is_err()
        {
            return Err(E::InvalidTarget);
        }
        self.reap();
        if self.source_cleanup_failed() {
            return Err(E::EglImageDestroyFailed);
        }
        let format = match frame.format {
            0x3432_5258 => gbm::Format::Xrgb8888,
            0x3432_5241 => gbm::Format::Argb8888,
            _ => return Err(E::InvalidTarget),
        };
        let slot = usize::from(format == gbm::Format::Argb8888);
        let candidates: Vec<_> = rendered_scanout_candidates(&[], Some(frame.format));
        let minimum_bytes = u64::from(frame.width)
            .checked_mul(u64::from(frame.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(E::InvalidTarget)?;
        let mut allocation = None;
        // A prior successful candidate is already verified for this exact
        // layout. Look through every compatible idle slot before retrying any
        // allocation candidate that the driver may have rejected while cold.
        let reusable = candidates.iter().find_map(|candidate| {
            let key = NativeCaptureAllocationKey::new(frame.width, frame.height, candidate);
            self.idle.iter().position(|entry| entry.key == key)
        });
        if let Some(index) = reusable {
            allocation = Some(self.idle.swap_remove(index));
            self.stats.context_reuses = self.stats.context_reuses.saturating_add(1);
            self.stats.reuses = self.stats.reuses.saturating_add(1);
        }
        for candidate in candidates {
            if allocation.is_some() {
                break;
            }
            let key = NativeCaptureAllocationKey::new(frame.width, frame.height, &candidate);
            // GBM does not expose allocation padding before allocation. Reuse
            // the actual prior size when known, then validate the new BO's
            // actual size before admission. A cold speculative allocation can
            // exceed its estimate; the admitted retained pool never may.
            let reserve_bytes = self
                .known_sizes
                .iter()
                .find_map(|(known, bytes)| (known == &key).then_some(*bytes))
                .unwrap_or(minimum_bytes)
                .max(minimum_bytes);
            self.stats.misses = self.stats.misses.saturating_add(1);
            self.trim_to(available_bytes.saturating_sub(reserve_bytes))?;
            while self.retained_count() >= allocation_limit && !self.idle.is_empty() {
                self.idle.remove(0);
            }
            if self.retained_count() >= allocation_limit
                || self.retained_bytes().saturating_add(self.source_bytes())
                    > available_bytes.saturating_sub(reserve_bytes)
                || reserve_bytes > available_bytes
            {
                return Err(E::RendererImageStoreFull);
            }
            let execution = if let Some(execution) = &self.execution[slot] {
                if !execution.reusable() {
                    return Err(E::EglImageDestroyFailed);
                }
                self.stats.context_reuses = self.stats.context_reuses.saturating_add(1);
                execution.clone()
            } else {
                self.stats.config_selections = self.stats.config_selections.saturating_add(1);
                let Some(config) = choose_scanout_config_for_format(
                    egl,
                    display,
                    candidate.config_attributes,
                    candidate.format,
                ) else {
                    continue;
                };
                match NativeCaptureExecution::new(
                    display,
                    config,
                    self.source_budget.clone(),
                    self.accounting.clone(),
                ) {
                    Ok(execution) => {
                        let execution = std::rc::Rc::new(execution);
                        self.execution[slot] = Some(execution.clone());
                        self.stats.context_creations += 1;
                        execution
                    }
                    Err(_) => continue,
                }
            };
            let id = self.next_allocation_id;
            self.next_allocation_id = id.checked_add(1).ok_or(E::InvalidTarget)?;
            match NativeCaptureAllocation::new(
                gbm_device,
                key,
                id,
                execution,
                self.accounting.clone(),
                available_bytes.saturating_sub(self.source_bytes()),
            ) {
                Ok(created) => {
                    self.stats.allocations += 1;
                    if let Some((_, size)) = self
                        .known_sizes
                        .iter_mut()
                        .find(|(known, _)| known == &created.key)
                    {
                        *size = (*size).max(created.bytes);
                    } else {
                        if self.known_sizes.len() == 32 {
                            self.known_sizes.remove(0);
                        }
                        self.known_sizes.push((created.key.clone(), created.bytes));
                    }
                    allocation = Some(std::rc::Rc::new(created));
                    break;
                }
                Err(E::RendererImageStoreFull) => return Err(E::RendererImageStoreFull),
                Err(_) => continue,
            }
        }
        let Some(allocation) = allocation else {
            self.stats.fallbacks += 1;
            return Ok(None);
        };
        if self.retained_bytes() > available_bytes {
            return Err(E::RendererImageStoreFull);
        }
        let max_source_bytes = if self.source_reuse_enabled {
            available_bytes.saturating_sub(self.retained_bytes())
        } else {
            0
        };
        if self.source_bytes() > max_source_bytes {
            self.trim_sources(max_source_bytes)?;
        }
        let execution = &allocation.execution;
        let other_source_bytes = self
            .source_bytes()
            .saturating_sub(execution.sources.borrow().resident_bytes());
        let source_allowance = if self.source_reuse_enabled {
            available_bytes
                .saturating_sub(self.retained_bytes())
                .saturating_sub(other_source_bytes)
        } else {
            0
        };
        drop(setup);
        let _copy = NativeCaptureTimingSpan::start(&self.accounting, NativeCaptureTimingKind::Copy);
        let result = allocation.draw(frame, source_allowance);
        finish_native_capture_attempt(allocation, result, &mut self.uncertain)
    }

    fn clear(&mut self) {
        for execution in self.execution.iter().flatten() {
            if let Some(pipeline) = execution.pipeline.borrow().as_ref() {
                self.stats.sampling = self
                    .stats
                    .sampling
                    .saturating_add(pipeline.sampling_stats());
            }
            let source = execution.sources.borrow().stats();
            self.stats.source_imports = self.stats.source_imports.saturating_add(source.imports);
            self.stats.source_hits = self.stats.source_hits.saturating_add(source.hits);
            self.stats.source_rebinds = self.stats.source_rebinds.saturating_add(source.rebinds);
        }
        for captured in &self.retired {
            captured.generation.abandon_reuse();
            if captured.copy_completion.is_some() {
                // clear() is also used while the display remains active. A
                // destroyed fence is not proof that the submitted copy ended.
                self.uncertain.push(captured.allocation.clone());
            }
        }
        self.retired.clear();
        self.idle.clear();
        self.execution = [None, None];
    }

    /// Final renderer teardown only, before EGL termination. Ordinary clear()
    /// must keep unknown GPU work charged and may not reuse its storage.
    fn destroy_quarantine(&mut self) {
        self.uncertain.clear();
    }

    /// Called after eglTerminate, while the GBM device is still alive. These
    /// records contain no GL-calling Drop and could not be freed earlier when
    /// EGL retained their images despite a failed explicit destruction.
    fn release_display_graveyards(&mut self) {
        self.accounting.graveyard.borrow_mut().clear();
        self.source_budget.graveyard.borrow_mut().clear();
    }
}

impl Drop for NativeCaptureReuse {
    fn drop(&mut self) {
        self.clear();
        self.destroy_quarantine();
    }
}

include!("capture_pool/target.rs");
include!("capture_pool/timing.rs");

#[cfg(test)]
mod capture_pool_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/capture_pool.rs"
    ));
}
