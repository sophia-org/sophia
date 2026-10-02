pub struct NativeGbmRenderedScanoutContextReport<T: std::os::fd::AsFd> {
    pub status: NativeGbmRenderedScanoutContextStatus,
    pub context: Option<NativeGbmRenderedScanoutContext<T>>,
}

pub fn export_gbm_scanout_buffer_from_backend_device_result<T: std::os::fd::AsFd>(
    device: std::io::Result<T>,
    width: u32,
    height: u32,
) -> NativeGbmOwnedScanoutBufferExportReport {
    if width == 0 || height == 0 {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
            detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    }

    let Ok(device) = device else {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::Unavailable,
            detail: NativeGbmScanoutBufferExportDetail::BackendDeviceUnavailable,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    };
    let Ok(device) = gbm::Device::new(device) else {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::Unavailable,
            detail: NativeGbmScanoutBufferExportDetail::GbmDeviceUnavailable,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    };
    let Ok(buffer) = device.create_buffer_object::<()>(
        width,
        height,
        gbm::Format::Xrgb8888,
        gbm::BufferObjectFlags::SCANOUT | gbm::BufferObjectFlags::RENDERING,
    ) else {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::Unavailable,
            detail: NativeGbmScanoutBufferExportDetail::GbmSurfaceUnavailable,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    };

    match native_owned_scanout_buffer_from_bo(width, height, buffer, None) {
        Ok(buffer) => exported_scanout_buffer_report(buffer),
        Err(detail) => failed_scanout_buffer_report(detail),
    }
}

/// Allocates and writes a scanout buffer without initializing EGL.
///
/// Mirror startup uses this fail-closed path before ownership moves to renderer
/// workers. A host that cannot expose a writable linear GBM scanout buffer is
/// rejected rather than silently falling back to an inline EGL context.
pub fn export_direct_cpu_xrgb8888_gbm_scanout_buffer_from_backend_device_result<
    T: std::os::fd::AsFd,
>(
    device: std::io::Result<T>,
    width: u32,
    height: u32,
    stride: u32,
    pixels: &[u8],
) -> NativeGbmOwnedScanoutBufferExportReport {
    let expected_stride = width.checked_mul(4);
    let expected_len = expected_stride
        .and_then(|stride| usize::try_from(stride).ok())
        .and_then(|stride| stride.checked_mul(usize::try_from(height).ok()?));
    if width == 0
        || height == 0
        || expected_stride != Some(stride)
        || expected_len != Some(pixels.len())
    {
        return failed_scanout_buffer_report(NativeGbmScanoutBufferExportDetail::InvalidTarget);
    }
    let Ok(device) = device else {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::Unavailable,
            detail: NativeGbmScanoutBufferExportDetail::BackendDeviceUnavailable,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    };
    let gbm_device = match gbm::Device::new(device) {
        Ok(device) => device,
        Err(_) => {
            return failed_scanout_buffer_report(
                NativeGbmScanoutBufferExportDetail::GbmDeviceUnavailable,
            );
        }
    };
    match write_direct_cpu_xrgb8888_scanout_buffer(&gbm_device, width, height, pixels) {
        Ok(buffer) => exported_scanout_buffer_report(buffer),
        Err(detail) => failed_scanout_buffer_report(detail),
    }
}

pub fn export_rendered_gbm_scanout_buffer_from_backend_device_result<T: std::os::fd::AsFd>(
    device: std::io::Result<T>,
    width: u32,
    height: u32,
) -> NativeGbmOwnedScanoutBufferExportReport {
    export_rendered_gbm_scanout_buffer_with_modifiers_from_backend_device_result(
        device,
        width,
        height,
        &[],
    )
}

pub fn export_rendered_gbm_scanout_buffer_with_modifiers_from_backend_device_result<
    T: std::os::fd::AsFd,
>(
    device: std::io::Result<T>,
    width: u32,
    height: u32,
    preferred_modifiers: &[u64],
) -> NativeGbmOwnedScanoutBufferExportReport {
    if width == 0 || height == 0 {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
            detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    }

    let Ok(device) = device else {
        return NativeGbmOwnedScanoutBufferExportReport {
            status: NativeGbmScanoutBufferExportStatus::Unavailable,
            detail: NativeGbmScanoutBufferExportDetail::BackendDeviceUnavailable,
            buffer: None,
            buffer_age: None,
            target_generation: None,
            repaint: NativeCompositionRepaintOutcome::Full,
        };
    };

    match render_gbm_scanout_front_buffer(device, width, height, preferred_modifiers) {
        Ok(buffer) => exported_scanout_buffer_report(buffer),
        Err(detail) => failed_scanout_buffer_report(detail),
    }
}

fn native_owned_scanout_buffer_from_bo(
    width: u32,
    height: u32,
    buffer: gbm::BufferObject<()>,
    surface: Option<gbm::Surface<()>>,
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    let pitch = buffer.stride();
    let format = buffer.format() as u32;
    let gem_handle = unsafe { buffer.handle().u32_ };
    let plane_count = buffer.plane_count();
    let plane_handles = scanout_plane_handles(&buffer, plane_count);
    let plane_pitches = scanout_plane_pitches(&buffer, plane_count);
    let plane_offsets = scanout_plane_offsets(&buffer, plane_count);
    let plane_fds = capture_scanout_plane_fds(&buffer, plane_count).ok();
    if pitch == 0
        || gem_handle == 0
        || !is_supported_scanout_format(format)
        || !is_valid_scanout_planes(gem_handle, plane_count, plane_handles, plane_pitches)
    {
        return Err(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor);
    }

    Ok(NativeGbmOwnedScanoutBuffer {
        width,
        height,
        pitch,
        format,
        gem_handle,
        plane_count: plane_count as u8,
        plane_handles,
        plane_pitches,
        plane_offsets,
        plane_fds,
        modifier: normalized_scanout_modifier(buffer.modifier()),
        _buffer: Some(buffer),
        _sampled_snapshots: Vec::new(),
        _egl_surface: None,
        _surface: surface,
        _frame_surface: None,
    })
}

fn normalized_scanout_modifier(modifier: gbm::Modifier) -> Option<u64> {
    (!matches!(modifier, gbm::Modifier::Invalid)).then(|| modifier.into())
}

fn scanout_plane_handles(buffer: &gbm::BufferObject<()>, plane_count: u32) -> [u32; 4] {
    [
        plane_handle(buffer, plane_count, 0),
        plane_handle(buffer, plane_count, 1),
        plane_handle(buffer, plane_count, 2),
        plane_handle(buffer, plane_count, 3),
    ]
}

fn scanout_plane_pitches(buffer: &gbm::BufferObject<()>, plane_count: u32) -> [u32; 4] {
    [
        plane_pitch(buffer, plane_count, 0),
        plane_pitch(buffer, plane_count, 1),
        plane_pitch(buffer, plane_count, 2),
        plane_pitch(buffer, plane_count, 3),
    ]
}

fn scanout_plane_offsets(buffer: &gbm::BufferObject<()>, plane_count: u32) -> [u32; 4] {
    [
        plane_offset(buffer, plane_count, 0),
        plane_offset(buffer, plane_count, 1),
        plane_offset(buffer, plane_count, 2),
        plane_offset(buffer, plane_count, 3),
    ]
}

fn plane_handle(buffer: &gbm::BufferObject<()>, plane_count: u32, plane: i32) -> u32 {
    if plane < plane_count as i32 {
        unsafe { buffer.handle_for_plane(plane).u32_ }
    } else {
        0
    }
}

fn plane_pitch(buffer: &gbm::BufferObject<()>, plane_count: u32, plane: i32) -> u32 {
    if plane < plane_count as i32 {
        buffer.stride_for_plane(plane)
    } else {
        0
    }
}

fn plane_offset(buffer: &gbm::BufferObject<()>, plane_count: u32, plane: i32) -> u32 {
    if plane < plane_count as i32 {
        buffer.offset(plane)
    } else {
        0
    }
}

fn capture_scanout_plane_fds(
    buffer: &gbm::BufferObject<()>,
    plane_count: u32,
) -> Result<[Option<OwnedFd>; 4], NativeGbmScanoutBufferExportDetail> {
    let mut plane_fds = std::array::from_fn(|_| None);
    let mut index = 0;
    while index < plane_count as usize {
        plane_fds[index] = Some(
            buffer
                .fd_for_plane(index as i32)
                .map_err(|_error| NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?,
        );
        index += 1;
    }
    Ok(plane_fds)
}

fn is_valid_scanout_planes(
    gem_handle: u32,
    plane_count: u32,
    plane_handles: [u32; 4],
    plane_pitches: [u32; 4],
) -> bool {
    plane_count > 0
        && plane_count <= 4
        && plane_handles[0] == gem_handle
        && plane_handles
            .iter()
            .zip(plane_pitches)
            .enumerate()
            .all(|(index, (handle, pitch))| {
                if index < plane_count as usize {
                    *handle != 0 && pitch != 0
                } else {
                    *handle == 0 && pitch == 0
                }
            })
}
