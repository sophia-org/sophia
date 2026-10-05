#[derive(Clone, Debug, Eq, PartialEq)]
struct NativeCaptureAllocationKey {
    width: u32,
    height: u32,
    format: gbm::Format,
    modifiers: Vec<gbm::Modifier>,
    usage: gbm::BufferObjectFlags,
    config_attributes: [khronos_egl::Int; 13],
}

impl NativeCaptureAllocationKey {
    fn new(width: u32, height: u32, candidate: &RenderedScanoutCandidate) -> Self {
        Self {
            width,
            height,
            format: candidate.format,
            modifiers: candidate.modifiers.clone(),
            usage: candidate.usage,
            config_attributes: candidate.config_attributes,
        }
    }
}

struct NativeCaptureExecution {
    // Every allocation retains this execution owner. Its GL objects disappear
    // before the context; root destroys all these owners before EGL terminate.
    egl: khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    context: khronos_egl::Context,
    gl: glow::Context,
    pipeline: std::cell::RefCell<Option<PersistentXrgb8888GlPipeline>>,
    sources: std::cell::RefCell<NativeCaptureSourceCache>,
    abandoned: std::cell::RefCell<Vec<NativeCaptureAbandonedAllocation>>,
    accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
}

// This record deliberately has no Rc back to execution/accounting: accounting
// may retain it until display teardown without creating an ownership cycle.
struct NativeCaptureAbandonedAllocation {
    _buffer: std::rc::Rc<NativeGbmOwnedScanoutBuffer>,
    _charge: std::rc::Rc<NativeCaptureAllocationCharge>,
    framebuffer: Option<glow::Framebuffer>,
    texture: Option<glow::Texture>,
    image: Option<khronos_egl::Image>,
}

// An allocation can lose its final cache owner while an output context is
// current. Cleanup must not change which context subsequent output GL calls use.
struct NativeCaptureCurrent {
    display: Option<khronos_egl::Display>,
    context: Option<khronos_egl::Context>,
    draw: Option<khronos_egl::Surface>,
    read: Option<khronos_egl::Surface>,
}

impl NativeCaptureCurrent {
    fn save(egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>) -> Self {
        Self {
            display: egl.get_current_display(),
            context: egl.get_current_context(),
            draw: egl.get_current_surface(khronos_egl::DRAW),
            read: egl.get_current_surface(khronos_egl::READ),
        }
    }

    fn restore(
        self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        fallback_display: khronos_egl::Display,
        destroyed_context: Option<khronos_egl::Context>,
    ) {
        if destroyed_context.is_some() && self.context == destroyed_context {
            let _ = egl.make_current(fallback_display, None, None, None);
        } else if let Some(display) = self.display {
            let _ = egl.make_current(display, self.draw, self.read, self.context);
        } else {
            let _ = egl.make_current(fallback_display, None, None, None);
        }
    }
}

impl NativeCaptureExecution {
    fn new(
        display: khronos_egl::Display,
        config: khronos_egl::Config,
        budget: std::rc::Rc<NativeCaptureSourceBudget>,
        accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
    ) -> Result<Self, NativeGbmScanoutBufferExportDetail> {
        use NativeGbmScanoutBufferExportDetail as E;
        let egl = unsafe { khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required() }
            .map_err(|_| E::EglUnavailable)?;
        egl.bind_api(khronos_egl::OPENGL_API)
            .map_err(|_| E::EglBindApiFailed)?;
        let context = egl
            .create_context(display, config, None, &context_attributes())
            .map_err(|_| E::EglContextUnavailable)?;
        if egl
            .make_current(display, None, None, Some(context))
            .is_err()
        {
            let _ = egl.destroy_context(display, context);
            return Err(E::EglMakeCurrentFailed);
        }
        let loader = |name: &str| {
            egl.get_proc_address(name)
                .map_or(ptr::null(), |function| function as *const c_void)
        };
        let gl = unsafe { glow::Context::from_loader_function(loader) };
        let pipeline_gl = unsafe { glow::Context::from_loader_function(loader) };
        let pipeline = unsafe { PersistentXrgb8888GlPipeline::new_image_target(pipeline_gl, 1, 1) };
        let _ = egl.make_current(display, None, None, None);
        match pipeline {
            Ok(pipeline) => Ok(Self {
                egl,
                display,
                context,
                gl,
                pipeline: std::cell::RefCell::new(Some(pipeline)),
                sources: std::cell::RefCell::new(NativeCaptureSourceCache::with_budget(budget)),
                abandoned: Default::default(),
                accounting,
            }),
            Err(_) => {
                let _ = egl.destroy_context(display, context);
                Err(E::GlSmokeFailed)
            }
        }
    }

    fn make_current(&self) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        self.egl
            .make_current(self.display, None, None, Some(self.context))
            .map_err(|_| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)
    }

    fn reusable(&self) -> bool {
        self.abandoned.borrow().is_empty()
    }
}

impl Drop for NativeCaptureExecution {
    fn drop(&mut self) {
        use glow::HasContext;
        let saved = NativeCaptureCurrent::save(&self.egl);
        let current = self.make_current().is_ok();
        if let Some(pipeline) = self.pipeline.get_mut().as_ref() {
            if current {
                if self
                    .sources
                    .get_mut()
                    .clear(&self.egl, self.display, pipeline)
                    .is_err()
                {
                    self.sources.get_mut().abandon(&self.egl, self.display);
                }
            } else {
                self.sources.get_mut().abandon(&self.egl, self.display);
            }
        }
        // PersistentXrgb8888GlPipeline has no GL-calling Drop; its objects are
        // reclaimed with the context, including after failed make_current.
        self.pipeline.get_mut().take();
        for abandoned in self.abandoned.get_mut().iter_mut() {
            if current {
                unsafe {
                    if let Some(framebuffer) = abandoned.framebuffer.take() {
                        self.gl.delete_framebuffer(framebuffer);
                    }
                    if let Some(texture) = abandoned.texture.take() {
                        self.gl.delete_texture(texture);
                    }
                }
            }
            if let Some(image) = abandoned.image
                && self.egl.destroy_image(self.display, image).is_ok()
            {
                abandoned.image = None;
            }
        }
        let _ = self.egl.make_current(self.display, None, None, None);
        let context_destroyed = self.egl.destroy_context(self.display, self.context).is_ok();
        for abandoned in self.abandoned.get_mut().drain(..) {
            if context_destroyed && abandoned.image.is_none() {
                drop(abandoned);
            } else {
                self.accounting.graveyard.borrow_mut().push(abandoned);
            }
        }
        saved.restore(&self.egl, self.display, Some(self.context));
    }
}

struct NativeCaptureAllocation {
    id: u64,
    key: NativeCaptureAllocationKey,
    buffer: std::rc::Rc<NativeGbmOwnedScanoutBuffer>,
    charge: std::rc::Rc<NativeCaptureAllocationCharge>,
    bytes: u64,
    next_generation: std::cell::Cell<u64>,
    framebuffer: Option<glow::Framebuffer>,
    texture: Option<glow::Texture>,
    image: Option<khronos_egl::Image>,
    execution: std::rc::Rc<NativeCaptureExecution>,
    accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
}

impl NativeCaptureAllocation {
    fn new<T: AsFd>(
        gbm_device: &gbm::Device<T>,
        key: NativeCaptureAllocationKey,
        id: u64,
        execution: std::rc::Rc<NativeCaptureExecution>,
        accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
        max_allocation_bytes: u64,
    ) -> Result<Self, NativeGbmScanoutBufferExportDetail> {
        use NativeGbmScanoutBufferExportDetail as E;
        use glow::HasContext;
        let bo = if key.modifiers.is_empty() {
            gbm_device.create_buffer_object::<()>(key.width, key.height, key.format, key.usage)
        } else {
            gbm_device.create_buffer_object_with_modifiers2::<()>(
                key.width,
                key.height,
                key.format,
                key.modifiers.iter().copied(),
                key.usage,
            )
        }
        .map_err(|_| E::GbmSurfaceUnavailable)?;
        let buffer = native_owned_scanout_buffer_from_bo(key.width, key.height, bo, None)?;
        if !is_supported_rendered_scanout_candidate_buffer(&buffer)
            || buffer.format() != key.format as u32
        {
            return Err(E::InvalidBufferDescriptor);
        }
        let fds = buffer.export_plane_fds()?.into_plane_fds();
        let mut seen = std::collections::BTreeSet::new();
        let mut bytes = 0_u64;
        for fd in fds.iter().flatten() {
            let stat = rustix::fs::fstat(fd).map_err(|_| E::InvalidBufferDescriptor)?;
            if seen.insert((stat.st_dev, stat.st_ino)) {
                let size = rustix::fs::seek(fd, rustix::fs::SeekFrom::End(0))
                    .map_err(|_| E::InvalidBufferDescriptor)?;
                bytes = bytes.checked_add(size).ok_or(E::InvalidBufferDescriptor)?;
            }
        }
        if bytes < u64::from(buffer.pitch()) * u64::from(buffer.height()) {
            return Err(E::InvalidBufferDescriptor);
        }
        let total = accounting
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or(E::RendererImageStoreFull)?;
        if total > max_allocation_bytes {
            return Err(E::RendererImageStoreFull);
        }
        let charge = NativeCaptureAllocationCharge::new(&accounting, bytes);
        let mut allocation = Self {
            id,
            key,
            buffer: std::rc::Rc::new(buffer),
            charge,
            bytes,
            next_generation: std::cell::Cell::new(1),
            framebuffer: None,
            texture: None,
            image: None,
            execution,
            accounting,
        };
        allocation.execution.make_current()?;
        let result = (|| {
            let frame = NativeMultiPlaneDmaBufFrame {
                width: allocation.key.width,
                height: allocation.key.height,
                format: allocation.buffer.format(),
                modifier: allocation
                    .buffer
                    .modifier()
                    .unwrap_or(u64::from(gbm::Modifier::Invalid)),
                plane_count: allocation.buffer.plane_count(),
                planes: std::array::from_fn(|index| {
                    fds[index].as_ref().map(|fd| NativeDmaBufPlane {
                        fd: fd.as_fd(),
                        offset: allocation.buffer.plane_offsets()[index],
                        stride: allocation.buffer.plane_pitches()[index],
                    })
                }),
            };
            let execution = &allocation.execution;
            let image = create_dma_buf_image(&execution.egl, execution.display, frame)?;
            allocation.image = Some(image);
            let pipeline = execution.pipeline.borrow();
            let texture = unsafe {
                pipeline
                    .as_ref()
                    .expect("live capture pipeline")
                    .create_egl_image_texture(&execution.egl, image.as_ptr())
            }?;
            allocation.texture = Some(texture);
            unsafe {
                let framebuffer = execution
                    .gl
                    .create_framebuffer()
                    .map_err(|_| E::CompositionDrawFailed)?;
                allocation.framebuffer = Some(framebuffer);
                execution
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                execution.gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(texture),
                    0,
                );
                if execution.gl.check_framebuffer_status(glow::FRAMEBUFFER)
                    != glow::FRAMEBUFFER_COMPLETE
                    || execution.gl.get_error() != glow::NO_ERROR
                {
                    return Err(E::CompositionDrawFailed);
                }
                execution.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
            Ok(())
        })();
        let _ =
            allocation
                .execution
                .egl
                .make_current(allocation.execution.display, None, None, None);
        result?;
        Ok(allocation)
    }

    fn cleanup_failed(&self) -> bool {
        self.accounting.cleanup_failed.get()
    }

    fn quarantine_output_image(&self, image: khronos_egl::Image) {
        self.accounting.cleanup_failed.set(true);
        self.accounting
            .graveyard
            .borrow_mut()
            .push(NativeCaptureAbandonedAllocation {
                _buffer: self.buffer.clone(),
                _charge: self.charge.clone(),
                framebuffer: None,
                texture: None,
                image: Some(image),
            });
    }

    fn draw(
        self: &std::rc::Rc<Self>,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
        source_budget: u64,
    ) -> Result<Option<NativePooledCapture>, NativeCaptureDrawFailure> {
        use NativeCaptureDrawFailure::{BeforeDestinationWork, DestinationMayHaveWork};
        use NativeGbmScanoutBufferExportDetail as E;
        use glow::HasContext;
        let next = self.next_generation.get();
        self.next_generation.set(
            next.checked_add(1)
                .ok_or(BeforeDestinationWork(E::InvalidTarget))?,
        );
        let generation = NativeSnapshotGeneration::new(self.id, next);
        let execution = &self.execution;
        if !execution.reusable() {
            return Err(BeforeDestinationWork(E::EglImageDestroyFailed));
        }
        execution.make_current().map_err(BeforeDestinationWork)?;
        let mut destination_may_have_work = false;
        let result = (|| {
            let mut pipeline = execution.pipeline.borrow_mut();
            let pipeline = pipeline.as_mut().expect("live capture pipeline");
            pipeline.set_extent(frame.width, frame.height);
            let source = execution.sources.borrow_mut().texture(
                &execution.egl,
                execution.display,
                pipeline,
                frame,
                source_budget,
            )?;
            unsafe {
                execution
                    .gl
                    .bind_framebuffer(glow::FRAMEBUFFER, self.framebuffer);
            }
            // The clear is the first command which can write this allocation.
            // Source import/binding failures above have submitted no such work.
            destination_may_have_work = true;
            pipeline.begin_composition_with_clear_alpha(0.0);
            let rendered = pipeline
                .draw_texture_layer(
                    source.texture(),
                    (frame.width, frame.height),
                    GlCompositionRect {
                        x: 0,
                        y: 0,
                        width: frame.width as i32,
                        height: frame.height as i32,
                    },
                    None,
                    1.0,
                    if frame.format == 0x3432_5241 {
                        crate::NativeCompositionAlphaMode::Premultiplied
                    } else {
                        crate::NativeCompositionAlphaMode::Opaque
                    },
                    crate::NativeCompositionSampling::ExactNearest,
                    None,
                )
                .and_then(|()| pipeline.validate_composition())
                .map_err(|_| E::CompositionDrawFailed);
            pipeline.flush_commands();
            let finished = {
                let _cleanup = NativeCaptureTimingSpan::start(
                    &self.accounting,
                    NativeCaptureTimingKind::Cleanup,
                );
                execution.sources.borrow_mut().finish_use(
                    &execution.egl,
                    execution.display,
                    pipeline,
                    source,
                )
            };
            rendered.and(finished)?;
            let sync = unsafe {
                execution.egl.create_sync(
                    execution.display,
                    khronos_egl::SYNC_FENCE as u32,
                    &[khronos_egl::ATTRIB_NONE],
                )
            }
            .map_err(|_| E::CompositionFinishFailed)?;
            let flushed = unsafe {
                execution.egl.client_wait_sync(
                    execution.display,
                    sync,
                    khronos_egl::SYNC_FLUSH_COMMANDS_BIT,
                    0,
                )
            };
            if flushed.is_err() {
                let _ = unsafe { execution.egl.destroy_sync(execution.display, sync) };
                return Err(E::CompositionFinishFailed);
            }
            Ok(sync)
        })();
        {
            let _cleanup =
                NativeCaptureTimingSpan::start(&self.accounting, NativeCaptureTimingKind::Cleanup);
            // Restore the default framebuffer before releasing this context.
            unsafe {
                execution.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
            let _ = execution
                .egl
                .make_current(execution.display, None, None, None);
        }
        match result {
            Ok(sync) => {
                let copy_use = generation.acquire_use();
                Ok(Some(NativePooledCapture {
                    allocation: self.clone(),
                    generation,
                    copy_completion: Some(sync),
                    copy_use: Some(copy_use),
                }))
            }
            Err(error) => {
                generation.abandon_reuse();
                Err(if destination_may_have_work {
                    // Retain the charge until final teardown when completion
                    // cannot be proved after any destination work.
                    DestinationMayHaveWork(error)
                } else {
                    BeforeDestinationWork(error)
                })
            }
        }
    }
}

impl Drop for NativeCaptureAllocation {
    fn drop(&mut self) {
        use glow::HasContext;
        let _reclaim =
            NativeCaptureTimingSpan::start(&self.accounting, NativeCaptureTimingKind::Reclaim);
        let saved = NativeCaptureCurrent::save(&self.execution.egl);
        if self.execution.make_current().is_ok() {
            unsafe {
                if let Some(framebuffer) = self.framebuffer.take() {
                    self.execution.gl.delete_framebuffer(framebuffer);
                }
                if let Some(texture) = self.texture.take() {
                    self.execution.gl.delete_texture(texture);
                }
            }
        }
        if let Some(image) = self.image
            && self
                .execution
                .egl
                .destroy_image(self.execution.display, image)
                .is_ok()
        {
            self.image = None;
        }
        saved.restore(&self.execution.egl, self.execution.display, None);
        if self.framebuffer.is_some() || self.texture.is_some() || self.image.is_some() {
            self.accounting.cleanup_failed.set(true);
            self.execution
                .abandoned
                .borrow_mut()
                .push(NativeCaptureAbandonedAllocation {
                    _buffer: self.buffer.clone(),
                    _charge: self.charge.clone(),
                    framebuffer: self.framebuffer.take(),
                    texture: self.texture.take(),
                    image: self.image.take(),
                });
        }
    }
}
