//! Resolve policy while discovery still owns only probe descriptors. Continuity
//! owns suspension and image custody; neither waiting nor refusal modifies it.

use sophia_backend_live::{
    LibdrmNativeOutputTiming, LiveNativeOutputDiscovery, LiveNativeOutputProbe,
    LiveNativeOutputRequest, LiveResolvedOutputReplacement,
};
use sophia_config::{
    DesktopOutputCandidate, DesktopOutputReconciliation, DesktopOutputResolution,
    DesktopOutputScaleCapabilities, DesktopOutputState, DesktopOutputTiming,
    DesktopOutputTopologyConnector, DesktopOutputTopologySnapshot, DesktopOutputTransform,
    DesktopOutputTransformSet, DesktopOutputVrrMode,
};
use std::error::Error;
use std::io;

mod recovery;
mod reload;
mod runtime;
mod startup;
pub(super) use recovery::OutputRecovery;
pub(super) use reload::{ReloadOutputReplacement, prepare_output_reload};
pub(super) use runtime::{
    RuntimeOutputRefusal, RuntimeOutputReplacement, resolve_runtime_output_replacement,
    runtime_output_notice_deadline, runtime_output_observe_availability,
    runtime_output_retry_after_failure, runtime_output_retry_after_observation,
    runtime_output_retry_delay,
};
pub(super) use startup::wait_for_startup_output;

pub(super) enum OutputReplacementDecision {
    Waiting,
    Active(Box<PreparedOutputReplacement>),
}

pub(super) struct PreparedOutputReplacement {
    pub native: LiveResolvedOutputReplacement,
    pub realization: DesktopOutputReconciliation,
}

pub(super) fn resolve_output_replacement(
    discovery: LiveNativeOutputDiscovery,
    profile: &DesktopOutputCandidate,
    previous: Option<&DesktopOutputReconciliation>,
    recovery: OutputRecovery,
) -> Result<OutputReplacementDecision, Box<dyn Error>> {
    if recovery == OutputRecovery::Exhausted {
        return Ok(OutputReplacementDecision::Waiting);
    }
    let resolution = resolve_probe_policy(discovery.connectors(), profile, previous)?;
    prepare_output_replacement(discovery, profile, recovery, resolution)
}

fn prepare_output_replacement(
    discovery: LiveNativeOutputDiscovery,
    profile: &DesktopOutputCandidate,
    recovery: OutputRecovery,
    resolution: DesktopOutputResolution,
) -> Result<OutputReplacementDecision, Box<dyn Error>> {
    let DesktopOutputResolution::Active(realization) = resolution else {
        return Ok(OutputReplacementDecision::Waiting);
    };
    let realization = if recovery == OutputRecovery::Conservative {
        recovery::conservative_realization(discovery.connectors(), profile, realization)?
    } else {
        realization
    };
    let requests = resolved_requests(discovery.connectors(), &realization)?;
    let native = discovery.resolve(requests)?;
    Ok(OutputReplacementDecision::Active(Box::new(
        PreparedOutputReplacement {
            native,
            realization,
        },
    )))
}

fn resolve_probe_policy(
    probes: &[LiveNativeOutputProbe],
    profile: &DesktopOutputCandidate,
    previous: Option<&DesktopOutputReconciliation>,
) -> Result<DesktopOutputResolution, Box<dyn Error>> {
    let topology = project_profile_probes(probes, profile, previous);
    Ok(sophia_config::resolve_desktop_output_candidate(
        profile, &topology, previous,
    )?)
}

pub(super) fn profile_head_mapping(
    profile: &DesktopOutputCandidate,
) -> sophia_protocol::OutputHeadMapping {
    match profile.mirror_fit() {
        Some(sophia_config::DesktopMirrorFit::Cover) => sophia_protocol::OutputHeadMapping::Cover,
        Some(sophia_config::DesktopMirrorFit::Exact) => sophia_protocol::OutputHeadMapping::Exact,
        Some(sophia_config::DesktopMirrorFit::Fit) | None => {
            sophia_protocol::OutputHeadMapping::Fit
        }
    }
}

fn timing(mode: LibdrmNativeOutputTiming) -> DesktopOutputTiming {
    DesktopOutputTiming::new(mode.width, mode.height, mode.refresh_millihz)
}

fn project_profile_probes(
    probes: &[LiveNativeOutputProbe],
    profile: &DesktopOutputCandidate,
    previous: Option<&DesktopOutputReconciliation>,
) -> DesktopOutputTopologySnapshot {
    let mut topology = project_probes(probes, previous);
    if profile.availability == sophia_config::DesktopOutputAvailability::Strict {
        // The strict native path historically exposed active-capable heads.
        // Empty, disconnected sockets must not invalidate an unrelated head.
        topology
            .connectors
            .retain(|head| head.connected && !head.modes.is_empty());
    }
    topology
}

fn project_probes(
    probes: &[LiveNativeOutputProbe],
    previous: Option<&DesktopOutputReconciliation>,
) -> DesktopOutputTopologySnapshot {
    let mut x = 0;
    let connectors = probes
        .iter()
        .map(|probe| {
            // A connected socket without an atomic target is unavailable to
            // this backend. Advertising its EDID modes would falsely promise
            // that adaptive policy can light it.
            let mut modes = if probe.usable {
                probe.modes.iter().copied().map(timing).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            modes.sort();
            modes.dedup();
            let preferred_mode = probe
                .preferred_mode
                .map(timing)
                .filter(|mode| modes.contains(mode));
            let safe = preferred_mode.or_else(|| {
                modes.iter().copied().min_by_key(|mode| {
                    (
                        mode.refresh_millihz.abs_diff(60_000),
                        std::cmp::Reverse(u64::from(mode.width) * u64::from(mode.height)),
                        *mode,
                    )
                })
            });
            let mut current = DesktopOutputState {
                connector: probe.connector.clone(),
                enabled: probe.connected && safe.is_some(),
                // Disabled, modeless connectors carry no selectable timing.
                mode: safe.unwrap_or(DesktopOutputTiming::new(0, 0, 0)),
                scale_milli: 1_000,
                position: (x, 0),
                transform: DesktopOutputTransform::Normal,
                vrr: DesktopOutputVrrMode::Disabled,
                mirror_of: None,
            };
            if let Some(old) = previous.and_then(|previous| {
                previous
                    .outputs
                    .iter()
                    .find(|output| output.connector == probe.connector)
            }) {
                current.enabled &= old.enabled;
                current.position = old.position;
                if modes.contains(&old.mode) {
                    current.mode = old.mode;
                }
                if old.scale_milli.is_multiple_of(1_000)
                    && (1_000..=8_000).contains(&old.scale_milli)
                {
                    current.scale_milli = old.scale_milli;
                }
                if probe.vrr_capable {
                    current.vrr = old.vrr;
                }
                // Mirror membership comes only from the current profile. An
                // unavailable primary must not survive as a stale group edge.
            }
            if current.enabled {
                x += (current.mode.width * 1_000 / current.scale_milli) as i32;
            }
            DesktopOutputTopologyConnector {
                connector: probe.connector.clone(),
                connected: probe.connected,
                modes,
                preferred_mode,
                scales: DesktopOutputScaleCapabilities {
                    minimum_milli: 1_000,
                    maximum_milli: 8_000,
                    step_milli: 1_000,
                    automatic_milli: 1_000,
                },
                transforms: DesktopOutputTransformSet::NORMAL,
                vrr_capable: probe.vrr_capable,
                current,
            }
        })
        .collect();
    DesktopOutputTopologySnapshot { connectors }
}

fn resolved_requests(
    probes: &[LiveNativeOutputProbe],
    realization: &DesktopOutputReconciliation,
) -> io::Result<Vec<LiveNativeOutputRequest>> {
    realization
        .outputs
        .iter()
        .filter(|output| output.enabled)
        .map(|output| {
            let probe = probes
                .iter()
                .find(|probe| probe.connector == output.connector)
                .ok_or_else(|| io::Error::other("resolved output escaped admitted discovery"))?;
            // Configuration names resolution and refresh. KMS needs all timing
            // fields; retain an actual advertised mode, including porch and flags.
            let mode = probe
                .preferred_mode
                .filter(|mode| timing(*mode) == output.mode)
                .or_else(|| {
                    probe
                        .modes
                        .iter()
                        .copied()
                        .filter(|mode| timing(*mode) == output.mode)
                        .min()
                })
                .ok_or_else(|| io::Error::other("resolved output has no advertised full timing"))?;
            if !output.scale_milli.is_multiple_of(1_000)
                || !(1_000..=8_000).contains(&output.scale_milli)
                || output.transform != DesktopOutputTransform::Normal
            {
                return Err(io::Error::other(
                    "resolved output settings exceed native support",
                ));
            }
            Ok(LiveNativeOutputRequest {
                connector: output.connector.clone(),
                mode,
                scale: output.scale_milli / 1_000,
                vrr: match output.vrr {
                    DesktopOutputVrrMode::Disabled => sophia_protocol::OutputVrrPolicy::Disabled,
                    DesktopOutputVrrMode::Automatic => sophia_protocol::OutputVrrPolicy::Automatic,
                    DesktopOutputVrrMode::Always => sophia_protocol::OutputVrrPolicy::Always,
                },
                mirror_of: output.mirror_of.clone(),
            })
        })
        .collect()
}

#[path = "../../tests/support/output_replacement.rs"]
mod tests;
