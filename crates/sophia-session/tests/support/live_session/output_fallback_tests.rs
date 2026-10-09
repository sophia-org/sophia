use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
};
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopNamedOutputCandidate, DesktopOutputAvailability,
    DesktopOutputCandidate, DesktopOutputMode, DesktopOutputReconcileError,
    reconcile_desktop_output_candidate,
};

fn profile() -> DesktopOutputCandidate {
    DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([31; 32]),
        inherit_sophia: false,
        availability: DesktopOutputAvailability::Adaptive,
        fallback_policy_key: Some(1),
        named: vec![DesktopNamedOutputCandidate {
            connector: "DP-1".into(),
            policy_key: Some(1),
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
    }
}

fn capability(output: u64, name: &str) -> LibdrmNativeOutputCapability {
    let timing = LibdrmNativeOutputTiming::new(2560, 1440, 60_000);
    LibdrmNativeOutputCapability::new(
        OutputId::from_raw(output),
        u32::try_from(output).unwrap(),
        name,
        [timing],
        Some(timing),
        timing,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
}

#[test]
fn moved_monitor_reaches_native_activation_with_explicit_workspace_affinity() {
    // The changed connector and native output ID are independent. The saved
    // 120 Hz request cannot be assumed to describe the fallback's capabilities.
    let capabilities = [capability(47, "DP-2")];
    let outputs = [sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(47),
        size: Size {
            width: 2560,
            height: 1440,
        },
        scale: 1,
    }];
    let topology = project_native_output_topology(&capabilities, &outputs).unwrap();
    let profile = profile();
    let reconciled = reconcile_desktop_output_candidate(&profile, &topology).unwrap();
    let activation =
        prepare_native_output_activation_plan(&capabilities, &topology, &reconciled).unwrap();
    assert_eq!(activation.focused_output(), Some(OutputId::from_raw(47)));
    assert_eq!(reconciled.outputs[0].mode.refresh_millihz, 60_000);
    let keys =
        startup_output_policy_keys(&profile, reconciled.fallback_connector.as_deref()).unwrap();
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(47), &keys, &capabilities).unwrap(),
        Some(1)
    );

    // A later return of the old port must not make two live outputs claim the
    // same workspace affinity. The selected binding lasts for this session.
    let returned = [capability(99, "DP-1"), capability(47, "DP-2")];
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(99), &keys, &returned).unwrap(),
        None
    );
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(47), &keys, &returned).unwrap(),
        Some(1)
    );

    let mut strict = profile;
    strict.availability = DesktopOutputAvailability::Strict;
    strict.fallback_policy_key = None;
    assert_eq!(
        reconcile_desktop_output_candidate(&strict, &topology),
        Err(DesktopOutputReconcileError::UnknownConnector("DP-1".into()))
    );
    assert!(startup_output_policy_keys(&strict, Some("DP-2")).is_err());
}

#[test]
fn fallback_does_not_invent_a_workspace_affinity() {
    let mut profile = profile();
    profile.fallback_policy_key = None;
    let keys = startup_output_policy_keys(&profile, Some("DP-2")).unwrap();
    let capabilities = [capability(47, "DP-2")];
    assert_eq!(
        resolve_output_policy_key(OutputId::from_raw(47), &keys, &capabilities).unwrap(),
        None
    );
    assert_eq!(keys.get("DP-1"), Some(&1));
}
