use super::*;

impl LiveProductionNativeScanout {
    /// This construction's identity, independent of its recurring output IDs.
    pub fn retirement_owner_identity(&self) -> u64 {
        self.native_frame_owner.raw()
    }

    /// Request only; safe to call before acknowledging seat release. No join,
    /// KMS operation, or wait occurs here.
    pub fn request_renderer_worker_shutdown(&self) {
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
    }

    /// Visit every exact owned worker, including when a sibling is unfinished.
    /// This is worker completion only, not disposition of scanout resources.
    pub fn poll_renderer_worker_shutdown(&self) -> std::io::Result<bool> {
        let mut joined = true;
        let mut failure = None;
        let mut observe = |result: std::io::Result<bool>| match result {
            Ok(done) => joined &= done,
            Err(error) => {
                joined = false;
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        };
        for exporter in &self.exporters {
            observe(exporter.poll_worker_shutdown());
        }
        for core in self
            .groups
            .iter()
            .filter_map(|group| group.renderer_core.as_ref())
        {
            observe(core.poll_shutdown());
        }
        failure.map_or(Ok(joined), Err)
    }

    /// Refuse rather than treat thread completion as KMS resource cleanup.
    /// No device operation is performed, including on a revoked seat. After
    /// this check, dropping the native owner disposes of its unsubmitted queue,
    /// pending results and exporter sources; those cannot publish Presented.
    pub fn validate_retirement_disposition(&self) -> Result<(), &'static str> {
        if self.output_topology_preparation.is_some()
            || self.any_head_cleanup_pending()
            || self.heads.iter().any(|head| {
                head.submitted_sequence.is_some()
                    || head.prepared_scanout.is_some()
                    || head.scanout_custody.submitted().is_some()
                    || head.scanout_custody.displayed().is_some()
            })
        {
            return Err("native retirement retains unresolved scanout or cleanup ownership");
        }
        Ok(())
    }

    /// Final normal shutdown only. Retain every owner on refusal/timeout and
    /// leave destructor fallback nonblocking for a stalled driver.
    pub fn shutdown_renderer_workers(
        &mut self,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.output_topology_preparation_quiescent() || self.any_head_cleanup_pending() {
            return Err("renderer shutdown requires native custody to be quiescent".into());
        }
        self.request_renderer_worker_shutdown();
        let deadline = Instant::now() + timeout;
        loop {
            if self.poll_renderer_worker_shutdown()? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("native renderer workers did not finish within shutdown budget".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
