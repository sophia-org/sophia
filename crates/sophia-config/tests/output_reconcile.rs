use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopNamedOutputCandidate, DesktopOutputCandidate,
    DesktopOutputMode, DesktopOutputReconcileError, DesktopOutputScale,
    DesktopOutputScaleCapabilities, DesktopOutputState, DesktopOutputTiming,
    DesktopOutputTopologyConnector, DesktopOutputTopologySnapshot, DesktopOutputTransform,
    DesktopOutputTransformSet, DesktopOutputVrrMode, reconcile_desktop_output_candidate,
    validate_desktop_output_reconciliation,
};

fn state(connector: &str, mode: DesktopOutputTiming, position: (i32, i32)) -> DesktopOutputState {
    DesktopOutputState {
        mirror_of: None,
        connector: connector.to_owned(),
        enabled: true,
        mode,
        scale_milli: 1_000,
        position,
        transform: DesktopOutputTransform::Normal,
        vrr: DesktopOutputVrrMode::Disabled,
    }
}

fn connector(
    name: &str,
    mode: DesktopOutputTiming,
    position: (i32, i32),
    vrr_capable: bool,
) -> DesktopOutputTopologyConnector {
    DesktopOutputTopologyConnector {
        connector: name.to_owned(),
        connected: true,
        modes: vec![mode],
        preferred_mode: Some(mode),
        scales: DesktopOutputScaleCapabilities {
            minimum_milli: 500,
            maximum_milli: 2_000,
            step_milli: 250,
            automatic_milli: 1_000,
        },
        transforms: DesktopOutputTransformSet::NORMAL,
        vrr_capable,
        current: state(name, mode, position),
    }
}

fn topology() -> DesktopOutputTopologySnapshot {
    DesktopOutputTopologySnapshot {
        connectors: vec![
            connector(
                "DP-1",
                DesktopOutputTiming::new(2560, 1440, 119_998),
                (0, 0),
                true,
            ),
            connector(
                "DP-2",
                DesktopOutputTiming::new(1920, 1080, 60_000),
                (2560, 0),
                false,
            ),
        ],
    }
}

fn named(
    connector: &str,
    mode: DesktopOutputMode,
    position: (i32, i32),
) -> DesktopNamedOutputCandidate {
    DesktopNamedOutputCandidate {
        policy_key: None,
        mirror_fit: None,
        connector: connector.to_owned(),
        mode: Some(mode),
        scale: Some(DesktopOutputScale::Automatic),
        position: Some(position),
        transform: None,
        enabled: Some(true),
        focus_at_startup: None,
        vrr: Some(DesktopOutputVrrMode::Disabled),
        mirror: Vec::new(),
    }
}

fn candidate() -> DesktopOutputCandidate {
    let mut first = named(
        "DP-1",
        DesktopOutputMode::Exact {
            width: 2560,
            height: 1440,
            refresh_millihz: 120_000,
        },
        (0, 0),
    );
    first.focus_at_startup = Some(true);
    first.vrr = Some(DesktopOutputVrrMode::Automatic);
    DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([7; 32]),
        inherit_sophia: true,
        named: vec![
            first,
            named("DP-2", DesktopOutputMode::Preferred, (2560, 0)),
        ],
    }
}

#[test]
fn reconciliation_is_pure_deterministic_and_resolves_rounded_refresh() {
    let candidate = candidate();
    let original = candidate.clone();
    let topology = topology();

    let first = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();
    let second = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();

    assert_eq!(candidate, original);
    assert_eq!(first, second);
    assert_eq!(first.generation, candidate.generation);
    assert_eq!(first.digest, candidate.digest);
    assert_eq!(first.focused_connector.as_deref(), Some("DP-1"));
    assert_eq!(first.outputs[0].mode.refresh_millihz, 119_998);
    assert_eq!(first.outputs[0].vrr, DesktopOutputVrrMode::Automatic);
    assert_eq!(first.outputs[1].position, (2560, 0));
}

#[test]
fn reconciliation_rejects_unknown_disconnected_and_unsupported_requests() {
    let topology = topology();

    let mut unknown = candidate();
    unknown.named[0].connector = "DP-9".to_owned();
    assert_eq!(
        reconcile_desktop_output_candidate(&unknown, &topology),
        Err(DesktopOutputReconcileError::UnknownConnector(
            "DP-9".to_owned()
        ))
    );

    let mut disconnected_topology = topology.clone();
    disconnected_topology.connectors[0].connected = false;
    disconnected_topology.connectors[0].current.enabled = false;
    assert_eq!(
        reconcile_desktop_output_candidate(&candidate(), &disconnected_topology),
        Err(DesktopOutputReconcileError::DisconnectedConnector(
            "DP-1".to_owned()
        ))
    );

    let mut unsupported_vrr = candidate();
    unsupported_vrr.named[1].vrr = Some(DesktopOutputVrrMode::Always);
    assert_eq!(
        reconcile_desktop_output_candidate(&unsupported_vrr, &topology),
        Err(DesktopOutputReconcileError::VrrUnsupported(
            "DP-2".to_owned()
        ))
    );

    let mut unsupported_transform = candidate();
    unsupported_transform.named[0].transform = Some(DesktopOutputTransform::Rotate90);
    assert_eq!(
        reconcile_desktop_output_candidate(&unsupported_transform, &topology),
        Err(DesktopOutputReconcileError::TransformUnsupported(
            "DP-1".to_owned()
        ))
    );
}

#[test]
fn reconciliation_rejects_ambiguous_modes_overlap_and_dark_topology() {
    let mut ambiguous_topology = topology();
    let lower = DesktopOutputTiming::new(2560, 1440, 119_500);
    let upper = DesktopOutputTiming::new(2560, 1440, 120_500);
    ambiguous_topology.connectors[0].modes = vec![lower, upper];
    ambiguous_topology.connectors[0].preferred_mode = Some(lower);
    ambiguous_topology.connectors[0].current.mode = lower;
    assert_eq!(
        reconcile_desktop_output_candidate(&candidate(), &ambiguous_topology),
        Err(DesktopOutputReconcileError::ModeAmbiguous(
            "DP-1".to_owned()
        ))
    );

    let mut overlapping = candidate();
    overlapping.named[1].position = Some((0, 0));
    assert_eq!(
        reconcile_desktop_output_candidate(&overlapping, &topology()),
        Err(DesktopOutputReconcileError::OutputOverlap {
            first: "DP-1".to_owned(),
            second: "DP-2".to_owned(),
        })
    );

    let mut dark = candidate();
    dark.named[0].enabled = Some(false);
    dark.named[0].focus_at_startup = Some(false);
    dark.named[1].enabled = Some(false);
    assert_eq!(
        reconcile_desktop_output_candidate(&dark, &topology()),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );
}

#[test]
fn non_inheriting_candidate_disables_unspecified_connectors() {
    let mut candidate = candidate();
    candidate.inherit_sophia = false;
    candidate.named.truncate(1);

    let reconciled = reconcile_desktop_output_candidate(&candidate, &topology()).unwrap();

    assert!(reconciled.outputs[0].enabled);
    assert!(!reconciled.outputs[1].enabled);
}

#[test]
fn invalid_topology_is_rejected_before_candidate_resolution() {
    let mut topology = topology();
    topology.connectors[1].connector = "DP-1".to_owned();
    topology.connectors[1].current.connector = "DP-1".to_owned();

    assert!(matches!(
        reconcile_desktop_output_candidate(&candidate(), &topology),
        Err(DesktopOutputReconcileError::InvalidTopology(_))
    ));
}

#[test]
fn manually_constructed_invalid_candidate_is_rejected() {
    let mut candidate = candidate();
    candidate.named[1].connector = "DP-1".to_owned();

    assert!(matches!(
        reconcile_desktop_output_candidate(&candidate, &topology()),
        Err(DesktopOutputReconcileError::InvalidCandidate(_))
    ));
}

#[test]
fn reconciliation_validation_rejects_fabricated_state() {
    let topology = topology();
    let reconciled = reconcile_desktop_output_candidate(&candidate(), &topology).unwrap();

    let mut missing = reconciled.clone();
    missing.outputs.pop();
    assert!(matches!(
        validate_desktop_output_reconciliation(&missing, &topology),
        Err(DesktopOutputReconcileError::InvalidReconciliation(_))
    ));

    let mut unavailable_mode = reconciled.clone();
    unavailable_mode.outputs[1].mode = DesktopOutputTiming::new(800, 600, 60_000);
    assert_eq!(
        validate_desktop_output_reconciliation(&unavailable_mode, &topology),
        Err(DesktopOutputReconcileError::ModeUnavailable(
            "DP-2".to_owned()
        ))
    );

    let mut disabled_focus = reconciled;
    disabled_focus.outputs[0].enabled = false;
    assert_eq!(
        validate_desktop_output_reconciliation(&disabled_focus, &topology),
        Err(DesktopOutputReconcileError::FocusedOutputDisabled(
            "DP-1".to_owned()
        ))
    );
}

fn mirrored(primary: &str, members: &[&str]) -> DesktopOutputCandidate {
    DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(1),
        digest: ConfigDigest::new([1; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            mirror_fit: None,
            connector: primary.to_owned(),
            mode: None,
            scale: None,
            position: None,
            transform: None,
            enabled: Some(true),
            focus_at_startup: None,
            vrr: None,
            mirror: members.iter().map(|member| (*member).to_owned()).collect(),
        }],
    }
}

#[test]
fn a_mirror_group_becomes_one_logical_output_at_each_head_s_own_mode() {
    // The gate is open. A group is admitted, its members bound to the primary, and
    // -- the point of the whole architecture -- a member that cannot present the
    // primary's mode is no longer refused. Each head runs its own mode and the
    // scene is placed onto it, so DP-2 offering only 1080p against a 1440p primary
    // is the ordinary case rather than an error.
    let mixed_modes = DesktopOutputTopologySnapshot {
        connectors: vec![
            connector(
                "DP-1",
                DesktopOutputTiming::new(2560, 1440, 60_000),
                (0, 0),
                false,
            ),
            connector(
                "DP-2",
                DesktopOutputTiming::new(1920, 1080, 60_000),
                (2560, 0),
                false,
            ),
        ],
    };

    let reconciliation =
        reconcile_desktop_output_candidate(&mirrored("DP-1", &["DP-2"]), &mixed_modes)
            .expect("a group whose heads run different modes is admitted");

    let primary = reconciliation
        .outputs
        .iter()
        .find(|output| output.connector == "DP-1")
        .expect("the primary survives");
    let member = reconciliation
        .outputs
        .iter()
        .find(|output| output.connector == "DP-2")
        .expect("the member survives");

    assert_eq!(primary.mirror_of, None, "the primary owns the group");
    assert_eq!(member.mirror_of.as_deref(), Some("DP-1"));
    // One logical output means one position -- the arrangement reject_overlaps
    // would otherwise refuse.
    assert_eq!(member.position, primary.position);
    assert_eq!(member.enabled, primary.enabled);
}

#[test]
fn mirror_groups_lead_with_their_primary_and_omit_ungrouped_outputs() {
    // The connection between configuration and the KMS layer. A group reaches the
    // session as connector names led by the primary, because the primary is the
    // output policy sees and the one members take their state from. An output
    // with no `mirror` contributes nothing: it is its own logical output, and a
    // single-member group would imply a shared identity it does not have.
    let mut candidate = mirrored("DP-1", &["DP-2", "DP-3"]);
    candidate.named.push(DesktopNamedOutputCandidate {
        policy_key: None,
        mirror_fit: None,
        connector: "HDMI-A-1".to_owned(),
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: Some(true),
        focus_at_startup: None,
        vrr: None,
        mirror: Vec::new(),
    });

    assert_eq!(
        candidate.mirror_groups(),
        vec![vec![
            "DP-1".to_owned(),
            "DP-2".to_owned(),
            "DP-3".to_owned()
        ]],
        "one group, led by its primary, and the ungrouped output absent"
    );

    // An unmirrored desktop asks for no grouping at all, which is what leaves
    // every connector as its own logical output.
    let plain = mirrored("DP-1", &[]);
    assert!(plain.mirror_groups().is_empty());
    // And no fit, because there is nothing to place.
    assert_eq!(plain.mirror_fit(), None);
}

#[test]
fn a_mirror_group_naming_an_absent_connector_says_so_first() {
    // An impossible group is reported for being impossible, not for anything the
    // later validation might have to say about it.
    assert_eq!(
        reconcile_desktop_output_candidate(&mirrored("DP-1", &["DP-9"]), &topology()),
        Err(DesktopOutputReconcileError::UnknownConnector(
            "DP-9".to_owned()
        ))
    );
}

#[test]
fn one_connector_cannot_belong_to_two_logical_outputs() {
    let mut candidate = mirrored("DP-1", &["DP-2"]);
    candidate.named.push(DesktopNamedOutputCandidate {
        policy_key: None,
        mirror_fit: None,
        connector: "DP-2".to_owned(),
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: Some(true),
        focus_at_startup: None,
        vrr: None,
        mirror: Vec::new(),
    });

    // DP-2 is both a mirror member and an output in its own right. Exactly the
    // arrangement that would make "one logical output backed by N connectors"
    // untrue, and the only rule the whole candidate is needed to check.
    assert_eq!(
        reconcile_desktop_output_candidate(&candidate, &topology()),
        Err(DesktopOutputReconcileError::MirrorConnectorClaimed {
            primary: "DP-1".to_owned(),
            mirrored: "DP-2".to_owned(),
        })
    );
}

#[test]
fn an_ordinary_candidate_is_unaffected_by_the_mirror_rules() {
    // The regression that matters: every non-mirrored configuration must reconcile
    // exactly as it did before mirroring was expressible.
    assert!(reconcile_desktop_output_candidate(&candidate(), &topology()).is_ok());
}
