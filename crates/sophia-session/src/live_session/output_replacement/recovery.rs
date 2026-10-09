use super::*;
use sophia_config::{DesktopOutputAdjustment, DesktopOutputAdjustmentReason as Reason};

/// One hardware recovery allowance per topology/profile/seat notice. Timer
/// retries may observe availability, but cannot replenish an activation budget.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::live_session) enum OutputRecovery {
    #[default]
    Desired,
    Conservative,
    Exhausted,
}

impl OutputRecovery {
    pub fn record_exhausted(self, phase: &'static str, profile: &DesktopOutputCandidate) {
        if self == Self::Exhausted {
            let attempt =
                if profile.availability == sophia_config::DesktopOutputAvailability::Adaptive {
                    2
                } else {
                    1
                };
            tracing::warn!(target: "sophia_scanout_evidence",
                "sophia_live_output_resolution schema=1 phase={phase} status=waiting reason=hardware attempt={attempt}");
        }
    }

    pub fn refused(&mut self, profile: &DesktopOutputCandidate) -> bool {
        *self = match *self {
            Self::Desired
                if profile.availability == sophia_config::DesktopOutputAvailability::Adaptive =>
            {
                Self::Conservative
            }
            _ => Self::Exhausted,
        };
        *self != Self::Exhausted
    }
}

/// Reduce only a normally admitted realization. This cannot reclaim excluded
/// connectors, split a mirror group or grant a key to an unadmitted head. Keep
/// the focused logical group, otherwise the first canonical group; one group
/// limits bandwidth and resource contention. Use advertised timings nearest
/// 60 Hz, unit scale, normal transform and no VRR for the single recovery try.
pub(super) fn conservative_realization(
    probes: &[LiveNativeOutputProbe],
    profile: &DesktopOutputCandidate,
    mut realized: DesktopOutputReconciliation,
) -> Result<DesktopOutputReconciliation, Box<dyn Error>> {
    if profile.availability != sophia_config::DesktopOutputAvailability::Adaptive {
        return Err("strict output policy does not permit conservative recovery".into());
    }
    let focused_group = realized.focused_connector.as_ref().and_then(|name| {
        realized
            .outputs
            .iter()
            .find(|state| state.enabled && &state.connector == name)
            .map(|state| state.mirror_of.as_ref().unwrap_or(&state.connector))
    });
    let primary = realized
        .outputs
        .iter()
        .filter(|state| state.enabled && state.mirror_of.is_none())
        .min_by_key(|state| (focused_group != Some(&state.connector), &state.connector))
        .ok_or("conservative recovery has no admitted logical output")?
        .connector
        .clone();
    for state in &mut realized.outputs {
        if !state.enabled {
            continue;
        }
        let mut changed = Vec::new();
        if state.connector != primary && state.mirror_of.as_ref() != Some(&primary) {
            state.enabled = false;
            state.mirror_of = None;
            changed.push(Reason::Unavailable);
        } else {
            let probe = probes
                .iter()
                .find(|probe| probe.connector == state.connector && probe.connected && probe.usable)
                .ok_or("conservative recovery lost an admitted probe")?;
            let mode = probe
                .modes
                .iter()
                .copied()
                .map(timing)
                .min_by_key(|mode| {
                    (
                        mode.refresh_millihz.abs_diff(60_000),
                        std::cmp::Reverse(u64::from(mode.width) * u64::from(mode.height)),
                        *mode,
                    )
                })
                .ok_or("conservative recovery has no advertised timing")?;
            if state.mode != mode {
                changed.push(Reason::Mode);
                state.mode = mode;
            }
            if state.scale_milli != 1_000 {
                changed.push(Reason::Scale);
                state.scale_milli = 1_000;
            }
            if state.transform != DesktopOutputTransform::Normal {
                changed.push(Reason::Transform);
                state.transform = DesktopOutputTransform::Normal;
            }
            if state.vrr != DesktopOutputVrrMode::Disabled {
                changed.push(Reason::Vrr);
                state.vrr = DesktopOutputVrrMode::Disabled;
            }
            if state.position != (0, 0) {
                changed.push(Reason::Position);
                state.position = (0, 0);
            }
        }
        for reason in changed {
            let adjustment = DesktopOutputAdjustment {
                connector: state.connector.clone(),
                reason,
            };
            if !realized.adjustments.contains(&adjustment) {
                realized.adjustments.push(adjustment);
            }
        }
    }
    realized.focused_connector = Some(primary.clone());
    realized
        .policy_keys
        .retain(|connector, _| connector == &primary);
    if realized
        .fallback_connector
        .as_ref()
        .is_some_and(|name| name != &primary)
    {
        realized.fallback_connector = None;
    }
    sophia_config::validate_desktop_output_reconciliation(
        &realized,
        &project_probes(probes, None),
    )?;
    Ok(realized)
}

#[path = "../../../tests/support/output_replacement_recovery.rs"]
mod tests;
