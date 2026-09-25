impl LiveProductionNativeScanout {
        pub fn queue_frame(
            &mut self,
            output: OutputId,
            frame: LiveProductionComposedFrame,
        ) -> LiveProductionCpuFrameQueueStatus {
            let Some(index) = self.primary_head_index(output) else {
                return LiveProductionCpuFrameQueueStatus::NoHead;
            };
            // A group's heads need the frame placed for each of their modes, which
            // the pure-CPU path below cannot express -- it uploads at the frame's
            // own size. An output with one head keeps that path exactly, so no
            // ordinary desktop changes.
            if self.head_indices(output).len() > 1 {
                let indices = self.head_indices(output);
                let statuses = indices
                    .iter()
                    .map(|head_index| {
                        let head = &self.heads[*head_index];
                        reduce_live_production_cpu_frame_queue(
                            head.pending_content,
                            head.submitted_content,
                            head.presented_content,
                            self.exporters[*head_index].worker_in_flight(),
                            head.callback_accepted != 0
                                || head.initial_modeset_submission.is_some(),
                            frame.checksum,
                        )
                    })
                    .collect::<Vec<_>>();
                for unchanged in [
                    LiveProductionCpuFrameQueueStatus::UnchangedPending,
                    LiveProductionCpuFrameQueueStatus::UnchangedSubmitted,
                    LiveProductionCpuFrameQueueStatus::UnchangedPresented,
                ] {
                    if statuses.iter().all(|status| *status == unchanged) {
                        return unchanged;
                    }
                }
                let projected = self.queue_projected_frame(output, &frame);
                return if projected.is_some() {
                    LiveProductionCpuFrameQueueStatus::Queued
                } else {
                    LiveProductionCpuFrameQueueStatus::NoHead
                };
            }
            let status = {
                let head = &self.heads[index];
                reduce_live_production_cpu_frame_queue(
                    head.pending_content,
                    head.submitted_content,
                    head.presented_content,
                    self.exporter(output).is_some_and(
                        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::worker_in_flight,
                    ),
                    head.callback_accepted != 0 || head.initial_modeset_submission.is_some(),
                    frame.checksum,
                )
            };
            if !matches!(
                status,
                LiveProductionCpuFrameQueueStatus::Queued
                    | LiveProductionCpuFrameQueueStatus::BaselineRequired
            ) {
                return status;
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
            status
        }
}
