impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn export_mixed_owned_scanout_buffer_with_modifiers(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        layers: &[LiveMixedCompositionLayer<'_>],
        preferred_modifiers: &[u64],
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        self.export_mixed_owned_scanout_buffer_with_modifiers_and_trace(
            target,
            layers,
            LiveCompositionOutputRequest {
                preferred_modifiers,
                format: None,
            },
            None,
            None,
            None,
        )
    }

    fn export_mixed_owned_scanout_buffer_with_modifiers_and_trace(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        layers: &[LiveMixedCompositionLayer<'_>],
        request: LiveCompositionOutputRequest<'_>,
        trace: Option<LiveCompositionTrace>,
        frame_slot: Option<(sophia_renderer_native_egl::NativeFrameTargetSetId, usize)>,
        repaint: Option<&sophia_renderer_native_egl::NativeCompositionRepaintTable>,
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        if !request.is_valid() || !target.is_valid_scanout_target() {
            return Err(LiveMixedCompositionError::InvalidOutput);
        }
        let native_layers = layers
            .iter()
            .map(|layer| match layer {
                LiveMixedCompositionLayer::Cpu { buffer, placement } => {
                    validate_placement(*placement)?;
                    if buffer.size.width <= 0
                        || buffer.size.height <= 0
                        || !matches!(buffer.format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888)
                    {
                        return Err(LiveMixedCompositionError::InvalidLayer);
                    }
                    Ok(sophia_renderer_native_egl::NativeCompositionLayer::Cpu(
                        sophia_renderer_native_egl::NativeCpuCompositionLayer {
                            width: buffer.size.width as u32,
                            height: buffer.size.height as u32,
                            stride: buffer.stride,
                            format: buffer.format,
                            pixels: buffer.bytes,
                            target: native_rect(placement.target),
                            clip: placement.clip.map(native_rect),
                            alpha: placement.alpha,
                            sampling: native_sampling(placement.sampling),
                        },
                    ))
                }
                LiveMixedCompositionLayer::DmaBuf {
                    image_id,
                    frame,
                    placement,
                } => {
                    validate_placement(*placement)?;
                    if !image_id.is_valid()
                        || frame.width == 0
                        || frame.height == 0
                        || frame.plane_count == 0
                        || usize::from(frame.plane_count) > frame.planes.len()
                    {
                        return Err(LiveMixedCompositionError::InvalidLayer);
                    }
                    let planes = std::array::from_fn(|index| {
                        frame.planes[index].as_ref().map(|plane| {
                            sophia_renderer_native_egl::NativeDmaBufPlane {
                                fd: plane.fd.as_fd(),
                                offset: plane.offset,
                                stride: plane.stride,
                            }
                        })
                    });
                    Ok(sophia_renderer_native_egl::NativeCompositionLayer::DmaBuf(
                        sophia_renderer_native_egl::NativeDmaBufCompositionLayer {
                            image_id: sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                                image_id.raw(),
                            ),
                            frame: sophia_renderer_native_egl::NativeMultiPlaneDmaBufFrame {
                                width: frame.width,
                                height: frame.height,
                                format: frame.format,
                                modifier: frame.modifier,
                                plane_count: frame.plane_count,
                                planes,
                            },
                            target: native_rect(placement.target),
                            clip: placement.clip.map(native_rect),
                            alpha: placement.alpha,
                            sampling: native_sampling(placement.sampling),
                        },
                    ))
                }
                LiveMixedCompositionLayer::RendererImage {
                    image_id,
                    placement,
                } => {
                    validate_placement(*placement)?;
                    if !image_id.is_valid() {
                        return Err(LiveMixedCompositionError::InvalidLayer);
                    }
                    Ok(
                        sophia_renderer_native_egl::NativeCompositionLayer::RendererImage(
                            sophia_renderer_native_egl::NativeRendererImageCompositionLayer {
                                image_id:
                                    sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                                        image_id.raw(),
                                    ),
                                target: native_rect(placement.target),
                                clip: placement.clip.map(native_rect),
                                alpha: placement.alpha,
                                sampling: native_sampling(placement.sampling),
                            },
                        ),
                    )
                }
                LiveMixedCompositionLayer::Solid { geometry, color } => {
                    if geometry.is_empty() {
                        return Err(LiveMixedCompositionError::InvalidLayer);
                    }
                    Ok(sophia_renderer_native_egl::NativeCompositionLayer::Solid(
                        sophia_renderer_native_egl::NativeSolidCompositionLayer {
                            target: native_rect(*geometry),
                            color: [color.red, color.green, color.blue],
                        },
                    ))
                }
            })
            .collect::<Result<Vec<_>, LiveMixedCompositionError>>()?;
        let native_frame = sophia_renderer_native_egl::NativeCompositionFrame {
            width: target.size.width as u32,
            height: target.size.height as u32,
            layers: &native_layers,
            trace: trace.map(|trace| sophia_renderer_native_egl::NativeCompositionTrace {
                output: trace.output.raw(),
                head: trace.head.raw(),
                scene_generation: trace.scene_generation,
            }),
            repaint,
        };
        let report = match frame_slot {
            Some((set, frame_slot)) => self
                .inner
                .export_composed_owned_scanout_buffer_in_frame_slot(
                    set,
                    frame_slot,
                    native_frame,
                    request,
                ),
            None => self
                .inner
                .export_composed_owned_scanout_buffer(native_frame, request),
        };
        Ok(reduced_native_owned_scanout_buffer_export_report(report))
    }
}
