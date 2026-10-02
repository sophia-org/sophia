impl LiveProductionNativeScanout {
    pub fn queue_present_cpu_frame(
        &mut self,
        output: OutputId,
        frame: LiveProductionComposedFrame,
    ) -> Result<LiveProductionNativeFrameId, &'static str> {
        if self.has_preview_frame_failure(output) {
            return Err("preview recovery owns this output");
        }
        let index = self
            .primary_head_index(output)
            .ok_or("native output has no head")?;
        if self.head_indices(output).len() > 1 {
            return self
                .queue_projected_frame(output, &frame)
                .ok_or("native mirror projection produced no head frame");
        }
        if self.pending_frame(output) {
            return Err("native output already has pending frame work");
        }
        let frame_id = self.allocate_frame_id();
        let identity = self.native_frame_identity(index, output, frame_id);
        let (head, exporter) = self.head_and_exporter(index, output);
        head.pending_nonzero_pixel_bytes = frame.nonzero_pixel_bytes;
        head.last_checksum = frame.checksum;
        head.queue_output_damage_snapshot(frame.output_damage_snapshot.clone());
        head.pending_content = Some(LiveProductionScanoutContent::Cpu {
            frame: frame_id,
            checksum: frame.checksum,
        });
        exporter.set_pending_identified_cpu_frame(
            frame.frame,
            frame.checksum,
            frame.output_damage_snapshot,
            Some(identity),
        );
        Ok(frame_id)
    }

    pub fn queue_projected_frame(
        &mut self,
        output: OutputId,
        frame: &LiveProductionComposedFrame,
    ) -> Option<LiveProductionNativeFrameId> {
        if self.has_preview_frame_failure(output) {
            return None;
        }
        let heads = self.head_indices(output);
        let targets = heads
            .iter()
            .map(|head_index| {
                crate::project_mirror_rect(
                    frame.frame.size,
                    self.heads[*head_index].output.size,
                    self.heads[*head_index].mapping,
                )
            })
            .collect::<Vec<_>>();
        if heads.is_empty()
            || targets
                .iter()
                .any(|target| target.width <= 0 || target.height <= 0)
        {
            return None;
        }
        let projected_damage = heads
            .iter()
            .zip(&targets)
            .map(|(head_index, _)| {
                frame
                    .output_damage_snapshot
                    .as_ref()
                    .map(|snapshot| {
                        project_mirror_output_damage_snapshot(
                            snapshot,
                            frame.frame.size,
                            self.heads[*head_index].output,
                            self.heads[*head_index].mapping,
                        )
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let frame_id = self.allocate_frame_id();
        let source = sophia_renderer_live::LiveSharedCpuBufferSource {
            handle: 0,
            size: frame.frame.size,
            stride: frame.frame.stride,
            format: frame.frame.format,
            generation: frame_id.raw(),
            bytes: std::sync::Arc::clone(&frame.frame.bytes).into(),
        };
        let heads = heads
            .into_iter()
            .zip(targets)
            .zip(projected_damage)
            .map(|((head_index, target), output_damage_snapshot)| {
                let layer = sophia_renderer_live::LiveOwnedMixedCompositionLayer::Cpu {
                    buffer: source.clone(),
                    placement: sophia_renderer_live::LiveCompositionPlacement {
                        target,
                        clip: None,
                        transform: sophia_protocol::Transform::IDENTITY,
                        alpha: 1.0,
                        sampling: sophia_engine::head_sampling_class(
                            source.size,
                            sophia_protocol::Size {
                                width: target.width,
                                height: target.height,
                            },
                        ),
                    },
                };
                LiveProductionQueuedMirrorHeadFrame {
                    head_index,
                    identity: self.native_frame_identity(head_index, output, frame_id),
                    content: LiveProductionScanoutContent::Cpu {
                        frame: frame_id,
                        checksum: frame.checksum,
                    },
                    frame: sophia_renderer_live::LiveOwnedMixedCompositionFrame {
                        image_reads: Default::default(),
                        layers: vec![layer],
                        output_damage_snapshot: output_damage_snapshot.clone(),
                        trace: None,
                        // A mirror head's CPU frame. Mirror outputs never take
                        // the direct path -- eligibility is proven about one
                        // head's plan, and a mirror cohort has several -- and
                        // a CPU buffer has no framebuffer to hand a plane
                        // anyway. Both reasons say compose.
                        direct_scanout: sophia_engine::DirectScanoutVerdict::default(),
                    },
                    output_damage_snapshot,
                    cpu_nonzero_pixel_bytes: frame.nonzero_pixel_bytes,
                }
            })
            .collect();
        self.queue_mirror_generation(LiveProductionQueuedMirrorGeneration {
            output,
            frame: frame_id,
            logical_content_checksum: Some(frame.checksum),
            heads,
        })
        .ok()?;
        Some(frame_id)
    }
}
