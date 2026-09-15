use super::*;

impl LiveProductionNativeScanout {
    /// Final normal shutdown only. Retain every owner on refusal/timeout and
    /// leave destructor fallback nonblocking for a stalled driver.
    pub fn shutdown_renderer_workers(
        &mut self,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.output_topology_preparation_quiescent() || self.any_head_cleanup_pending() {
            return Err("renderer shutdown requires native custody to be quiescent".into());
        }
        for exporter in &self.exporters {
            exporter.request_worker_shutdown();
        }
        for core in self
            .groups
            .iter()
            .filter_map(|group| group.renderer_core.as_ref())
        {
            core.request_shutdown();
        }
        let deadline = Instant::now() + timeout;
        loop {
            let mut joined = true;
            for exporter in &self.exporters {
                joined &= exporter.poll_worker_shutdown()?;
            }
            for core in self
                .groups
                .iter()
                .filter_map(|group| group.renderer_core.as_ref())
            {
                joined &= core.poll_shutdown()?;
            }
            if joined {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("native renderer workers did not finish within shutdown budget".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
