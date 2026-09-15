use super::*;

impl LiveMetadataShell {
    /// Final session stop. Close admission before waiting on the process, and
    /// attempt process termination even if disconnect bookkeeping fails.
    pub(in crate::live_session) fn stop_for_session_shutdown(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.presentation_paused = true;
        self.reconnect_at = None;
        self.connected = false;
        let disconnect = self.retire_connection_state("session_shutdown");
        let terminate = self.supervisor.terminate();
        disconnect?;
        terminate?;
        Ok(())
    }

    /// Called only after the owner loop returned successfully and its runtime
    /// and CPU scene have actually dropped. Consume the remaining native owner
    /// before settling disconnected submissions. No destructor of the protocol
    /// accounting owner can substitute for the final collection below.
    pub(in crate::live_session) fn finish_content_shutdown(
        &mut self,
        native: &mut Option<LiveProductionNativeScanout>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.content.owns_work_area() {
            return Ok(());
        }
        if !self.presentation_paused || self.connected {
            return Err("shell content shutdown still has live admission".into());
        }
        if native.as_ref().is_some_and(|native| {
            !native.output_topology_preparation_quiescent() || native.any_head_cleanup_pending()
        }) {
            return Err("shell content shutdown still has native work or cleanup".into());
        }
        native
            .as_mut()
            .ok_or("shell content shutdown has no native owner")?
            .shutdown_renderer_workers(Duration::from_secs(2))?;
        let report = match self
            .transport
            .finish_content_after_backend_drop(native.take())
        {
            Ok(report) => report,
            Err(owner) => {
                *native = owner;
                return Err("shell content shutdown refused a live transport epoch".into());
            }
        };
        let accounting = report.accounting;
        content_accounting::emit(
            "sophia_shell_content_shutdown",
            if accounting.quiescent() {
                "quiescent"
            } else {
                "retained"
            },
            1,
            report.settled_candidates,
            accounting,
        );
        if !accounting.quiescent() {
            return Err(
                "shell content shutdown retained real resource or response ownership".into(),
            );
        }
        Ok(())
    }
}
