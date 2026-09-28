use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn temporary_directory() -> PathBuf {
    let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sophia-desktop-profile-cli-{}-{serial}",
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

fn sophia() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sophia"))
}

#[test]
fn printed_effective_inline_commands_are_valid_source_and_stay_out_of_policy() {
    let root = temporary_directory();
    let path = root.join("commands.kdl");
    write_profile(
        &path,
        r#"schema 1
session { application "named" { exec "named-program"; }; }
shortcut {
  profile "desktop"
  bind "Super+a" { exec "inline-program" "literal-private-argv" "a space" ""; }
  bind "Super+b" { launch "named"; }
}
policy { layout "tile"; }
"#,
    );
    let option = format!("--desktop-profile={}", path.display());
    let output = sophia()
        .args(["config", "print-effective", &option])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let source = String::from_utf8(output.stdout).unwrap();
    assert!(!source.contains("__shortcut_"));
    let effective = root.join("effective.kdl");
    write_profile(&effective, &source);
    let check = sophia()
        .args([
            "config",
            "check",
            &format!("--desktop-profile={}", effective.display()),
        ])
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");
    let original = sophia_config::load_prepared_desktop_profile(
        Some(&path),
        sophia_config::ConfigGeneration::INITIAL,
    )
    .unwrap();
    let restored = sophia_config::load_prepared_desktop_profile(
        Some(&effective),
        sophia_config::ConfigGeneration::INITIAL,
    )
    .unwrap();
    assert_eq!(
        original.candidates.shortcut.bindings,
        restored.candidates.shortcut.bindings
    );
    assert_eq!(
        original.candidates.session.applications.len(),
        restored.candidates.session.applications.len()
    );
    for (before, after) in original
        .candidates
        .session
        .applications
        .iter()
        .zip(&restored.candidates.session.applications)
    {
        assert_eq!(before.name, after.name);
        assert_eq!(before.command, after.command);
    }
    let policy = sophia()
        .args(["config", "print-policy", &option])
        .output()
        .unwrap();
    assert!(policy.status.success(), "{policy:?}");
    let policy = String::from_utf8(policy.stdout).unwrap();
    for private in [
        "inline-program",
        "literal-private-argv",
        "named-program",
        "__shortcut_",
    ] {
        assert!(!policy.contains(private), "{private}");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn config_check_validates_every_typed_desktop_candidate() {
    let root = temporary_directory();
    let path = root.join("config.kdl");
    write_profile(
        &path,
        r#"schema 1
policy { layout scroller; view-count 3; }
shell { enabled #false; }
shortcut { profile migrated; }
session { terminal kitty; logout #true; }
input {
  inherit-sophia #true
  keyboard { repeat-rate 40; repeat-delay 300; numlock #true; }
  pointer { accel-profile flat; scroll-factor 1.0; }
}
output {
  inherit-sophia #true
  named DP-1 {
    mode "2560x1440@120"
    scale "auto"
    position 0 0
    enabled #true
    focus-at-startup #true
    vrr 1
  }
}
broker { enabled #false; }
"#,
    );

    let output = sophia()
        .args([
            "config",
            "check",
            &format!("--desktop-profile={}", path.display()),
        ])
        .output()
        .expect("run Sophia profile check");
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("valid domain=desktop-profile schema=1")
    );

    write_profile(
        &path,
        "schema 1\noutput { named DP-1 { enabled #true; vrr 3; } }\n",
    );
    let rejected = sophia()
        .args([
            "config",
            "check",
            &format!("--desktop-profile={}", path.display()),
        ])
        .output()
        .expect("run Sophia profile rejection");
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("output candidate"));

    fs::remove_dir_all(root).expect("remove test directory");
}

#[test]
fn desktop_profile_check_rejects_ambiguous_or_relative_selection() {
    let relative = sophia()
        .args(["config", "check", "--desktop-profile=relative.kdl"])
        .output()
        .expect("run relative profile rejection");
    assert!(!relative.status.success());
    assert!(String::from_utf8_lossy(&relative.stderr).contains("absolute path"));

    let ambiguous = sophia()
        .args([
            "config",
            "check",
            "--desktop-profile=/tmp/profile.kdl",
            "--wm",
        ])
        .output()
        .expect("run ambiguous profile rejection");
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("rejects option"));
}

#[test]
fn desktop_inspection_and_policy_export_keep_component_identity_out_of_wm_input() {
    let root = temporary_directory();
    let path = root.join("desktop.kdl");
    let included = root.join("policy.kdl");
    write_profile(&included, "policy { layout \"third-party-layout\"; }\n");
    write_profile(
        &path,
        "schema 1\ninclude \"policy.kdl\"\nshell { enabled #true; }\nsession { window-manager \"/usr/bin/test wm\"; shell-component \"metadata\" \"descriptor\" { executable \"/usr/bin/test-shell\"; }; startup; }\n",
    );
    let option = format!("--desktop-profile={}", path.display());
    let exported = sophia()
        .args(["config", "print-policy", &option])
        .output()
        .unwrap();
    assert!(exported.status.success(), "{exported:?}");
    let policy = String::from_utf8(exported.stdout).unwrap();
    assert!(policy.contains("third-party-layout"));
    assert!(!policy.contains("/usr/bin"));
    let policy_path = root.join("exported.kdl");
    write_profile(&policy_path, &policy);
    sophia_config::load_desktop_profile(
        Some(&policy_path),
        sophia_config::ConfigGeneration::INITIAL,
    )
    .unwrap();
    let inspected = sophia()
        .args(["config", "print-effective", &option])
        .output()
        .unwrap();
    assert!(inspected.status.success(), "{inspected:?}");
    assert!(String::from_utf8_lossy(&inspected.stdout).contains("/usr/bin/test wm"));
    let selected = sophia()
        .args([
            "config",
            "print-component",
            &option,
            "--component=window-manager",
        ])
        .output()
        .unwrap();
    assert!(selected.status.success());
    assert_eq!(selected.stdout, b"/usr/bin/test wm\n");
    let descriptor = sophia()
        .args([
            "config",
            "print-component",
            &option,
            "--component=descriptor",
        ])
        .output()
        .unwrap();
    assert!(descriptor.status.success());
    assert_eq!(descriptor.stdout, b"/usr/bin/test-shell\n");
    assert!(
        !sophia()
            .args([
                "config",
                "print-component",
                &option,
                "--component=shell-client"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    for operation in ["check", "print-effective", "print-policy"] {
        assert!(
            !sophia()
                .args(["config", operation, &option, "--component=window-manager"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn canonical_session_command_validates_without_entering_hardware() {
    let output = sophia()
        .args([
            "session",
            "run",
            "--validate-session-args",
            "--session-mode=normal",
            "--display=:77",
            "--native-scanout",
            "--startup-ready-timeout-ms=8000",
            "--no-config",
            "--session-app=standalone=/usr/bin/true",
            "--session-start=standalone",
            "--exit-when-startup-exits",
        ])
        .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
        .output()
        .expect("validate canonical Sophia session command");

    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("sophia_live_session_args schema=1 status=accepted")
    );
}
