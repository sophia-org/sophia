//! G3-5d d-1, test-support only: the running worker's deterministic render
//! of bound combined frames, in place of the GPU, over the worker's own slot
//! and lease table.
use super::*;

/// G3-5d d-1, test-support only: the running worker's render of a bound
/// combined frame in place of the GPU. It answers the descriptor of what it
/// exported into the target, or the failure. Everything around it (slot,
/// lease, result route) is the worker's own.
pub(crate) type CombinedRenderSeam = Box<
    dyn FnMut(
            &PendingRenderedFrame,
            LiveGbmEglFrameTargetRecord,
        )
            -> Result<LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail>
        + Send,
>;

/// G3-5d d-1, test-support only: a bound combined frame rendered by the
/// seam, then leased by the shared step into its slot. The seam replaces
/// only the GPU render: no repaint table, no damage settlement and no
/// capture. `attempt` invalidates and frees the slot on failure, as for a
/// GPU render.
pub(super) fn render_deterministic(
    seam: &mut Option<CombinedRenderSeam>,
    state: &mut WorkerOutputState,
    next_lease_id: &mut u64,
    frame: PendingRenderedFrame,
    target: LiveGbmEglFrameTargetRecord,
    format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    slot_token: LiveRendererFrameSlotToken,
) -> WorkerOutcome {
    let Some(render) = seam.as_mut() else {
        return WorkerOutcome::Failed(
            LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable,
        );
    };
    let descriptor = match render(&frame, target) {
        Ok(descriptor) => descriptor,
        Err(detail) => return WorkerOutcome::Failed(detail),
    };
    let lease = WorkerLeaseBuffer {
        backing: LeaseBacking::Deterministic,
        cpu: None,
        slot_token,
    };
    match lease_export(state, next_lease_id, format, descriptor, lease) {
        Ok(lease_id) => WorkerOutcome::Exported {
            capture: None,
            descriptor,
            lease_id,
            slot_token,
        },
        Err(detail) => WorkerOutcome::Failed(detail),
    }
}
