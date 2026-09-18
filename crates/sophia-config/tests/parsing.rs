use sophia_config::{
    COMPILED_CORE_CONFIG, COMPILED_WM_CONFIG, ConfigGeneration, ConfigParseError, CoreConfigDelta,
    CoreConfigState, ExternalWmInterface, FocusRingStyle, InputSourceConfig, ReloadDisposition,
    WmActionBehavior, WmConfigState, WmLayoutKind, parse_core_config, parse_wm_config,
};

const CORE: &str = r##"
/- kdl-version 2
schema 2
session {
    application "terminal" id=1 executable="/usr/bin/kitty" placement-class=2 {
        arg "--single-instance"
    }
    startup 1
}
input {
    seat "seat0"
    keyboard rules="evdev" model="pc105" layout="us" variant="" options=""
    repeat delay-ms=500 interval-ms=25
}
outputs {
    output "DP-1" x=0 y=0 mode="preferred" scale=1 primary=#true
}
compositor {
    chrome-fallback {
        focus-ring enabled=#true width=3 color="#70b7ff"
        frame enabled=#false width=0 focused-color="#70b7ff" unfocused-color="#303030"
    }
    chrome-limits max-width=12
    cursor theme="Adwaita" size=24 shape="text"
}
namespace profile="classic-shared"
external-wm executable="/usr/bin/hagia" {
    arg "--replace"
}
diagnostics verbose=#true
"##;

const WM: &str = r##"
/- kdl-version 2
schema 2
policy timeout-ms=250
workspace 1
workspace 2
layout "columns"
action "next" id=1 behavior="focus-next"
action "workspace-two" id=2 behavior="activate-workspace" workspace=2
action "terminal" id=3 behavior="launch-application" application=1
binding action=1 keycode=36 modifiers="super"
binding action=2 keycode=3 modifiers="super"
chrome {
    focus-ring enabled=#true width=2 color="#70b7ff"
    frame enabled=#false width=0 focused-color="#70b7ff" unfocused-color="#303030"
}
"##;

#[test]
fn parses_complete_core_snapshot() {
    let snapshot =
        parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).expect("valid core config");

    assert_eq!(snapshot.session.applications.len(), 1);
    assert_eq!(
        snapshot.session.applications[0].placement_classification,
        Some(2)
    );
    assert_eq!(snapshot.session.startup, [1]);
    assert_eq!(
        snapshot.input.source,
        InputSourceConfig::Seat("seat0".to_owned())
    );
    assert_eq!(snapshot.input.repeat.delay_msec, 500);
    assert_eq!(snapshot.outputs.len(), 1);
    assert_eq!(snapshot.fallback_chrome.focus_ring.width, 3);
    assert_eq!(snapshot.max_chrome_width, 12);
    assert_eq!(snapshot.cursor.theme, "Adwaita");
    assert_eq!(snapshot.cursor.size, 24);
    assert_eq!(snapshot.cursor.shape, "text");
    assert_eq!(
        snapshot.external_wm.as_ref().map(|wm| wm.interface),
        Some(ExternalWmInterface::SophiaWmV1)
    );
    assert!(snapshot.verbose_diagnostics);
    assert!(snapshot.application_stderr);
}

#[test]
fn application_stderr_defaults_on_and_can_change_live_independently() {
    let active = parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).unwrap();
    let disabled = CORE.replace(
        "diagnostics verbose=#true",
        "diagnostics verbose=#true application-stderr=#false",
    );
    let candidate = parse_core_config(disabled.as_bytes(), ConfigGeneration::INITIAL).unwrap();
    assert!(!candidate.application_stderr);
    let delta = CoreConfigDelta::between(&active, &candidate);
    assert!(delta.diagnostics_changed);
    assert!(!delta.restart_required);
    let absent = CORE.replace("diagnostics verbose=#true", "");
    assert!(
        parse_core_config(absent.as_bytes(), ConfigGeneration::INITIAL)
            .unwrap()
            .application_stderr
    );
    assert!(
        parse_core_config(
            CORE.replace(
                "diagnostics verbose=#true",
                "diagnostics application-stderr=\"yes\""
            )
            .as_bytes(),
            ConfigGeneration::INITIAL
        )
        .is_err()
    );
}

#[test]
fn parses_public_external_wm_interface() {
    let source = CORE.replace(
        "external-wm executable=\"/usr/bin/hagia\"",
        "external-wm executable=\"/usr/bin/hagia\" interface=\"sophia_wm_v1\"",
    );
    let snapshot = parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL)
        .expect("public WM interface must parse");
    assert_eq!(
        snapshot.external_wm.as_ref().map(|wm| wm.interface),
        Some(ExternalWmInterface::SophiaWmV1)
    );
}

#[test]
fn rejects_removed_api_v7_external_wm_interface() {
    let source = CORE.replace(
        "external-wm executable=\"/usr/bin/hagia\"",
        "external-wm executable=\"/usr/bin/hagia\" interface=\"api_v7\"",
    );
    let error = parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL)
        .expect_err("the removed private transport must not remain selectable");
    assert!(
        error
            .to_string()
            .contains("unsupported external WM interface")
    );
}

#[test]
fn parses_complete_native_wm_snapshot() {
    let snapshot =
        parse_wm_config(WM.as_bytes(), ConfigGeneration::INITIAL).expect("valid WM config");

    assert_eq!(snapshot.timeout_msec, 250);
    assert_eq!(snapshot.workspaces, [1, 2]);
    assert_eq!(snapshot.bindings.len(), 2);
    assert_eq!(
        snapshot.actions[1].behavior,
        WmActionBehavior::ActivateWorkspace { workspace: 2 }
    );
}

#[test]
fn parses_natural_wm_layout_policy() {
    let source = WM.replace("layout \"columns\"", "layout \"natural\"");
    let snapshot = parse_wm_config(source.as_bytes(), ConfigGeneration::INITIAL)
        .expect("natural layout policy must be valid");

    assert_eq!(snapshot.layout, WmLayoutKind::Natural);
    assert_eq!(snapshot.layout.name(), "natural");
}

#[test]
fn rejects_unknown_and_duplicate_fields() {
    let unknown = "schema 2\nmystery #true\n";
    assert!(matches!(
        parse_core_config(unknown.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("unknown node")
    ));

    let duplicate = "schema 2\ndiagnostics verbose=#true verbose=#false\n";
    assert!(matches!(
        parse_core_config(duplicate.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("duplicate property")
    ));
}

#[test]
fn rejects_kdl_one_boolean_spelling() {
    let source = "schema 2\ndiagnostics verbose=true\n";
    assert!(parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL).is_err());
}

#[test]
fn rejects_invalid_cross_references_and_reserved_chord() {
    let unknown_workspace = r##"
schema 2
workspace 1
action "bad" id=1 behavior="activate-workspace" workspace=2
"##;
    assert!(matches!(
        parse_wm_config(unknown_workspace.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("unknown workspace")
    ));

    let reserved = r##"
schema 2
action "bad" id=1 behavior="logout"
binding action=1 keycode=14 modifiers="control+alt"
"##;
    assert!(matches!(
        parse_wm_config(reserved.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("reserved emergency chord")
    ));
}

#[test]
fn default_workspace_set_is_validated() {
    let source = r##"
schema 2
action "bad" id=1 behavior="activate-workspace" workspace=10
"##;
    assert!(matches!(
        parse_wm_config(source.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("unknown workspace")
    ));
}

#[test]
fn core_delta_never_partially_applies_restart_fields() {
    let active =
        parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).expect("valid active config");
    let candidate_source = CORE.replace("profile=\"classic-shared\"", "profile=\"confined\"");
    let candidate = parse_core_config(candidate_source.as_bytes(), ConfigGeneration::from_raw(2))
        .expect("valid candidate config");

    let delta = CoreConfigDelta::between(&active, &candidate);
    assert!(delta.restart_required);
    assert!(!delta.applications_changed);
}

#[test]
fn rejects_default_chrome_outside_tighter_limit() {
    let source = "schema 2\ncompositor { chrome-limits max-width=1 }\n";
    assert!(matches!(
        parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("exceeds")
    ));
}

#[test]
fn cursor_configuration_is_bounded_and_semantic() {
    for (replacement, expected) in [
        (
            "cursor theme=\"../personal\" size=24 shape=\"text\"",
            "unsupported character",
        ),
        (
            "cursor theme=\"Adwaita\" size=0 shape=\"text\"",
            "must be in",
        ),
        (
            "cursor theme=\"Adwaita\" size=129 shape=\"text\"",
            "must be in",
        ),
        (
            "cursor theme=\"Adwaita\" size=24 shape=\"renderer-private\"",
            "unsupported semantic cursor shape",
        ),
    ] {
        let source = CORE.replace(
            "cursor theme=\"Adwaita\" size=24 shape=\"text\"",
            replacement,
        );
        let error = parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL)
            .expect_err("invalid cursor config must fail closed");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn a_cursor_change_applies_without_a_restart() {
    // It used to be staged for restart, because the KMS path refused a second
    // cursor asset once its buffer existed. The buffer is sized to the
    // driver's cursor dimensions rather than to the raster, so a replacement
    // repaints the buffer the plane is already scanning; nothing about the
    // cursor needs the session to start again.
    let active =
        parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).expect("valid active config");
    let mut state = CoreConfigState::from_snapshot(active);
    let candidate = CORE.replace("shape=\"text\"", "shape=\"pointer\"");

    let report = state
        .reload(candidate.as_bytes())
        .expect("valid cursor candidate");

    assert_eq!(report.disposition, ReloadDisposition::Applied);
    assert!(report.delta.cursor_changed);
    assert!(!report.delta.restart_required);
    assert_eq!(state.active().cursor.shape, "pointer");
    assert!(state.pending_restart().is_none());
}

#[test]
fn empty_documents_and_missing_schema_are_rejected() {
    assert!(parse_core_config(b"", ConfigGeneration::INITIAL).is_err());
    assert!(parse_wm_config(b"", ConfigGeneration::INITIAL).is_err());
    assert_eq!(FocusRingStyle::default().width, 2);
}

#[test]
fn rejects_schema_one_with_migration_guidance() {
    let error = parse_core_config(b"schema 1\n", ConfigGeneration::INITIAL)
        .expect_err("schema one must not be accepted");
    assert!(error.to_string().contains("migrate to schema 2"));
}

#[test]
fn rejects_inconsistent_disabled_chrome_width() {
    let source = r##"
schema 2
compositor {
    chrome-fallback {
        focus-ring enabled=#false width=2 color="#70b7ff"
    }
}
"##;
    assert!(matches!(
        parse_core_config(source.as_bytes(), ConfigGeneration::INITIAL),
        Err(ConfigParseError::Schema(message)) if message.contains("zero width")
    ));
}

#[test]
fn compiled_defaults_are_valid_kdl_two() {
    parse_core_config(COMPILED_CORE_CONFIG.as_bytes(), ConfigGeneration::INITIAL)
        .expect("valid compiled core config");
    parse_wm_config(COMPILED_WM_CONFIG.as_bytes(), ConfigGeneration::INITIAL)
        .expect("valid compiled WM config");
}

#[test]
fn last_known_good_survives_rejected_reload() {
    let active =
        parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).expect("valid active config");
    let digest = active.digest;
    let mut state = CoreConfigState::from_snapshot(active);

    assert!(state.reload(b"not valid {").is_err());
    assert_eq!(state.active().digest, digest);
    assert!(state.pending_restart().is_none());
}

#[test]
fn restart_candidate_does_not_partially_replace_active_snapshot() {
    let active =
        parse_core_config(CORE.as_bytes(), ConfigGeneration::INITIAL).expect("valid active config");
    let active_profile = active.namespace_profile.clone();
    let mut state = CoreConfigState::from_snapshot(active);
    let candidate = CORE.replace("classic-shared", "confined");

    let report = state
        .reload(candidate.as_bytes())
        .expect("valid restart candidate");

    assert_eq!(report.disposition, ReloadDisposition::PendingRestart);
    assert_eq!(state.active().namespace_profile, active_profile);
    assert_eq!(
        state
            .pending_restart()
            .expect("pending restart snapshot")
            .namespace_profile,
        "confined"
    );
}

#[test]
fn wm_reload_is_atomic_and_generation_ordered() {
    let active =
        parse_wm_config(WM.as_bytes(), ConfigGeneration::INITIAL).expect("valid active WM config");
    let mut state = WmConfigState::from_snapshot(active);
    let candidate = WM.replace("timeout-ms=250", "timeout-ms=400");

    let report = state
        .reload(candidate.as_bytes())
        .expect("valid WM candidate");

    assert_eq!(report.disposition, ReloadDisposition::Applied);
    assert_eq!(state.active().generation.raw(), 2);
    assert_eq!(state.active().timeout_msec, 400);
}
