// Capture source imports, scoped to one unshared GL context and EGLDisplay.
// Cached objects describe storage, never a retained content generation. Every
// use refreshes the texture binding; every Present still owes synchronization.

const CAPTURE_SOURCE_LIMIT: usize = 16;
const CAPTURE_SOURCE_BYTE_LIMIT: u64 = 128 * 1024 * 1024;

#[derive(Default)]
struct NativeCaptureSourceBudget {
    usage: std::cell::Cell<(usize, u64)>,
    poisoned: std::cell::Cell<usize>,
    unbudgeted: std::cell::Cell<usize>,
    // This owner survives capture-manager clear and is dropped only after the
    // renderer's eglTerminate. An EGLImage can outlive its creating context.
    graveyard: std::cell::RefCell<Vec<NativeCaptureSourceQuarantine>>,
}

impl NativeCaptureSourceBudget {
    fn usage(&self) -> (usize, u64) {
        let (count, bytes) = self.usage.get();
        let unbudgeted = self.unbudgeted.get();
        (
            count.saturating_add(unbudgeted),
            if unbudgeted == 0 { bytes } else { u64::MAX },
        )
    }

    fn poisoned(&self) -> bool {
        self.poisoned.get() != 0
    }

    fn can_reserve(&self, bytes: u64) -> bool {
        let (count, used) = self.usage();
        !self.poisoned()
            && bytes > 0
            && count < CAPTURE_SOURCE_LIMIT
            && bytes <= CAPTURE_SOURCE_BYTE_LIMIT.saturating_sub(used)
    }

    fn reserve(self: &std::rc::Rc<Self>, bytes: u64) -> Option<NativeCaptureSourceCharge> {
        if !self.can_reserve(bytes) {
            return None;
        }
        let (count, used) = self.usage();
        self.usage.set((count + 1, used + bytes));
        Some(NativeCaptureSourceCharge {
            budget: std::rc::Rc::downgrade(self),
            bytes,
        })
    }
}

struct NativeCaptureSourceCharge {
    // Weak avoids a cycle when the budget itself owns a failed-cleanup record.
    budget: std::rc::Weak<NativeCaptureSourceBudget>,
    bytes: u64,
}

impl Drop for NativeCaptureSourceCharge {
    fn drop(&mut self) {
        if let Some(budget) = self.budget.upgrade() {
            let (count, bytes) = budget.usage.get();
            budget.usage.set((count - 1, bytes - self.bytes));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeCaptureSourceFingerprint {
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    plane_count: u8,
    planes: [(u64, u64, u32, u32); 4],
}

impl NativeCaptureSourceFingerprint {
    fn from_frame(
        frame: NativeMultiPlaneDmaBufFrame<'_>,
    ) -> Result<Self, NativeGbmScanoutBufferExportDetail> {
        if !frame.is_valid() {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
        }
        let mut planes = [(0, 0, 0, 0); 4];
        for (index, destination) in planes.iter_mut().enumerate().take(frame.plane_count.into()) {
            let plane = frame.planes[index]
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
            let stat = rustix::fs::fstat(plane.fd)
                .map_err(|_| NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
            *destination = (stat.st_dev, stat.st_ino, plane.offset, plane.stride);
        }
        Ok(Self {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            plane_count: frame.plane_count,
            planes,
        })
    }

    /// Unknown/zero sizes are usable for a transient import, never retained.
    /// A plane pair sharing one DMA-BUF is charged once, including its padding.
    fn allocation_bytes(self, frame: NativeMultiPlaneDmaBufFrame<'_>) -> Option<u64> {
        let mut seen = std::collections::BTreeSet::new();
        let mut bytes = 0_u64;
        for index in 0..usize::from(self.plane_count) {
            let (device, inode, _, _) = self.planes[index];
            if seen.insert((device, inode)) {
                let plane = frame.planes[index]?;
                let size = rustix::fs::seek(plane.fd, rustix::fs::SeekFrom::End(0)).ok()?;
                if size == 0 {
                    return None;
                }
                bytes = bytes.checked_add(size)?;
            }
        }
        Some(bytes)
    }
}

struct NativeCaptureSourceImport {
    fingerprint: NativeCaptureSourceFingerprint,
    image: khronos_egl::Image,
    texture: glow::NativeTexture,
    // These references keep allocation identity alive, including between uses.
    _planes: Vec<std::os::fd::OwnedFd>,
    charge: Option<NativeCaptureSourceCharge>,
    last_use: u64,
}

struct NativeCaptureSourceQuarantine {
    image: khronos_egl::Image,
    _planes: Vec<std::os::fd::OwnedFd>,
    charge: Option<NativeCaptureSourceCharge>,
    _poison: NativeCaptureSourcePoison,
}

struct NativeCaptureSourcePoison {
    budget: std::rc::Weak<NativeCaptureSourceBudget>,
    unbudgeted: bool,
}

impl Drop for NativeCaptureSourcePoison {
    fn drop(&mut self) {
        if let Some(budget) = self.budget.upgrade() {
            budget.poisoned.set(budget.poisoned.get() - 1);
            if self.unbudgeted {
                budget.unbudgeted.set(budget.unbudgeted.get() - 1);
            }
        }
    }
}

impl NativeCaptureSourceQuarantine {
    fn new(
        budget: &std::rc::Rc<NativeCaptureSourceBudget>,
        image: khronos_egl::Image,
        planes: Vec<std::os::fd::OwnedFd>,
        charge: Option<NativeCaptureSourceCharge>,
    ) -> Self {
        let unbudgeted = charge.is_none();
        budget.poisoned.set(budget.poisoned.get() + 1);
        if unbudgeted {
            budget.unbudgeted.set(budget.unbudgeted.get() + 1);
        }
        Self {
            image,
            _planes: planes,
            charge,
            _poison: NativeCaptureSourcePoison {
                budget: std::rc::Rc::downgrade(budget),
                unbudgeted,
            },
        }
    }

    fn from_import(
        budget: &std::rc::Rc<NativeCaptureSourceBudget>,
        entry: NativeCaptureSourceImport,
    ) -> Self {
        Self::new(budget, entry.image, entry._planes, entry.charge)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct NativeCaptureSourceStats {
    imports: usize,
    hits: usize,
    rebinds: usize,
    evictions: usize,
    transient_imports: usize,
}

struct NativeCaptureSourceUse {
    texture: glow::NativeTexture,
    serial: u64,
    owner: std::rc::Rc<()>,
}

impl NativeCaptureSourceUse {
    fn texture(&self) -> glow::NativeTexture {
        self.texture
    }
}

struct NativeCaptureSourceCache {
    identity: std::rc::Rc<()>,
    budget: std::rc::Rc<NativeCaptureSourceBudget>,
    entries: Vec<NativeCaptureSourceImport>,
    transient: Option<NativeCaptureSourceImport>,
    quarantine: Vec<NativeCaptureSourceQuarantine>,
    active: Option<u64>,
    serial: u64,
    stats: NativeCaptureSourceStats,
}

impl Default for NativeCaptureSourceCache {
    fn default() -> Self {
        Self::with_budget(std::rc::Rc::new(NativeCaptureSourceBudget::default()))
    }
}

impl NativeCaptureSourceCache {
    fn with_budget(budget: std::rc::Rc<NativeCaptureSourceBudget>) -> Self {
        Self {
            identity: std::rc::Rc::new(()),
            budget,
            entries: Vec::new(),
            transient: None,
            quarantine: Vec::new(),
            active: None,
            serial: 0,
            stats: NativeCaptureSourceStats::default(),
        }
    }

    fn stats(&self) -> NativeCaptureSourceStats {
        self.stats
    }

    fn resident_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|entry| entry.charge.as_ref())
            .chain(
                self.quarantine
                    .iter()
                    .filter_map(|entry| entry.charge.as_ref()),
            )
            .map(|charge| charge.bytes)
            .sum()
    }

    /// `budget_available` is this cache's total allowed resident bytes, not
    /// additional headroom. All caches also share the renderer's count/byte cap.
    /// One use must be finished before requesting another or trimming the cache.
    fn texture(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
        budget_available: u64,
    ) -> Result<NativeCaptureSourceUse, NativeGbmScanoutBufferExportDetail> {
        if self.active.is_some() {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        if self.budget.poisoned() {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        let fingerprint = NativeCaptureSourceFingerprint::from_frame(frame)?;
        self.trim_to(egl, display, pipeline, budget_available)?;
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.fingerprint == fingerprint)
        {
            let entry = &mut self.entries[index];
            // X readiness is not a GPU fence. This refreshes external content;
            // GPU ordering still belongs to each submission's synchronization.
            if let Err(error) = unsafe {
                pipeline.rebind_egl_image_texture(egl, entry.texture, entry.image.as_ptr())
            } {
                let _ = self.remove(index, egl, display, pipeline);
                return Err(error);
            }
            entry.last_use = self.serial;
            self.stats.hits = self.stats.hits.saturating_add(1);
            self.stats.rebinds = self.stats.rebinds.saturating_add(1);
            self.active = Some(self.serial);
            return Ok(NativeCaptureSourceUse {
                texture: entry.texture,
                serial: self.serial,
                owner: self.identity.clone(),
            });
        }
        let bytes = fingerprint.allocation_bytes(frame);
        let charge = if let Some(bytes) =
            bytes.filter(|bytes| *bytes <= budget_available && *bytes <= CAPTURE_SOURCE_BYTE_LIMIT)
        {
            while (!self.budget.can_reserve(bytes)
                || bytes > budget_available.saturating_sub(self.resident_bytes()))
                && !self.entries.is_empty()
            {
                self.remove_oldest(egl, display, pipeline)?;
            }
            self.budget.reserve(bytes)
        } else {
            None
        };
        let mut planes = Vec::with_capacity(frame.plane_count.into());
        for plane in frame.planes.iter().take(frame.plane_count.into()) {
            planes.push(
                plane
                    .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?
                    .fd
                    .try_clone_to_owned()
                    .map_err(|_| NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?,
            );
        }
        let image = create_dma_buf_image(egl, display, frame)?;
        let texture = match unsafe { pipeline.create_egl_image_texture(egl, image.as_ptr()) } {
            Ok(texture) => texture,
            Err(error) => {
                if egl.destroy_image(display, image).is_err() {
                    self.quarantine.push(NativeCaptureSourceQuarantine::new(
                        &self.budget,
                        image,
                        planes,
                        charge,
                    ));
                }
                return Err(error);
            }
        };
        let entry = NativeCaptureSourceImport {
            fingerprint,
            image,
            texture,
            _planes: planes,
            charge,
            last_use: self.serial,
        };
        if entry.charge.is_some() {
            self.entries.push(entry);
        } else {
            self.stats.transient_imports = self.stats.transient_imports.saturating_add(1);
            self.transient = Some(entry);
        }
        self.stats.imports = self.stats.imports.saturating_add(1);
        self.active = Some(self.serial);
        Ok(NativeCaptureSourceUse {
            texture,
            serial: self.serial,
            owner: self.identity.clone(),
        })
    }

    fn finish_use(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        source: NativeCaptureSourceUse,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        if !std::rc::Rc::ptr_eq(&self.identity, &source.owner) || self.active != Some(source.serial)
        {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        self.active = None;
        if let Some(entry) = self.transient.take() {
            self.destroy(entry, egl, display, Some(pipeline))?;
        }
        Ok(())
    }

    fn trim_to(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        max_bytes: u64,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        if self.active.is_some() {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        if self.budget.poisoned() {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        while self.resident_bytes() > max_bytes {
            self.remove_oldest(egl, display, pipeline)?;
        }
        Ok(())
    }

    fn remove_oldest(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        let index = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_use)
            .map(|(index, _)| index)
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        self.remove(index, egl, display, pipeline)
    }

    fn remove(
        &mut self,
        index: usize,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        let entry = self.entries.swap_remove(index);
        self.destroy(entry, egl, display, Some(pipeline))
    }

    fn destroy(
        &mut self,
        entry: NativeCaptureSourceImport,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: Option<&PersistentXrgb8888GlPipeline>,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        self.stats.evictions = self.stats.evictions.saturating_add(1);
        let Some(pipeline) = pipeline else {
            // Without a current context the texture sibling is still alive.
            // Even destroying its EGLImage would not prove storage released,
            // since the following context destruction can also fail.
            self.budget
                .graveyard
                .borrow_mut()
                .push(NativeCaptureSourceQuarantine::from_import(
                    &self.budget,
                    entry,
                ));
            return Ok(());
        };
        unsafe { pipeline.delete_texture(entry.texture) };
        let destroyed = egl.destroy_image(display, entry.image).is_ok();
        if destroyed {
            // FD and budget custody ends only after successful destruction.
            Ok(())
        } else {
            // The GL name was already deleted; never delete or reuse it again.
            // Keep both storage identity and the charge until cleanup succeeds.
            self.quarantine
                .push(NativeCaptureSourceQuarantine::from_import(
                    &self.budget,
                    entry,
                ));
            Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed)
        }
    }

    fn retry_quarantine(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
    ) {
        self.retry_quarantine_with(|image| egl.destroy_image(display, image).is_ok());
    }

    fn retry_quarantine_with(&mut self, mut destroyed: impl FnMut(khronos_egl::Image) -> bool) {
        self.quarantine.retain(|entry| !destroyed(entry.image));
    }

    fn clear(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        self.active = None;
        self.retry_quarantine(egl, display);
        let mut error = None;
        while let Some(entry) = self.entries.pop().or_else(|| self.transient.take()) {
            if let Err(detail) = self.destroy(entry, egl, display, Some(pipeline)) {
                error = Some(detail);
            }
        }
        if !self.quarantine.is_empty() {
            error = Some(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        error.map_or(Ok(()), Err)
    }

    /// Only for a lost context which the caller immediately destroys. Imports
    /// with un-deleted texture siblings stay charged through display destruction.
    fn abandon(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
    ) {
        self.active = None;
        self.retry_quarantine(egl, display);
        while let Some(entry) = self.entries.pop().or_else(|| self.transient.take()) {
            let _ = self.destroy(entry, egl, display, None);
        }
    }
}

impl Drop for NativeCaptureSourceCache {
    fn drop(&mut self) {
        // NativeCaptureExecution explicitly clears/abandons before destroying
        // the GL context. Any unresolved EGLImages live until eglTerminate,
        // including an unexpected ordinary entry from an incomplete teardown.
        let mut graveyard = self.budget.graveyard.borrow_mut();
        graveyard.extend(self.quarantine.drain(..));
        graveyard.extend(
            self.entries
                .drain(..)
                .map(|entry| NativeCaptureSourceQuarantine::from_import(&self.budget, entry)),
        );
        graveyard.extend(
            self.transient
                .take()
                .map(|entry| NativeCaptureSourceQuarantine::from_import(&self.budget, entry)),
        );
    }
}

#[cfg(test)]
mod source_import_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/source_imports.rs"
    ));
}
