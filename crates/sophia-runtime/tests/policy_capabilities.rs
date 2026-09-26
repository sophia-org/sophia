use sophia_protocol::*;
use sophia_runtime::{PolicyConnectionState, PolicyTransferError, select_policy_capabilities};

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
    let dependent =
        SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS | SOPHIA_WM_CAPABILITY_OUTPUT_LAUNCH_CONTEXT;
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

#[test]
fn current_ipc_keeps_revision_refusal_state_order_and_identical_welcome_selection() {
    let hello = WmV1ClientHello {
        minimum_revision: SOPHIA_WM_INTERFACE_REVISION,
        maximum_revision: SOPHIA_WM_INTERFACE_REVISION,
        capabilities: u64::MAX,
    };
    let invalid = WmV1ClientHello {
        minimum_revision: 0,
        ..hello
    };
    let mut connection = PolicyConnectionState::default();
    assert_eq!(
        connection.negotiate(&invalid),
        Err(PolicyTransferError::NotConnected)
    );
    connection.connect(7).unwrap();
    assert_eq!(
        connection.negotiate(&invalid),
        Err(PolicyTransferError::UnsupportedRevision)
    );
    assert_eq!(connection.selected_capabilities(), 0);
    let welcome = connection.negotiate(&hello).unwrap();
    assert_eq!(welcome.connection_epoch, 7);
    assert_eq!(welcome.selected_revision, SOPHIA_WM_INTERFACE_REVISION);
    assert_eq!(
        welcome.capabilities,
        select_policy_capabilities(hello.capabilities, u64::MAX, false)
    );
    assert_eq!(
        connection.negotiate(&invalid),
        Err(PolicyTransferError::AlreadyNegotiated)
    );
    assert_eq!(connection.selected_capabilities(), welcome.capabilities);
}
