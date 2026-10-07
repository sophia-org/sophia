//! Fixed codes for a desktop output profile that the hardware refused.
//!
//! The refusal names a connector, a mode or a pair of outputs. Ordinary
//! records keep none of those: connector names are reduced away from every
//! other output record, so a refusal carries only its kind. The profile and the
//! connected monitors at the next login say which one it was.
use sophia_config::DesktopOutputReconcileError as Refusal;

pub(super) const CODES: &[&str] = &[
    "output_profile_invalid_candidate",
    "output_profile_invalid_topology",
    "output_profile_invalid_reconciliation",
    "output_profile_unknown_connector",
    "output_profile_disconnected_connector",
    "output_profile_preferred_mode_unavailable",
    "output_profile_mode_unavailable",
    "output_profile_mode_ambiguous",
    "output_profile_scale_unsupported",
    "output_profile_transform_unsupported",
    "output_profile_vrr_unsupported",
    "output_profile_focused_output_disabled",
    "output_profile_output_overlap",
    "output_profile_mirror_connector_claimed",
    "output_profile_no_enabled_output",
];

/// Matches every variant, so a new refusal cannot reach a record unclassified
/// without a compiler error here.
pub(super) fn failure_code(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    Some(match error.downcast_ref::<Refusal>()? {
        Refusal::InvalidCandidate(_) => "output_profile_invalid_candidate",
        Refusal::InvalidTopology(_) => "output_profile_invalid_topology",
        Refusal::InvalidReconciliation(_) => "output_profile_invalid_reconciliation",
        Refusal::UnknownConnector(_) => "output_profile_unknown_connector",
        Refusal::DisconnectedConnector(_) => "output_profile_disconnected_connector",
        Refusal::PreferredModeUnavailable(_) => "output_profile_preferred_mode_unavailable",
        Refusal::ModeUnavailable(_) => "output_profile_mode_unavailable",
        Refusal::ModeAmbiguous(_) => "output_profile_mode_ambiguous",
        Refusal::ScaleUnsupported(_) => "output_profile_scale_unsupported",
        Refusal::TransformUnsupported(_) => "output_profile_transform_unsupported",
        Refusal::VrrUnsupported(_) => "output_profile_vrr_unsupported",
        Refusal::FocusedOutputDisabled(_) => "output_profile_focused_output_disabled",
        Refusal::OutputOverlap { .. } => "output_profile_output_overlap",
        Refusal::MirrorConnectorClaimed { .. } => "output_profile_mirror_connector_claimed",
        Refusal::NoEnabledOutput => "output_profile_no_enabled_output",
    })
}
