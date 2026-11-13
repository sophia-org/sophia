//! The lease step every exported render shares, moved out of service.rs
//! unchanged (G3-5d d-2a) so the service stays within its size bound.
use super::*;

/// The step every exported render shares, whatever its input (G3-5d d-1):
/// the required format, then a lease id with its exhaustion check, then the
/// lease in this output's table. A refusal leases nothing and drops the
/// lease's backing. Damage settlement is not here; it stays with the Mixed
/// arm that rendered.
pub(super) fn lease_export(
    state: &mut WorkerOutputState,
    next_lease_id: &mut u64,
    format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    descriptor: LiveRendererScanoutBufferDescriptor,
    lease: WorkerLeaseBuffer,
) -> Result<LiveRendererWorkerLeaseId, LiveRendererScanoutBufferExportDetail> {
    if matches!(format, Some(sophia_renderer_live::LiveCompositionFormatRequest::Required(format)) if descriptor.format != format)
    {
        return Err(LiveRendererScanoutBufferExportDetail::InvalidTarget);
    }
    let Some(next_id) = next_lease_id.checked_add(1) else {
        return Err(LiveRendererScanoutBufferExportDetail::WorkerLeaseIdExhausted);
    };
    let lease_id = LiveRendererWorkerLeaseId(*next_lease_id);
    *next_lease_id = next_id;
    state.leases.insert(lease_id, lease);
    Ok(lease_id)
}
