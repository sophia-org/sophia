//! G3-5d d-1, test-support only: the running worker's deterministic render
//! of bound combined frames, in place of the GPU, over the worker's own slot,
//! damage history and lease table.
use super::*;

pub(crate) mod gate;

/// G3-5d, test-support only: what the running worker holds for its tests,
/// the d-1 render seam and the d-2 renderer gate, each installed by its own
/// worker command.
#[derive(Default)]
pub(super) struct Hooks {
    pub(super) seam: Option<CombinedRenderSeam>,
    pub(super) gate: Option<gate::CombinedGate>,
}

impl Hooks {
    /// Arms the gate, then acknowledges it, before the worker serves anything
    /// else: a job admitted after the acknowledgement meets an armed gate.
    pub(super) fn install_gate(&mut self, gate: gate::CombinedGate) {
        gate.armed();
        self.gate = Some(gate);
    }
}

/// G3-5d d-1, test-support only: the running worker's render of a bound
/// combined frame in place of the GPU. It is handed what the GPU draw is
/// handed: the frame, the target and the slot's repaint table. It answers
/// the descriptor of what it exported into the target, or the failure.
/// Everything around it (slot, repaint table, damage settlement, lease,
/// result route) is the worker's own.
pub(crate) type CombinedRenderSeam = Box<
    dyn FnMut(
            &PendingRenderedFrame,
            LiveGbmEglFrameTargetRecord,
            Option<&sophia_renderer_live::NativeCompositionRepaintTable>,
        )
            -> Result<LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail>
        + Send,
>;

/// The target-bundle generation the seam reports to damage settlement: it
/// draws into one notional bundle that is never rebuilt, so a slot's history
/// survives between its renders as it does on a stable GBM bundle.
const DETERMINISTIC_TARGET_GENERATION: u64 = 1;

/// G3-5d d-1, test-support only: a bound combined frame rendered by the
/// seam, then leased by the shared step into its slot. As in the Mixed GPU
/// arm, a Mixed frame's repaint table is built from the slot's history
/// before the draw, and a successful draw settles the slot's damage with the
/// frame's snapshot before the lease step. The seam replaces only the draw
/// and constructs no capture. A failed draw returns before settlement, and
/// `attempt` invalidates and frees the slot, as for a GPU failure.
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
    let damage = match &frame {
        PendingRenderedFrame::Mixed(mixed, _) => Some(mixed.output_damage_snapshot.clone()),
        _ => None,
    };
    let repaint = damage.as_ref().and_then(|snapshot| {
        state
            .slot_damage
            .repaint_table(slot_token.slot_id(), snapshot.as_ref(), target.size)
    });
    let descriptor = match render(&frame, target, repaint.as_ref()) {
        Ok(descriptor) => descriptor,
        Err(detail) => return WorkerOutcome::Failed(detail),
    };
    if let Some(snapshot) = damage {
        state.slot_damage.settle(
            slot_token.slot_id(),
            true,
            Some(DETERMINISTIC_TARGET_GENERATION),
            snapshot,
        );
    }
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
