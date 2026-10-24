#[derive(Default)]
struct ManagedCaptures {
    epochs: Vec<std::sync::Weak<NativeRendererSnapshotEpoch>>,
    images: std::collections::BTreeMap<
        NativeRendererImageId,
        std::sync::Weak<NativeManagedCaptureLease>,
    >,
}

fn preserve_uncertain_managed(allocations: &mut Vec<std::rc::Rc<NativeCaptureAllocation>>) {
    for allocation in allocations.drain(..) {
        if allocation.managed.borrow().is_some() {
            shell_capture::poison_managed_capture();
            // Teardown is not completion. Retain the native owner and charge
            // for process lifetime, as the shell source escrow does.
            std::mem::forget(allocation);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeManagedCaptureProgress {
    pub needs_wake: bool,
    pub pending_fences: usize,
}

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_managed_shell_capture_destination(
        &mut self,
        width: u32,
        height: u32,
        budget: std::sync::Arc<NativeRendererSnapshotBudget>,
        epoch: std::sync::Arc<NativeRendererSnapshotEpoch>,
        wake: sophia_wake::Notifier,
    ) -> Result<
        Option<sophia_renderer_image::PreparedCaptureDestination>,
        NativeGbmScanoutBufferExportDetail,
    > {
        if !self.snapshot_reuse_enabled
            || self.shell_capture_owner.is_retired()
            || !epoch.is_valid()
            || width == 0
            || height == 0
            || i32::try_from(width).is_err()
            || i32::try_from(height).is_err()
        {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        self.maintain_managed_shell_captures()?;
        let owner = std::sync::Arc::downgrade(&epoch);
        if !self
            .managed_captures
            .epochs
            .iter()
            .any(|old| old.ptr_eq(&owner))
        {
            self.managed_captures.epochs.push(owner);
        }
        let other_bytes = self
            .renderer_images
            .values()
            .filter(|i| i.pooled.is_none())
            .fold(self.image_bridge_bytes(), |sum, i| {
                sum.saturating_add(i.bytes)
            });
        let other_count = self
            .renderer_images
            .values()
            .filter(|i| i.pooled.is_none())
            .count();
        self.capture_reuse.prepare_managed(
            &self.egl,
            self.display,
            &self.gbm_device,
            width,
            height,
            DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET.saturating_sub(other_bytes),
            DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY.saturating_sub(other_count),
            budget,
            epoch,
            wake,
        )
    }

    fn retain_managed_shell_capture(
        &mut self,
        image_id: NativeRendererImageId,
        budget: &std::sync::Arc<NativeRendererSnapshotBudget>,
        epoch: std::sync::Arc<NativeRendererSnapshotEpoch>,
        reservation: NativeRendererSnapshotReservation,
        captured: NativePooledCapture,
    ) -> Result<std::sync::Arc<NativeCapturedImage>, NativeShellCaptureOutputFailure> {
        use std::sync::Arc;
        let info = sophia_renderer_image::ManagedCaptureInfo {
            image_id,
            allocation_id: captured.allocation.id,
            generation: captured.generation.generation(),
            width: captured.allocation.key.width,
            height: captured.allocation.key.height,
            bytes: captured.allocation.bytes,
        };
        let Some(prepared) = reservation.prepared() else {
            return Err(NativeShellCaptureOutputFailure {
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                reservation,
            });
        };
        if prepared.info().allocation_id != info.allocation_id
            || self.renderer_images.contains_key(&image_id)
        {
            return Err(NativeShellCaptureOutputFailure {
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                reservation,
            });
        }
        let lease = match NativeManagedCaptureLease::new(info, epoch.clone(), prepared.notifier()) {
            Ok(lease) => lease,
            Err(detail) => {
                return Err(NativeShellCaptureOutputFailure {
                    detail: detail.into(),
                    reservation,
                });
            }
        };
        let charge = reservation
            .retain_managed(budget, &epoch, captured.allocation.bytes)
            .map_err(|(detail, reservation)| NativeShellCaptureOutputFailure {
                detail: detail.into(),
                reservation,
            })?;
        *captured.allocation.managed.borrow_mut() =
            Some(ManagedAllocationBacking { charge, epoch });
        let bytes = captured.allocation.bytes;
        let mut image =
            NativeRendererImage::staged(captured.allocation.buffer.clone(), Some(captured), bytes);
        image.promote();
        self.renderer_images.insert(image_id, image);
        self.renderer_image_bytes = self.renderer_image_bytes.saturating_add(bytes);
        self.managed_captures
            .images
            .insert(image_id, Arc::downgrade(&lease));
        self.stats.snapshot_captures = self.stats.snapshot_captures.saturating_add(1);
        self.stats.snapshot_promotions = self.stats.snapshot_promotions.saturating_add(1);
        self.update_renderer_image_stats();
        Ok(Arc::new(NativeCapturedImage::Managed(lease)))
    }

    /// Called only on the context owner, while the queued layer still holds its
    /// lease. No bare id crosses the queue or enters preview restoration.
    pub fn managed_shell_capture_image(
        &self,
        lease: &std::sync::Arc<NativeManagedCaptureLease>,
    ) -> Option<NativeRendererImageId> {
        let info = lease.info();
        let weak = self.managed_captures.images.get(&info.image_id)?;
        let image = self.renderer_images.get(&info.image_id)?;
        let captured = image.pooled.as_ref()?;
        (lease.is_current()
            && std::sync::Weak::ptr_eq(weak, &std::sync::Arc::downgrade(lease))
            && image.is_promoted()
            && captured.allocation.id == info.allocation_id
            && captured.generation.generation() == info.generation)
            .then_some(info.image_id)
    }

    /// The worker services this after clearing its command wake, even with no
    /// capture executor. A quiet last lease must not require a future frame.
    pub fn maintain_managed_shell_captures(
        &mut self,
    ) -> Result<NativeManagedCaptureProgress, NativeGbmScanoutBufferExportDetail> {
        self.poll_snapshot_fences();
        if self
            .snapshot_quarantine
            .iter()
            .any(|a| a.managed.borrow().is_some())
        {
            shell_capture::poison_managed_capture();
        }
        let retired: Vec<_> = self
            .managed_captures
            .images
            .iter()
            .filter(|(_, weak)| weak.upgrade().is_none_or(|lease| !lease.is_current()))
            .map(|(id, _)| *id)
            .collect();
        for id in retired {
            self.managed_captures.images.remove(&id);
            self.evict_renderer_image(id)?;
        }
        self.capture_reuse.reap();
        self.capture_reuse.maintain_managed_preparations();
        // A closed endpoint may survive only in a cached import, after idle
        // trimming. Observe its epoch independently of the idle list.
        let closed: Vec<_> = self
            .managed_captures
            .epochs
            .iter()
            .filter(|epoch| epoch.upgrade().is_none_or(|epoch| !epoch.is_valid()))
            .cloned()
            .collect();
        if !closed.is_empty() {
            self.clear_renderer_import_caches()?;
        }
        // Remove only the epochs whose cache cleanup was observed above. A
        // concurrent invalidation must survive for the next maintenance turn.
        self.managed_captures
            .epochs
            .retain(|epoch| !closed.iter().any(|closed| closed.ptr_eq(epoch)));
        self.capture_reuse.idle.retain(|a| {
            a.managed
                .borrow()
                .as_ref()
                .is_none_or(|m| m.epoch.is_valid())
        });
        self.capture_reuse.maintain_managed_preparations();
        let pending_fences = self.snapshot_fences.len();
        Ok(NativeManagedCaptureProgress {
            needs_wake: !self.managed_captures.images.is_empty()
                || !self.capture_reuse.managed_prepared.is_empty()
                || pending_fences != 0,
            pending_fences,
        })
    }

    fn invalidate_managed_shell_captures(&mut self) {
        for epoch in self
            .managed_captures
            .epochs
            .drain(..)
            .filter_map(|epoch| epoch.upgrade())
        {
            epoch.invalidate();
        }
        for lease in self
            .managed_captures
            .images
            .values()
            .filter_map(std::sync::Weak::upgrade)
        {
            lease.invalidate();
        }
        self.managed_captures.images.clear();
        for prepared in self.capture_reuse.managed_prepared.values() {
            prepared.epoch.invalidate();
        }
    }
}
