use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopAuthority, DesktopNamedOutputCandidate,
    DesktopOutputAvailability, DesktopOutputCandidate, DesktopOutputMode,
    DesktopOutputReconcileError, DesktopOutputScale, DesktopOutputScaleCapabilities,
    DesktopOutputState, DesktopOutputTiming, DesktopOutputTopologyConnector,
    DesktopOutputTopologySnapshot, DesktopOutputTransform, DesktopOutputTransformSet,
    DesktopOutputVrrMode, DesktopProfileError, load_desktop_profile,
    prepare_desktop_output_candidate, reconcile_desktop_output_candidate,
    validate_desktop_output_reconciliation,
};

#[path = "support/output_runtime.rs"]
mod runtime;

const DAILY: DesktopOutputTiming = DesktopOutputTiming::new(2560, 1440, 119_998);
const OFFICE: DesktopOutputTiming = DesktopOutputTiming::new(1920, 1080, 60_000);

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn parse(output: &str) -> Result<DesktopOutputCandidate, DesktopProfileError> {
    let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "sophia-output-availability-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("create test directory");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .expect("make test directory private");
    let path: PathBuf = root.join("config.kdl");
    fs::write(&path, format!("schema 1\noutput {{\n{output}\n}}\n")).expect("write profile");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("make profile private");
    let result = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).and_then(|profile| {
        prepare_desktop_output_candidate(&profile.candidates[&DesktopAuthority::Output])
    });
    fs::remove_dir_all(root).unwrap();
    result
}

fn head(name: &str, mode: DesktopOutputTiming, x: i32) -> DesktopOutputTopologyConnector {
    DesktopOutputTopologyConnector {
        connector: name.to_owned(),
        connected: true,
        modes: vec![mode],
        preferred_mode: Some(mode),
        scales: DesktopOutputScaleCapabilities {
            minimum_milli: 500,
            maximum_milli: 2_000,
            step_milli: 250,
            automatic_milli: 1_250,
        },
        transforms: DesktopOutputTransformSet::ALL,
        vrr_capable: true,
        current: DesktopOutputState {
            connector: name.to_owned(),
            enabled: true,
            mode,
            scale_milli: 1_000,
            position: (x, 0),
            transform: DesktopOutputTransform::Normal,
            vrr: DesktopOutputVrrMode::Disabled,
            mirror_of: None,
        },
    }
}

fn unplugged(name: &str) -> DesktopOutputTopologyConnector {
    let mut connector = head(name, DAILY, 0);
    connector.connected = false;
    connector.current.enabled = false;
    connector
}

fn topology(connectors: Vec<DesktopOutputTopologyConnector>) -> DesktopOutputTopologySnapshot {
    DesktopOutputTopologySnapshot { connectors }
}

fn excluded(connector: &str) -> DesktopNamedOutputCandidate {
    DesktopNamedOutputCandidate {
        connector: connector.to_owned(),
        policy_key: None,
        mode: None,
        scale: None,
        position: None,
        transform: None,
        enabled: Some(false),
        focus_at_startup: None,
        vrr: None,
        mirror_fit: None,
        mirror: Vec::new(),
    }
}

/// The installed daily profile at the moment the monitor moved: DP-1 named
/// with its exact mode and startup focus, everything unnamed excluded.
fn daily(availability: DesktopOutputAvailability) -> DesktopOutputCandidate {
    let adaptive = availability == DesktopOutputAvailability::Adaptive;
    DesktopOutputCandidate {
        generation: ConfigGeneration::INITIAL,
        digest: ConfigDigest::new([3; 32]),
        inherit_sophia: false,
        availability,
        fallback_policy_key: adaptive.then_some(1),
        named: vec![DesktopNamedOutputCandidate {
            connector: "DP-1".to_owned(),
            policy_key: Some(1),
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
            mirror_fit: None,
            mirror: Vec::new(),
        }],
    }
}

fn enabled(reconciliation: &sophia_config::DesktopOutputReconciliation) -> Vec<&str> {
    reconciliation
        .outputs
        .iter()
        .filter(|output| output.enabled)
        .map(|output| output.connector.as_str())
        .collect()
}

#[test]
fn availability_and_fallback_key_parse_explicitly_with_strict_default() {
    let named = "named \"DP-1\" { policy-key 1; enabled #true; }";
    let default = parse(named).unwrap();
    assert_eq!(default.availability, DesktopOutputAvailability::Strict);
    assert_eq!(default.fallback_policy_key, None);

    let strict = parse(&format!("availability \"strict\"\n{named}")).unwrap();
    assert_eq!(strict.availability, DesktopOutputAvailability::Strict);

    // The key may name the same affinity as the absent preferred output; the
    // session moves the claim, so the profile has to be able to say it.
    let adaptive = parse(&format!(
        "fallback-policy-key 1\ninherit-sophia #false\navailability \"adaptive\"\n{named}\nnamed \"HDMI-A-2\" {{ enabled #false; }}"
    ))
    .unwrap();
    assert_eq!(adaptive.availability, DesktopOutputAvailability::Adaptive);
    assert_eq!(adaptive.fallback_policy_key, Some(1));
    assert!(!adaptive.inherit_sophia);
    assert_eq!(adaptive.named[1].enabled, Some(false));

    let without_key = parse(&format!("availability \"adaptive\"\n{named}")).unwrap();
    assert_eq!(without_key.fallback_policy_key, None);

    for (source, reason) in [
        ("availability \"lenient\"", "unknown availability"),
        ("availability #true", "non-string availability"),
        (
            "availability \"strict\"\navailability \"adaptive\"",
            "duplicate availability",
        ),
        ("fallback-policy-key 1", "key without adaptive availability"),
        (
            "availability \"strict\"\nfallback-policy-key 1",
            "key on a strict profile",
        ),
        (
            "availability \"adaptive\"\nfallback-policy-key 0",
            "zero key",
        ),
        (
            "availability \"adaptive\"\nfallback-policy-key -1",
            "negative key",
        ),
        (
            "availability \"adaptive\"\nfallback-policy-key 1\nfallback-policy-key 2",
            "duplicate key",
        ),
    ] {
        assert!(parse(&format!("{source}\n{named}")).is_err(), "{reason}");
    }
}

#[test]
fn strict_default_still_refuses_the_moved_monitor() {
    let moved = topology(vec![head("DP-2", DAILY, 0)]);
    assert_eq!(
        reconcile_desktop_output_candidate(&daily(DesktopOutputAvailability::Strict), &moved),
        Err(DesktopOutputReconcileError::UnknownConnector(
            "DP-1".to_owned()
        ))
    );

    // Strict also keeps refusing an exclusion of a connector that is absent,
    // which is why the strict idiom for exclusion is inherit-sophia #false.
    let mut excluding = daily(DesktopOutputAvailability::Strict);
    excluding.named.push(excluded("HDMI-A-2"));
    assert_eq!(
        reconcile_desktop_output_candidate(&excluding, &topology(vec![head("DP-1", DAILY, 0)])),
        Err(DesktopOutputReconcileError::UnknownConnector(
            "HDMI-A-2".to_owned()
        ))
    );
}

#[test]
fn adaptive_daily_profile_lands_on_the_port_the_monitor_moved_to() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named.push(excluded("HDMI-A-2"));
    let moved = topology(vec![head("DP-2", DAILY, 2560)]);

    let reconciled = reconcile_desktop_output_candidate(&profile, &moved).unwrap();

    assert_eq!(reconciled.fallback_connector.as_deref(), Some("DP-2"));
    assert_eq!(reconciled.focused_connector.as_deref(), Some("DP-2"));
    // The fallback takes nothing from DP-1's settings: its preferred mode, the
    // automatic scale, no VRR, normal transform, at the origin.
    assert_eq!(
        reconciled.outputs,
        vec![DesktopOutputState {
            connector: "DP-2".to_owned(),
            enabled: true,
            mode: DAILY,
            scale_milli: 1_250,
            position: (0, 0),
            transform: DesktopOutputTransform::Normal,
            vrr: DesktopOutputVrrMode::Disabled,
            mirror_of: None,
        }]
    );
    assert_eq!(reconciled.generation, profile.generation);
    assert_eq!(reconciled.digest, profile.digest);
    validate_desktop_output_reconciliation(&reconciled, &moved).unwrap();
    assert_eq!(
        reconcile_desktop_output_candidate(&profile, &moved).unwrap(),
        reconciled
    );
}

#[test]
fn adaptive_skips_absent_and_disconnected_alike_where_strict_names_each() {
    let absent = topology(vec![head("DP-2", OFFICE, 0)]);
    let disconnected = topology(vec![unplugged("DP-1"), head("DP-2", OFFICE, 0)]);

    assert_eq!(
        reconcile_desktop_output_candidate(&daily(DesktopOutputAvailability::Strict), &absent),
        Err(DesktopOutputReconcileError::UnknownConnector(
            "DP-1".to_owned()
        ))
    );
    assert_eq!(
        reconcile_desktop_output_candidate(
            &daily(DesktopOutputAvailability::Strict),
            &disconnected
        ),
        Err(DesktopOutputReconcileError::DisconnectedConnector(
            "DP-1".to_owned()
        ))
    );

    let adaptive = daily(DesktopOutputAvailability::Adaptive);
    let from_absent = reconcile_desktop_output_candidate(&adaptive, &absent).unwrap();
    let from_disconnected = reconcile_desktop_output_candidate(&adaptive, &disconnected).unwrap();
    assert_eq!(from_absent.fallback_connector.as_deref(), Some("DP-2"));
    assert_eq!(
        from_disconnected.fallback_connector.as_deref(),
        Some("DP-2")
    );
    assert_eq!(enabled(&from_disconnected), vec!["DP-2"]);
    assert!(!from_disconnected.outputs[0].enabled);
    assert_eq!(from_absent.outputs[0], from_disconnected.outputs[1]);
}

#[test]
fn adaptive_never_reclaims_an_excluded_or_configured_connector() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named.push(excluded("HDMI-A-2"));

    // HDMI-A-2 sorts before eDP-1, so only the exclusion keeps it dark.
    let both = topology(vec![
        head("eDP-1", OFFICE, 0),
        head("HDMI-A-2", OFFICE, 1920),
    ]);
    let reconciled = reconcile_desktop_output_candidate(&profile, &both).unwrap();
    assert_eq!(reconciled.fallback_connector.as_deref(), Some("eDP-1"));
    assert_eq!(enabled(&reconciled), vec!["eDP-1"]);

    // Only the excluded monitor is attached: refuse rather than light it.
    assert_eq!(
        reconcile_desktop_output_candidate(&profile, &topology(vec![head("HDMI-A-2", OFFICE, 0)])),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );

    // A connector the profile names but leaves dark by inheritance already
    // says what the operator wants; the fallback does not override it.
    let mut inherited = daily(DesktopOutputAvailability::Adaptive);
    inherited.inherit_sophia = true;
    let mut dark = excluded("DP-2");
    dark.enabled = None;
    dark.scale = Some(DesktopOutputScale::FixedMilli(2_000));
    inherited.named.push(dark);
    let mut unlit = head("DP-2", OFFICE, 0);
    unlit.current.enabled = false;
    assert_eq!(
        reconcile_desktop_output_candidate(&inherited, &topology(vec![unlit])),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );
}

#[test]
fn adaptive_inheriting_profile_keeps_what_sophia_lights_without_a_fallback() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.inherit_sophia = true;
    let reconciled =
        reconcile_desktop_output_candidate(&profile, &topology(vec![head("DP-2", OFFICE, 0)]))
            .unwrap();
    assert_eq!(reconciled.fallback_connector, None);
    assert_eq!(reconciled.focused_connector, None);
    assert_eq!(enabled(&reconciled), vec!["DP-2"]);
    assert_eq!(reconciled.outputs[0].scale_milli, 1_000);
}

#[test]
fn fallback_choice_follows_connector_names_not_enumeration() {
    let profile = daily(DesktopOutputAvailability::Adaptive);
    let forward = topology(vec![
        head("DP-10", OFFICE, 0),
        head("DP-2", OFFICE, 1920),
        head("HDMI-A-1", OFFICE, 3840),
    ]);
    let mut reversed = forward.clone();
    reversed.connectors.reverse();

    let first = reconcile_desktop_output_candidate(&profile, &forward).unwrap();
    let second = reconcile_desktop_output_candidate(&profile, &reversed).unwrap();
    // Byte order of the name: "DP-10" precedes "DP-2".
    assert_eq!(first.fallback_connector.as_deref(), Some("DP-10"));
    assert_eq!(second.fallback_connector.as_deref(), Some("DP-10"));
    assert_eq!(enabled(&first), vec!["DP-10"]);
    assert_eq!(enabled(&second), vec!["DP-10"]);

    let mut skipping = profile.clone();
    skipping.named.push(excluded("DP-10"));
    let third = reconcile_desktop_output_candidate(&skipping, &reversed).unwrap();
    assert_eq!(third.fallback_connector.as_deref(), Some("DP-2"));
}

#[test]
fn a_fallback_without_a_preferred_mode_uses_an_advertised_timing() {
    let mut first = head("DP-2", OFFICE, 0);
    first.preferred_mode = None;
    let attached = topology(vec![first, head("DP-3", OFFICE, 1920)]);
    let resolved =
        reconcile_desktop_output_candidate(&daily(DesktopOutputAvailability::Adaptive), &attached)
            .unwrap();
    assert_eq!(resolved.fallback_connector.as_deref(), Some("DP-2"));
    assert_eq!(resolved.outputs[0].mode, OFFICE);
}

#[test]
fn nothing_usable_is_still_no_enabled_output() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named.push(excluded("DP-2"));
    for attached in [
        topology(vec![head("DP-2", OFFICE, 0)]),
        topology(vec![head("DP-2", OFFICE, 0), unplugged("DP-3")]),
    ] {
        assert_eq!(
            reconcile_desktop_output_candidate(&profile, &attached),
            Err(DesktopOutputReconcileError::NoEnabledOutput)
        );
    }
}

#[test]
fn startup_focus_follows_the_output_actually_lit() {
    // The focused output is absent but another named output remains: no
    // fallback, and no focus is invented for the survivor.
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    let mut survivor = excluded("DP-3");
    survivor.enabled = Some(true);
    profile.named.push(survivor);
    let reconciled =
        reconcile_desktop_output_candidate(&profile, &topology(vec![head("DP-3", OFFICE, 0)]))
            .unwrap();
    assert_eq!(reconciled.fallback_connector, None);
    assert_eq!(reconciled.focused_connector, None);
    assert_eq!(enabled(&reconciled), vec!["DP-3"]);

    // Focusing an output the profile itself disables is a contradiction
    // whether or not that output is attached.
    let mut contradictory = daily(DesktopOutputAvailability::Adaptive);
    contradictory.named[0].enabled = Some(false);
    assert_eq!(
        reconcile_desktop_output_candidate(
            &contradictory,
            &topology(vec![head("DP-2", OFFICE, 0)])
        ),
        Err(DesktopOutputReconcileError::FocusedOutputDisabled(
            "DP-1".to_owned()
        ))
    );
}

#[test]
fn the_preferred_monitor_returning_restores_the_strict_result() {
    let both = topology(vec![head("DP-1", DAILY, 0), head("DP-2", OFFICE, 2560)]);
    let strict =
        reconcile_desktop_output_candidate(&daily(DesktopOutputAvailability::Strict), &both)
            .unwrap();
    let adaptive =
        reconcile_desktop_output_candidate(&daily(DesktopOutputAvailability::Adaptive), &both)
            .unwrap();

    assert_eq!(adaptive.fallback_connector, None);
    assert_eq!(adaptive.outputs, strict.outputs);
    assert_eq!(adaptive.focused_connector.as_deref(), Some("DP-1"));
    assert_eq!(enabled(&adaptive), vec!["DP-1"]);
    assert_eq!(adaptive.outputs[0].vrr, DesktopOutputVrrMode::Automatic);
}

#[test]
fn adaptive_uses_safe_settings_but_strict_keeps_its_refusals() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named[0].mode = Some(DesktopOutputMode::Exact {
        width: 3840,
        height: 2160,
        refresh_millihz: 60_000,
    });
    let attached = topology(vec![head("DP-1", DAILY, 0), head("DP-2", OFFICE, 2560)]);
    let resolved = reconcile_desktop_output_candidate(&profile, &attached).unwrap();
    assert_eq!(resolved.outputs[0].mode, DAILY);
    profile.availability = DesktopOutputAvailability::Strict;
    profile.fallback_policy_key = None;
    assert_eq!(
        reconcile_desktop_output_candidate(&profile, &attached),
        Err(DesktopOutputReconcileError::ModeUnavailable(
            "DP-1".to_owned()
        ))
    );

    let mut unsupported = daily(DesktopOutputAvailability::Adaptive);
    unsupported.named[0].vrr = Some(DesktopOutputVrrMode::Always);
    let mut fixed = head("DP-1", DAILY, 0);
    fixed.vrr_capable = false;
    let resolved =
        reconcile_desktop_output_candidate(&unsupported, &topology(vec![fixed.clone()])).unwrap();
    assert_eq!(resolved.outputs[0].vrr, DesktopOutputVrrMode::Disabled);
    unsupported.availability = DesktopOutputAvailability::Strict;
    unsupported.fallback_policy_key = None;
    assert_eq!(
        reconcile_desktop_output_candidate(&unsupported, &topology(vec![fixed])),
        Err(DesktopOutputReconcileError::VrrUnsupported(
            "DP-1".to_owned()
        ))
    );
}

#[test]
fn adaptive_mirror_groups_are_unavailable_as_a_whole() {
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named[0].mirror = vec!["DP-3".to_owned()];

    assert_eq!(
        reconcile_desktop_output_candidate(&profile, &topology(vec![head("DP-3", DAILY, 0)])),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );
    assert_eq!(
        reconcile_desktop_output_candidate(
            &profile,
            &topology(vec![head("DP-1", DAILY, 0), unplugged("DP-3")])
        ),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );
    assert_eq!(
        reconcile_desktop_output_candidate(
            &profile,
            &topology(vec![unplugged("DP-1"), head("DP-3", DAILY, 0)])
        ),
        Err(DesktopOutputReconcileError::NoEnabledOutput)
    );

    // A dark group's member is the group's, not a fallback candidate.
    profile.named[0].enabled = Some(false);
    profile.named[0].focus_at_startup = None;
    let reconciled = reconcile_desktop_output_candidate(
        &profile,
        &topology(vec![
            head("DP-1", DAILY, 0),
            head("DP-3", DAILY, 0),
            head("HDMI-A-1", OFFICE, 2560),
        ]),
    )
    .unwrap();
    assert_eq!(reconciled.fallback_connector.as_deref(), Some("HDMI-A-1"));
    assert_eq!(enabled(&reconciled), vec!["HDMI-A-1"]);
}

#[test]
fn a_fabricated_fallback_or_strict_key_is_refused() {
    let moved = topology(vec![head("DP-1", DAILY, 0), head("DP-2", OFFICE, 2560)]);
    let mut profile = daily(DesktopOutputAvailability::Adaptive);
    profile.named[0].connector = "DP-9".to_owned();
    let reconciled = reconcile_desktop_output_candidate(&profile, &moved).unwrap();
    assert_eq!(reconciled.fallback_connector.as_deref(), Some("DP-1"));

    let mut unfocused = reconciled.clone();
    unfocused.fallback_connector = Some("DP-2".to_owned());
    let mut dark = reconciled.clone();
    dark.focused_connector = Some("DP-2".to_owned());
    dark.fallback_connector = Some("DP-2".to_owned());
    let mut missing = reconciled.clone();
    missing.fallback_connector = Some("DP-7".to_owned());
    for fabricated in [unfocused, missing] {
        assert!(matches!(
            validate_desktop_output_reconciliation(&fabricated, &moved),
            Err(DesktopOutputReconcileError::InvalidReconciliation(_))
        ));
    }
    // DP-2 is dark, which the focus rule names before the fallback rule.
    assert!(validate_desktop_output_reconciliation(&dark, &moved).is_err());

    let mut strict_key = daily(DesktopOutputAvailability::Strict);
    strict_key.fallback_policy_key = Some(1);
    let mut zero_key = daily(DesktopOutputAvailability::Adaptive);
    zero_key.fallback_policy_key = Some(0);
    for candidate in [strict_key, zero_key] {
        assert!(matches!(
            reconcile_desktop_output_candidate(&candidate, &moved),
            Err(DesktopOutputReconcileError::InvalidCandidate(_))
        ));
    }
}
