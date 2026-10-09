use super::*;
use crate::desktop_output_topology::{
    prepare_native_output_activation_plan, prepare_native_output_authority_candidate,
    project_native_output_topology,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputHeadMapping, Rect};
use std::collections::BTreeMap;

/// Startup may have used an authority transaction on the same native owner.
/// Admit the saved realization only if that transaction actually published its
/// settings, rather than treating construction or a refused TEST_ONLY as proof.
pub(in crate::live_session) fn matches_presented(
    realization: &DesktopOutputReconciliation,
    capabilities: &[LibdrmNativeOutputCapability],
    outputs: &[HeadlessOutput],
    snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    mapping: OutputHeadMapping,
) -> Result<bool, Box<dyn std::error::Error>> {
    let topology = project_native_output_topology(capabilities, outputs)?;
    let mut active = realization.clone();
    active.outputs.retain(|output| output.enabled);
    let Ok(plan) = prepare_native_output_activation_plan(capabilities, &topology, &active) else {
        return Ok(false);
    };
    let Ok(candidate) =
        prepare_native_output_authority_candidate(&plan, capabilities, snapshot, mapping)
    else {
        return Ok(false);
    };
    Ok(
        candidate.groups[usize::from(candidate.primary_group_index)].output
            == snapshot.primary_output
            && candidate.groups.len() == snapshot.groups.len()
            && candidate.groups.iter().all(|group| {
                snapshot.groups.iter().any(|published| {
                    published.output == group.output
                        && published.logical == group.logical
                        && published.members == group.members
                })
            })
            && candidate.heads.len() == snapshot.heads.iter().filter(|head| head.enabled).count()
            && candidate.heads.iter().all(|target| {
                snapshot.heads.iter().any(|head| {
                    head.head == target.head
                        && head.enabled
                        && head.current_mode == Some(target.mode)
                })
            }),
    )
}

/// One WM-facing view of a replacement. Capabilities here join opaque outputs
/// to realized keys; they do not replace the output authority's published view.
pub(in crate::live_session) struct OutputPolicyLayout {
    pub bounds: Vec<(OutputId, Rect)>,
    pub primary: OutputId,
    pub keys: BTreeMap<String, u64>,
    pub capabilities: Vec<LibdrmNativeOutputCapability>,
    pub timings: BTreeMap<OutputId, sophia_backend_live::LibdrmNativeOutputTiming>,
}

impl OutputPolicyLayout {
    pub fn apply_authority_geometry(
        &self,
        snapshot: &mut sophia_protocol::OutputAuthoritySnapshot,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for group in &mut snapshot.groups {
            group.logical = self
                .bounds
                .iter()
                .find(|(output, _)| *output == group.output)
                .ok_or("resolved output layout lost authority geometry")?
                .1;
        }
        snapshot.primary_output = self.primary;
        snapshot
            .validate()
            .map_err(|error| format!("invalid resolved authority topology: {error:?}"))?;
        Ok(())
    }
    pub fn prepare(
        realization: &DesktopOutputReconciliation,
        capabilities: &[LibdrmNativeOutputCapability],
        outputs: &[HeadlessOutput],
        mapping: OutputHeadMapping,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let topology = project_native_output_topology(capabilities, outputs)?;
        let mut active = realization.clone();
        active.outputs.retain(|output| output.enabled);
        let plan = prepare_native_output_activation_plan(capabilities, &topology, &active)?;
        // Reuse the normal profile-to-authority projection, including negative
        // origin normalization and mirror-primary geometry. This snapshot is a
        // local validation input and never reaches an output-role client.
        let snapshot =
            sophia_backend_live::project_live_output_authority_snapshot(capabilities, outputs, 1)?;
        let candidate =
            prepare_native_output_authority_candidate(&plan, capabilities, &snapshot, mapping)?;
        let primary = candidate.groups[usize::from(candidate.primary_group_index)].output;
        Ok(Self {
            bounds: candidate
                .groups
                .iter()
                .map(|group| (group.output, group.logical))
                .collect(),
            primary,
            keys: realization.policy_keys.clone(),
            capabilities: capabilities.to_vec(),
            timings: capabilities
                .iter()
                .filter(|capability| {
                    active.outputs.iter().any(|state| {
                        state.connector == capability.connector_key() && state.mirror_of.is_none()
                    })
                })
                .map(|capability| (capability.output(), capability.selected_mode()))
                .collect(),
        })
    }

    pub fn frontend_snapshot(
        &self,
        outputs: &[HeadlessOutput],
        generation: u64,
    ) -> Result<sophia_protocol::OutputTopologySnapshot, Box<dyn std::error::Error>> {
        let mut snapshot = crate::live_session::output_topology_from_engine_outputs_at_generation(
            outputs, generation,
        )?;
        snapshot.primary = self.primary;
        for output in &mut snapshot.outputs {
            output.logical = self
                .bounds
                .iter()
                .find(|(id, _)| *id == output.output)
                .ok_or("resolved output layout lost frontend geometry")?
                .1;
            let timing = self
                .timings
                .get(&output.output)
                .ok_or("resolved output layout lost its primary head timing")?;
            output.refresh_millihz = timing.refresh_millihz;
            output.timing = timing.mode;
        }
        snapshot
            .validate()
            .map_err(|error| format!("invalid resolved frontend topology: {error:?}"))?;
        Ok(snapshot)
    }
}
