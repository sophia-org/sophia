use super::*;
use sophia_backend_live::{LiveProductionNativeScanout, LiveSeatController};
use std::time::{Duration, Instant};

/// What one runtime rescan found. Nothing here touches the suspended owner,
/// its retained images or the hotplug quarantine: discovery holds only probe
/// descriptors until a resolved replacement is constructed, and a refusal at
/// runtime is unavailability, never a session failure, for a strict profile
/// as for an adaptive one.
pub(in crate::live_session) enum RuntimeOutputReplacement {
    Waiting,
    Refused(RuntimeOutputRefusal),
    Active(
        Box<LiveProductionNativeScanout>,
        Box<DesktopOutputReconciliation>,
        Box<crate::live_session::output_realization::OutputPolicyLayout>,
    ),
}

/// Keep the failed boundary and bounded error identity before Display erases
/// them. Free-form text is console-only; daily capture retains these fields.
pub(in crate::live_session) struct RuntimeOutputRefusal {
    pub stage: &'static str,
    pub code: &'static str,
    pub errno: u32,
    pub validation: &'static str,
    message: String,
}

impl RuntimeOutputRefusal {
    pub fn new(stage: &'static str, error: &(dyn Error + 'static)) -> Self {
        let mut code = "unclassified";
        let mut errno = 0;
        let mut source = Some(error);
        // Typed causes can sit below a contextual wrapper. Bound traversal
        // even for an erroneous Error::source cycle, and keep only fixed codes.
        for _ in 0..16 {
            let Some(cause) = source else { break };
            if code == "unclassified" {
                code = crate::diagnostics::failure_code(cause);
            }
            if errno == 0 {
                errno = cause
                    .downcast_ref::<io::Error>()
                    .and_then(io::Error::raw_os_error)
                    .and_then(|errno| u32::try_from(errno).ok())
                    .unwrap_or(0);
            }
            source = cause.source();
        }
        Self {
            stage,
            code,
            errno,
            validation: "not_attempted",
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for RuntimeOutputRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
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
    let discovery = match LiveNativeOutputDiscovery::probe(&controller.device_opener()) {
        Ok(discovery) => discovery,
        Err(error) => {
            return RuntimeOutputReplacement::Refused(RuntimeOutputRefusal::new("probe", &error));
        }
    };
    let resolution = resolve_output_replacement(discovery, profile, previous, recovery);
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
                            Err(error) => RuntimeOutputReplacement::Refused(
                                RuntimeOutputRefusal::new("layout", error.as_ref()),
                            ),
                        }
                    }
                    Ok(activation) => RuntimeOutputReplacement::Refused(RuntimeOutputRefusal {
                        validation: activation.validation,
                        ..RuntimeOutputRefusal::new(
                            "validation",
                            &io::Error::other(
                                "replacement output activation was refused by hardware",
                            ),
                        )
                    }),
                    Err(error) => RuntimeOutputReplacement::Refused(RuntimeOutputRefusal::new(
                        "activation",
                        error.as_ref(),
                    )),
                },
                Err(error) => RuntimeOutputReplacement::Refused(RuntimeOutputRefusal::new(
                    "construction",
                    error.as_ref(),
                )),
            }
        }
        Err(error) => RuntimeOutputReplacement::Refused(RuntimeOutputRefusal::new(
            "resolution",
            error.as_ref(),
        )),
    }
}

/// Kernel and processed udev notices can describe one cable transition. Hold
/// input immediately, but wait for the burst before destroying a live owner.
pub(in crate::live_session) fn runtime_output_notice_deadline(
    now: Instant,
    pending: Option<Instant>,
) -> Instant {
    let deadline = now + Duration::from_millis(250);
    // Do not let a continuing stream postpone recovery forever. A new notice
    // can shorten an old retry delay but cannot extend this coalescing window.
    pending.map_or(deadline, |pending| pending.min(deadline))
}

/// A runtime refusal can be transient while the display link settles. Keep
/// the finite rescan series, including conservative retries, rather than
/// spending the entire hardware allowance in two adjacent owner-loop turns.
pub(in crate::live_session) fn runtime_output_retry_after_failure(
    recovery: &mut OutputRecovery,
    profile: &DesktopOutputCandidate,
    attempts: &mut usize,
) -> Option<Duration> {
    if *recovery == OutputRecovery::Exhausted {
        return None;
    }
    let delay = runtime_output_retry_delay(*attempts);
    *attempts = attempts.saturating_add(1);
    if delay.is_none() {
        *recovery = OutputRecovery::Exhausted;
    } else if profile.availability == sophia_config::DesktopOutputAvailability::Adaptive {
        *recovery = OutputRecovery::Conservative;
    }
    delay
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
