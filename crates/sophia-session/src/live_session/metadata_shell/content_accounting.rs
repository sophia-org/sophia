use super::*;

impl LiveMetadataShell {
    /// Called by the existing bounded resource sampler, never per frame.
    /// Observes owners without collecting them or draining any response.
    pub(in crate::live_session) fn record_content_accounting(&self) {
        let Some(limits) = self.transport.content_limits() else {
            return;
        };
        let accounting = self.transport.content_accounting();
        emit("sophia_shell_content_sample", "active", 0, 0, accounting);
        crate::session_println!(
            "sophia_shell_content_budget schema=1 connection_epoch={} content_grant_epoch={} limits_generation={} max_staging_bytes={} max_resident_bytes={} max_retiring_bytes={} max_live_resources={} max_resource_ids={} max_open_transfers={} max_allocations_total={} max_open_candidates_total={} max_pending_candidates_total={} max_control_records={} max_input_queue_bytes={} max_output_queue_bytes={}",
            limits.grant.connection_epoch,
            limits.grant.content_grant_epoch,
            limits.limits_generation,
            limits.max_staging_bytes,
            limits.max_resident_bytes,
            limits.max_retiring_bytes,
            limits.max_live_resources,
            limits.max_resource_ids,
            limits.max_open_transfers,
            limits.max_allocations_total,
            limits.max_open_candidates_total,
            limits.max_pending_candidates_total,
            limits.max_control_records,
            limits.max_input_queue_bytes,
            limits.max_output_queue_bytes,
        );
    }
}

pub(super) fn emit(
    record: &'static str,
    status: &'static str,
    workers_joined: u8,
    settled_candidates: usize,
    accounting: sophia_runtime::ShellContentAccounting,
) {
    let epoch = accounting.epochs;
    let time = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let monotonic_usec = u64::try_from(time.tv_sec)
        .unwrap_or_default()
        .saturating_mul(1_000_000)
        .saturating_add(u64::try_from(time.tv_nsec).unwrap_or_default() / 1_000);
    crate::session_println!(
        "{record} schema=1 status={} connection_epoch={} content_grant_epoch={} monotonic_usec={} workers_joined={workers_joined} settled_candidates={} active_epochs={} retired_epochs={} candidates={} resources={} resource_ids={} transfers={} allocations={} permits={} demands={} staging_bytes={} resident_bytes={} retiring_bytes={} backing_bytes={} reserved_resident_bytes={} reserved_bytes={} reserved_backing_bytes={} response_records={} response_bytes={} input_records={} input_bytes={}",
        status,
        epoch.grant.connection_epoch,
        epoch.grant.content_grant_epoch,
        monotonic_usec,
        settled_candidates,
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
}
