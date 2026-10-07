use std::error::Error;

/// The owning phase at the point an error left the session. Cleanup must not
/// replace the phase of an earlier runtime failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionFailurePhase {
    Startup,
    OwnerLoop,
    Topology,
    Lifecycle,
    WindowManagement,
    Authority,
    InputProof,
    Control,
    Quiescence,
    Cleanup,
    InputTiming,
    FrameValidation,
    ApplicationProof,
    LayoutValidation,
    PresentationValidation,
    ControlDrain,
    KeyDrain,
}

impl SessionFailurePhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::OwnerLoop => "owner_loop",
            Self::Topology => "topology",
            Self::Lifecycle => "lifecycle",
            Self::WindowManagement => "window_management",
            Self::Authority => "authority",
            Self::InputProof => "input_proof",
            Self::Control => "control",
            Self::Quiescence => "quiescence",
            Self::Cleanup => "cleanup",
            Self::InputTiming => "input_timing",
            Self::FrameValidation => "frame_validation",
            Self::ApplicationProof => "application_proof",
            Self::LayoutValidation => "layout_validation",
            Self::PresentationValidation => "presentation_validation",
            Self::ControlDrain => "control_drain",
            Self::KeyDrain => "key_drain",
        }
    }
}

pub fn session_failure_record(phase: SessionFailurePhase, error: &(dyn Error + 'static)) -> String {
    format!(
        "sophia_session_failure schema=1 status=failed phase={} failure_code={}",
        phase.as_str(),
        super::failure_code(error),
    )
}

/// How far a session run got before an error left it.
///
/// The owner loop records its own failure with the phase it was in. An error
/// that leaves before the loop starts, or after the loop finished cleanly, has
/// no such record, so a startup refusal of the output profile ended a daily
/// login with nothing but a failed result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRunStage {
    Startup,
    OwnerLoop,
    Finished,
}

/// The failure record a run still owes, if the owner loop did not write one.
pub fn unrecorded_session_failure(
    stage: SessionRunStage,
    error: &(dyn Error + 'static),
) -> Option<String> {
    let phase = match stage {
        SessionRunStage::Startup => SessionFailurePhase::Startup,
        SessionRunStage::OwnerLoop => return None,
        SessionRunStage::Finished => SessionFailurePhase::Cleanup,
    };
    Some(session_failure_record(phase, error))
}

pub(super) fn approved_phase(value: &str) -> bool {
    use SessionFailurePhase::*;
    [
        Startup,
        OwnerLoop,
        Topology,
        Lifecycle,
        WindowManagement,
        Authority,
        InputProof,
        Control,
        Quiescence,
        Cleanup,
        InputTiming,
        FrameValidation,
        ApplicationProof,
        LayoutValidation,
        PresentationValidation,
        ControlDrain,
        KeyDrain,
    ]
    .iter()
    .any(|phase| phase.as_str() == value)
}
