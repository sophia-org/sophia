struct NativeEglScanoutDevice<'a, T: std::os::fd::AsFd> {
    egl: &'a khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    gbm_device: &'a gbm::Device<T>,
}

struct RenderTargetSpec {
    width: u32,
    height: u32,
    config: khronos_egl::Config,
    candidate: RenderedScanoutCandidate,
}

struct RenderedFrontBufferSpec<'a> {
    width: u32,
    height: u32,
    config: khronos_egl::Config,
    surface_format: gbm::Format,
    surface_modifiers: &'a [gbm::Modifier],
    surface_usage: gbm::BufferObjectFlags,
}

fn render_gbm_scanout_front_buffer<T: std::os::fd::AsFd>(
    device: T,
    width: u32,
    height: u32,
    preferred_modifiers: &[u64],
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    use gbm::AsRaw as _;

    let gbm_device = gbm::Device::new(device)
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::GbmDeviceUnavailable)?;
    let egl = unsafe { khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required() }
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglUnavailable)?;

    let native_display = gbm_device.as_raw() as khronos_egl::NativeDisplayType;
    let display = unsafe {
        egl.get_platform_display(
            EGL_PLATFORM_GBM_KHR,
            native_display,
            &[khronos_egl::ATTRIB_NONE],
        )
    }
    .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglDisplayUnavailable)?;

    egl.initialize(display)
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglInitializeFailed)?;
    let result = render_initialized_gbm_scanout_front_buffer(
        &egl,
        display,
        &gbm_device,
        width,
        height,
        preferred_modifiers,
        None,
    );
    let _ = egl.terminate(display);
    result
}

fn render_initialized_gbm_scanout_front_buffer<T: std::os::fd::AsFd>(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    gbm_device: &gbm::Device<T>,
    width: u32,
    height: u32,
    preferred_modifiers: &[u64],
    frame: Option<NativeXrgb8888Frame<'_>>,
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    egl.bind_api(khronos_egl::OPENGL_API)
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglBindApiFailed)?;

    let preferred_modifiers = reduced_gbm_scanout_modifiers(preferred_modifiers);
    let mut last_detail = NativeGbmScanoutBufferExportDetail::EglConfigUnavailable;
    for candidate in rendered_scanout_candidates(&preferred_modifiers, None) {
        let Some(config) = choose_scanout_config_for_format(
            egl,
            display,
            candidate.config_attributes,
            candidate.format,
        ) else {
            continue;
        };

        match render_initialized_gbm_scanout_front_buffer_with_config(
            NativeEglScanoutDevice {
                egl,
                display,
                gbm_device,
            },
            RenderedFrontBufferSpec {
                width,
                height,
                config,
                surface_format: candidate.format,
                surface_modifiers: &candidate.modifiers,
                surface_usage: candidate.usage,
            },
            frame,
        ) {
            Ok(buffer) if is_supported_rendered_scanout_candidate_buffer(&buffer) => {
                return Ok(buffer);
            }
            Ok(_buffer) => {
                last_detail = preferred_scanout_failure_detail(
                    last_detail,
                    NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor,
                );
            }
            Err(detail) => last_detail = preferred_scanout_failure_detail(last_detail, detail),
        }
    }

    Err(last_detail)
}

fn create_native_render_target<T: std::os::fd::AsFd>(
    device: NativeEglScanoutDevice<'_, T>,
    spec: RenderTargetSpec,
) -> Result<
    (
        NativeRenderTarget,
        std::rc::Rc<NativeFrameSurface>,
        std::time::Duration,
    ),
    NativeGbmScanoutBufferExportDetail,
> {
    let NativeEglScanoutDevice {
        egl,
        display,
        gbm_device,
    } = device;
    let RenderTargetSpec {
        width,
        height,
        config,
        candidate,
    } = spec;
    let surface_started = Instant::now();
    let surface =
        create_native_frame_surface(egl, display, gbm_device, width, height, config, &candidate)?;
    let surface_create_duration = surface_started.elapsed();
    let egl_surface = surface.egl_surface();
    let egl_context = match egl.create_context(display, config, None, &context_attributes()) {
        Ok(context) => context,
        Err(_) => {
            return Err(NativeGbmScanoutBufferExportDetail::EglContextUnavailable);
        }
    };
    if egl
        .make_current(
            display,
            Some(egl_surface),
            Some(egl_surface),
            Some(egl_context),
        )
        .is_err()
    {
        let _ = egl.destroy_context(display, egl_context);
        return Err(NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed);
    }
    let loader = |name: &str| {
        egl.get_proc_address(name)
            .map_or(ptr::null(), |proc| proc as *const c_void)
    };
    let gl = unsafe { glow::Context::from_loader_function(loader) };
    let pipeline = match unsafe { PersistentXrgb8888GlPipeline::new(gl, width, height) } {
        Ok(pipeline) => pipeline,
        Err(_) => {
            let _ = egl.make_current(display, None, None, None);
            let _ = egl.destroy_context(display, egl_context);
            return Err(NativeGbmScanoutBufferExportDetail::GlSmokeFailed);
        }
    };
    let _ = egl.make_current(display, None, None, None);
    trace_native_lifecycle("native_render_target_created");
    Ok((
        NativeRenderTarget {
            width,
            height,
            surface_format: candidate.format,
            egl_context,
            pipeline,
        },
        surface,
        surface_create_duration,
    ))
}

fn create_native_frame_surface<T: std::os::fd::AsFd>(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    gbm_device: &gbm::Device<T>,
    width: u32,
    height: u32,
    config: khronos_egl::Config,
    candidate: &RenderedScanoutCandidate,
) -> Result<std::rc::Rc<NativeFrameSurface>, NativeGbmScanoutBufferExportDetail> {
    use gbm::AsRaw as _;

    let gbm_surface = create_rendered_scanout_surface(
        gbm_device,
        width,
        height,
        candidate.format,
        &candidate.modifiers,
        candidate.usage,
    )?;
    let native_window = gbm_surface.as_raw() as khronos_egl::NativeWindowType;
    let egl_surface = unsafe { egl.create_window_surface(display, config, native_window, None) }
        .map_err(|_| NativeGbmScanoutBufferExportDetail::EglSurfaceUnavailable)?;
    let Some(destroy_surface) = egl.get_proc_address("eglDestroySurface") else {
        let _ = egl.destroy_surface(display, egl_surface);
        return Err(NativeGbmScanoutBufferExportDetail::EglSurfaceUnavailable);
    };
    trace_native_lifecycle("frame_surface_created");
    Ok(std::rc::Rc::new(NativeFrameSurface {
        egl_surface: NativeEglSurfaceOwner {
            destroy_surface: unsafe {
                std::mem::transmute::<
                    extern "system" fn(),
                    unsafe extern "system" fn(*mut c_void, *mut c_void) -> u32,
                >(destroy_surface)
            },
            display,
            surface: egl_surface,
        },
        gbm_surface,
    }))
}

fn render_native_target_frame(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    target: &mut NativeRenderTarget,
    surface: std::rc::Rc<NativeFrameSurface>,
    pixels: &[u8],
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    let egl_surface = surface.egl_surface();
    if egl
        .make_current(
            display,
            Some(egl_surface),
            Some(egl_surface),
            Some(target.egl_context),
        )
        .is_err()
    {
        return Err(NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed);
    }
    trace_native_lifecycle("egl_surface_current");
    let result = target
        .pipeline
        .upload(pixels)
        .map_err(|_| NativeGbmScanoutBufferExportDetail::GlSmokeFailed)
        .and_then(|()| {
            trace_native_lifecycle("cpu_frame_uploaded");
            egl.swap_buffers(display, egl_surface)
                .map_err(|_| NativeGbmScanoutBufferExportDetail::EglSwapBuffersFailed)
        })
        .and_then(|()| {
            trace_native_lifecycle("egl_surface_swapped");
            let buffer = unsafe { surface.gbm_surface().lock_front_buffer() }
                .map_err(|_| NativeGbmScanoutBufferExportDetail::FrontBufferLockFailed)?;
            trace_native_lifecycle("scanout_front_buffer_locked");
            let mut buffer =
                native_owned_scanout_buffer_from_bo(target.width, target.height, buffer, None)?;
            buffer._frame_surface = Some(surface.clone());
            Ok(buffer)
        });
    let _ = egl.make_current(display, None, None, None);
    result
}

fn render_native_target_dmabuf(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    target: &mut NativeRenderTarget,
    surface: std::rc::Rc<NativeFrameSurface>,
    frame: NativeDmaBufFrame<'_>,
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    const EGL_LINUX_DMA_BUF_EXT: khronos_egl::Enum = 0x3270;
    const EGL_WIDTH: khronos_egl::Attrib = 0x3057;
    const EGL_HEIGHT: khronos_egl::Attrib = 0x3056;
    const EGL_LINUX_DRM_FOURCC_EXT: khronos_egl::Attrib = 0x3271;
    const EGL_DMA_BUF_PLANE0_FD_EXT: khronos_egl::Attrib = 0x3272;
    const EGL_DMA_BUF_PLANE0_OFFSET_EXT: khronos_egl::Attrib = 0x3273;
    const EGL_DMA_BUF_PLANE0_PITCH_EXT: khronos_egl::Attrib = 0x3274;
    const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: khronos_egl::Attrib = 0x3443;
    const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: khronos_egl::Attrib = 0x3444;

    let egl_surface = surface.egl_surface();
    if egl
        .make_current(
            display,
            Some(egl_surface),
            Some(egl_surface),
            Some(target.egl_context),
        )
        .is_err()
    {
        return Err(NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed);
    }
    trace_dmabuf_lifecycle("egl_surface_current");

    let mut attributes = vec![
        EGL_WIDTH,
        frame.width as khronos_egl::Attrib,
        EGL_HEIGHT,
        frame.height as khronos_egl::Attrib,
        EGL_LINUX_DRM_FOURCC_EXT,
        frame.format as khronos_egl::Attrib,
        EGL_DMA_BUF_PLANE0_FD_EXT,
        frame.fd.as_raw_fd() as khronos_egl::Attrib,
        EGL_DMA_BUF_PLANE0_OFFSET_EXT,
        frame.offset as khronos_egl::Attrib,
        EGL_DMA_BUF_PLANE0_PITCH_EXT,
        frame.stride as khronos_egl::Attrib,
    ];
    if frame.modifier != u64::from(gbm::Modifier::Invalid) {
        attributes.extend_from_slice(&[
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            (frame.modifier & u64::from(u32::MAX)) as khronos_egl::Attrib,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            (frame.modifier >> 32) as khronos_egl::Attrib,
        ]);
    }
    attributes.push(khronos_egl::ATTRIB_NONE);
    let no_context = unsafe { khronos_egl::Context::from_ptr(khronos_egl::NO_CONTEXT) };
    let no_buffer = unsafe { khronos_egl::ClientBuffer::from_ptr(ptr::null_mut()) };
    let image = egl
        .create_image(
            display,
            no_context,
            EGL_LINUX_DMA_BUF_EXT,
            no_buffer,
            &attributes,
        )
        .map_err(|_| NativeGbmScanoutBufferExportDetail::DmaBufImportFailed);
    let result = match image {
        Ok(image) => {
            trace_dmabuf_lifecycle("egl_image_created");
            let result = egl
                .get_proc_address("glEGLImageTargetTexture2DOES")
                .ok_or(NativeGbmScanoutBufferExportDetail::DmaBufImportFailed)
                .map(|image_target| unsafe {
                    std::mem::transmute::<
                        extern "system" fn(),
                        unsafe extern "system" fn(u32, *const c_void),
                    >(image_target)
                })
                .and_then(|image_target| {
                    unsafe { target.pipeline.draw_egl_image(image_target, image.as_ptr()) }
                        .map_err(|_| NativeGbmScanoutBufferExportDetail::DmaBufImportFailed)
                })
                .and_then(|()| {
                    trace_dmabuf_lifecycle("egl_image_texture_released");
                    trace_dmabuf_lifecycle("egl_image_rendered");
                    egl.swap_buffers(display, egl_surface)
                        .map_err(|_| NativeGbmScanoutBufferExportDetail::EglSwapBuffersFailed)
                })
                .and_then(|()| {
                    trace_dmabuf_lifecycle("egl_surface_swapped");
                    let buffer = unsafe { surface.gbm_surface().lock_front_buffer() }
                        .map_err(|_| NativeGbmScanoutBufferExportDetail::FrontBufferLockFailed)?;
                    trace_dmabuf_lifecycle("scanout_front_buffer_locked");
                    let mut buffer = native_owned_scanout_buffer_from_bo(
                        target.width,
                        target.height,
                        buffer,
                        None,
                    )?;
                    buffer._frame_surface = Some(surface.clone());
                    Ok(buffer)
                });
            let image_destroyed = egl.destroy_image(display, image).is_ok();
            if image_destroyed {
                trace_dmabuf_lifecycle("egl_image_destroyed");
            }
            match result {
                Ok(buffer) if image_destroyed => {
                    trace_dmabuf_lifecycle("scanout_owner_returned");
                    Ok(buffer)
                }
                Ok(_) => Err(NativeGbmScanoutBufferExportDetail::DmaBufImportFailed),
                Err(detail) => Err(detail),
            }
        }
        Err(detail) => Err(detail),
    };
    let _ = egl.make_current(display, None, None, None);
    result
}

#[derive(Clone, Copy, Debug, Default)]
struct NativeCompositionRenderEvidence {
    captured_pixels: Option<NativeCompositionPixelMetrics>,
    traced_nonzero_rgb_pixels: usize,
    /// The age the surface reported for the buffer this render drew into.
    buffer_age: Option<u32>,
    /// Whether this render repainted the whole target or only its damage.
    repaint: NativeCompositionRepaintOutcome,
    repaint_pixels: u64,
}

/// Ask the surface how old the buffer it just handed back is, in renders into
/// this same surface.
///
/// Anything the driver will not vouch for reads as `None`, and `None` means a
/// full repaint: an age of zero is defined as "contents undefined", and a
/// negative or absurd value is a driver disagreeing with itself. Guessing here
/// would produce a frame that looks right and is stale in one corner.
fn query_native_surface_buffer_age(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    surface: khronos_egl::Surface,
    supported: bool,
) -> Option<u32> {
    if !supported {
        return None;
    }
    match egl.query_surface(display, surface, EGL_BUFFER_AGE_EXT) {
        Ok(age) if age > 0 => u32::try_from(age).ok(),
        _ => None,
    }
}

fn render_native_target_composition(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    target: &mut NativeRenderTarget,
    surface: std::rc::Rc<NativeFrameSurface>,
    import_cache: &mut NativeDmaBufImportCache,
    renderer_images: &std::collections::BTreeMap<NativeRendererImageId, NativeRendererImage>,
    frame: NativeCompositionFrame<'_>,
    capture_pixels: bool,
    preserve_target_alpha: bool,
    buffer_age_supported: bool,
) -> Result<
    (NativeGbmOwnedScanoutBuffer, NativeCompositionRenderEvidence),
    NativeGbmScanoutBufferExportDetail,
> {
    let egl_surface = surface.egl_surface();
    if egl
        .make_current(
            display,
            Some(egl_surface),
            Some(egl_surface),
            Some(target.egl_context),
        )
        .is_err()
    {
        return Err(NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed);
    }

    trace_native_lifecycle("composition_surface_current");
    // The age belongs to the buffer this context just made current, so it can
    // only be read here -- and only before anything is drawn into it.
    let buffer_age =
        query_native_surface_buffer_age(egl, display, egl_surface, buffer_age_supported);
    let damage = frame
        .repaint
        .and_then(|table| table.damage_for_age(buffer_age.unwrap_or(0)));
    // A full repaint is one unconfined pass. A damage-limited repaint is one
    // pass per rectangle, each clearing and redrawing only inside itself, so
    // the pixels the buffer is being reused for survive between them.
    let passes: Vec<Option<GlCompositionRect>> = match damage {
        None => vec![None],
        Some(rects) => rects
            .iter()
            .map(|rect| {
                Some(GlCompositionRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                })
            })
            .collect(),
    };
    let repaint_pixels = damage.map_or(u64::from(frame.width) * u64::from(frame.height), |rects| {
        rects.iter().fold(0u64, |sum, r| sum.saturating_add(
            u64::try_from(r.width.max(0)).unwrap_or(0)
                .saturating_mul(u64::try_from(r.height.max(0)).unwrap_or(0))))
    });
    let repaint = match damage {
        None => NativeCompositionRepaintOutcome::Full,
        Some(rects) => NativeCompositionRepaintOutcome::Partial { rects: rects.len() },
    };
    static PIXEL_TRACE_CLAIMED: AtomicBool = AtomicBool::new(false);
    let pixel_trace = std::env::var("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE").ok();
    let final_regions_only = pixel_trace.as_deref() == Some("final-regions");
    let continuous_trace = pixel_trace.as_deref() == Some("continuous") || final_regions_only;
    let trace_pixels = continuous_trace
        || (pixel_trace.is_some()
            && PIXEL_TRACE_CLAIMED
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok());
    let trace_layer_pixels = trace_pixels && !final_regions_only;
    if trace_pixels {
        tracing::info!(
            "sophia_native_composition_pixels schema=1 status=enabled width={} height={} layers={}",
            frame.width,
            frame.height,
            frame.layers.len(),
        );
    }
    let mut draw_result = Ok(());
    for pass in &passes {
        if draw_result.is_err() {
            break;
        }
        target.pipeline.set_ambient_clip(*pass);
        if preserve_target_alpha {
            // A snapshot is a source image, not an opaque output frame.
            // Clearing alpha to zero preserves ARGB content through
            // premultiplied blending.
            target.pipeline.begin_composition_with_clear_alpha(0.0);
        } else {
            target.pipeline.begin_composition();
        }
        trace_native_lifecycle("composition_started");
    for (layer_index, layer) in frame.layers.iter().enumerate() {
        if draw_result.is_err() {
            break;
        }
        draw_result = match layer {
            NativeCompositionLayer::Cpu(layer) => {
                trace_native_lifecycle("composition_cpu_layer_started");
                let result = target
                    .pipeline
                    .draw_cpu_layer(
                        GlCpuLayer {
                            width: layer.width,
                            height: layer.height,
                            stride: layer.stride,
                            pixels: layer.pixels,
                            alpha: layer.alpha,
                            alpha_mode: if layer.format == 0x3432_5241 {
                                crate::NativeCompositionAlphaMode::Premultiplied
                            } else {
                                crate::NativeCompositionAlphaMode::Opaque
                            },
                        },
                        layer.target.into(),
                        layer.clip.map(Into::into),
                        layer.sampling,
                        frame.trace,
                    )
                    .map_err(|_| NativeGbmScanoutBufferExportDetail::CpuLayerUploadFailed);
                if result.is_ok() {
                    trace_native_lifecycle("composition_cpu_layer_finished");
                    if trace_layer_pixels {
                        trace_composition_pixels(
                            &target.pipeline,
                            "cpu",
                            layer_index,
                            layer.target,
                            layer.format,
                            u64::from(gbm::Modifier::Invalid),
                            layer.stride,
                        );
                    }
                }
                result
            }
            NativeCompositionLayer::DmaBuf(layer) => {
                trace_native_lifecycle("composition_dmabuf_layer_started");
                let result = import_cache
                    .texture(egl, display, &target.pipeline, *layer)
                    .and_then(|texture| {
                        target
                            .pipeline
                            .draw_texture_layer(
                                texture,
                                (layer.frame.width, layer.frame.height),
                                layer.target.into(),
                                layer.clip.map(Into::into),
                                layer.alpha,
                                if layer.frame.format == 0x3432_5241 {
                                    crate::NativeCompositionAlphaMode::Premultiplied
                                } else {
                                    crate::NativeCompositionAlphaMode::Opaque
                                },
                                layer.sampling,
                                frame.trace,
                            )
                                .map_err(|_| {
                                    NativeGbmScanoutBufferExportDetail::CompositionDrawFailed
                                })
                    });
                if result.is_ok() {
                    trace_native_lifecycle("composition_dmabuf_layer_finished");
                    if trace_layer_pixels {
                        trace_composition_pixels(
                            &target.pipeline,
                            "dmabuf",
                            layer_index,
                            layer.target,
                            layer.frame.format,
                            layer.frame.modifier,
                            layer.frame.planes[0].map_or(0, |plane| plane.stride),
                        );
                    }
                }
                result
            }
            NativeCompositionLayer::RendererImage(layer) => {
                trace_native_lifecycle("composition_renderer_image_layer_started");
                let result = (|| {
                    let image = renderer_images
                        .get(&layer.image_id)
                        .ok_or(NativeGbmScanoutBufferExportDetail::InvalidRendererImageId)?;
                    let plane_count = image.buffer.plane_count();
                    let plane_offsets = image.buffer.plane_offsets();
                    let plane_strides = image.buffer.plane_pitches();
                    let plane_fds = image.buffer.export_plane_fds()?.into_plane_fds();
                    let planes = std::array::from_fn(|index| {
                        plane_fds[index].as_ref().map(|fd| NativeDmaBufPlane {
                            fd: fd.as_fd(),
                            offset: plane_offsets[index],
                            stride: plane_strides[index],
                        })
                    });
                    let imported = NativeDmaBufCompositionLayer {
                        image_id: layer.image_id,
                        frame: NativeMultiPlaneDmaBufFrame {
                            width: image.buffer.width(),
                            height: image.buffer.height(),
                            format: image.buffer.format(),
                            modifier: image
                                .buffer
                                .modifier()
                                .unwrap_or(u64::from(gbm::Modifier::Invalid)),
                            plane_count,
                            planes,
                        },
                        target: layer.target,
                        clip: layer.clip,
                        alpha: layer.alpha,
                        sampling: layer.sampling,
                    };
                        let texture =
                            import_cache.texture(egl, display, &target.pipeline, imported)?;
                    target
                        .pipeline
                        .draw_texture_layer(
                            texture,
                            (image.buffer.width(), image.buffer.height()),
                            layer.target.into(),
                            layer.clip.map(Into::into),
                            layer.alpha,
                            if image.buffer.format() == 0x3432_5241 {
                                crate::NativeCompositionAlphaMode::Premultiplied
                            } else {
                                crate::NativeCompositionAlphaMode::Opaque
                            },
                            layer.sampling,
                            frame.trace,
                        )
                        .map_err(|_| NativeGbmScanoutBufferExportDetail::CompositionDrawFailed)
                })();
                if result.is_ok() {
                    trace_native_lifecycle("composition_renderer_image_layer_finished");
                }
                result
            }
            NativeCompositionLayer::Solid(layer) => {
                trace_native_lifecycle("composition_solid_layer_started");
                let result = target
                    .pipeline
                    .draw_solid_layer(layer.target.into(), layer.color)
                    .map_err(|_| NativeGbmScanoutBufferExportDetail::CompositionFinishFailed);
                if result.is_ok() {
                    trace_native_lifecycle("composition_solid_layer_finished");
                    if trace_layer_pixels {
                        trace_composition_pixels(
                            &target.pipeline,
                            "solid",
                            layer_index,
                            layer.target,
                            0,
                            u64::from(gbm::Modifier::Invalid),
                            0,
                        );
                    }
                }
                result
            }
        };
    }
    }
    // Validation and pixel capture read the whole target, so the confinement
    // ends with the last pass rather than with the render.
    target.pipeline.set_ambient_clip(None);
    let mut evidence = NativeCompositionRenderEvidence {
        buffer_age,
        repaint,
        repaint_pixels,
        ..NativeCompositionRenderEvidence::default()
    };
    let result = draw_result
        .and_then(|()| {
            target
                .pipeline
                .validate_composition()
                .map_err(|_| NativeGbmScanoutBufferExportDetail::CompositionFinishFailed)
        })
        .and_then(|()| {
            trace_native_lifecycle("composition_finished");
            if trace_pixels {
                for (layer_index, layer) in frame.layers.iter().enumerate() {
                    let (source_stage, layer_target) = match layer {
                        NativeCompositionLayer::Cpu(layer) => ("cpu", layer.target),
                        NativeCompositionLayer::DmaBuf(layer) => ("dmabuf", layer.target),
                        NativeCompositionLayer::RendererImage(layer) => {
                            ("renderer_image", layer.target)
                        }
                        NativeCompositionLayer::Solid(layer) => ("solid", layer.target),
                    };
                    if let Some(nonzero_rgb_pixels) = trace_final_composition_region(
                        &target.pipeline,
                        source_stage,
                        layer_index,
                        layer_target,
                        (target.width, target.height),
                        frame.trace,
                    ) {
                        evidence.traced_nonzero_rgb_pixels = evidence
                            .traced_nonzero_rgb_pixels
                            .max(nonzero_rgb_pixels);
                    }
                }
            }
            if capture_pixels {
                match target.pipeline.read_composition_pixels() {
                    Ok(metrics) => {
                        tracing::info!(
                            "sophia_native_composition_frame schema=2 status=verified width={} height={} pixels={} nonzero_rgb_pixels={} luminance_sum={} luminance_mean_millis={} checksum={}",
                            target.width,
                            target.height,
                            metrics.pixels,
                            metrics.nonzero_rgb_pixels,
                            metrics.luminance_sum,
                            metrics.luminance_mean_millis(),
                            metrics.checksum,
                        );
                        evidence.captured_pixels = Some(metrics);
                    }
                    Err(_) => tracing::warn!(
                        "sophia_native_composition_frame schema=2 status=unverified width={} height={}",
                        target.width,
                        target.height,
                    ),
                }
            }
            egl.swap_buffers(display, egl_surface)
                .map_err(|_| NativeGbmScanoutBufferExportDetail::EglSwapBuffersFailed)
        })
        .and_then(|()| {
            trace_native_lifecycle("composition_surface_swapped");
            let buffer = unsafe { surface.gbm_surface().lock_front_buffer() }
                .map_err(|_| NativeGbmScanoutBufferExportDetail::FrontBufferLockFailed)?;
            trace_native_lifecycle("composition_front_buffer_locked");
            // Locked front buffers are opaque scanout leases. Do not CPU-map
            // them for diagnostics; future resource reuse must not inherit
            // mapping side effects from this ownership boundary.
            let mut buffer = native_owned_scanout_buffer_from_bo(
                target.width,
                target.height,
                buffer,
                None,
            )?;
            buffer._frame_surface = Some(surface.clone());
            Ok((buffer, evidence))
        });
    let _ = egl.make_current(display, None, None, None);
    result
}

fn trace_composition_pixels(
    pipeline: &PersistentXrgb8888GlPipeline,
    stage: &str,
    layer: usize,
    target: NativeCompositionRect,
    format: u32,
    modifier: u64,
    stride: u32,
) {
    let region_metrics = pipeline.read_composition_region_pixels(target.into());
    match (pipeline.read_composition_pixels(), region_metrics) {
        (Ok(metrics), Ok(region)) => tracing::info!(
            "sophia_native_composition_pixels schema=3 status=read stage={stage} layer={layer} target={}x{}_{}_{} format={format:#x} modifier={modifier:#x} stride={stride} pixels={} nonzero_rgb_pixels={} alpha_zero_pixels={} alpha_partial_pixels={} alpha_opaque_pixels={} luminance_sum={} luminance_mean_millis={} checksum={} region_pixels={} region_nonzero_rgb_pixels={} region_red_pixels={} region_green_pixels={} region_blue_pixels={} region_yellow_pixels={} region_cyan_pixels={} region_magenta_pixels={} region_gray_pixels={} region_other_pixels={} region_luminance_sum={} region_luminance_mean_millis={} region_luminance_histogram={} region_checksum={}",
            target.width,
            target.height,
            target.x,
            target.y,
            metrics.pixels,
            metrics.nonzero_rgb_pixels,
            metrics.alpha_zero_pixels,
            metrics.alpha_partial_pixels,
            metrics.alpha_opaque_pixels,
            metrics.luminance_sum,
            metrics.luminance_mean_millis(),
            metrics.checksum,
            region.pixels,
            region.nonzero_rgb_pixels,
            region.red_pixels,
            region.green_pixels,
            region.blue_pixels,
            region.yellow_pixels,
            region.cyan_pixels,
            region.magenta_pixels,
            region.gray_pixels,
            region.other_pixels,
            region.luminance_sum,
            region.luminance_mean_millis(),
            region.luminance_histogram_field(),
            region.checksum,
        ),
        _ => tracing::warn!(
            "sophia_native_composition_pixels schema=3 status=unavailable stage={stage} layer={layer} target={}x{}_{}_{} format={format:#x} modifier={modifier:#x} stride={stride}",
            target.width,
            target.height,
            target.x,
            target.y,
        ),
    }
}

fn trace_final_composition_region(
    pipeline: &PersistentXrgb8888GlPipeline,
    source_stage: &str,
    layer: usize,
    target: NativeCompositionRect,
    output: (u32, u32),
    trace: Option<NativeCompositionTrace>,
) -> Option<usize> {
    match pipeline.read_composition_region_pixels(target.into()) {
        Ok(region) => {
            // Keep the existing region schema for historical gates. This
            // additive identity record ties the same readback to an opaque
            // head scene, which the backend maps to an exact retired frame.
            if let Some(trace) = trace {
                tracing::info!(
                    "sophia_native_composition_region_frame schema=1 status=read output={} head={} scene_generation={} layer={layer} source_stage={source_stage} target={}x{}_{}_{} region_pixels={} nonzero_rgb_pixels={} checksum={}",
                    trace.output,
                    trace.head,
                    trace.scene_generation,
                    target.width,
                    target.height,
                    target.x,
                    target.y,
                    region.pixels,
                    region.nonzero_rgb_pixels,
                    region.checksum,
                );
            }
            tracing::info!(
                "sophia_native_composition_region schema=3 status=read composition=final source_stage={source_stage} layer={layer} output={}x{} target={}x{}_{}_{} region_pixels={} region_nonzero_rgb_pixels={} region_red_pixels={} region_green_pixels={} region_blue_pixels={} region_yellow_pixels={} region_cyan_pixels={} region_magenta_pixels={} region_gray_pixels={} region_other_pixels={} region_luminance_sum={} region_luminance_mean_millis={} region_luminance_histogram={} region_checksum={}",
                output.0,
                output.1,
                target.width,
                target.height,
                target.x,
                target.y,
                region.pixels,
                region.nonzero_rgb_pixels,
                region.red_pixels,
                region.green_pixels,
                region.blue_pixels,
                region.yellow_pixels,
                region.cyan_pixels,
                region.magenta_pixels,
                region.gray_pixels,
                region.other_pixels,
                region.luminance_sum,
                region.luminance_mean_millis(),
                region.luminance_histogram_field(),
                region.checksum,
            );
            Some(region.nonzero_rgb_pixels)
        }
        Err(_) => {
            tracing::warn!(
                "sophia_native_composition_region schema=3 status=unavailable composition=final source_stage={source_stage} layer={layer} output={}x{} target={}x{}_{}_{}",
                output.0,
                output.1,
                target.width,
                target.height,
                target.x,
                target.y,
            );
            None
        }
    }
}

fn trace_dmabuf_lifecycle(stage: &str) {
    if std::env::var_os("SOPHIA_WAYLAND_DMABUF_DIAGNOSTIC").is_some() {
        tracing::info!("sophia_dmabuf_lifecycle schema=1 stage={stage}");
    }
}

fn trace_native_lifecycle(stage: &str) {
    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
        tracing::info!("sophia_native_lifecycle schema=1 stage={stage}");
    }
}

fn render_initialized_gbm_scanout_front_buffer_with_config<T: std::os::fd::AsFd>(
    device: NativeEglScanoutDevice<'_, T>,
    spec: RenderedFrontBufferSpec<'_>,
    frame: Option<NativeXrgb8888Frame<'_>>,
) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
    let NativeEglScanoutDevice {
        egl,
        display,
        gbm_device,
    } = device;
    let RenderedFrontBufferSpec {
        width,
        height,
        config,
        surface_format,
        surface_modifiers,
        surface_usage,
    } = spec;
    use gbm::AsRaw as _;

    let gbm_surface = create_rendered_scanout_surface(
        gbm_device,
        width,
        height,
        surface_format,
        surface_modifiers,
        surface_usage,
    )?;
    let native_window = gbm_surface.as_raw() as khronos_egl::NativeWindowType;
    let surface = unsafe { egl.create_window_surface(display, config, native_window, None) }
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglSurfaceUnavailable)?;
    let context = match egl.create_context(display, config, None, &context_attributes()) {
        Ok(context) => context,
        Err(_error) => {
            let _ = egl.destroy_surface(display, surface);
            return Err(NativeGbmScanoutBufferExportDetail::EglContextUnavailable);
        }
    };

    let result = egl
        .make_current(display, Some(surface), Some(surface), Some(context))
        .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)
        .and_then(|()| {
            let loader = |name: &str| {
                egl.get_proc_address(name)
                    .map_or(ptr::null(), |proc| proc as *const c_void)
            };
            match frame {
                Some(frame) => draw_xrgb8888_current_gl_context_with_loader(
                    loader,
                    width,
                    height,
                    frame.stride,
                    frame.pixels,
                ),
                None => smoke_current_gl_context_with_loader(loader),
            }
            .map_err(|_error| NativeGbmScanoutBufferExportDetail::GlSmokeFailed)
        })
        .and_then(|()| {
            egl.swap_buffers(display, surface)
                .map_err(|_error| NativeGbmScanoutBufferExportDetail::EglSwapBuffersFailed)
        })
        .and_then(|()| {
            // `gbm` releases this lock when the returned BufferObject is
            // dropped. The owner retains the surface so the release callback
            // remains valid until KMS scanout has retired the buffer.
            let buffer = unsafe { gbm_surface.lock_front_buffer() }
                .map_err(|_error| NativeGbmScanoutBufferExportDetail::FrontBufferLockFailed)?;
            native_owned_scanout_buffer_from_bo(width, height, buffer, Some(gbm_surface))
        });
    let _ = egl.make_current(display, None, None, None);
    let _ = egl.destroy_context(display, context);
    let _ = egl.destroy_surface(display, surface);

    result
}
