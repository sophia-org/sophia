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

    /// Called after the owner loop returned and its runtime, CPU scene and
    /// renderer handoff have actually dropped. Require exact native retirement
    /// before settling disconnected submissions. No destructor of the protocol
    /// accounting owner can substitute for the final collection below.
    pub(in crate::live_session) fn finish_content_shutdown(
        &mut self,
        retirement: &crate::live_session::native_owner_retirement::NativeRetirement,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.content.owns_work_area() {
            return Ok(());
        }
        if !self.presentation_paused || self.connected {
            return Err("shell content shutdown still has live admission".into());
        }
        let completed = retirement.completion()?;
        let report = match self.transport.finish_content_after_backend_drop(completed) {
            Ok(report) => report,
            Err(_) => {
                return Err("shell content shutdown refused a live transport epoch".into());
            }
        };
        let accounting = report.accounting;
        crate::session_println!(
            "sophia_shell_native_retirement schema=1 owner={} disposition={:?}",
            completed.identity,
            completed.mode,
        );
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
