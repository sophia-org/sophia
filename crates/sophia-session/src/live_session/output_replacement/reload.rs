use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LiveProductionNativeScanout, LiveSeatController,
};
use sophia_protocol::{OutputAuthoritySnapshot, OutputHeadMapping, OutputTopologyCandidate};
use std::collections::BTreeSet;

pub(in crate::live_session) enum ReloadOutputReplacement {
    /// A changed enabled set needs the same continuity path as hotplug. The
    /// passive preflight has no renderer, modeset or retained-image effects.
    Rebuild,
    Candidate {
        candidate: Box<OutputTopologyCandidate>,
        realization: Box<DesktopOutputReconciliation>,
    },
}

pub(in crate::live_session) fn prepare_output_reload(
    controller: &LiveSeatController,
    profile: &DesktopOutputCandidate,
    previous: Option<&DesktopOutputReconciliation>,
    native: &LiveProductionNativeScanout,
    snapshot: &OutputAuthoritySnapshot,
) -> Result<ReloadOutputReplacement, Box<dyn Error>> {
    // Inspect the complete admitted inventory, including currently dark heads.
    // A strict refusal leaves the viable owner alone. Rebuild resolves again
    // after suspension, so it never acts on this preflight's stale probes.
    let discovery = LiveNativeOutputDiscovery::probe(&controller.device_opener())?;
    match resolve_probe_policy(discovery.connectors(), profile, previous)? {
        DesktopOutputResolution::Waiting { .. } => Err(
            "reloaded output preferences have no usable output; retaining the live owner".into(),
        ),
        DesktopOutputResolution::Active(realization) => classify_reload(
            realization,
            &native.output_capabilities()?,
            &native.outputs(),
            snapshot,
            profile_head_mapping(profile),
        ),
    }
}

fn classify_reload(
    realization: DesktopOutputReconciliation,
    capabilities: &[LibdrmNativeOutputCapability],
    outputs: &[sophia_engine::HeadlessOutput],
    snapshot: &OutputAuthoritySnapshot,
    mapping: OutputHeadMapping,
) -> Result<ReloadOutputReplacement, Box<dyn Error>> {
    let enabled = realization
        .outputs
        .iter()
        .filter(|state| state.enabled)
        .map(|state| state.connector.as_str())
        .collect::<BTreeSet<_>>();
    let current = capabilities
        .iter()
        .map(|capability| capability.connector_key())
        .collect::<BTreeSet<_>>();
    if enabled != current {
        return Ok(ReloadOutputReplacement::Rebuild);
    }
    for state in realization.outputs.iter().filter(|state| state.enabled) {
        let capability = capabilities
            .iter()
            .find(|cap| cap.connector_key() == state.connector)
            .ok_or("reloaded output has no live capability")?;
        let head = capability
            .head()
            .ok_or("reloaded output has no opaque head")?;
        let group = snapshot
            .groups
            .iter()
            .find(|group| {
                group
                    .members
                    .iter()
                    .any(|member| member.head.raw() == head.raw())
            })
            .ok_or("reloaded output has no live logical group")?;
        let primary = capabilities
            .iter()
            .find(|cap| {
                cap.head()
                    .is_some_and(|head| head.raw() == group.members[0].head.raw())
            })
            .ok_or("reloaded output group has no primary capability")?;
        let mirror_of =
            (state.connector != primary.connector_key()).then_some(primary.connector_key());
        if state.mirror_of.as_deref() != mirror_of
            || group.members.iter().any(|member| member.mapping != mapping)
        {
            return Ok(ReloadOutputReplacement::Rebuild);
        }
    }
    let topology =
        crate::desktop_output_topology::project_native_output_topology(capabilities, outputs)?;
    let mut active = realization.clone();
    active.outputs.retain(|state| state.enabled);
    let plan = crate::desktop_output_topology::prepare_native_output_activation_plan(
        capabilities,
        &topology,
        &active,
    )?;
    let candidate = crate::desktop_output_topology::prepare_native_output_authority_candidate(
        &plan,
        capabilities,
        snapshot,
        mapping,
    )?;
    Ok(ReloadOutputReplacement::Candidate {
        candidate: Box::new(candidate),
        realization: Box::new(realization),
    })
}

#[path = "../../../tests/support/output_replacement_reload.rs"]
mod tests;
