#[derive(Debug)]
pub struct NativeGbmOwnedScanoutBuffer {
    width: u32,
    height: u32,
    pitch: u32,
    format: u32,
    gem_handle: u32,
    plane_count: u8,
    plane_handles: [u32; 4],
    plane_pitches: [u32; 4],
    plane_offsets: [u32; 4],
    plane_fds: Option<[Option<OwnedFd>; 4]>,
    modifier: Option<u64>,
    // Drop explicitly releases the locked front buffer before its surface.
    _buffer: Option<gbm::BufferObject<()>>,
    _sampled_snapshots: Vec<std::sync::Arc<NativeRendererImageSnapshot>>,
    _egl_surface: Option<NativeEglSurfaceOwner>,
    _surface: Option<gbm::Surface<()>>,
    _frame_surface: Option<std::rc::Rc<NativeFrameSurface>>,
}

#[derive(Debug)]
struct NativeEglSurfaceOwner {
    destroy_surface: unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32,
    display: khronos_egl::Display,
    surface: khronos_egl::Surface,
}

#[derive(Debug)]
struct NativeFrameSurface {
    egl_surface: NativeEglSurfaceOwner,
    gbm_surface: gbm::Surface<()>,
}

impl NativeFrameSurface {
    const fn egl_surface(&self) -> khronos_egl::Surface {
        self.egl_surface.surface
    }

    fn gbm_surface(&self) -> &gbm::Surface<()> {
        &self.gbm_surface
    }
}

impl Drop for NativeEglSurfaceOwner {
    fn drop(&mut self) {
        unsafe {
            (self.destroy_surface)(self.display.as_ptr(), self.surface.as_ptr());
        }
        trace_native_lifecycle("egl_surface_destroyed");
    }
}

impl Drop for NativeGbmOwnedScanoutBuffer {
    fn drop(&mut self) {
        trace_native_lifecycle("scanout_owner_drop_started");
        drop(self._buffer.take());
        trace_native_lifecycle("front_buffer_released");
        drop(self._egl_surface.take());
        drop(self._surface.take());
        trace_native_lifecycle("originating_surface_released");
        drop(self._frame_surface.take());
        trace_native_lifecycle("frame_surface_lease_released");
    }
}

impl NativeGbmOwnedScanoutBuffer {
    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub const fn pitch(&self) -> u32 {
        self.pitch
    }

    pub const fn format(&self) -> u32 {
        self.format
    }

    pub const fn gem_handle(&self) -> u32 {
        self.gem_handle
    }

    pub const fn plane_count(&self) -> u8 {
        self.plane_count
    }

    pub const fn plane_handles(&self) -> [u32; 4] {
        self.plane_handles
    }

    pub const fn plane_pitches(&self) -> [u32; 4] {
        self.plane_pitches
    }

    pub const fn plane_offsets(&self) -> [u32; 4] {
        self.plane_offsets
    }

    pub const fn modifier(&self) -> Option<u64> {
        self.modifier
    }

    pub fn rewrite_xrgb8888_damage(
        &mut self,
        pixels: &[u8],
        damage: &[NativeCompositionRect],
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;

        let source_stride = self
            .width
            .checked_mul(4)
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        let source_len = usize::try_from(source_stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(self.height).ok()?))
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        if self.format != DRM_FORMAT_XRGB8888
            || self.pitch < source_stride
            || pixels.len() != source_len
        {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget);
        }
        if damage.is_empty() {
            return Ok(());
        }
        let width = self.width;
        let height = self.height;
        let buffer = self
            ._buffer
            .as_mut()
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
        buffer
            .map_mut(0, 0, width, height, |mapped| {
                let target_stride = mapped.stride();
                copy_xrgb8888_damage(
                    pixels,
                    source_stride,
                    width,
                    height,
                    mapped.buffer_mut(),
                    target_stride,
                    damage,
                )
            })
            .map_err(|_| NativeGbmScanoutBufferExportDetail::CpuLayerUploadFailed)??;
        trace_native_lifecycle("cpu_frame_damage_written");
        Ok(())
    }

    pub fn export_plane_fds(
        &self,
    ) -> Result<NativeGbmOwnedScanoutBufferPlaneFds, NativeGbmScanoutBufferExportDetail> {
        if self.plane_count == 0 || self.plane_count as usize > self.plane_handles.len() {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
        }

        let Some(retained_plane_fds) = &self.plane_fds else {
            return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
        };

        let mut plane_fds = std::array::from_fn(|_| None);
        let mut index = 0;
        while index < self.plane_count as usize {
            let Some(fd) = &retained_plane_fds[index] else {
                return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
            };
            plane_fds[index] =
                Some(fd.try_clone().map_err(|_error| {
                    NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor
                })?);
            index += 1;
        }

        Ok(NativeGbmOwnedScanoutBufferPlaneFds {
            plane_count: self.plane_count,
            plane_fds,
        })
    }
}

fn copy_xrgb8888_damage(
    source: &[u8],
    source_stride: u32,
    width: u32,
    height: u32,
    target: &mut [u8],
    target_stride: u32,
    damage: &[NativeCompositionRect],
) -> Result<(), NativeGbmScanoutBufferExportDetail> {
    let target_len = usize::try_from(target_stride)
        .ok()
        .and_then(|stride| stride.checked_mul(usize::try_from(height).ok()?))
        .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
    if target.len() < target_len {
        return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
    }
    for rect in damage {
        let left = i64::from(rect.x).clamp(0, i64::from(width));
        let top = i64::from(rect.y).clamp(0, i64::from(height));
        let right = i64::from(rect.x)
            .saturating_add(i64::from(rect.width))
            .clamp(0, i64::from(width));
        let bottom = i64::from(rect.y)
            .saturating_add(i64::from(rect.height))
            .clamp(0, i64::from(height));
        if left >= right || top >= bottom {
            continue;
        }
        let left = usize::try_from(left).unwrap_or_default();
        let top = usize::try_from(top).unwrap_or_default();
        let right = usize::try_from(right).unwrap_or(left);
        let bottom = usize::try_from(bottom).unwrap_or(top);
        let row_bytes = right
            .saturating_sub(left)
            .checked_mul(4)
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
        for row in top..bottom {
            let source_start = row
                .checked_mul(usize::try_from(source_stride).unwrap_or(0))
                .and_then(|offset| offset.checked_add(left.saturating_mul(4)))
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
            let target_start = row
                .checked_mul(usize::try_from(target_stride).unwrap_or(0))
                .and_then(|offset| offset.checked_add(left.saturating_mul(4)))
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
            let source_end = source_start
                .checked_add(row_bytes)
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
            let target_end = target_start
                .checked_add(row_bytes)
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
            let source_row = source
                .get(source_start..source_end)
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidTarget)?;
            let target_row = target
                .get_mut(target_start..target_end)
                .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
            target_row.copy_from_slice(source_row);
        }
    }
    Ok(())
}

mod tests;

pub struct NativeGbmOwnedScanoutBufferPlaneFds {
    plane_count: u8,
    plane_fds: [Option<OwnedFd>; 4],
}

impl NativeGbmOwnedScanoutBufferPlaneFds {
    pub const fn plane_count(&self) -> u8 {
        self.plane_count
    }

    pub fn into_plane_fds(self) -> [Option<OwnedFd>; 4] {
        self.plane_fds
    }
}
