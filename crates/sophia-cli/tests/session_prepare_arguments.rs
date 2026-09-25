use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sophia argument test {} {suffix}",
            std::process::id()
        ));
        fs::create_dir_all(path.join("tools/fixtures")).unwrap();
        fs::create_dir_all(path.join("state")).unwrap();
        for name in ["direct_scanout_core.kdl", "direct_scanout_desktop.kdl"] {
            fs::write(path.join("tools/fixtures").join(name), "schema 1\n").unwrap();
        }
        fs::write(path.join("desktop.kdl"), "schema 1\n").unwrap();
        Self(path)
    }

    fn compare(&self, profile: &str, settings: &[(&str, &str)], extra: &[&str]) {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut old = Command::new("bash");
        old.args(["-eu", "-c", r#"
source "$SOURCE/tools/lib/session_terminal.sh"
SESSION_PROFILE="$SOPHIA_TTY_PROFILE"
SESSION_STARTUP="${SOPHIA_SESSION_STARTUP:-terminal}"
SESSION_WATCHDOG_SECONDS="${SOPHIA_SESSION_WATCHDOG_SECONDS:-}"
TRUECOLOR_PROOF="${SOPHIA_TRUECOLOR_PROOF:-false}"
FIREFOX_M10_PROOF=false
FIREFOX_M10_RENDERING_PROOF=false
FIREFOX_M10_DIALOG_PROOF=false
FIREFOX_M10_PRIMARY_PROOF=false
FIREFOX_M10_SELECTION_PROOF=false
FIREFOX_M10_LIFECYCLE_PROOF=false
FIREFOX_M10_ANY_PROOF=false
for arg in "$@"; do
 case "$arg" in
 --firefox-m10-proof) FIREFOX_M10_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 --firefox-m10-rendering-proof) FIREFOX_M10_RENDERING_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 --firefox-m10-dialog-proof) FIREFOX_M10_DIALOG_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 --firefox-m10-primary-proof) FIREFOX_M10_PRIMARY_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 --firefox-m10-selection-proof) FIREFOX_M10_SELECTION_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 --firefox-m10-lifecycle-proof) FIREFOX_M10_LIFECYCLE_PROOF=true; FIREFOX_M10_ANY_PROOF=true ;;
 esac
done
normal_application_defaults=false
if sophia_session_uses_application_defaults "$SESSION_PROFILE" "$FIREFOX_M10_ANY_PROOF" "$TRUECOLOR_PROOF"; then
 normal_application_defaults=true
fi
DISPLAY_NAME="${SOPHIA_LIVE_SESSION_DISPLAY:-:77}"
if [[ -n "${SOPHIA_OPERATOR_INPUT_DEVICES:-}" ]]; then
 input_source_args=("--input-devices=$SOPHIA_OPERATOR_INPUT_DEVICES")
else
 input_source_args=("--input-seat=${SOPHIA_OPERATOR_INPUT_SEAT:-seat0}")
fi
terminal_kind="${SOPHIA_TERMINAL_KIND:-kitty}"
standalone_workload="${SOPHIA_STANDALONE_WORKLOAD:-vkcube}"
source "$SOURCE/crates/sophia-cli/tests/fixtures/session_arguments_before_t027.sh"
printf '%s\0' "${session_args[@]}"
"#, "oracle"]);
        old.args(extra);
        let mut new = Command::new(env!("CARGO_BIN_EXE_sophia"));
        new.args(["session", "prepare-arguments"])
            .arg(format!("--profile={profile}"))
            .arg(format!("--root={}", self.0.display()))
            .arg(format!("--state-dir={}/state", self.0.display()))
            .args([
                "--binary=/bin/true",
                "--terminal=/bin/true",
                "--browser=/bin/true",
                "--standalone=/bin/true",
                "--wm=/bin/true",
                "--firefox-profile=/private firefox profile",
            ]);
        let kind = settings
            .iter()
            .find(|(key, _)| *key == "SOPHIA_TERMINAL_KIND")
            .map_or("kitty", |(_, value)| *value);
        new.arg(format!("--terminal-kind={kind}"))
            .arg("--")
            .args(extra);
        for command in [&mut old, &mut new] {
            command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("SOURCE", &source)
                .env("ROOT_DIR", &self.0)
                .env("STATE_DIR", self.0.join("state"))
                .env("SOPHIA_TTY_PROFILE", profile)
                .env("SOPHIA_DESKTOP_PROFILE", self.0.join("desktop.kdl"))
                .env("terminal_bin", "/bin/true")
                .env("standalone_bin", "/bin/true")
                .env("hagia_browser_bin", "/bin/true")
                .env("SOPHIA_BIN", "/bin/true")
                .env("SOPHIA_HAGIA_BIN", "/bin/true")
                .env("firefox_m10_profile_dir", "/private firefox profile")
                .envs(settings.iter().copied());
        }
        // The old builder stages proof files as a side effect. The pure Rust
        // builder receives those existing paths from its adapter.
        let old = old.output().unwrap();
        let new = new.output().unwrap();
        assert_eq!(
            new.status.success(),
            old.status.success(),
            "{profile} {settings:?} old={old:?} new={new:?}"
        );
        if old.status.success() {
            let header = b"sophia_session_arguments schema=1 status=prepared\0";
            assert!(new.stdout.starts_with(header), "{new:?}");
            assert_eq!(
                &new.stdout[header.len()..],
                old.stdout,
                "{profile} {settings:?} {extra:?}"
            );
        } else {
            assert!(new.stdout.is_empty());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn live_vectors_match_retained_builder_across_profiles_and_proofs() {
    let fixture = Fixture::new();
    for profile in ["hagia", "native", "kitty", "standalone"] {
        fixture.compare(profile, &[], &[]);
        fixture.compare(
            profile,
            &[
                ("SOPHIA_ADMIT_XTEST", "1"),
                (
                    "SOPHIA_OPERATOR_INPUT_DEVICES",
                    "/dev/input/one,/dev/input/two",
                ),
            ],
            &[
                "--max-ticks=17",
                "--session-app-arg=terminal=spaces 'quotes' $(touch never)\nnewline",
            ],
        );
        fixture.compare(profile, &[("SOPHIA_ATOMIC_CURSOR", "1")], &[]);
        fixture.compare(profile, &[("SOPHIA_TERMINAL_KIND", "xterm")], &[]);
    }
    fixture.compare("hagia", &[("SOPHIA_SESSION_STARTUP", "none")], &[]);
    fixture.compare("hagia", &[("SOPHIA_TRUECOLOR_PROOF", "true")], &[]);
    for flag in [
        "--firefox-m10-proof",
        "--firefox-m10-rendering-proof",
        "--firefox-m10-dialog-proof",
        "--firefox-m10-primary-proof",
        "--firefox-m10-selection-proof",
        "--firefox-m10-lifecycle-proof",
    ] {
        fixture.compare("hagia", &[], &[flag]);
    }
    for workload in ["vkcube", "kitty", "glxgears", "xterm"] {
        fixture.compare(
            "standalone",
            &[("SOPHIA_STANDALONE_WORKLOAD", workload)],
            &[],
        );
        fixture.compare(
            "standalone",
            &[
                ("SOPHIA_STANDALONE_WORKLOAD", workload),
                ("SOPHIA_ENABLE_DIRECT_SCANOUT", "1"),
                ("SOPHIA_ATOMIC_CURSOR", "1"),
                ("SOPHIA_DIRECT_OVERLAY_PROOF", "1"),
                ("SOPHIA_DIRECT_OVERLAY_HOLD_TICKS", "50"),
            ],
            &[],
        );
    }
    fixture.compare(
        "standalone",
        &[
            ("SOPHIA_STANDALONE_FRAME_COUNT", "75"),
            ("SOPHIA_STANDALONE_WIDTH", "1280"),
            ("SOPHIA_STANDALONE_HEIGHT", "720"),
            ("SOPHIA_STANDALONE_PRESENT_MODE", "3"),
        ],
        &[],
    );
    fixture.compare("standalone", &[("SOPHIA_STANDALONE_FRAME_COUNT", "0")], &[]);
    fixture.compare("standalone", &[("SOPHIA_DIRECT_CURSOR_PROOF", "1")], &[]);
}

#[test]
fn preparation_rejects_unknown_and_duplicate_options_without_output() {
    for options in [
        vec!["--unknown=true"],
        vec!["--profile=hagia", "--profile=native"],
        vec!["--profile=wrong"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(["session", "prepare-arguments"])
            .args(options)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn adapter_preserves_argument_boundaries_and_refuses_old_binaries() {
    let fixture = Fixture::new();
    let launcher = include_str!("../../../tools/run_sophia_session.sh");
    let start = launcher.find("prepared_arguments=\"").unwrap();
    let end = launcher[start..].find("prepared_environment=").unwrap() + start;
    let script = format!(
        "{}\nprintf '%s\\0' \"${{session_args[@]}}\"",
        &launcher[start..end]
    );
    let literal = "--session-app-arg=terminal=spaces 'quotes' $(touch should-not-exist)\nnext";
    for (binary, accepted) in [
        (env!("CARGO_BIN_EXE_sophia"), true),
        ("/bin/true", false),
        ("/bin/false", false),
    ] {
        let output = Command::new("bash")
            .args(["-eu", "-c", &script, "adapter", literal])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("SOPHIA_BIN", binary)
            .env("SESSION_PROFILE", "hagia")
            .env("ROOT_DIR", &fixture.0)
            .env("STATE_DIR", fixture.0.join("state"))
            .env("SOPHIA_DESKTOP_PROFILE", fixture.0.join("desktop.kdl"))
            .env("terminal_bin", "/bin/true")
            .env("terminal_kind", "kitty")
            .env("hagia_browser_bin", "")
            .env("standalone_bin", "")
            .env("SOPHIA_HAGIA_BIN", "/bin/true")
            .env("firefox_m10_profile_dir", "")
            .current_dir(&fixture.0)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), accepted, "{output:?}");
        if accepted {
            let fields = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
            assert_eq!(fields[0], b"session");
            assert_eq!(fields[fields.len() - 2], literal.as_bytes());
            assert!(!fixture.0.join("state/session-arguments.bin").exists());
        }
        assert!(!fixture.0.join("should-not-exist").exists());
    }
}

#[test]
fn environment_matches_trace_overrides_proof_priority_and_bus_ownership() {
    let fixture = Fixture::new();
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/session_environment_before_t027.sh");
    for settings in [
        vec![],
        vec![("SOPHIA_SESSION_VERBOSE_TRACE", "true")],
        vec![
            ("SOPHIA_SESSION_VERBOSE_TRACE", "true"),
            ("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE", ""),
        ],
        vec![("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE", "continuous")],
        vec![("SOPHIA_ISOLATE_SESSION_BUS", "1")],
        vec![("DBUS_SESSION_BUS_ADDRESS", "unix:path=/private bus/socket")],
        vec![("DBUS_SESSION_BUS_ADDRESS", "unix:path=/dev/null")],
    ] {
        for proofs in [
            vec![],
            vec!["rendering"],
            vec!["promotion"],
            vec!["selection", "primary", "dialog", "rendering", "lifecycle"],
        ] {
            for path in ["/usr/bin:/bin", fixture.0.to_str().unwrap()] {
                let mut old = Command::new("/bin/bash");
                old.args(["-eu", "-c", "source \"$ORACLE\"; printf '%s\\0' \"$session_bus_mode\" \"${session_environment[@]}\""]);
                let mut new = Command::new(env!("CARGO_BIN_EXE_sophia"));
                new.args([
                    "session",
                    "prepare-environment",
                    "--tty=/dev/tty37",
                    "--firefox-probe=/private probe directory",
                    "--",
                ]);
                for name in &proofs {
                    new.arg(if *name == "promotion" {
                        "--firefox-m10-proof".to_owned()
                    } else {
                        format!("--firefox-m10-{name}-proof")
                    });
                }
                for command in [&mut old, &mut new] {
                    command
                        .env_clear()
                        .env("PATH", path)
                        .env("ORACLE", &oracle)
                        .env("tty_name", "/dev/tty37")
                        .env("firefox_m10_probe_dir", "/private probe directory")
                        .env(
                            "FIREFOX_M10_ANY_PROOF",
                            if proofs.is_empty() { "false" } else { "true" },
                        )
                        .envs(settings.iter().copied());
                    for name in ["selection", "primary", "dialog", "rendering", "lifecycle"] {
                        command.env(
                            format!("FIREFOX_M10_{}_PROOF", name.to_uppercase()),
                            if proofs.contains(&name) {
                                "true"
                            } else {
                                "false"
                            },
                        );
                    }
                }
                let old = old.output().unwrap();
                let new = new.output().unwrap();
                assert!(
                    old.status.success() && new.status.success(),
                    "old={old:?} new={new:?}"
                );
                let header = b"sophia_session_environment schema=1 status=prepared\0";
                assert!(new.stdout.starts_with(header));
                assert_eq!(
                    &new.stdout[header.len()..],
                    old.stdout,
                    "{settings:?} {proofs:?} {path}"
                );
            }
        }
    }
}
