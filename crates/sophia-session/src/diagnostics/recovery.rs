//! Fixed diagnostics for recovery failures; never carries client text.

#[derive(Clone, Copy, Debug)]
pub enum SessionCompletionFailure {
    IncompleteLayoutRecovery,
    ResizeProofIncomplete,
    NoCommittedLayout,
    PendingWork(u16),
    ControlsNotSettled,
}

impl SessionCompletionFailure {
    pub const fn code(self) -> &'static str {
        match self {
            Self::IncompleteLayoutRecovery => "completion_layout_recovery",
            Self::ResizeProofIncomplete => "completion_resize_proof",
            Self::NoCommittedLayout => "completion_no_committed_layout",
            Self::ControlsNotSettled => "completion_controls_not_settled",
            Self::PendingWork(mask) => {
                if mask & (1 << 6) != 0 {
                    "completion_pending_input"
                } else if mask & 1 != 0 {
                    "completion_pending_layout"
                } else if mask & 2 != 0 {
                    "completion_pending_wm_update"
                } else if mask & 4 != 0 {
                    "completion_pending_wm_request"
                } else if mask & 8 != 0 {
                    "completion_pending_actions"
                } else if mask & 16 != 0 {
                    "completion_pending_launch"
                } else if mask & 32 != 0 {
                    "completion_pending_admission"
                } else if mask & 128 != 0 {
                    "completion_topology_quarantined"
                } else {
                    "completion_policy_degraded"
                }
            }
        }
    }
}
impl std::fmt::Display for SessionCompletionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for SessionCompletionFailure {}

pub(super) const CODES: &[&str] = &[
    "completion_layout_recovery",
    "completion_resize_proof",
    "completion_no_committed_layout",
    "completion_controls_not_settled",
    "completion_pending_input",
    "completion_pending_layout",
    "completion_pending_wm_update",
    "completion_pending_wm_request",
    "completion_pending_actions",
    "completion_pending_launch",
    "completion_pending_admission",
    "completion_topology_quarantined",
    "completion_policy_degraded",
    "input_missing_ticket",
    "input_client_failure",
    "input_proof_timeout",
    "control_capacity",
    "control_duplicate",
    "control_rejected",
    "control_timeout",
    "control_unexpected_ack",
    "control_disconnected",
    "control_client_disconnected",
    "control_quiescing",
    "input_recovery_shutdown_failed",
    "input_registry_poisoned",
];

pub(super) fn failure_code(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    use crate::{
        input_delivery::InputDeliveryError as Input,
        session_control::SessionControlFailure as Control,
    };
    if let Some(error) = error.downcast_ref::<SessionCompletionFailure>() {
        return Some(error.code());
    }
    if let Some(error) = error.downcast_ref::<Input>() {
        return Some(match error {
            Input::MissingTicket(_) => "input_missing_ticket",
            Input::ClientFailure(_) => "input_client_failure",
            Input::ProofTimeout => "input_proof_timeout",
        });
    }
    if let Some(error) = error.downcast_ref::<Control>() {
        return Some(match error {
            Control::Capacity => "control_capacity",
            Control::Duplicate => "control_duplicate",
            Control::Rejected(_) => "control_rejected",
            Control::TimedOut => "control_timeout",
            Control::UnexpectedAcknowledgement => "control_unexpected_ack",
            Control::Disconnected => "control_disconnected",
            Control::ClientDisconnected => "control_client_disconnected",
            Control::Quiescing => "control_quiescing",
        });
    }
    if let Some(error) = error.downcast_ref::<sophia_x_authority::XServerFrontendRouteError>() {
        return match error {
            sophia_x_authority::XServerFrontendRouteError::RecoveryShutdownFailed { .. } => {
                Some("input_recovery_shutdown_failed")
            }
            sophia_x_authority::XServerFrontendRouteError::RegistryPoisoned => {
                Some("input_registry_poisoned")
            }
            _ => None,
        };
    }
    None
}

pub(super) fn field(key: &str, value: &str) -> bool {
    match key {
        "schema" | "delivery" | "client" | "surface" | "generation" | "seat" | "control_epoch"
        | "age_msec" | "transaction" | "held_controls" | "pending_mask" => {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        }
        "release_barrier" => matches!(value, "true" | "false"),
        "status" => matches!(value, "revoked" | "retired" | "failed" | "settled"),
        "reason" => matches!(
            value,
            "delivery_deadline" | "control_deadline" | "seat_handoff" | "client_disconnected"
        ),
        "outcome" => matches!(
            value,
            "Flushed"
                | "TargetGone"
                | "EpochRevoked"
                | "RouteRejected"
                | "WriteFailed"
                | "ClientDisconnected"
                | "TimedOut"
        ),
        "content" => value == "redacted",
        _ => false,
    }
}
