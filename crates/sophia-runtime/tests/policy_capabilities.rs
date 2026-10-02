use sophia_protocol::*;
use sophia_runtime::select_policy_capabilities;

#[test]
fn selected_mechanisms_are_bounded_by_offer_ceiling_and_profile_admission() {
    let unconditional = SOPHIA_WM_CAPABILITY_BINDINGS
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
        | SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES;
    let dependent = SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS
        | SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT
        | SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE;
    assert_eq!(
        select_policy_capabilities(u64::MAX, u64::MAX, false),
        unconditional | dependent
    );
    assert_eq!(
        select_policy_capabilities(u64::MAX, u64::MAX, true),
        unconditional | dependent | SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
    );
    for bit in 0..64 {
        let one = 1_u64 << bit;
        let expected = one & unconditional;
        assert_eq!(select_policy_capabilities(one, u64::MAX, false), expected);
        assert_eq!(select_policy_capabilities(u64::MAX, one, false), expected);
        assert_eq!(select_policy_capabilities(one, !one, true), 0);
    }
}

#[test]
fn each_dependency_is_removed_after_either_offer_or_ceiling_excludes_its_prerequisite() {
    for (base, dependent) in [
        (
            SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES,
            SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ),
        (
            SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
            SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT,
        ),
    ] {
        for offer in [0, base, dependent, base | dependent] {
            for ceiling in [0, base, dependent, base | dependent] {
                let expected = (offer & ceiling & base)
                    | if offer & ceiling & base != 0 {
                        offer & ceiling & dependent
                    } else {
                        0
                    };
                assert_eq!(select_policy_capabilities(offer, ceiling, true), expected);
            }
        }
    }
}

/// The chord lifecycle needs both actions and configuration: it is dropped
/// when the offer or the ceiling excludes either one, or both.
#[test]
fn action_lifecycle_needs_both_actions_and_configuration() {
    let actions = SOPHIA_WM_CAPABILITY_ACTIONS;
    let configuration = SOPHIA_WM_CAPABILITY_CONFIGURATION;
    let lifecycle = SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE;
    let all = actions | configuration | lifecycle;
    for offer in 0..8_u64 {
        for ceiling in 0..8_u64 {
            let bits = |set: u64| {
                (if set & 1 != 0 { actions } else { 0 })
                    | (if set & 2 != 0 { configuration } else { 0 })
                    | (if set & 4 != 0 { lifecycle } else { 0 })
            };
            let both = bits(offer) & bits(ceiling);
            let expected = if both & (actions | configuration) == actions | configuration {
                both
            } else {
                both & !lifecycle
            };
            assert_eq!(
                select_policy_capabilities(bits(offer), bits(ceiling), true) & all,
                expected,
                "offer {offer:03b} ceiling {ceiling:03b}"
            );
        }
    }
}
