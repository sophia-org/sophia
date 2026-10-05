// The device-wide renderer-image store, textually included by `context.rs`.
//
// Images are imported once per device and shared by every output the context
// serves, which is the point of one context per device rather than one per
// head. Their lifecycle -- capture, promote, roll back, export, evict -- is
// self-contained enough to read on its own, and separating it keeps both
// halves inside the source-layout ceiling.

impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: std::os::fd::AsFd,
{
    pub fn evict_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        self.poll_snapshot_fences();
        let pooled = self.renderer_images.get(&image_id).is_some_and(|image| image.pooled.is_some());
        let evicted_import = if pooled && self.snapshot_import_reuse_enabled {
            false
        } else {
            self.evict_renderer_image_imports(image_id)?
        };
        // Pooled imports own allocation keepalives independently of logical
        // content. Fresh-surface imports still retire before their backing.
        let evicted_image = self.renderer_images.remove(&image_id);
        if let Some(image) = evicted_image {
            self.renderer_image_bytes = self.renderer_image_bytes.saturating_sub(image.bytes);
            let NativeRendererImage { buffer, pooled, .. } = image;
            drop(buffer);
            if let Some(captured) = pooled {
                self.capture_reuse.retire(captured);
            }
            self.stats.snapshot_evictions = self.stats.snapshot_evictions.saturating_add(1);
            self.update_renderer_image_stats();
            return Ok(true);
        }
        Ok(evicted_import)
    }
    pub fn evict_renderer_image_imports(
        &mut self,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let mut evicted_import = self.evict_current_target_import(image_id)?;
        // Every set: an image is device-wide, so an eviction that skipped one
        // output's bundles would leave that head importing a dead DMA-BUF.
        for set in self.target_sets.keys().copied().collect::<Vec<_>>() {
            for frame_slot in 0..NATIVE_FRAME_TARGET_SLOT_CAPACITY {
                evicted_import |= self
                    .with_frame_target_slot(set, frame_slot, |context| {
                        context.evict_current_target_import(image_id)
                    })
                    .expect("bounded native frame target slot")?;
            }
        }
        Ok(evicted_import)
    }
    fn evict_current_target_import(
        &mut self,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let mut evicted_import = false;
        if let Some(persistent) = self.composition_target.as_mut() {
            let surface = persistent.surface.egl_surface();
            self.egl
                .make_current(
                    self.display,
                    Some(surface),
                    Some(surface),
                    Some(persistent.target.egl_context),
                )
                .map_err(|_| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)?;
            let result = persistent.import_cache.evict(
                &self.egl,
                self.display,
                &persistent.target.pipeline,
                image_id,
            );
            let _ = self.egl.make_current(self.display, None, None, None);
            if result.is_err()
                && let Some(persistent) = self.composition_target.take()
            {
                self.stats.target_recreations = self.stats.target_recreations.saturating_add(1);
                self.stats.recovery_replacements =
                    self.stats.recovery_replacements.saturating_add(1);
                self.destroy_persistent_composition_target(persistent);
                evicted_import = true;
            } else {
                evicted_import = result?;
            }
        }
        Ok(evicted_import)
    }
    pub fn clear_renderer_images(&mut self) -> Result<usize, NativeGbmScanoutBufferExportDetail> {
        let mut cleared_imports = self.clear_current_target_imports()?;
        for set in self.target_sets.keys().copied().collect::<Vec<_>>() {
            for frame_slot in 0..NATIVE_FRAME_TARGET_SLOT_CAPACITY {
                cleared_imports = cleared_imports.saturating_add(
                    self.with_frame_target_slot(set, frame_slot, |context| {
                        context.clear_current_target_imports()
                    })
                    .expect("bounded native frame target slot")?,
                );
            }
        }
        let cleared_images = self.renderer_images.len();
        // Ordinary clear retires logical content, not the display. Keep valid
        // pending GPU proofs and their allocation owners until completion.
        self.poll_snapshot_fences();
        self.renderer_images.clear();
        self.capture_reuse.clear();
        self.renderer_image_bytes = 0;
        self.stats.snapshot_evictions =
            self.stats.snapshot_evictions.saturating_add(cleared_images);
        self.update_renderer_image_stats();
        Ok(cleared_imports.max(cleared_images))
    }
    fn clear_current_target_imports(
        &mut self,
    ) -> Result<usize, NativeGbmScanoutBufferExportDetail> {
        let mut cleared_imports = 0;
        if let Some(persistent) = self.composition_target.as_mut() {
            let surface = persistent.surface.egl_surface();
            self.egl
                .make_current(
                    self.display,
                    Some(surface),
                    Some(surface),
                    Some(persistent.target.egl_context),
                )
                .map_err(|_| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)?;
            let live_entries = persistent.import_cache.stats().live_entries;
            let result =
                persistent
                    .import_cache
                    .clear(&self.egl, self.display, &persistent.target.pipeline);
            let _ = self.egl.make_current(self.display, None, None, None);
            if result.is_err()
                && let Some(persistent) = self.composition_target.take()
            {
                self.stats.target_recreations = self.stats.target_recreations.saturating_add(1);
                self.stats.recovery_replacements =
                    self.stats.recovery_replacements.saturating_add(1);
                self.destroy_persistent_composition_target(persistent);
                cleared_imports = live_entries;
            } else {
                cleared_imports = result?;
            }
        }
        Ok(cleared_imports)
    }
    pub fn promote_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let Some(image) = self.renderer_images.get_mut(&image_id) else {
            return Ok(false);
        };
        if image.state == NativeRendererImageState::Promoted {
            return Ok(false);
        }
        image.state = NativeRendererImageState::Promoted;
        self.stats.snapshot_promotions = self.stats.snapshot_promotions.saturating_add(1);
        Ok(true)
    }
    pub fn export_promoted_renderer_image(
        &self,
        image_id: NativeRendererImageId,
    ) -> Result<Option<NativeRendererImageSnapshot>, NativeGbmScanoutBufferExportDetail> {
        let Some(image) = self.renderer_images.get(&image_id) else {
            return Ok(None);
        };
        if image.state != NativeRendererImageState::Promoted {
            return Ok(None);
        }
        if let Some(captured) = &image.pooled {
            // FDs duplicated by the caller can outlive every snapshot wrapper.
            // Even a partial/failed export must never return this storage to reuse.
            captured.generation.mark_exported();
        }
        let mut plane_fds = image.buffer.export_plane_fds()?.into_plane_fds();
        let pitches = image.buffer.plane_pitches();
        let offsets = image.buffer.plane_offsets();
        let planes = std::array::from_fn(|index| {
            plane_fds[index].take().map(|fd| NativeOwnedDmaBufPlane {
                fd,
                offset: offsets[index],
                stride: pitches[index],
            })
        });
        if planes[..usize::from(image.buffer.plane_count())]
            .iter()
            .any(Option::is_none)
        {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
        }
        Ok(Some(NativeRendererImageSnapshot {
            image_id,
            width: image.buffer.width(),
            height: image.buffer.height(),
            format: image.buffer.format(),
            modifier: image
                .buffer
                .modifier()
                .unwrap_or(u64::from(gbm::Modifier::Invalid)),
            plane_count: image.buffer.plane_count(),
            planes,
            charge: None,
            epoch: None,
        }))
    }
    pub fn restore_promoted_renderer_image(
        &mut self,
        snapshot: NativeRendererImageSnapshot,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let image_id = snapshot.image_id();
        if !self.capture_renderer_image(image_id, snapshot.as_frame())? {
            return Ok(false);
        }
        if !self.promote_renderer_image(image_id)? {
            let _ = self.rollback_renderer_image(image_id);
            return Err(NativeGbmScanoutBufferExportDetail::InvalidRendererImageId);
        }
        Ok(true)
    }
    pub fn rollback_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        if self
            .renderer_images
            .get(&image_id)
            .is_none_or(|image| image.state != NativeRendererImageState::Staged)
        {
            return Ok(false);
        }
        self.stats.snapshot_rollbacks = self.stats.snapshot_rollbacks.saturating_add(1);
        self.evict_renderer_image(image_id)
    }
    pub fn capture_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        if self.capture_reuse.source_cleanup_failed() {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        if !image_id.is_valid() || !frame.is_valid() {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        if self.renderer_images.contains_key(&image_id) {
            return Ok(false);
        }
        self.poll_snapshot_fences();
        let estimated_bytes = u64::from(frame.width)
            .checked_mul(u64::from(frame.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        if self.renderer_images.len() >= DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY {
            return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
        }
        let (buffer, pooled) = image_transfer_policy::capture_direct_or_transfer(
            self,
            |context| {
                let result = context.capture_direct_renderer_image(image_id, frame, estimated_bytes);
                if context.capture_reuse.source_cleanup_failed() {
                    return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
                }
                result
            },
            image_import_failure,
            |context, detail| context.transfer_renderer_image(image_id, frame, detail)
                .map(|buffer| (std::rc::Rc::new(buffer), None)),
        )?;
        if buffer.format() != frame.format {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
        }
        let bytes = u64::from(buffer.pitch())
            .checked_mul(u64::from(buffer.height()))
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
        if self
            .retained_renderer_image_bytes()
            .saturating_add(self.image_bridge_bytes())
            .saturating_add(if pooled.is_none() { bytes } else { 0 })
            > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
        {
            return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
        }
        self.renderer_images.insert(
            image_id,
            NativeRendererImage {
                buffer,
                pooled,
                state: NativeRendererImageState::Staged,
                bytes,
            },
        );
        self.renderer_image_bytes = self.renderer_image_bytes.saturating_add(bytes);
        self.stats.snapshot_captures = self.stats.snapshot_captures.saturating_add(1);
        self.update_renderer_image_stats();
        Ok(true)
    }
    fn update_renderer_image_stats(&mut self) {
        self.stats.snapshot_live_entries = self.renderer_images.len();
        self.stats.snapshot_live_bytes = self.renderer_image_bytes;
    }

    fn retained_renderer_image_bytes(&self) -> u64 {
        self.renderer_images.values().filter(|image| image.pooled.is_none())
            .fold(0_u64, |sum, image| sum.saturating_add(image.bytes))
            .saturating_add(self.capture_reuse.retained_bytes())
            .saturating_add(self.capture_reuse.source_bytes())
    }

    fn clear_renderer_import_caches(&mut self) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        self.clear_current_target_imports()?;
        for set in self.target_sets.keys().copied().collect::<Vec<_>>() {
            for slot in 0..NATIVE_FRAME_TARGET_SLOT_CAPACITY {
                self.with_frame_target_slot(set, slot, |context| context.clear_current_target_imports())
                    .expect("bounded native frame target slot")?;
            }
        }
        Ok(())
    }

    fn capture_direct_renderer_image(
        &mut self,
        image_id: NativeRendererImageId,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
        estimated_bytes: u64,
    ) -> Result<(std::rc::Rc<NativeGbmOwnedScanoutBuffer>, Option<NativePooledCapture>), NativeGbmScanoutBufferExportDetail> {
        if self.snapshot_reuse_enabled {
            let other_bytes = self.renderer_images.values().filter(|image| image.pooled.is_none())
                .fold(self.image_bridge_bytes(), |sum, image| sum.saturating_add(image.bytes));
            let other_count = self.renderer_images.values().filter(|image| image.pooled.is_none()).count();
            let available = DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET.saturating_sub(other_bytes);
            let count = DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY.saturating_sub(other_count);
            let mut result = self.capture_reuse.capture(
                &self.egl, self.display, &self.gbm_device, frame,
                available, count,
            );
            if matches!(result, Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull)) {
                // Idle GPU attachments must not displace live image admission.
                // Drop attachments first, then release spare storage, then retry
                // once. Outstanding GPU readers remain fenced and charged.
                self.clear_renderer_import_caches()?;
                self.capture_reuse.trim_to(available.saturating_sub(estimated_bytes))?;
                result = self.capture_reuse.capture(
                    &self.egl, self.display, &self.gbm_device, frame, available, count,
                );
            }
            if let Some(mut captured) = result? {
                if let Some((sync, use_guard)) = captured.take_copy_completion() {
                    let uses = vec![NativeSnapshotGpuUse {
                        allocation: captured.allocation.clone(),
                        use_guard,
                    }];
                    if self.snapshot_fences.len() < DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY {
                        self.snapshot_fences.push(NativeSnapshotFence { sync, uses });
                    } else {
                        quarantine_snapshot_uses(uses, &mut self.snapshot_quarantine);
                        let _ = unsafe { self.egl.destroy_sync(self.display, sync) };
                    }
                } else {
                    quarantine_snapshot_uses(vec![NativeSnapshotGpuUse {
                        allocation: captured.allocation.clone(),
                        use_guard: captured.generation.acquire_use(),
                    }], &mut self.snapshot_quarantine);
                }
                return Ok((captured.allocation.buffer.clone(), Some(captured)));
            }
        }
        if self.retained_renderer_image_bytes().saturating_add(self.image_bridge_bytes())
            .saturating_add(estimated_bytes) > DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET
        {
            return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
        }
        self.probe_renderer_image_import(frame)?;
        self.render_renderer_image_snapshot(image_id, frame, false)
            .map(|buffer| (std::rc::Rc::new(buffer), None))
    }
}
