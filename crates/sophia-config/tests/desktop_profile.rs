use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_config::{
    ConfigDigest, ConfigGeneration, ConfigIoError, DESKTOP_PROFILE_MAX_BYTES, DesktopAuthority,
    DesktopMirrorFit, DesktopOutputMode, DesktopOutputScale, DesktopOutputTransform,
    DesktopOutputVrrMode, DesktopPointerAccelProfile, DesktopProfileActivationKey,
    DesktopProfileError, DesktopSessionShortcut, DesktopShortcutBindingKind,
    DesktopShortcutModifiers, DesktopShortcutTarget, SHELL_PANEL_MAX_THICKNESS_PX, ShellGpuMode,
    desktop_profile_shell_content_enabled, desktop_profile_shell_content_input_enabled,
    desktop_profile_shell_enabled, desktop_profile_shell_gpu_mode,
    desktop_profile_shell_panel_thickness, discover_desktop_profile_source,
    load_desktop_authority_fragment, load_desktop_profile, load_prepared_desktop_profile,
    prepare_desktop_input_candidate, prepare_desktop_output_candidate,
    prepare_desktop_profile_candidates, prepare_desktop_session_candidate,
    prepare_desktop_shortcut_candidate, restage_desktop_profile, stage_desktop_profile,
    validate_desktop_profile_fragments,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn temporary_directory(name: &str) -> PathBuf {
    let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sophia-desktop-profile-{name}-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("create test directory");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .expect("make test directory private");
    path
}

fn write_profile(path: &Path, source: &str) {
    fs::write(path, source).expect("write profile");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("make profile private");
}

#[test]
fn compiled_profile_partitions_every_authority_deterministically() {
    let first = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let second = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();

    assert_eq!(first.digest, second.digest);
    assert_eq!(first.sources, vec![PathBuf::from("<compiled>")]);
    assert_eq!(first.candidates.len(), DesktopAuthority::ALL.len());
    assert!(desktop_profile_shell_enabled(&first));
    for authority in DesktopAuthority::ALL {
        let candidate = first.candidates.get(&authority).unwrap();
        assert_eq!(candidate.authority, authority);
        assert_eq!(candidate.generation, first.generation);
        assert_eq!(candidate.digest, first.digest);
        assert!(
            candidate
                .values
                .iter()
                .all(|value| value.key.starts_with(authority.name()))
        );
    }
}

#[test]
fn prepared_load_retains_one_profile_identity_and_typed_bundle() {
    let prepared = load_prepared_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();

    assert_eq!(
        prepared.activation_key().generation(),
        prepared.profile.generation
    );
    assert_eq!(prepared.activation_key().digest(), prepared.profile.digest);
    assert_eq!(
        prepared.candidates.shortcut.generation,
        prepared.profile.generation
    );
    assert_eq!(prepared.candidates.shortcut.digest, prepared.profile.digest);
    assert_eq!(
        prepared.candidates.session.generation,
        prepared.profile.generation
    );
    assert_eq!(prepared.candidates.session.digest, prepared.profile.digest);
    assert_eq!(
        prepared.candidates.input.generation,
        prepared.profile.generation
    );
    assert_eq!(prepared.candidates.input.digest, prepared.profile.digest);
    assert_eq!(
        prepared.candidates.output.generation,
        prepared.profile.generation
    );
    assert_eq!(prepared.candidates.output.digest, prepared.profile.digest);
}

#[test]
fn preparation_rejects_identity_drift_in_every_authority_candidate() {
    for authority in DesktopAuthority::ALL {
        let mut profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
        profile.candidates.get_mut(&authority).unwrap().generation = ConfigGeneration::from_raw(2);

        assert!(matches!(
            prepare_desktop_profile_candidates(&profile),
            Err(DesktopProfileError::Schema(message))
                if message.contains(authority.name()) && message.contains("identity")
        ));
    }
}

#[test]
fn preparation_rejects_digest_drift_in_every_authority_candidate() {
    for authority in DesktopAuthority::ALL {
        let mut profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
        profile.candidates.get_mut(&authority).unwrap().digest = ConfigDigest::new([0xff; 32]);

        assert!(matches!(
            prepare_desktop_profile_candidates(&profile),
            Err(DesktopProfileError::Schema(message))
                if message.contains(authority.name()) && message.contains("identity")
        ));
    }
}

#[test]
fn includes_expand_in_place_with_per_value_provenance() {
    let root = temporary_directory("include");
    let main = root.join("config.kdl");
    let policy = root.join("policy.kdl");
    write_profile(
        &main,
        "schema 1\ninclude \"policy.kdl\"\nsession { terminal \"foot\"; }\n",
    );
    write_profile(
        &policy,
        "policy { layout \"scroller\"; view-count 9; outer-gap 4; inner-gap 2; }\n",
    );

    let profile = load_desktop_profile(Some(&main), ConfigGeneration::INITIAL).unwrap();
    assert_eq!(profile.sources, vec![main.clone(), policy.clone()]);
    let policy_candidate = profile.candidates.get(&DesktopAuthority::Policy).unwrap();
    assert_eq!(policy_candidate.values.len(), 4);
    assert!(
        policy_candidate
            .values
            .iter()
            .all(|value| value.provenance.path == policy)
    );
    assert_eq!(
        profile
            .candidates
            .get(&DesktopAuthority::Session)
            .unwrap()
            .values[0]
            .provenance
            .path,
        main
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_policy_layout_cycle_is_partitioned_for_wm_validation() {
    let root = temporary_directory("layout-cycle");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        "schema 1\npolicy { layout \"grid\"; layout-cycle \"grid\" \"monocle\"; }\n",
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let policy = profile.candidates.get(&DesktopAuthority::Policy).unwrap();
    assert_eq!(policy.values.len(), 2);

    for source in [
        "schema 1\npolicy { layout \"unsupported\"; }\n",
        "schema 1\npolicy { layout-cycle \"grid\" \"grid\"; }\n",
        "schema 1\npolicy { layout-cycle \"grid\" \"unsupported\"; }\n",
    ] {
        write_profile(&profile_path, source);
        load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shortcut_candidate_prepares_typed_chords_and_authority_targets() {
    let root = temporary_directory("shortcut-candidate");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
shortcut {
  profile "daily"
  bind "Super+q" "session:close-window"
  bind "Super+1" "policy:activate-view 1"
  pointer-bind "Super+left" "policy:move"
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let shortcut = prepare_desktop_shortcut_candidate(
        profile.candidates.get(&DesktopAuthority::Shortcut).unwrap(),
    )
    .unwrap();

    assert_eq!(shortcut.generation, profile.generation);
    assert_eq!(shortcut.digest, profile.digest);
    assert_eq!(shortcut.profile, "daily");
    assert_eq!(shortcut.bindings.len(), 3);
    assert_eq!(
        shortcut.bindings[0].chord.kind,
        DesktopShortcutBindingKind::Key
    );
    assert_eq!(
        shortcut.bindings[0].chord.modifiers,
        DesktopShortcutModifiers::SUPER
    );
    assert_eq!(
        shortcut.bindings[0].target,
        DesktopShortcutTarget::Session(DesktopSessionShortcut::CloseFocused)
    );
    assert_eq!(
        shortcut.bindings[1].target,
        DesktopShortcutTarget::PolicyAction("activate-view 1".to_owned())
    );
    assert_eq!(
        shortcut.bindings[2].chord.kind,
        DesktopShortcutBindingKind::Pointer
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shortcut_candidate_rejects_ambiguous_reserved_and_cross_authority_bindings() {
    let root = temporary_directory("shortcut-rejections");
    let profile_path = root.join("config.kdl");
    for source in [
        "schema 1\nshortcut { profile \"daily\"; bind \"Super+q\" \"close-window\"; }\n",
        "schema 1\nshortcut { profile \"daily\"; bind \"Ctrl+Alt+Backspace\" \"session:logout\"; }\n",
        "schema 1\nshortcut { profile \"daily\"; pointer-bind \"Super+left\" \"session:logout\"; }\n",
        "schema 1\nshortcut { profile \"daily\"; bind \"Super+q\" \"policy:first\"; bind \"super+Q\" \"policy:second\"; }\n",
        "schema 1\nshortcut { profile \"daily\"; bind \"Hyper+q\" \"policy:first\"; }\n",
        "schema 1\nshortcut { bind \"Super+q\" \"policy:first\"; }\n",
    ] {
        write_profile(&profile_path, source);
        assert!(matches!(
            load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL),
            Err(DesktopProfileError::Schema(message)) if message.contains("shortcut candidate")
        ));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_candidate_prepares_bounded_application_selectors() {
    let root = temporary_directory("session-candidate");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
session {
  terminal "kitty"
  browser "helium"
  startup "kitty"
  logout #false
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let session = prepare_desktop_session_candidate(
        profile.candidates.get(&DesktopAuthority::Session).unwrap(),
    )
    .unwrap();

    assert_eq!(session.generation, profile.generation);
    assert_eq!(session.digest, profile.digest);
    assert_eq!(session.terminal.as_deref(), Some("kitty"));
    assert_eq!(session.browser.as_deref(), Some("helium"));
    assert_eq!(session.startup, Some(vec!["kitty".to_owned()]));
    assert_eq!(session.logout_enabled, Some(false));

    for source in [
        "schema 1\nsession { terminal \"kitty shell\"; }\n",
        "schema 1\nsession { browser; }\n",
        "schema 1\nsession { startup \"kitty\" { arg \"bad\"; }; }\n",
        "schema 1\nsession { logout \"yes\"; }\n",
    ] {
        write_profile(&profile_path, source);
        assert!(matches!(
            load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL),
            Err(DesktopProfileError::Schema(message)) if message.contains("session candidate")
        ));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn input_candidate_prepares_keyboard_and_pointer_values() {
    let root = temporary_directory("input-candidate");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
input {
  inherit-sophia #true
  keyboard {
    repeat-rate 40
    repeat-delay 300
    numlock #true
    capslock #false
    xkb { rules "evdev"; model "pc105"; layout "us"; variant ""; options ""; }
  }
  pointer {
    natural-scroll #false
    accel-profile "flat"
    accel-speed 0.0
    left-handed #false
    middle-emulation #false
    scroll-factor 1.0
  }
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let input =
        prepare_desktop_input_candidate(profile.candidates.get(&DesktopAuthority::Input).unwrap())
            .unwrap();

    assert!(input.inherit_sophia);
    let keyboard = input.keyboard.unwrap();
    assert_eq!(keyboard.repeat_rate, Some(40));
    assert_eq!(keyboard.repeat_delay_msec, Some(300));
    assert_eq!(keyboard.num_lock, Some(true));
    assert_eq!(keyboard.xkb.unwrap().layout.as_deref(), Some("us"));
    let pointer = input.pointer.unwrap();
    assert_eq!(
        pointer.accel_profile,
        Some(DesktopPointerAccelProfile::Flat)
    );
    assert_eq!(pointer.accel_speed, Some(0.0));
    assert_eq!(pointer.scroll_factor, Some(1.0));

    for source in [
        "schema 1\ninput { inherit-sophia \"yes\"; }\n",
        "schema 1\ninput { keyboard { repeat-rate 0; } }\n",
        "schema 1\ninput { keyboard { xkb { layout \"\"; } } }\n",
        "schema 1\ninput { pointer { accel-profile \"fast\"; } }\n",
        "schema 1\ninput { pointer { accel-speed 2.0; } }\n",
        "schema 1\ninput { pointer { scroll-factor 0.0; } }\n",
    ] {
        write_profile(&profile_path, source);
        assert!(matches!(
            load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL),
            Err(DesktopProfileError::Schema(message)) if message.contains("input candidate")
        ));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn input_candidate_carries_a_cursor_the_profile_may_state_in_part() {
    // Every cursor field is optional because the profile overrides the core
    // config rather than replacing it. A desktop that names only a theme has
    // said nothing about size, and must not be read as having asked for the
    // default one -- that is the difference between one file describing a
    // whole desktop and one file quietly overwriting another.
    let root = temporary_directory("input-cursor-candidate");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
input {
  cursor {
    theme "WhiteSur-cursors"
    size 24
    shake-to-find #true
  }
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let input =
        prepare_desktop_input_candidate(profile.candidates.get(&DesktopAuthority::Input).unwrap())
            .unwrap();
    let cursor = input.cursor.unwrap();
    assert_eq!(cursor.theme.as_deref(), Some("WhiteSur-cursors"));
    assert_eq!(cursor.size, Some(24));
    assert_eq!(cursor.shake_to_find, Some(true));

    write_profile(
        &profile_path,
        "schema 1\ninput { cursor { theme \"Adwaita\"; } }\n",
    );
    let partial = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let partial =
        prepare_desktop_input_candidate(partial.candidates.get(&DesktopAuthority::Input).unwrap())
            .unwrap();
    let partial = partial.cursor.unwrap();
    assert_eq!(partial.theme.as_deref(), Some("Adwaita"));
    assert_eq!(partial.size, None);
    assert_eq!(partial.shake_to_find, None);

    // The migration writes string values bare, as it does for every other
    // migrated string, so the form it emits has to be the form this accepts.
    write_profile(
        &profile_path,
        "schema 1\ninput { cursor { theme WhiteSur-cursors; size 24; } }\n",
    );
    let migrated = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let migrated =
        prepare_desktop_input_candidate(migrated.candidates.get(&DesktopAuthority::Input).unwrap())
            .unwrap();
    let migrated = migrated.cursor.unwrap();
    assert_eq!(migrated.theme.as_deref(), Some("WhiteSur-cursors"));
    assert_eq!(migrated.size, Some(24));

    // A profile with no cursor block has not overridden anything, which is
    // what leaves the core config in charge for a session that has one.
    write_profile(&profile_path, "schema 1\ninput { inherit-sophia #true }\n");
    let absent = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let absent =
        prepare_desktop_input_candidate(absent.candidates.get(&DesktopAuthority::Input).unwrap())
            .unwrap();
    assert!(absent.cursor.is_none());

    for source in [
        // Beyond the size the renderer will raster.
        "schema 1\ninput { cursor { size 129; } }\n",
        "schema 1\ninput { cursor { size 0; } }\n",
        // A theme name becomes a directory under an icon path.
        "schema 1\ninput { cursor { theme \"../etc\"; } }\n",
        "schema 1\ninput { cursor { theme \"\"; } }\n",
        // Stated twice, so which one was meant is unknowable.
        "schema 1\ninput { cursor { theme \"a\"; theme \"b\"; } }\n",
        // The shape stays core-only, so naming it here is a mistake worth
        // reporting rather than a value to ignore.
        "schema 1\ninput { cursor { shape \"left_ptr\"; } }\n",
    ] {
        write_profile(&profile_path, source);
        assert!(
            matches!(
                load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL),
                Err(DesktopProfileError::Schema(message)) if message.contains("input candidate")
            ),
            "{source}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_mirror_group_carries_its_fit_policy() {
    // Named rather than inferred: an operator who would rather crop than see
    // black bars says so. An unknown name is refused, because silently defaulting
    // would put the wrong thing on a screen and give nothing to search for.
    let root = temporary_directory("output-mirror-fit");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
output {
  named "DP-1" {
    mode "1920x1080@60"
    mirror "DP-2"
    mirror-fit "cover"
  }
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let output = prepare_desktop_output_candidate(
        profile.candidates.get(&DesktopAuthority::Output).unwrap(),
    )
    .unwrap();
    assert_eq!(output.named[0].mirror, vec!["DP-2".to_owned()]);
    assert_eq!(output.named[0].mirror_fit, Some(DesktopMirrorFit::Cover));

    // Omitted means the default rather than a third state.
    write_profile(
        &profile_path,
        r#"schema 1
output {
  named "DP-1" { mode "1920x1080@60" ; mirror "DP-2" }
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let output = prepare_desktop_output_candidate(
        profile.candidates.get(&DesktopAuthority::Output).unwrap(),
    )
    .unwrap();
    assert_eq!(output.named[0].mirror_fit, None);

    write_profile(
        &profile_path,
        r#"schema 1
output {
  named "DP-1" { mirror "DP-2" ; mirror-fit "stretch" }
}
"#,
    );
    assert!(
        load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).is_err(),
        "an unknown fit is refused rather than defaulted"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn output_candidate_prepares_named_outputs_without_touching_hardware() {
    let root = temporary_directory("output-candidate");
    let profile_path = root.join("config.kdl");
    write_profile(
        &profile_path,
        r#"schema 1
output {
  inherit-sophia #true
  named "DP-1" {
    mode "2560x1440@119.999"
    scale "auto"
    position 0 0
    transform "normal"
    enabled #true
    focus-at-startup #true
    vrr 1
  }
  named "DP-2" {
    mode "preferred"
    scale 1.25
    position 2560 0
    enabled #true
    vrr 0
  }
}
"#,
    );
    let profile = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL).unwrap();
    let output = prepare_desktop_output_candidate(
        profile.candidates.get(&DesktopAuthority::Output).unwrap(),
    )
    .unwrap();

    assert!(output.inherit_sophia);
    assert_eq!(output.named.len(), 2);
    assert_eq!(output.named[0].connector, "DP-1");
    assert_eq!(
        output.named[0].mode,
        Some(DesktopOutputMode::Exact {
            width: 2560,
            height: 1440,
            refresh_millihz: 119_999,
        })
    );
    assert_eq!(output.named[0].scale, Some(DesktopOutputScale::Automatic));
    assert_eq!(
        output.named[0].transform,
        Some(DesktopOutputTransform::Normal)
    );
    assert_eq!(output.named[0].vrr, Some(DesktopOutputVrrMode::Automatic));
    assert_eq!(
        output.named[1].scale,
        Some(DesktopOutputScale::FixedMilli(1_250))
    );
    assert!(matches!(
        prepare_desktop_output_candidate(
            profile.candidates.get(&DesktopAuthority::Input).unwrap()
        ),
        Err(DesktopProfileError::Schema(message)) if message.contains("authority boundary")
    ));

    for source in [
        "schema 1\noutput { inherit-sophia #false; }\n",
        "schema 1\noutput { named \"../DP-1\" { enabled #true; } }\n",
        "schema 1\noutput { named \"DP-1\" {} }\n",
        "schema 1\noutput { named \"DP-1\" { mode \"2560x1440\"; } }\n",
        "schema 1\noutput { named \"DP-1\" { scale 1.0001; } }\n",
        "schema 1\noutput { named \"DP-1\" { position 0 \"left\"; } }\n",
        "schema 1\noutput { named \"DP-1\" { transform \"diagonal\"; } }\n",
        "schema 1\noutput { named \"DP-1\" { vrr 3; } }\n",
        "schema 1\noutput { named \"DP-1\" { enabled #true; enabled #false; } }\n",
        "schema 1\noutput { named \"DP-1\" { enabled #true; }; named \"DP-1\" { enabled #false; } }\n",
        "schema 1\noutput { named \"DP-1\" { focus-at-startup #true; }; named \"DP-2\" { focus-at-startup #true; } }\n",
    ] {
        write_profile(&profile_path, source);
        let result = load_desktop_profile(Some(&profile_path), ConfigGeneration::INITIAL);
        assert!(
            matches!(result, Err(DesktopProfileError::Schema(_))),
            "source unexpectedly passed a different path: {source:?}: {result:?}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_cycles_duplicates_unsupported_and_reserved_controls() {
    let root = temporary_directory("rejections");
    let main = root.join("config.kdl");
    let child = root.join("child.kdl");
    write_profile(&main, "schema 1\ninclude \"child.kdl\"\n");
    write_profile(&child, "include \"config.kdl\"\n");
    assert!(matches!(
        load_desktop_profile(Some(&main), ConfigGeneration::INITIAL),
        Err(DesktopProfileError::Schema(message)) if message.contains("cycle")
    ));

    for source in [
        "schema 1\nshell { enabled #true; enabled #false; }\n",
        "schema 1\nshell { unknown 1; }\n",
        "schema 1\npolicy { max-surfaces 2; }\n",
        "schema 1\nshell { enabled \"yes\"; }\n",
    ] {
        write_profile(&main, source);
        assert!(matches!(
            load_desktop_profile(Some(&main), ConfigGeneration::INITIAL),
            Err(DesktopProfileError::Schema(_))
        ));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_unsafe_sources_symlinks_depth_and_aggregate_size() {
    let root = temporary_directory("safety");
    let main = root.join("config.kdl");
    write_profile(&main, "schema 1\npolicy { layout \"scroller\"; }\n");
    fs::set_permissions(&main, fs::Permissions::from_mode(0o622)).unwrap();
    assert_eq!(
        load_desktop_profile(Some(&main), ConfigGeneration::INITIAL),
        Err(DesktopProfileError::Io(ConfigIoError::UnsafeMode))
    );
    fs::set_permissions(&main, fs::Permissions::from_mode(0o600)).unwrap();
    let link = root.join("linked.kdl");
    symlink(&main, &link).unwrap();
    assert_eq!(
        load_desktop_profile(Some(&link), ConfigGeneration::INITIAL),
        Err(DesktopProfileError::Io(ConfigIoError::NotRegularFile))
    );

    for index in 0..=11 {
        let next = if index == 11 {
            "policy { layout \"scroller\"; }\n".to_owned()
        } else {
            format!("include \"depth-{}.kdl\"\n", index + 1)
        };
        write_profile(&root.join(format!("depth-{index}.kdl")), &next);
    }
    assert!(matches!(
        load_desktop_profile(
            Some(&root.join("depth-0.kdl")),
            ConfigGeneration::INITIAL
        ),
        Err(DesktopProfileError::Limit(message)) if message.contains("depth")
    ));

    let large = root.join("large.kdl");
    let mut large_source = "policy { layout \"scroller\"; }\n".to_owned();
    large_source.push_str(&" ".repeat(DESKTOP_PROFILE_MAX_BYTES - large_source.len()));
    write_profile(&large, &large_source);
    write_profile(&main, "schema 1\ninclude \"large.kdl\"\n");
    assert!(matches!(
        load_desktop_profile(Some(&main), ConfigGeneration::INITIAL),
        Err(DesktopProfileError::Limit(message)) if message.contains("aggregate")
    ));

    let mut includes = "schema 1\n".to_owned();
    for index in 0..64 {
        let child = root.join(format!("part-{index}.kdl"));
        write_profile(&child, "session {}\n");
        includes.push_str(&format!("include \"part-{index}.kdl\"\n"));
    }
    write_profile(&main, &includes);
    assert!(matches!(
        load_desktop_profile(Some(&main), ConfigGeneration::INITIAL),
        Err(DesktopProfileError::Limit(message)) if message.contains("64 files")
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restaging_preserves_fragments_owned_by_the_previous_generation() {
    let root = temporary_directory("restage");
    let first = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let staged = stage_desktop_profile(&first, &root).unwrap();
    let paths_before = DesktopAuthority::ALL
        .into_iter()
        .map(|authority| staged.path(authority).to_path_buf())
        .collect::<Vec<_>>();

    let second = load_desktop_profile(None, ConfigGeneration::from_raw(7)).unwrap();
    let restaged = restage_desktop_profile(&second, &staged).unwrap();

    for (index, authority) in DesktopAuthority::ALL.into_iter().enumerate() {
        let path = restaged.path(authority);
        assert_ne!(
            path, paths_before[index],
            "each generation must own distinct immutable fragment paths"
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600,
            "a staged fragment stays owner-only"
        );
        let source = fs::read_to_string(path).unwrap();
        assert!(source.contains("profile-generation 7"));
        assert!(source.contains(&format!("profile-digest \"{}\"", second.digest)));
        assert!(
            fs::read_to_string(&paths_before[index])
                .unwrap()
                .contains("profile-generation 1")
        );
    }
    assert_eq!(restaged.generation, ConfigGeneration::from_raw(7));

    drop(staged);
    for (index, authority) in DesktopAuthority::ALL.into_iter().enumerate() {
        assert!(!paths_before[index].exists());
        assert!(restaged.path(authority).exists());
    }
    drop(restaged);
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn stages_isolated_owner_only_authority_fragments() {
    let root = temporary_directory("stage");
    let profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let fragments = stage_desktop_profile(&profile, &root).unwrap();

    for authority in DesktopAuthority::ALL {
        let path = fragments.path(authority);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let source = fs::read_to_string(path).unwrap();
        assert!(source.contains(&format!("profile-generation {}", profile.generation.raw())));
        assert!(source.contains(&format!("profile-digest \"{}\"", profile.digest)));
        for other in DesktopAuthority::ALL {
            if other != authority {
                assert!(
                    !source
                        .lines()
                        .any(|line| line == format!("{} {{", other.name()))
                );
            }
        }
    }
    drop(fragments);
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn every_staged_fragment_round_trips_only_its_exact_authority_and_key() {
    let root = temporary_directory("fragment-round-trip");
    let profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let expected_key = DesktopProfileActivationKey::from(&profile);
    let fragments = stage_desktop_profile(&profile, &root).unwrap();
    validate_desktop_profile_fragments(&fragments, expected_key).unwrap();
    assert!(matches!(
        validate_desktop_profile_fragments(
            &fragments,
            DesktopProfileActivationKey::new(
                ConfigGeneration::from_raw(2),
                expected_key.digest(),
            ),
        ),
        Err(DesktopProfileError::Schema(message)) if message.contains("set identity")
    ));

    for authority in DesktopAuthority::ALL {
        let loaded =
            load_desktop_authority_fragment(fragments.path(authority), authority, expected_key)
                .unwrap();
        let original = profile.candidates.get(&authority).unwrap();
        assert_eq!(loaded.authority, authority);
        assert_eq!(loaded.generation, profile.generation);
        assert_eq!(loaded.digest, profile.digest);
        assert_eq!(
            loaded
                .values
                .iter()
                .map(|value| (&value.key, &value.encoded))
                .collect::<Vec<_>>(),
            original
                .values
                .iter()
                .map(|value| (&value.key, &value.encoded))
                .collect::<Vec<_>>()
        );
    }
    drop(fragments);
    fs::remove_dir(root).unwrap();
}

#[test]
fn fragment_admission_rejects_cross_authority_and_identity_mismatch() {
    let root = temporary_directory("fragment-identity");
    let profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let fragments = stage_desktop_profile(&profile, &root).unwrap();
    let policy = fragments.path(DesktopAuthority::Policy);

    assert!(matches!(
        load_desktop_authority_fragment(
            policy,
            DesktopAuthority::Session,
            DesktopProfileActivationKey::from(&profile),
        ),
        Err(DesktopProfileError::Schema(message)) if message.contains("authority boundary")
    ));
    assert!(matches!(
        load_desktop_authority_fragment(
            policy,
            DesktopAuthority::Policy,
            DesktopProfileActivationKey::new(ConfigGeneration::from_raw(2), profile.digest),
        ),
        Err(DesktopProfileError::Schema(message)) if message.contains("activation key")
    ));
    assert!(matches!(
        load_desktop_authority_fragment(
            policy,
            DesktopAuthority::Policy,
            DesktopProfileActivationKey::new(profile.generation, ConfigDigest::new([0xff; 32])),
        ),
        Err(DesktopProfileError::Schema(message)) if message.contains("activation key")
    ));
    drop(fragments);
    fs::remove_dir(root).unwrap();
}

#[test]
fn fragment_admission_reuses_owner_safe_file_constraints() {
    let root = temporary_directory("fragment-file-safety");
    let profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    let fragments = stage_desktop_profile(&profile, &root).unwrap();
    let key = DesktopProfileActivationKey::from(&profile);
    let policy = fragments.path(DesktopAuthority::Policy);
    fs::set_permissions(policy, fs::Permissions::from_mode(0o620)).unwrap();
    assert!(matches!(
        load_desktop_authority_fragment(policy, DesktopAuthority::Policy, key),
        Err(DesktopProfileError::Io(ConfigIoError::UnsafeMode))
    ));
    fs::set_permissions(policy, fs::Permissions::from_mode(0o600)).unwrap();

    let link = root.join("policy-link.kdl");
    symlink(policy, &link).unwrap();
    assert!(matches!(
        load_desktop_authority_fragment(&link, DesktopAuthority::Policy, key),
        Err(DesktopProfileError::Io(ConfigIoError::NotRegularFile))
    ));
    fs::remove_file(link).unwrap();
    drop(fragments);
    fs::remove_dir(root).unwrap();
}

#[test]
fn staging_revalidates_a_mutated_shortcut_candidate() {
    let root = temporary_directory("stage-shortcut-revalidation");
    let mut profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    profile
        .candidates
        .get_mut(&DesktopAuthority::Shortcut)
        .unwrap()
        .values[0]
        .encoded = "bind \"Super+q\" \"close-window\"".to_owned();

    assert!(matches!(
        stage_desktop_profile(&profile, &root),
        Err(DesktopProfileError::Schema(message)) if message.contains("shortcut candidate")
    ));
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn staging_revalidates_a_mutated_output_candidate() {
    let root = temporary_directory("stage-output-revalidation");
    let mut profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();
    profile
        .candidates
        .get_mut(&DesktopAuthority::Output)
        .unwrap()
        .values[0]
        .encoded = "named \"DP-1\" { mode \"unbounded\"; }".to_owned();

    assert!(matches!(
        stage_desktop_profile(&profile, &root),
        Err(DesktopProfileError::Schema(message)) if message.contains("output candidate")
    ));
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(root).unwrap();
}

#[test]
fn desktop_profile_discovery_prefers_explicit_then_xdg() {
    let root = temporary_directory("discovery");
    let user = root.join("hagia/config.kdl");
    fs::create_dir(user.parent().unwrap()).unwrap();
    write_profile(&user, "schema 1\n");
    let explicit = Path::new("/explicit/profile.kdl");

    assert_eq!(
        discover_desktop_profile_source(Some(explicit), Some(&root)).as_deref(),
        Some(explicit)
    );
    assert_eq!(
        discover_desktop_profile_source(None, Some(&root)).as_deref(),
        Some(user.as_path())
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_compiled_profile_keeps_the_switcher_out_of_the_work_area() {
    let profile = load_desktop_profile(None, ConfigGeneration::INITIAL).unwrap();

    assert!(desktop_profile_shell_enabled(&profile));
    assert_eq!(desktop_profile_shell_panel_thickness(&profile), None);
}

#[test]
fn a_profile_without_a_panel_reserves_nothing() {
    let root = temporary_directory("shell-panel-absent");
    let path = root.join("config.kdl");
    write_profile(&path, "schema 1\nshell { enabled #true; }\n");

    let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();

    // Every profile written before the key existed says this, and it has to
    // keep meaning what it meant: an enabled shell that claims no work area.
    assert!(desktop_profile_shell_enabled(&profile));
    assert_eq!(desktop_profile_shell_panel_thickness(&profile), None);
}

#[test]
fn a_zero_panel_is_not_a_claim() {
    let root = temporary_directory("shell-panel-zero");
    let path = root.join("config.kdl");
    write_profile(&path, "schema 1\nshell { enabled #true; panel 0; }\n");

    let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();

    // Zero is admitted by validation and means no reservation, rather than a
    // strip of no height that the coordinator would have to special-case.
    assert_eq!(desktop_profile_shell_panel_thickness(&profile), None);
}

#[test]
fn a_panel_beyond_the_reservation_maximum_is_refused_at_validation() {
    let root = temporary_directory("shell-panel-oversized");
    let path = root.join("config.kdl");
    write_profile(
        &path,
        &format!(
            "schema 1\nshell {{ enabled #true; panel {}; }}\n",
            u32::from(SHELL_PANEL_MAX_THICKNESS_PX) + 1
        ),
    );

    // Refused when the profile is read, not when the shell first claims: a
    // session that cannot honour its own configuration should not start.
    assert!(load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
}

#[test]
fn a_panel_that_is_not_one_integer_is_refused() {
    for source in [
        "schema 1\nshell { enabled #true; panel; }\n",
        "schema 1\nshell { enabled #true; panel \"28\"; }\n",
        "schema 1\nshell { enabled #true; panel 28 32; }\n",
        "schema 1\nshell { enabled #true; panel -1; }\n",
    ] {
        let root = temporary_directory("shell-panel-shape");
        let path = root.join("config.kdl");
        write_profile(&path, source);
        assert!(
            load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err(),
            "accepted a malformed panel: {source}"
        );
    }
}

#[test]
fn shell_content_and_direct_gpu_permission_are_explicit_and_independent() {
    let root = temporary_directory("shell-content-profile");
    let path = root.join("config.kdl");
    write_profile(
        &path,
        "schema 1\nshell { enabled #true; content #true; content-input #true; panel 32; gpu \"direct\"; }\n",
    );
    let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();

    assert!(desktop_profile_shell_content_enabled(&profile));
    assert!(desktop_profile_shell_content_input_enabled(&profile));
    assert_eq!(
        desktop_profile_shell_gpu_mode(&profile),
        ShellGpuMode::Direct
    );
}

#[test]
fn old_and_descriptor_only_profiles_do_not_gain_content_permission() {
    for source in [
        "schema 1\nshell { enabled #true; }\n",
        "schema 1\nshell { enabled #true; panel 32; }\n",
        "schema 1\nshell { enabled #true; content #false; }\n",
    ] {
        let root = temporary_directory("shell-content-denied");
        let path = root.join("config.kdl");
        write_profile(&path, source);
        let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
        assert!(!desktop_profile_shell_content_enabled(&profile));
        assert!(!desktop_profile_shell_content_input_enabled(&profile));
        assert_eq!(
            desktop_profile_shell_gpu_mode(&profile),
            ShellGpuMode::Denied
        );
    }
}

#[test]
fn shell_content_profile_fields_are_exact() {
    for source in [
        "schema 1\nshell { content; }\n",
        "schema 1\nshell { content 1; }\n",
        "schema 1\nshell { content #true #false; }\n",
        "schema 1\nshell { content-input; }\n",
        "schema 1\nshell { content-input 1; }\n",
        "schema 1\nshell { content-input #true #false; }\n",
        "schema 1\nshell { gpu; }\n",
        "schema 1\nshell { gpu 1; }\n",
        "schema 1\nshell { gpu \"direct\" \"denied\"; }\n",
        "schema 1\nshell { gpu \"automatic\"; }\n",
    ] {
        let root = temporary_directory("shell-content-shape");
        let path = root.join("config.kdl");
        write_profile(&path, source);
        assert!(
            load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err(),
            "accepted malformed shell content authority: {source}"
        );
    }
}

#[test]
fn retired_gpu_memory_limit_has_an_actionable_migration_error() {
    let root = temporary_directory("shell-retired-gpu-limit");
    let path = root.join("config.kdl");
    write_profile(&path, "schema 1\nshell { gpu-memory-bytes 268435456; }\n");
    let error = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap_err();
    assert!(error.to_string().contains("use gpu \"direct\""));
}

#[test]
fn direct_gpu_permission_does_not_imply_content_permission() {
    let root = temporary_directory("shell-gpu-independent");
    let path = root.join("config.kdl");
    write_profile(
        &path,
        "schema 1\nshell { enabled #true; content #false; gpu \"direct\"; }\n",
    );
    let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    assert!(!desktop_profile_shell_content_enabled(&profile));
    assert_eq!(
        desktop_profile_shell_gpu_mode(&profile),
        ShellGpuMode::Direct
    );
}

#[test]
fn hagia_native_tree_layouts_and_current_numeric_settings_are_admitted() {
    let root = temporary_directory("hagia-trees");
    let path = root.join("config.kdl");
    for layout in ["frame-tree", "notion", "i3", "split-tree"] {
        write_profile(
            &path,
            &format!(
                "schema 1\npolicy {{ layout \"{layout}\"; master-count 1; master-ratio 50; gap-step 2; }}\n"
            ),
        );
        load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    }
    write_profile(
        &path,
        "schema 1\npolicy {layout-cycle \"i3\" \"split-tree\";}\n",
    );
    load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wm_policy_records_round_trip_without_engine_semantics_or_deduplication() {
    let root = temporary_directory("delegated-policy");
    let path = root.join("config.kdl");
    let included = root.join("views.kdl");
    write_profile(
        &included,
        "policy { view-name 1 \"code\"; view-name 2 \"web\"; }\n",
    );
    write_profile(
        &path,
        r#"schema 1
include "views.kdl"
policy {
    layout "dwindle"
    scratchpad-size 70 60
    floating-size 0 60
    preset-column-widths { proportion 0.33; proportion 0.5; proportion 0.67; }
    view-layout 1 "notion"
    view-layout 2 "split-tree"
    future-wm-setting "opaque" value=42
    named 7
    outer-gap -1
    outer-gap 513
}
"#,
    );
    let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    let policy = &profile.candidates[&DesktopAuthority::Policy];
    assert_eq!(policy.values.len(), 12);
    assert_eq!(policy.values[0].key, "policy.view-name");
    assert_eq!(policy.values[1].key, "policy.view-name");
    assert_eq!(policy.values[0].provenance.path, included);
    assert_eq!(policy.values[2].provenance.path, path);
    let stage = root.join("stage");
    fs::create_dir(&stage).unwrap();
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
    let fragments = stage_desktop_profile(&profile, &stage).unwrap();
    let key = DesktopProfileActivationKey::from(&profile);
    validate_desktop_profile_fragments(&fragments, key).unwrap();
    let restored = load_desktop_authority_fragment(
        fragments.path(DesktopAuthority::Policy),
        DesktopAuthority::Policy,
        key,
    )
    .unwrap();
    assert_eq!(restored.generation, policy.generation);
    assert_eq!(restored.digest, policy.digest);
    assert_eq!(
        restored
            .values
            .iter()
            .map(|v| (&v.key, &v.encoded))
            .collect::<Vec<_>>(),
        policy
            .values
            .iter()
            .map(|v| (&v.key, &v.encoded))
            .collect::<Vec<_>>()
    );
    assert!(
        restored
            .values
            .iter()
            .all(|v| v.provenance.path == fragments.path(DesktopAuthority::Policy))
    );
    // Staging rewrites provenance to its private file, never the payload order.
    assert!(
        load_desktop_authority_fragment(
            fragments.path(DesktopAuthority::Policy),
            DesktopAuthority::Shell,
            key,
        )
        .is_err()
    );
    drop(fragments);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opaque_policy_still_respects_envelope_limits_and_reserved_controls() {
    let root = temporary_directory("delegated-policy-limits");
    let path = root.join("config.kdl");
    for setting in [
        "(typed)future 1".to_owned(),
        format!("future {}", vec!["1"; 33].join(" ")),
        "renderer \"gpu\"".to_owned(),
        "scanout #true".to_owned(),
        "emergency-chord \"Super+x\"".to_owned(),
    ] {
        write_profile(&path, &format!("schema 1\npolicy {{ {setting}; }}\n"));
        assert!(load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_startup_list_is_ordered_bounded_and_unique() {
    let root = temporary_directory("startup-list");
    let path = root.join("config.kdl");
    write_profile(
        &path,
        "schema 1\nsession { startup \"terminal\" \"panel\"; }\n",
    );
    let profile = load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    assert_eq!(
        profile.candidates.session.startup,
        Some(vec!["terminal".to_owned(), "panel".to_owned()])
    );
    write_profile(&path, "schema 1\nsession { startup; }\n");
    assert_eq!(
        load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL)
            .unwrap()
            .candidates
            .session
            .startup,
        Some(Vec::new())
    );
    for value in [
        "\"panel\" \"panel\"",
        "1",
        "name=\"panel\"",
        "(tag)\"panel\"",
    ] {
        write_profile(
            &path,
            &format!("schema 1\nsession {{ startup {value}; }}\n"),
        );
        assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    }
    let names = (0..33)
        .map(|id| format!("\"app{id}\""))
        .collect::<Vec<_>>()
        .join(" ");
    write_profile(
        &path,
        &format!("schema 1\nsession {{ startup {names}; }}\n"),
    );
    assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn desktop_components_are_private_to_session_and_validate_paths() {
    let root = temporary_directory("composition");
    let path = root.join("desktop.kdl");
    write_profile(
        &path,
        "schema 1\nshell { enabled #true; }\nsession { window-manager \"/usr/bin/example-wm\" \"--quiet\"; shell-client \"/usr/bin/example-shell\"; shell-config \"/home/user/private shell.kdl\"; }\npolicy { layout \"custom\"; }\n",
    );
    let prepared = load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    let components = &prepared.candidates.session.components;
    let wm = components.window_manager.as_ref().unwrap();
    assert_eq!(wm.executable, Path::new("/usr/bin/example-wm"));
    assert_eq!(wm.arguments, ["--quiet"]);
    assert_eq!(
        components.shell_client.as_deref(),
        Some(Path::new("/usr/bin/example-shell"))
    );
    fs::create_dir(root.join("staged")).unwrap();
    fs::set_permissions(root.join("staged"), fs::Permissions::from_mode(0o700)).unwrap();
    let fragments = stage_desktop_profile(&prepared.profile, &root.join("staged")).unwrap();
    // Inspect all emitted files: executable identity may appear only in the
    // session authority fragment, never in what the blind policy client reads.
    for authority in DesktopAuthority::ALL {
        let contents = fs::read_to_string(fragments.path(authority)).unwrap();
        assert_eq!(
            contents.contains("/usr/bin/example-wm"),
            authority == DesktopAuthority::Session
        );
        assert_eq!(
            contents.contains("private shell.kdl"),
            authority == DesktopAuthority::Session
        );
    }
    for entry in [
        "window-manager \"relative\"",
        "window-manager \"/bin/wm\" command=\"oops\"",
        "window-manager (path)\"/bin/wm\"",
        "window-manager \"/bin/wm\" { arg \"oops\"; }",
        "shell-client \"/bin/shell\" \"extra\"",
        "shell-config \"~/shell.kdl\"",
        "shell-client \"/bin/a\"; shell-client \"/bin/b\"",
        "shell-client \"/bin/a\\nother\"",
    ] {
        write_profile(
            &path,
            &format!("schema 1\nshell {{ enabled #true; }}\nsession {{ {entry}; }}\n"),
        );
        assert!(
            load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err(),
            "{entry}"
        );
    }
    write_profile(
        &path,
        "schema 1\nshell { enabled #false; }\nsession { shell-client \"/bin/shell\"; }\n",
    );
    assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sophia_desktop_discovery_prefers_user_composition_without_merging_legacy() {
    let root = temporary_directory("composition-discovery");
    fs::create_dir(root.join("hagia")).unwrap();
    fs::create_dir(root.join("sophia")).unwrap();
    let legacy = root.join("hagia/config.kdl");
    let desktop = root.join("sophia/desktop.kdl");
    write_profile(&legacy, "schema 1\n");
    assert_eq!(
        discover_desktop_profile_source(None, Some(&root)),
        Some(legacy)
    );
    write_profile(&desktop, "invalid");
    assert_eq!(
        discover_desktop_profile_source(None, Some(&root)),
        Some(desktop.clone())
    );
    assert!(load_desktop_profile(Some(&desktop), ConfigGeneration::INITIAL).is_err());
    let explicit = root.join("explicit.kdl");
    assert_eq!(
        discover_desktop_profile_source(Some(&explicit), Some(&root)),
        Some(explicit)
    );
    fs::remove_file(&desktop).unwrap();
    symlink(root.join("missing"), &desktop).unwrap();
    assert_eq!(
        discover_desktop_profile_source(None, Some(&root)),
        Some(desktop)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn help_aliases_share_a_physical_chord_and_display_metadata_is_bounded() {
    let root = temporary_directory("shortcut-help");
    let path = root.join("config.kdl");
    let mut chords = Vec::new();
    for chord in [
        "Super+?",
        "Super+Shift+/",
        "Super+Shift+slash",
        "Super+Question",
    ] {
        write_profile(
            &path,
            &format!(
                "schema 1\nshortcut {{ profile \"help\"; bind \"{chord}\" \"session:shortcut-help\" label=\"Keyboard shortcuts\" group=\"Session\"; }}\n"
            ),
        );
        let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
        let candidate =
            prepare_desktop_shortcut_candidate(&profile.candidates[&DesktopAuthority::Shortcut])
                .unwrap();
        assert_eq!(
            candidate.bindings[0].label.as_deref(),
            Some("Keyboard shortcuts")
        );
        chords.push(candidate.bindings[0].chord.clone());
    }
    assert!(chords.windows(2).all(|w| w[0] == w[1]));
    for property in [
        "label=1".to_owned(),
        "group=\"\"".to_owned(),
        format!("label=\"{}\"", "a".repeat(129)),
        "unknown=\"x\"".to_owned(),
    ] {
        write_profile(
            &path,
            &format!(
                "schema 1\nshortcut {{ profile \"help\"; bind \"Super+?\" \"session:shortcut-help\" {property}; }}\n"
            ),
        );
        assert!(load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    }
    write_profile(
        &path,
        "schema 1\nshortcut { profile \"help\"; bind \"Super+?\" \"session:shortcut-help\"; bind \"Super+Shift+/\" \"session:shortcut-help\"; }\n",
    );
    assert!(load_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    fs::remove_dir_all(root).unwrap();
}
