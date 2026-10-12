//! Test-only items that used to sit in production sources under
//! `#[cfg(test)]` (t026): a channel standing in for the routed-input
//! ingress, and two helpers the session tests call. They live with the
//! tests now; the production files carry nothing that is only for tests.

use crate::live_session::*;
use std::sync::mpsc::SyncSender;

/// Existing environment/custody fixtures exercise the common host constructor;
/// recipe-selection joins live in application_recipe_launches.rs.
#[allow(clippy::too_many_arguments)]
pub(in crate::live_session) fn spawn_session_application(
    app: &SessionApplicationSpec,
    display: &str,
    xauthority: &std::path::Path,
    control_socket: Option<&std::path::Path>,
    inspection_socket: Option<&std::path::Path>,
    confinement: crate::application_catalog::ApplicationConfinement<'_>,
    context: crate::diagnostics::application::LaunchContext,
) -> std::io::Result<std::process::Child> {
    crate::application_catalog::spawn_application_process(
        &crate::application_catalog::ApplicationLaunchCommand {
            executable: app.executable.clone(),
            arguments: app.arguments.clone(),
            working_directory: None,
            namespace_recipe: None,
        },
        crate::application_catalog::CatalogProcessEnvironment {
            display,
            xauthority,
            control_socket,
            inspection_socket,
            confinement,
        },
        Some(&format!("registered:{}", app.id)),
        context,
    )
}

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
