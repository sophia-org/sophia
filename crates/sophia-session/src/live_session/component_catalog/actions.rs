//! Connected native actions use the same immutable catalog and actual queue.
use super::*;
use crate::live_session::metadata_shell::NativeLauncherActionService;
use sophia_runtime::ShellTransportConnection;

impl ComponentCatalog {
    pub(in crate::live_session) fn action_now_msec(
        &self,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        Ok(u64::try_from(
            self.started
                .ok_or("native catalog clock absent")?
                .elapsed()
                .as_millis(),
        )?)
    }
    pub(in crate::live_session) fn service_actions(
        &mut self,
        actions: &mut NativeLauncherActionService,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &LiveProductionVisualRuntime,
        launches: &mut SessionLaunchQueue,
        active_children: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let transaction = self.mint_transaction()?;
        let now_msec = self.action_now_msec()?;
        let Some(publication) = transport
            .content_grant()
            .and_then(|grant| self.publication(grant))
            .and_then(|p| p.published())
        else {
            return Ok(());
        };
        let presented = runtime
            .input_projections()
            .iter()
            .flat_map(|p| p.content.iter().cloned())
            .collect::<Vec<_>>();
        let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let now_usec = u64::try_from(clock.tv_sec)?
            .checked_mul(1_000_000)
            .and_then(|s| s.checked_add(u64::try_from(clock.tv_nsec).ok()? / 1000))
            .ok_or("native action clock overflow")?;
        actions.service_connected(
            transport,
            publication,
            &presented,
            transaction,
            launches,
            LAUNCHER_APPLICATION_ID,
            active_children,
            now_usec,
            now_msec,
        )?;
        Ok(())
    }
}
