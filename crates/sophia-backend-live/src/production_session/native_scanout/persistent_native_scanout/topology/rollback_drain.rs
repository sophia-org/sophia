impl LiveProductionNativeScanout {
    /// Stops unsubmitted candidate frames without touching displayed custody.
    /// Submitted flips must still retire against the candidate's trackers before
    /// rollback installation replaces them. Polling a worker only takes back its
    /// export; this path never creates a framebuffer or submits a new frame.
    pub(crate) fn drain_output_topology_rollback_frames(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.output_topology_preparation_phase()
            != Some(LiveProductionNativeTopologyPreparationPhase::RollingBack)
        {
            return Err("rollback first-frame drain requires pending rollback".into());
        }
        self.invalidate_layout_probes();
        self.retry_output_topology_cleanup();
        self.service_layout_probe_cleanup();
        for output in self.outputs() {
            self.cancel_prepared_output(output.id);
        }
        for index in 0..self.heads.len() {
            let exporter = &mut self.exporters[index];
            if exporter.worker_in_flight() {
                let export =
                    crate::LiveRenderedScanoutBufferExporter::export_rendered_scanout_buffer(
                        exporter,
                        crate::LiveGbmEglFrameTargetRecord::new(self.heads[index].selection.size()),
                    );
                // No DRM owner exists for this export. Dropping a completed
                // result returns its renderer lease; Pending retains the worker.
                drop(export);
            }
            if !exporter.worker_in_flight() {
                exporter.discard_pending_frame();
                self.heads[index].rendering_content = None;
                self.heads[index].pending_content = None;
            }
        }
        Ok(())
    }
}
