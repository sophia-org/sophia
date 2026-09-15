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
        let epoch = accounting.epochs;
        let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let monotonic_usec = u64::try_from(time.tv_sec)
            .unwrap_or_default()
            .saturating_mul(1_000_000)
            .saturating_add(u64::try_from(time.tv_nsec).unwrap_or_default() / 1_000);
        crate::session_println!(
            "sophia_shell_content_shutdown schema=1 status={} connection_epoch={} content_grant_epoch={} monotonic_usec={} workers_joined=1 settled_candidates={} active_epochs={} retired_epochs={} candidates={} resources={} resource_ids={} transfers={} allocations={} permits={} demands={} staging_bytes={} resident_bytes={} retiring_bytes={} backing_bytes={} reserved_resident_bytes={} reserved_bytes={} reserved_backing_bytes={} response_records={} response_bytes={} input_records={} input_bytes={}",
            if accounting.quiescent() {
                "quiescent"
            } else {
                "retained"
            },
            epoch.grant.connection_epoch,
            epoch.grant.content_grant_epoch,
            monotonic_usec,
            report.settled_candidates,
            epoch.active_epochs,
            epoch.retired_epochs,
            epoch.candidates,
            epoch.resources,
            epoch.resource_ids,
            epoch.transfers,
            epoch.allocations,
            epoch.permits,
            epoch.demands,
            epoch.memory.staging,
            epoch.memory.resident,
            epoch.memory.retiring,
            epoch.memory.backing,
            epoch.memory.reserved_resident,
            epoch.reserved_bytes,
            epoch.reserved_backing_bytes,
            accounting.response_records,
            accounting.response_bytes,
            accounting.input_records,
            accounting.input_bytes,
        );
        if !accounting.quiescent() {
            return Err(
                "shell content shutdown retained real resource or response ownership".into(),
            );
        }
        Ok(())
    }
}
