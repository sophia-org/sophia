struct NativeCaptureTarget {
    config: khronos_egl::Config,
    config_attributes: [khronos_egl::Int; 13],
    target: NativeRenderTarget,
}

impl<T: std::os::fd::AsFd> NativeGbmRenderedScanoutContext<T> {
    fn probe_renderer_image_import(
        &self,
        frame: NativeMultiPlaneDmaBufFrame<'_>,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        // Import the actual descriptors before allocating capture storage. A
        // previous layout success is only an ordering hint for fallback devices.
        let image = create_dma_buf_image(&self.egl, self.display, frame)?;
        self.egl
            .destroy_image(self.display, image)
            .map_err(|_| NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed)
    }

    fn render_renderer_image_snapshot(
        &mut self,
        image_id: NativeRendererImageId,
        source: NativeMultiPlaneDmaBufFrame<'_>,
        linear_bridge: bool,
    ) -> Result<NativeGbmOwnedScanoutBuffer, NativeGbmScanoutBufferExportDetail> {
        self.render_renderer_image_snapshot_with_completion(image_id, source, linear_bridge, false)
            .map(|(buffer, _)| buffer)
    }

    fn render_renderer_image_snapshot_with_completion(
        &mut self,
        image_id: NativeRendererImageId,
        source: NativeMultiPlaneDmaBufFrame<'_>,
        linear_bridge: bool,
        completion_required: bool,
    ) -> Result<
        (NativeGbmOwnedScanoutBuffer, Option<khronos_egl::Sync>),
        NativeGbmScanoutBufferExportDetail,
    > {
        let format = match source.format {
            0x3432_5258 => gbm::Format::Xrgb8888,
            0x3432_5241 => gbm::Format::Argb8888,
            _ => return Err(NativeGbmScanoutBufferExportDetail::InvalidTarget),
        };
        self.egl
            .bind_api(khronos_egl::OPENGL_API)
            .map_err(|_| NativeGbmScanoutBufferExportDetail::EglBindApiFailed)?;
        let layer = NativeCompositionLayer::DmaBuf(NativeDmaBufCompositionLayer {
            custody: None,
            image_id,
            frame: source,
            target: NativeCompositionRect {
                x: 0,
                y: 0,
                width: i32::try_from(source.width).unwrap_or(i32::MAX),
                height: i32::try_from(source.height).unwrap_or(i32::MAX),
            },
            clip: None,
            alpha: 1.0,
            sampling: crate::NativeCompositionSampling::ExactNearest,
        });
        let layers = [layer];
        let frame = NativeCompositionFrame {
            width: source.width,
            height: source.height,
            layers: &layers,
            trace: None,
            repaint: None,
        };
        let mut last_detail = NativeGbmScanoutBufferExportDetail::EglConfigUnavailable;
        let candidates = if linear_bridge {
            vec![RenderedScanoutCandidate {
                format,
                modifiers: vec![gbm::Modifier::Linear],
                // Explicit modifiers and the GBM LINEAR usage flag are mutually exclusive.
                usage: gbm::BufferObjectFlags::RENDERING,
                config_attributes: if format == gbm::Format::Argb8888 {
                    window_config_attributes()
                } else {
                    xrgb_window_config_attributes()
                },
            }]
        } else {
            rendered_scanout_candidates(&[], None)
                .into_iter()
                .filter(|candidate| candidate.format == format)
                .collect()
        };
        let mut import_failure = None;
        for candidate in candidates {
            // The retained execution context belongs to this EGLDisplay. Its
            // configuration is reusable only for the selector's exact inputs;
            // modifiers still go through fresh surface allocation below.
            let slot = usize::from(candidate.format == gbm::Format::Argb8888);
            let attributes = candidate.config_attributes;
            let config = self.capture_targets[slot]
                .as_ref()
                .filter(|cached| {
                    cached.target.surface_format == candidate.format
                        && cached.config_attributes == attributes
                })
                .map(|cached| cached.config)
                .or_else(|| {
                    self.stats.capture_config_selections =
                        self.stats.capture_config_selections.saturating_add(1);
                    choose_scanout_config_for_format(
                        &self.egl,
                        self.display,
                        attributes,
                        candidate.format,
                    )
                });
            let Some(config) = config else {
                continue;
            };
            let setup_started = self.render_timing_enabled.then(RenderStageTimer::start);
            let (mut target, surface, _) = match self.create_capture_target(RenderTargetSpec {
                width: source.width,
                height: source.height,
                config,
                candidate,
            }) {
                Ok(created) => created,
                Err(detail) => {
                    last_detail = preferred_scanout_failure_detail(last_detail, detail);
                    continue;
                }
            };
            if let Some(started) = setup_started {
                self.stats.capture_setup_cpu = self
                    .stats
                    .capture_setup_cpu
                    .saturating_add(started.cpu_elapsed());
                self.stats.capture_setup_elapsed = self
                    .stats
                    .capture_setup_elapsed
                    .saturating_add(started.elapsed());
            }
            self.stats.capture_surface_creations =
                self.stats.capture_surface_creations.saturating_add(1);
            self.stats.dmabuf_target_creations =
                self.stats.dmabuf_target_creations.saturating_add(1);
            let mut import_cache = NativeDmaBufImportCache::with_capacity_and_stats(
                1,
                NativeDmaBufImportCacheStats::default(),
            );
            let empty_images = std::collections::BTreeMap::new();
            let copy_started = self.render_timing_enabled.then(RenderStageTimer::start);
            let rendered = render_native_target_composition(
                &self.egl,
                self.display,
                &mut target,
                surface.clone(),
                &mut import_cache,
                &empty_images,
                frame,
                false,
                true,
                self.buffer_age_supported,
            );
            if let Some(started) = copy_started {
                self.stats.capture_copy_cpu = self
                    .stats
                    .capture_copy_cpu
                    .saturating_add(started.cpu_elapsed());
                self.stats.capture_copy_elapsed = self
                    .stats
                    .capture_copy_elapsed
                    .saturating_add(started.elapsed());
            }
            let generation = self.allocate_target_generation();
            let persistent = PersistentCompositionTarget {
                target,
                surface,
                import_cache,
                preferred_modifiers: Vec::new(),
                generation,
            };
            match rendered {
                Ok((buffer, _))
                    if is_supported_rendered_scanout_candidate_buffer(&buffer)
                        && buffer.format() == source.format
                        && buffer.width() == source.width
                        && buffer.height() == source.height
                        && (!linear_bridge
                            || (buffer.modifier() == Some(0) && buffer.plane_count() == 1)) =>
                {
                    let completion = if completion_required {
                        self.capture_completion(&persistent)
                    } else {
                        Ok(None)
                    };
                    let retained = completion.is_ok();
                    if let Err(error) =
                        self.finish_renderer_image_capture(persistent, config, attributes, retained)
                    {
                        if let Ok(Some(sync)) = completion {
                            let _ = unsafe { self.egl.destroy_sync(self.display, sync) };
                        }
                        self.stats.capture_failures = self.stats.capture_failures.saturating_add(1);
                        return Err(error);
                    }
                    return completion.map(|completion| (buffer, completion));
                }
                Ok(_) => {
                    last_detail = NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor;
                }
                Err(detail) => {
                    if image_import_failure(detail) {
                        import_failure = Some(detail);
                    }
                    last_detail = preferred_scanout_failure_detail(last_detail, detail);
                }
            }
            // This candidate already failed. A cleanup error invalidates its
            // execution cache, but must not hide the import error or prevent
            // the remaining format/modifier candidates from being tried.
            if self
                .finish_renderer_image_capture(persistent, config, attributes, false)
                .is_err()
            {
                self.stats.capture_failures = self.stats.capture_failures.saturating_add(1);
            }
        }
        self.stats.capture_failures = self.stats.capture_failures.saturating_add(1);
        Err(import_failure.unwrap_or(last_detail))
    }

    fn capture_completion(
        &self,
        target: &PersistentCompositionTarget,
    ) -> Result<Option<khronos_egl::Sync>, NativeGbmScanoutBufferExportDetail> {
        use khronos_egl as egl;
        self.egl
            .make_current(
                self.display,
                Some(target.surface.egl_surface()),
                Some(target.surface.egl_surface()),
                Some(target.target.egl_context),
            )
            .map_err(|_| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)?;
        // The fence follows the destination copy. A zero-time flush submits it
        // without waiting; the owning worker polls before reusing source storage.
        let result = unsafe {
            self.egl
                .create_sync(self.display, egl::SYNC_FENCE as u32, &[egl::ATTRIB_NONE])
        }
        .map_err(|_| NativeGbmScanoutBufferExportDetail::CompositionFinishFailed)
        .and_then(|sync| {
            match unsafe {
                self.egl
                    .client_wait_sync(self.display, sync, egl::SYNC_FLUSH_COMMANDS_BIT, 0)
            } {
                Ok(_) => Ok(Some(sync)),
                Err(_) => {
                    let _ = unsafe { self.egl.destroy_sync(self.display, sync) };
                    Err(NativeGbmScanoutBufferExportDetail::CompositionFinishFailed)
                }
            }
        });
        let _ = self.egl.make_current(self.display, None, None, None);
        result
    }

    fn create_capture_target(
        &mut self,
        spec: RenderTargetSpec,
    ) -> Result<
        (
            NativeRenderTarget,
            std::rc::Rc<NativeFrameSurface>,
            std::time::Duration,
        ),
        NativeGbmScanoutBufferExportDetail,
    > {
        let slot = usize::from(spec.candidate.format == gbm::Format::Argb8888);
        if let Some(cached) = self.capture_targets[slot].take() {
            let NativeCaptureTarget {
                config,
                config_attributes,
                mut target,
            } = cached;
            if config == spec.config
                && target.surface_format == spec.candidate.format
                && config_attributes == spec.candidate.config_attributes
            {
                let started = Instant::now();
                // Never recycle the previous image's surface or BO. Its exported
                // FDs may still be in use even after local image-store eviction.
                let surface = create_native_frame_surface(
                    &self.egl,
                    self.display,
                    &self.gbm_device,
                    spec.width,
                    spec.height,
                    spec.config,
                    &spec.candidate,
                );
                let surface = match surface {
                    Ok(surface) => surface,
                    Err(error) => {
                        // Surface allocation has not bound or used the context.
                        // Preserve it for the next modifier candidate; failed
                        // allocation says nothing about execution validity.
                        self.capture_targets[slot] = Some(NativeCaptureTarget {
                            config,
                            config_attributes,
                            target,
                        });
                        return Err(error);
                    }
                };
                let elapsed = started.elapsed();
                target.width = spec.width;
                target.height = spec.height;
                target.pipeline.set_extent(spec.width, spec.height);
                self.stats.capture_context_reuses =
                    self.stats.capture_context_reuses.saturating_add(1);
                self.stats.frame_surface_creations =
                    self.stats.frame_surface_creations.saturating_add(1);
                self.stats.max_frame_surface_create =
                    self.stats.max_frame_surface_create.max(elapsed);
                return Ok((target, surface, elapsed));
            }
            self.stats.sampling = self
                .stats
                .sampling
                .saturating_add(target.pipeline.sampling_stats());
            self.destroy_native_render_target(target);
        }
        let created = self.create_render_target(spec)?;
        self.stats.capture_context_creations =
            self.stats.capture_context_creations.saturating_add(1);
        Ok(created)
    }

    fn finish_renderer_image_capture(
        &mut self,
        mut persistent: PersistentCompositionTarget,
        config: khronos_egl::Config,
        config_attributes: [khronos_egl::Int; 13],
        retain: bool,
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        let started = self.render_timing_enabled.then(RenderStageTimer::start);
        let surface = persistent.surface.egl_surface();
        let cleaned = self
            .egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(persistent.target.egl_context),
            )
            .map_err(|_| NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed)
            .and_then(|()| {
                // Swap/lock exports an implicitly synchronized DMA-BUF. Flush
                // explicitly as well: correctness must not rely on destroying
                // a context. Cross-device scratch reuse additionally retains
                // the completion fence made by capture_completion above.
                persistent.target.pipeline.flush_commands();
                persistent
                    .import_cache
                    .clear(&self.egl, self.display, &persistent.target.pipeline)
                    .map(|_| ())
            });
        if cleaned.is_err() {
            persistent.import_cache.abandon(&self.egl, self.display);
        }
        accumulate_import_cache_stats(
            &mut self.stats.import_cache,
            persistent.import_cache.stats(),
        );
        let _ = self.egl.make_current(self.display, None, None, None);
        let slot = usize::from(persistent.target.surface_format == gbm::Format::Argb8888);
        if retain && cleaned.is_ok() {
            self.capture_targets[slot] = Some(NativeCaptureTarget {
                config,
                config_attributes,
                target: persistent.target,
            });
        } else {
            self.stats.sampling = self
                .stats
                .sampling
                .saturating_add(persistent.target.pipeline.sampling_stats());
            self.destroy_native_render_target(persistent.target);
        }
        // The capture surface lives with the returned buffer, never the cache.
        if let Some(started) = started {
            self.stats.capture_cleanup_cpu = self
                .stats
                .capture_cleanup_cpu
                .saturating_add(started.cpu_elapsed());
            self.stats.capture_cleanup_elapsed = self
                .stats
                .capture_cleanup_elapsed
                .saturating_add(started.elapsed());
        }
        cleaned
    }
}
