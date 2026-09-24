//! Test-only items that used to sit in production sources under
//! `#[cfg(test)]` (t026): a channel standing in for the routed-input
//! ingress, and two helpers the session tests call. They live with the
//! tests now; the production files carry nothing that is only for tests.

use crate::live_session::*;
use std::sync::mpsc::SyncSender;

impl RoutedInputIngress for SyncSender<XAuthorityRoutedInput> {
    fn try_send(
        &self,
        route: XAuthorityRoutedInput,
    ) -> Result<(), std::sync::mpsc::TrySendError<XAuthorityRoutedInput>> {
        SyncSender::try_send(self, route)
    }

    fn capacity(&self) -> usize {
        // A plain channel has no capacity accessor. The value reaches only a
        // diagnostic field, never an admission decision.
        8
    }
}

pub(super) fn observe_public_output_topology(
    generations: &mut BTreeMap<sophia_protocol::OutputId, u64>,
    live: &mut BTreeSet<sophia_protocol::OutputId>,
    active: &mut sophia_protocol::OutputId,
    outputs: &[sophia_engine::HeadlessOutput],
) -> Result<bool, Box<dyn std::error::Error>> {
    let topology = output_topology_from_engine_outputs(outputs)?;
    let next = outputs
        .iter()
        .map(|output| output.id)
        .collect::<BTreeSet<_>>();
    let changed = next != *live;
    let mut candidate_generations = generations.clone();
    let mut candidate_live = live.clone();
    observe_public_output_generations(&mut candidate_generations, &mut candidate_live, outputs)?;
    let candidate_active = if next.contains(active) {
        *active
    } else {
        topology.primary
    };
    *generations = candidate_generations;
    *live = candidate_live;
    *active = candidate_active;
    Ok(changed)
}

pub(super) fn completed_pointer_gesture_geometry(
    gesture: sophia_protocol::WmPointerGestureCompleted,
    initial: Rect,
) -> Rect {
    let delta_x = gesture.end.x.saturating_sub(gesture.start.x);
    let delta_y = gesture.end.y.saturating_sub(gesture.start.y);
    match gesture.mode {
        sophia_protocol::WmPointerGestureMode::Move => Rect {
            x: initial.x.saturating_add(delta_x),
            y: initial.y.saturating_add(delta_y),
            ..initial
        },
        sophia_protocol::WmPointerGestureMode::Resize => Rect {
            width: initial.width.saturating_add(delta_x).max(1),
            height: initial.height.saturating_add(delta_y).max(1),
            ..initial
        },
    }
}
