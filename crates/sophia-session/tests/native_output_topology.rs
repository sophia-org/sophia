#![cfg(feature = "native-session")]

use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopNamedOutputCandidate, DesktopOutputCandidate,
    DesktopOutputMode, DesktopOutputScale, DesktopOutputTransformSet, DesktopOutputVrrMode,
    reconcile_desktop_output_candidate,
};
use sophia_engine::{HeadlessOutput, RenderHeadId};
use sophia_protocol::{OutputHeadMapping, OutputId, OutputVrrPolicy, Size};
use sophia_session::desktop_output_topology::{
    NativeOutputActivationPlanError, NativeOutputTopologyProjectionError,
    prepare_native_output_activation_plan, prepare_native_output_authority_candidate,
    project_native_output_topology,
};

fn capability(
    output: u64,
    connector: &str,
    width: u32,
    height: u32,
    vrr: bool,
) -> LibdrmNativeOutputCapability {
    let selected = LibdrmNativeOutputTiming::new(width, height, 60_000);
    let alternate = LibdrmNativeOutputTiming::new(width, height, 120_000);
    LibdrmNativeOutputCapability::new(
        OutputId::from_raw(output),
        u32::try_from(output).unwrap(),
        connector,
        [selected, alternate],
        Some(alternate),
        selected,
        if vrr {
            LibdrmNativeVrrPropertyDiscoveryStatus::Discovered
        } else {
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported
        },
    )
    .unwrap()
}

fn output(output: u64, width: i32, height: i32, scale: u32) -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(output),
        size: Size { width, height },
        scale,
    }
}

#[test]
fn native_capabilities_project_in_engine_semantic_order() {
    let capabilities = [
        capability(1, "DP-1", 2560, 1440, true),
        capability(2, "DP-2", 1920, 1080, false),
    ];
    let outputs = [output(2, 1920, 1080, 2), output(1, 2560, 1440, 1)];

    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();

    assert_eq!(topology.connectors.len(), 2);
    assert_eq!(topology.connectors[0].connector, "DP-2");
    assert_eq!(topology.connectors[0].current.position, (0, 0));
    assert_eq!(topology.connectors[0].current.scale_milli, 2_000);
    assert_eq!(topology.connectors[1].connector, "DP-1");
    assert_eq!(topology.connectors[1].current.position, (960, 0));
    assert!(topology.connectors[1].vrr_capable);
    assert_eq!(
        topology.connectors[1].transforms,
        DesktopOutputTransformSet::NORMAL
    );
    assert_eq!(
        topology.connectors[1].current.vrr,
        DesktopOutputVrrMode::Disabled
    );
}

#[test]
fn native_projection_rejects_cross_owner_inconsistency() {
    let capability = capability(1, "DP-1", 2560, 1440, true);
    assert_eq!(
        project_native_output_topology(
            std::slice::from_ref(&capability),
            &[output(1, 1920, 1080, 1)]
        ),
        Err(NativeOutputTopologyProjectionError::PixelSizeMismatch(1))
    );
    assert_eq!(
        project_native_output_topology(
            std::slice::from_ref(&capability),
            &[output(2, 2560, 1440, 1)]
        ),
        Err(NativeOutputTopologyProjectionError::MissingCapability(2))
    );
    assert_eq!(
        project_native_output_topology(
            std::slice::from_ref(&capability),
            &[output(1, 2560, 1440, 1), output(1, 2560, 1440, 1),]
        ),
        Err(NativeOutputTopologyProjectionError::DuplicateOutput(1))
    );
    assert_eq!(
        project_native_output_topology(&[capability], &[output(1, 2560, 1440, 9)]),
        Err(NativeOutputTopologyProjectionError::ScaleUnsupported(1))
    );
}

#[test]
fn migrated_output_candidate_reconciles_against_native_projection() {
    let capabilities = [
        capability(1, "DP-1", 2560, 1440, true),
        capability(2, "DP-2", 1920, 1080, false),
    ];
    let outputs = [output(1, 2560, 1440, 1), output(2, 1920, 1080, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([7; 32]),
        inherit_sophia: true,
        named: vec![
            DesktopNamedOutputCandidate {
                policy_key: None,
                mirror_fit: None,
                connector: "DP-1".to_owned(),
                mode: Some(DesktopOutputMode::Exact {
                    width: 2560,
                    height: 1440,
                    refresh_millihz: 120_000,
                }),
                scale: Some(DesktopOutputScale::Automatic),
                position: Some((0, 0)),
                transform: None,
                enabled: Some(true),
                focus_at_startup: Some(true),
                vrr: Some(DesktopOutputVrrMode::Automatic),
                mirror: Vec::new(),
            },
            DesktopNamedOutputCandidate {
                policy_key: None,
                mirror_fit: None,
                connector: "DP-2".to_owned(),
                mode: Some(DesktopOutputMode::Exact {
                    width: 1920,
                    height: 1080,
                    refresh_millihz: 60_000,
                }),
                scale: Some(DesktopOutputScale::Automatic),
                position: Some((2560, 0)),
                transform: None,
                enabled: Some(true),
                focus_at_startup: None,
                vrr: None,
                mirror: Vec::new(),
            },
        ],
    };

    let reconciled = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();

    assert_eq!(reconciled.outputs.len(), 2);
    assert_eq!(reconciled.outputs[0].mode.refresh_millihz, 120_000);
    assert_eq!(reconciled.focused_connector.as_deref(), Some("DP-1"));
}

#[test]
fn native_activation_plan_retains_stable_targets_and_rollback_state() {
    let capabilities = [
        capability(2, "DP-2", 1920, 1080, false),
        capability(1, "DP-1", 2560, 1440, true),
    ];
    let outputs = [output(1, 2560, 1440, 1), output(2, 1920, 1080, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(9),
        digest: ConfigDigest::new([9; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            mirror_fit: None,
            connector: "DP-1".to_owned(),
            mode: Some(DesktopOutputMode::Exact {
                width: 2560,
                height: 1440,
                refresh_millihz: 120_000,
            }),
            scale: None,
            position: None,
            transform: None,
            enabled: None,
            focus_at_startup: Some(true),
            vrr: Some(DesktopOutputVrrMode::Always),
            mirror: Vec::new(),
        }],
    };
    let reconciliation = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();

    let plan =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap();

    assert_eq!(plan.generation(), candidate.generation);
    assert_eq!(plan.digest(), candidate.digest);
    assert_eq!(plan.focused_output(), Some(OutputId::from_raw(1)));
    assert_eq!(
        plan.targets()
            .iter()
            .map(|target| target.output())
            .collect::<Vec<_>>(),
        vec![OutputId::from_raw(1), OutputId::from_raw(2)]
    );
    assert_eq!(plan.targets()[0].rollback().mode.refresh_millihz, 60_000);
    assert_eq!(plan.targets()[0].requested().mode.refresh_millihz, 120_000);
    assert_eq!(
        plan.targets()[0].requested().vrr,
        DesktopOutputVrrMode::Always
    );
    assert_eq!(plan.targets()[1].rollback(), plan.targets()[1].requested());
}

#[test]
fn startup_authority_candidate_preserves_profile_geometry_modes_and_focus() {
    let capabilities = [
        capability(1, "DP-1", 2560, 1440, true)
            .bind_head(RenderHeadId::from_raw(11))
            .unwrap(),
        capability(2, "DP-2", 1920, 1080, false)
            .bind_head(RenderHeadId::from_raw(12))
            .unwrap(),
    ];
    let outputs = [output(1, 2560, 1440, 1), output(2, 1920, 1080, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let profile = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(4),
        digest: ConfigDigest::new([4; 32]),
        inherit_sophia: true,
        named: vec![
            DesktopNamedOutputCandidate {
                policy_key: None,
                connector: "DP-1".to_owned(),
                mode: Some(DesktopOutputMode::Exact {
                    width: 2560,
                    height: 1440,
                    refresh_millihz: 120_000,
                }),
                scale: Some(DesktopOutputScale::FixedMilli(2_000)),
                position: Some((-1280, 0)),
                transform: None,
                enabled: Some(true),
                focus_at_startup: None,
                vrr: Some(DesktopOutputVrrMode::Always),
                mirror_fit: None,
                mirror: Vec::new(),
            },
            DesktopNamedOutputCandidate {
                policy_key: None,
                connector: "DP-2".to_owned(),
                mode: None,
                scale: None,
                position: Some((0, 0)),
                transform: None,
                enabled: Some(true),
                focus_at_startup: Some(true),
                vrr: None,
                mirror_fit: None,
                mirror: Vec::new(),
            },
        ],
    };
    let reconciliation = reconcile_desktop_output_candidate(&profile, &topology).unwrap();
    let plan =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap();
    let snapshot = project_live_output_authority_snapshot(&capabilities, &outputs, 7).unwrap();

    let candidate = prepare_native_output_authority_candidate(
        &plan,
        &capabilities,
        &snapshot,
        OutputHeadMapping::Exact,
    )
    .unwrap();

    assert_eq!(candidate.base_topology_epoch, 7);
    assert_eq!(candidate.primary_group_index, 1);
    assert_eq!(candidate.groups[0].logical.x, 0);
    assert_eq!(candidate.groups[0].logical.width, 1280);
    assert_eq!(candidate.groups[0].logical.height, 720);
    assert_eq!(candidate.groups[1].logical.x, 1280);
    assert_eq!(
        candidate.groups[0].members[0].mapping,
        OutputHeadMapping::Exact
    );
    assert_eq!(candidate.heads[0].vrr, OutputVrrPolicy::Always);
    let first_mode = snapshot.heads[0]
        .modes
        .iter()
        .find(|mode| mode.mode == candidate.heads[0].mode)
        .unwrap();
    assert_eq!(first_mode.refresh_millihz, 120_000);
    candidate.validate_against(&snapshot).unwrap();
}

#[test]
fn native_activation_plan_rejects_capability_drift_and_aliases() {
    let capabilities = [
        capability(1, "DP-1", 2560, 1440, true),
        capability(2, "DP-2", 1920, 1080, false),
    ];
    let outputs = [output(1, 2560, 1440, 1), output(2, 1920, 1080, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let candidate = DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([3; 32]),
        inherit_sophia: true,
        named: Vec::new(),
    };
    let reconciliation = reconcile_desktop_output_candidate(&candidate, &topology).unwrap();

    let drifted = [
        capability(1, "DP-1", 1920, 1080, true),
        capabilities[1].clone(),
    ];
    assert_eq!(
        prepare_native_output_activation_plan(&drifted, &topology, &reconciliation),
        Err(NativeOutputActivationPlanError::CapabilityDrift(
            "DP-1".to_owned()
        ))
    );

    let duplicate = [
        capabilities[0].clone(),
        capability(2, "DP-1", 1920, 1080, false),
    ];
    assert_eq!(
        prepare_native_output_activation_plan(&duplicate, &topology, &reconciliation),
        Err(NativeOutputActivationPlanError::DuplicateConnector(
            "DP-1".to_owned()
        ))
    );

    let invalid_output = [
        capability(0, "DP-1", 2560, 1440, true),
        capabilities[1].clone(),
    ];
    assert_eq!(
        prepare_native_output_activation_plan(&invalid_output, &topology, &reconciliation),
        Err(NativeOutputActivationPlanError::InvalidOutput(0))
    );
}

#[test]
fn two_connectors_on_one_logical_output_are_a_mirror_group() {
    // One SnapshotOutput backed by N connectors. Both rows describe real hardware,
    // and they share a position, which is what makes them one logical output rather
    // than two side by side.
    let mode = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let capabilities = [
        LibdrmNativeOutputCapability::new(
            OutputId::from_raw(1),
            1,
            "DP-1",
            [mode],
            Some(mode),
            mode,
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
        )
        .unwrap(),
        LibdrmNativeOutputCapability::new(
            OutputId::from_raw(1),
            2,
            "DP-2",
            [mode],
            Some(mode),
            mode,
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
        )
        .unwrap(),
    ];
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }];

    let topology = project_native_output_topology(&capabilities, &outputs)
        .expect("a same-mode mirror group projects");

    assert_eq!(topology.connectors.len(), 2);
    assert_eq!(topology.connectors[0].connector, "DP-1");
    assert_eq!(topology.connectors[1].connector, "DP-2");
    assert_eq!(
        topology.connectors[0].current.position, topology.connectors[1].current.position,
        "mirror group members occupy the same logical rect"
    );
}

#[test]
fn a_mirror_group_admits_heads_running_different_modes() {
    // What used to be MirrorModeMismatch. Heads of a group no longer share a mode:
    // the logical output is sized by its primary, because that is what the scene is
    // composed at, and every other head runs its own with the scene placed onto it.
    // One framebuffer cannot satisfy two scanout sizes, and no plane scaling exists
    // on this path, so the alternative to refusing is letterboxing a screen the
    // operator asked to match.
    let first = LibdrmNativeOutputTiming::new(1920, 1080, 60_000);
    let second = LibdrmNativeOutputTiming::new(2560, 1440, 60_000);
    let capabilities = [
        LibdrmNativeOutputCapability::new(
            OutputId::from_raw(1),
            1,
            "DP-1",
            [first],
            Some(first),
            first,
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
        )
        .unwrap(),
        LibdrmNativeOutputCapability::new(
            OutputId::from_raw(1),
            2,
            "DP-2",
            [second],
            Some(second),
            second,
            LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
        )
        .unwrap(),
    ];
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 1920,
            height: 1080,
        },
        scale: 1,
    }];

    let topology =
        project_native_output_topology(&capabilities, &outputs).expect("a group projects");

    assert_eq!(topology.connectors.len(), 2);
    // Each connector reports the mode it actually scans out, not the group's.
    assert_ne!(
        topology.connectors[0].current.mode, topology.connectors[1].current.mode,
        "heads keep their own modes"
    );
    // One logical output still means one position.
    assert_eq!(
        topology.connectors[0].current.position,
        topology.connectors[1].current.position
    );
}

#[test]
fn a_profile_mode_the_hardware_lacks_is_refused_and_names_the_connector() {
    // The reload path builds its candidate through exactly these stages, so
    // this is what a typo'd mode in a reloaded profile costs: a refusal that
    // says which display it was about, and no topology change. Getting this
    // wrong in the other direction -- quietly substituting a mode that does
    // exist -- would leave the operator looking at a display they did not ask
    // for with nothing in the log to explain it.
    let capabilities = [capability(1, "DP-1", 2560, 1440, true)
        .bind_head(RenderHeadId::from_raw(11))
        .unwrap()];
    let outputs = [output(1, 2560, 1440, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let profile = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(4),
        digest: ConfigDigest::new([4; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            connector: "DP-1".to_owned(),
            mode: Some(DesktopOutputMode::Exact {
                width: 2560,
                height: 1440,
                refresh_millihz: 144_000,
            }),
            scale: None,
            position: Some((0, 0)),
            transform: None,
            enabled: Some(true),
            focus_at_startup: Some(true),
            vrr: None,
            mirror_fit: None,
            mirror: Vec::new(),
        }],
    };

    let error = reconcile_desktop_output_candidate(&profile, &topology)
        .expect_err("a refresh the connector does not advertise cannot be reconciled");
    let refusal = error.to_string();
    assert!(refusal.contains("DP-1"), "{refusal}");
    assert!(refusal.contains("mode"), "{refusal}");
}

#[test]
fn a_profile_mode_the_hardware_offers_is_taken_exactly() {
    // The companion to the refusal above, and the reason the exact match is
    // safe to insist on: DRM reports an integer vertical refresh, so a monitor
    // whose 120 Hz timing is really 119.9976 Hz still answers to 120_000 and a
    // profile never has to name a fraction.
    let capabilities = [capability(1, "DP-1", 2560, 1440, true)
        .bind_head(RenderHeadId::from_raw(11))
        .unwrap()];
    let outputs = [output(1, 2560, 1440, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let profile = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(4),
        digest: ConfigDigest::new([4; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            connector: "DP-1".to_owned(),
            mode: Some(DesktopOutputMode::Exact {
                width: 2560,
                height: 1440,
                refresh_millihz: 120_000,
            }),
            scale: None,
            position: Some((0, 0)),
            transform: None,
            enabled: Some(true),
            focus_at_startup: Some(true),
            vrr: None,
            mirror_fit: None,
            mirror: Vec::new(),
        }],
    };

    let reconciliation = reconcile_desktop_output_candidate(&profile, &topology).unwrap();
    let plan =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap();
    let snapshot = project_live_output_authority_snapshot(&capabilities, &outputs, 7).unwrap();
    let candidate = prepare_native_output_authority_candidate(
        &plan,
        &capabilities,
        &snapshot,
        OutputHeadMapping::Exact,
    )
    .unwrap();

    let chosen = snapshot.heads[0]
        .modes
        .iter()
        .find(|mode| mode.mode == candidate.heads[0].mode)
        .expect("the candidate names a mode the head advertises");
    assert_eq!(chosen.refresh_millihz, 120_000);
    candidate.validate_against(&snapshot).unwrap();
}

/// A measured timing must not disturb what a profile matches.
///
/// The mode matcher compares `refresh_millihz` with exact equality, and a
/// profile writes `@120` as 120000. The panel actually runs at 119.997 Hz, and
/// now that the server carries that number, the temptation is to let it become
/// the refresh -- at which point `mode "2560x1440@120"` matches nothing and the
/// display silently drops to its other mode.
///
/// So the nominal rate stays nominal. This holds that line.
#[test]
fn a_measured_timing_does_not_change_what_a_profile_matches() {
    let measured = sophia_protocol::OutputModeTiming {
        clock_khz: 497_751,
        hdisplay: 2560,
        hsync_start: 2608,
        hsync_end: 2640,
        htotal: 2720,
        hskew: 0,
        vdisplay: 1440,
        vsync_start: 1443,
        vsync_end: 1448,
        vtotal: 1525,
        flags: 0,
    };
    // The two numbers disagree, which is the entire point.
    assert_eq!(measured.measured_refresh_millihz(), Some(119_997));

    let mut nominal = LibdrmNativeOutputTiming::new(2560, 1440, 120_000);
    nominal.mode = Some(measured);
    assert_eq!(nominal.refresh_millihz, 120_000);

    let sixty = LibdrmNativeOutputTiming::new(2560, 1440, 60_000);
    let capabilities = [LibdrmNativeOutputCapability::new(
        OutputId::from_raw(1),
        1,
        "DP-1",
        [sixty, nominal],
        Some(nominal),
        sixty,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
    .bind_head(RenderHeadId::from_raw(11))
    .unwrap()];
    let outputs = [output(1, 2560, 1440, 1)];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let profile = DesktopOutputCandidate {
        generation: ConfigGeneration::from_raw(4),
        digest: ConfigDigest::new([4; 32]),
        inherit_sophia: true,
        named: vec![DesktopNamedOutputCandidate {
            policy_key: None,
            connector: "DP-1".to_owned(),
            mode: Some(DesktopOutputMode::Exact {
                width: 2560,
                height: 1440,
                refresh_millihz: 120_000,
            }),
            scale: None,
            position: Some((0, 0)),
            transform: None,
            enabled: Some(true),
            focus_at_startup: Some(true),
            vrr: None,
            mirror_fit: None,
            mirror: Vec::new(),
        }],
    };

    let reconciliation = reconcile_desktop_output_candidate(&profile, &topology)
        .expect("a nominal @120 must still match a mode measured at 119.997 Hz");
    let plan =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciliation).unwrap();
    assert_eq!(plan.targets()[0].requested().mode.refresh_millihz, 120_000);
}
