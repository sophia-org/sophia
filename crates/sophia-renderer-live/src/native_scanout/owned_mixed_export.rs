impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn export_owned_mixed_frame_with_modifiers(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: &LiveOwnedMixedCompositionFrame,
        preferred_modifiers: &[u64],
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        self.export_owned_mixed_frame(
            target,
            frame,
            LiveCompositionOutputRequest {
                preferred_modifiers,
                format: None,
            },
        )
    }

    pub fn export_owned_mixed_frame(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: &LiveOwnedMixedCompositionFrame,
        request: LiveCompositionOutputRequest<'_>,
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        self.export_owned_mixed_frame_with_modifiers_and_frame_slot(
            target, frame, request, None, None,
        )
    }

    /// Export into a frame slot, optionally limiting the repaint to the damage
    /// the caller's history says the slot's buffer owes at each possible age.
    pub fn export_owned_mixed_frame_with_modifiers_in_frame_slot(
        &mut self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
        frame_slot: usize,
        target: LiveGbmEglFrameTargetRecord,
        frame: &LiveOwnedMixedCompositionFrame,
        preferred_modifiers: &[u64],
        repaint: Option<&sophia_renderer_native_egl::NativeCompositionRepaintTable>,
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        self.export_owned_mixed_frame_in_frame_slot(
            set,
            frame_slot,
            target,
            frame,
            LiveCompositionOutputRequest {
                preferred_modifiers,
                format: None,
            },
            repaint,
        )
    }

    pub fn export_owned_mixed_frame_in_frame_slot(
        &mut self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
        frame_slot: usize,
        target: LiveGbmEglFrameTargetRecord,
        frame: &LiveOwnedMixedCompositionFrame,
        request: LiveCompositionOutputRequest<'_>,
        repaint: Option<&sophia_renderer_native_egl::NativeCompositionRepaintTable>,
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        self.export_owned_mixed_frame_with_modifiers_and_frame_slot(
            target,
            frame,
            request,
            Some((set, frame_slot)),
            repaint,
        )
    }

    fn export_owned_mixed_frame_with_modifiers_and_frame_slot(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: &LiveOwnedMixedCompositionFrame,
        request: LiveCompositionOutputRequest<'_>,
        frame_slot: Option<(sophia_renderer_native_egl::NativeFrameTargetSetId, usize)>,
        repaint: Option<&sophia_renderer_native_egl::NativeCompositionRepaintTable>,
    ) -> Result<NativeGbmOwnedScanoutBufferExportReport, LiveMixedCompositionError> {
        if !request.is_valid() {
            return Err(LiveMixedCompositionError::InvalidOutput);
        }
        // Capture client DMA-BUFs before assembling the output frame. Retained
        // scene state below then refers only to compositor-owned images.
        for layer in &frame.layers {
            let LiveOwnedMixedCompositionLayer::DmaBuf {
                image_id, frame, ..
            } = layer
            else {
                continue;
            };
            let planes = std::array::from_fn(|index| {
                frame.planes[index].as_ref().map(|plane| {
                    sophia_renderer_native_egl::NativeDmaBufPlane {
                        fd: plane.fd.as_fd(),
                        offset: plane.offset,
                        stride: plane.stride,
                    }
                })
            });
            self.inner
                .capture_renderer_image(
                    sophia_renderer_native_egl::NativeRendererImageId::from_raw(image_id.raw()),
                    sophia_renderer_native_egl::NativeMultiPlaneDmaBufFrame {
                        width: frame.width,
                        height: frame.height,
                        format: frame.format,
                        modifier: frame.modifier,
                        plane_count: frame.plane_count,
                        planes,
                    },
                )
                .map_err(|detail| {
                    LiveMixedCompositionError::Renderer(
                        reduced_native_owned_scanout_buffer_export_detail(detail),
                    )
                })?;
        }
        let layers = frame
            .layers
            .iter()
            .map(|layer| match layer {
                LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } => {
                    LiveMixedCompositionLayer::Cpu {
                        buffer: LiveCpuBufferSourceRef {
                            handle: buffer.handle,
                            size: buffer.size,
                            stride: buffer.stride,
                            format: buffer.format,
                            generation: buffer.generation,
                            bytes: buffer.bytes.as_slice(),
                        },
                        placement: *placement,
                    }
                }
                LiveOwnedMixedCompositionLayer::DmaBuf {
                    image_id,
                    placement,
                    ..
                } => LiveMixedCompositionLayer::RendererImage {
                    image_id: *image_id,
                    placement: *placement,
                },
                LiveOwnedMixedCompositionLayer::RendererImage {
                    image_id,
                    placement,
                    ..
                } => LiveMixedCompositionLayer::RendererImage {
                    image_id: *image_id,
                    placement: *placement,
                },
                LiveOwnedMixedCompositionLayer::Solid { geometry, color } => {
                    LiveMixedCompositionLayer::Solid {
                        geometry: *geometry,
                        color: *color,
                    }
                }
            })
            .collect::<Vec<_>>();
        self.export_mixed_owned_scanout_buffer_with_modifiers_and_trace(
            target,
            &layers,
            request,
            frame.trace,
            frame_slot,
            repaint,
        )
    }
}
