//! Mechanism selection shared by WM transports. This does not admit a peer,
//! mutate a connection or decide whether a client's required set is satisfied.
use sophia_protocol::*;

const POLICY_SUPPORTED_CAPABILITIES: u64 = SOPHIA_WM_CAPABILITY_BINDINGS
    | SOPHIA_WM_CAPABILITY_ACTIONS
    | SOPHIA_WM_CAPABILITY_MULTI_OUTPUT
    | SOPHIA_WM_CAPABILITY_POINTER_INTERACTIONS
    | SOPHIA_WM_CAPABILITY_CHROME
    | SOPHIA_WM_CAPABILITY_POLICY_DIRTY
    | SOPHIA_WM_CAPABILITY_CONFIGURATION
    | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS
    | SOPHIA_WM_CAPABILITY_INDICATORS
    | SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT
    | SOPHIA_WM_CAPABILITY_TAB_GROUPS
    | SOPHIA_WM_CAPABILITY_TRANSLATION_GROUPS
    | SOPHIA_WM_CAPABILITY_POINTER_FOCUS
    | SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN
    | SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS
    | SOPHIA_WM_CAPABILITY_OUTPUT_POLICY_KEYS
    | SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT
    | SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES
    | SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;

/// Select only offered mechanisms available to this Session, removing dependent
/// mechanisms whose prerequisites did not survive selection. The caller owns
/// the immutable connection ceiling and any required-capability refusal.
pub const fn select_policy_capabilities(
    offered: u64,
    ceiling: u64,
    profile_activation: bool,
) -> u64 {
    let supported = POLICY_SUPPORTED_CAPABILITIES
        | if profile_activation {
            SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
        } else {
            0
        };
    let mut selected = offered & ceiling & supported;
    if selected & SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES == 0 {
        selected &= !SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS;
    }
    if selected & SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN == 0 {
        selected &= !SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT;
    }
    selected
}
