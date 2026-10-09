use super::*;

pub(super) fn output_topology_from_engine_outputs(
    outputs: &[sophia_engine::HeadlessOutput],
) -> Result<sophia_protocol::OutputTopologySnapshot, Box<dyn std::error::Error>> {
    output_topology_from_engine_outputs_at_generation(outputs, 1)
}

pub(super) fn output_topology_from_engine_outputs_at_generation(
    outputs: &[sophia_engine::HeadlessOutput],
    generation: u64,
) -> Result<sophia_protocol::OutputTopologySnapshot, Box<dyn std::error::Error>> {
    let primary = outputs
        .first()
        .ok_or("live session requires at least one Engine output")?
        .id;
    let mut logical_x = 0i32;
    let entries = outputs
        .iter()
        .map(|output| {
            let scale = output.scale.max(1);
            let scale_i32 = i32::try_from(scale).unwrap_or(i32::MAX);
            let logical_size = Size {
                width: output.size.width.saturating_div(scale_i32).max(1),
                height: output.size.height.saturating_div(scale_i32).max(1),
            };
            let logical = Rect {
                x: logical_x,
                y: 0,
                width: logical_size.width,
                height: logical_size.height,
            };
            logical_x = logical_x.saturating_add(logical_size.width);
            sophia_protocol::OutputTopologyEntry {
                output: output.id,
                logical,
                pixel_size: output.size,
                scale,
                refresh_millihz: 60_000,
                timing: None,
            }
        })
        .collect();
    let snapshot = sophia_protocol::OutputTopologySnapshot {
        generation,
        primary,
        outputs: entries,
    };
    snapshot
        .validate()
        .map_err(|error| -> Box<dyn std::error::Error> {
            format!("invalid live Engine output topology: {error:?}").into()
        })?;
    Ok(snapshot)
}

pub(super) fn resolved_output_bounds(
    resolved: &sophia_backend_live::LiveResolvedOutputTopology,
) -> Vec<(sophia_protocol::OutputId, Rect)> {
    resolved
        .logical_viewports
        .iter()
        .map(|viewport| (viewport.output, viewport.logical))
        .collect()
}

pub(super) fn wm_output_bounds(
    outputs: &[sophia_engine::HeadlessOutput],
) -> Vec<(sophia_protocol::OutputId, Rect)> {
    let mut x = 0;
    outputs
        .iter()
        .map(|output| {
            let scale = i32::try_from(output.scale.max(1)).unwrap_or(i32::MAX);
            let bounds = Rect {
                x,
                y: 0,
                width: output.size.width.saturating_div(scale).max(1),
                height: output.size.height.saturating_div(scale).max(1),
            };
            x = x.saturating_add(bounds.width);
            (output.id, bounds)
        })
        .collect()
}

/// The rectangle every output bound sits inside.
///
/// Reservation bands are root-relative, so a claim on one output has to be
/// measured against this and not against the output alone. Returns `None`
/// when the bounds do not describe a usable root, which the callers report
/// rather than reserving against a rectangle they invented.
pub(super) fn wm_root_bounds(bounds: &[(sophia_protocol::OutputId, Rect)]) -> Option<Rect> {
    bounds
        .iter()
        .try_fold(
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            |root, (_, bounds)| {
                Some(Rect {
                    x: 0,
                    y: 0,
                    width: root.width.max(bounds.x.checked_add(bounds.width)?),
                    height: root.height.max(bounds.y.checked_add(bounds.height)?),
                })
            },
        )
        .filter(|root| !root.is_empty())
}

/// Native output authority state retained from read-only startup validation.
///
/// The candidate is deliberately still protocol-shaped and resource-free here.
/// The session loop admits it as a private transaction, then the ordinary live
/// output path composes frames and owns every renderer/KMS/rollback effect.
pub(super) struct LiveOutputAuthorityBootstrap {
    pub(super) snapshot: sophia_protocol::OutputAuthoritySnapshot,
    pub(super) capabilities: Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    pub(super) startup_candidate: Option<sophia_protocol::OutputTopologyCandidate>,
    pub(super) fallback_connector: Option<String>,
    pub(super) realized_policy_keys: Option<BTreeMap<String, u64>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PreparedOutputProfile {
    slot: sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopOutputCandidate>,
}

impl PreparedOutputProfile {
    pub(super) fn new(
        candidate: sophia_config::DesktopOutputCandidate,
    ) -> Result<Self, sophia_config::DesktopProfileCandidateSlotError> {
        Ok(Self {
            slot: sophia_config::DesktopProfileCandidateSlot::with_candidate(candidate)?,
        })
    }

    // Public-policy admission activates this slot before runtime setup. A
    // reload stages a prepared candidate again; read the payload for that phase.
    pub(super) fn current(&self) -> &sophia_config::DesktopOutputCandidate {
        use sophia_config::DesktopProfileParticipantPhase;
        match self.slot().participant().phase() {
            DesktopProfileParticipantPhase::Prepared => self.slot().candidate(),
            DesktopProfileParticipantPhase::Activated => self.slot().active(),
            _ => None,
        }
        .expect("runtime setup requires a prepared or activated output profile")
    }

    pub(super) const fn slot(
        &self,
    ) -> &sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopOutputCandidate> {
        &self.slot
    }

    pub(super) const fn slot_mut(
        &mut self,
    ) -> &mut sophia_config::DesktopProfileCandidateSlot<sophia_config::DesktopOutputCandidate>
    {
        &mut self.slot
    }
}
