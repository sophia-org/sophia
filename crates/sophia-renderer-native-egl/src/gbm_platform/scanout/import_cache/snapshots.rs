// Allocation attachments survive content retirement. They own storage only;
// every draw separately resolves a current image and holds its GPU-use guard.
struct NativeSnapshotImport {
    allocation: std::rc::Rc<super::NativeCaptureAllocation>,
    image_id: NativeRendererImageId,
    generation: u64,
    image: khronos_egl::Image,
    texture: glow::NativeTexture,
}

impl NativeDmaBufImportCache {
    fn evict_snapshot_imports(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let mut removed = false;
        while let Some(index) = self
            .snapshot_entries
            .iter()
            .position(|entry| entry.image_id == image_id)
        {
            let entry = self
                .snapshot_entries
                .remove(index)
                .expect("checked snapshot entry");
            self.stats.live_entries = self.stats.live_entries.saturating_sub(1);
            self.stats.evictions = self.stats.evictions.saturating_add(1);
            self.destroy_snapshot_import(entry, egl, display, pipeline)?;
            removed = true;
        }
        Ok(removed)
    }

    fn destroy_snapshot_import(
        &mut self,
        entry: NativeSnapshotImport,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        unsafe { pipeline.delete_texture(entry.texture) };
        if egl.destroy_image(display, entry.image).is_err() {
            entry.allocation.quarantine_output_image(entry.image);
            self.snapshot_cleanup_failed = true;
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        Ok(())
    }

    fn make_snapshot_room(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        if self.snapshot_cleanup_failed {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        while self.stats.live_entries >= self.entries.len() {
            let Some(entry) = self.snapshot_entries.pop_front() else {
                self.stats.capacity_rejections += 1;
                return Err(NativeGbmScanoutBufferExportDetail::DmaBufImportCacheFull);
            };
            self.stats.live_entries = self.stats.live_entries.saturating_sub(1);
            self.stats.evictions = self.stats.evictions.saturating_add(1);
            self.destroy_snapshot_import(entry, egl, display, pipeline)?;
        }
        Ok(())
    }

    pub(super) fn snapshot_texture(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        captured: &super::NativePooledCapture,
        layer: NativeDmaBufCompositionLayer<'_>,
        reuse: bool,
    ) -> Result<glow::NativeTexture, NativeGbmScanoutBufferExportDetail> {
        if self.snapshot_cleanup_failed || captured.allocation.cleanup_failed() {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        let generation = captured.generation.generation();
        if let Some(index) = self
            .snapshot_entries
            .iter()
            .position(|entry| std::rc::Rc::ptr_eq(&entry.allocation, &captured.allocation))
        {
            let mut entry = self
                .snapshot_entries
                .remove(index)
                .expect("checked snapshot entry");
            if !reuse {
                // Attribution mode keeps the same storage ownership while
                // performing a fresh import for every use of the allocation.
                self.stats.live_entries = self.stats.live_entries.saturating_sub(1);
                self.stats.evictions = self.stats.evictions.saturating_add(1);
                self.destroy_snapshot_import(entry, egl, display, pipeline)?;
            } else {
                if entry.generation != generation {
                    // The EGLImage names the same storage but its pixels were
                    // rewritten in the capture context. Refresh external visibility.
                    if let Err(error) = unsafe {
                        pipeline.rebind_egl_image_texture(egl, entry.texture, entry.image.as_ptr())
                    } {
                        self.stats.live_entries = self.stats.live_entries.saturating_sub(1);
                        self.stats.evictions = self.stats.evictions.saturating_add(1);
                        self.destroy_snapshot_import(entry, egl, display, pipeline)?;
                        return Err(error);
                    }
                    entry.generation = generation;
                }
                let texture = entry.texture;
                entry.image_id = layer.image_id;
                self.snapshot_entries.push_back(entry);
                self.stats.hits = self.stats.hits.saturating_add(1);
                return Ok(texture);
            }
        }
        self.make_snapshot_room(egl, display, pipeline)?;
        let image = create_dma_buf_image(egl, display, layer.frame)?;
        let texture = match unsafe { pipeline.create_egl_image_texture(egl, image.as_ptr()) } {
            Ok(texture) => texture,
            Err(error) => {
                if egl.destroy_image(display, image).is_err() {
                    captured.allocation.quarantine_output_image(image);
                    self.snapshot_cleanup_failed = true;
                    return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
                }
                return Err(error);
            }
        };
        self.snapshot_entries.push_back(NativeSnapshotImport {
            allocation: captured.allocation.clone(),
            image_id: layer.image_id,
            generation,
            image,
            texture,
        });
        self.stats.imports = self.stats.imports.saturating_add(1);
        self.stats.live_entries = self.stats.live_entries.saturating_add(1);
        Ok(texture)
    }
}
