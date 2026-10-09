use super::*;
use sophia_backend_live::{LiveProductionNativeScanout, LiveSeatController};
use std::time::Duration;

/// What one runtime rescan found. Nothing here touches the suspended owner,
/// its retained images or the hotplug quarantine: discovery holds only probe
/// descriptors until a resolved replacement is constructed, and a refusal at
/// runtime is unavailability, never a session failure, for a strict profile
/// as for an adaptive one.
pub(in crate::live_session) enum RuntimeOutputReplacement {
    Waiting,
    Refused(String),
    Active(
        Box<LiveProductionNativeScanout>,
        Box<DesktopOutputReconciliation>,
        Box<crate::live_session::output_realization::OutputPolicyLayout>,
    ),
}

pub(in crate::live_session) fn resolve_runtime_output_replacement(
    controller: &LiveSeatController,
    profile: &DesktopOutputCandidate,
    previous: Option<&DesktopOutputReconciliation>,
    mapping: sophia_protocol::OutputHeadMapping,
    cursor: &sophia_engine::CursorAsset,
    recovery: OutputRecovery,
) -> RuntimeOutputReplacement {
    if recovery == OutputRecovery::Exhausted {
        return RuntimeOutputReplacement::Waiting;
    }
    let resolution = LiveNativeOutputDiscovery::probe(&controller.device_opener())
        .map_err(|error| Box::new(error) as Box<dyn Error>)
        .and_then(|discovery| resolve_output_replacement(discovery, profile, previous, recovery));
    match resolution {
        Ok(OutputReplacementDecision::Waiting) => RuntimeOutputReplacement::Waiting,
        Ok(OutputReplacementDecision::Active(prepared)) => {
            let PreparedOutputReplacement {
                native,
                realization,
            } = *prepared;
            // This constructor performs no modeset and starts no renderer
            // workers. Refused preflight owners have no scanout custody and
            // may be dropped here. Every owner passed to resume is instead
            // admitted to NativeRetirement before it can acquire payloads.
            match LiveProductionNativeScanout::from_resolved_replacement(
                native,
                mapping,
                cursor.clone(),
            ) {
                Ok(native) => match crate::live_session::output_startup_activation::prepare(
                    &native,
                    &realization,
                ) {
                    Ok(activation) if !activation.refused => {
                        match crate::live_session::output_realization::OutputPolicyLayout::prepare(
                            &realization,
                            &activation.capabilities,
                            &native.outputs(),
                            mapping,
                        ) {
                            Ok(layout) => RuntimeOutputReplacement::Active(
                                Box::new(native),
                                Box::new(realization),
                                Box::new(layout),
                            ),
                            Err(error) => RuntimeOutputReplacement::Refused(error.to_string()),
                        }
                    }
                    Ok(_) => RuntimeOutputReplacement::Refused(
                        "replacement output activation was refused by hardware".into(),
                    ),
                    Err(error) => RuntimeOutputReplacement::Refused(error.to_string()),
                },
                Err(error) => RuntimeOutputReplacement::Refused(error.to_string()),
            }
        }
        Err(error) => RuntimeOutputReplacement::Refused(error.to_string()),
    }
}

/// The delay before retry `attempt` of one rescan series, the same series
/// startup uses, then none: only a new topology notice or seat enable starts
/// another.
pub(in crate::live_session) fn runtime_output_retry_delay(attempt: usize) -> Option<Duration> {
    [250, 1_000, 4_000]
        .get(attempt)
        .map(|millis| Duration::from_millis(*millis))
}

#[path = "../../../tests/support/output_replacement_runtime.rs"]
mod tests;
