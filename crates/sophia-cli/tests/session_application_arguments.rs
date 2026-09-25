//! Exercise production fallback selection and argument preparation, excluding
//! all TTY, service, guard and graphics operations.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: PathBuf,
    terminal: PathBuf,
    browser: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("sophia-fallback-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let terminal = root.join("custom terminal $literal;not-a-command");
        let browser = root.join("custom browser [literal]");
        for path in [&terminal, &browser] {
            fs::write(path, "#!/bin/sh\nexit 91\n").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(root.join("desktop.kdl"), "schema 1\n").unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        for name in ["mktemp", "timeout", "rm", "chmod"] {
            std::os::unix::fs::symlink(format!("/usr/bin/{name}"), root.join("bin").join(name))
                .unwrap();
        }
        Self {
            root,
            terminal,
            browser,
        }
    }

    fn assemble(&self, changes: &[(&str, Option<&str>)], extra: &[&str]) -> Output {
        let source = include_str!("../../../tools/run_sophia_session.sh");
        let block = |start: &str, end: &str| {
            assert_eq!(source.matches(start).count(), 1);
            let a = source.find(start).unwrap();
            let b = source[a..].find(end).unwrap() + a;
            &source[a..b]
        };
        let script = format!(
            "set -euo pipefail\nsource \"$ROOT_DIR/tools/lib/session_preparation.sh\"\n{}\n{}\nprintf '%s\\0' \"${{session_args[@]}}\"",
            block(
                "sophia_load_preparation 'sophia_session_inputs",
                "lifecycle_phase complete preflight"
            ),
            block("prepared_arguments=\"", "prepared_environment=\"")
        );
        let mut command = Command::new("/bin/bash");
        command
            .args(["-c", &script, "fallback"])
            .args(extra)
            .env_clear()
            .env("PATH", self.root.join("bin"))
            .env(
                "ROOT_DIR",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            )
            .env("STATE_DIR", &self.root)
            .env("SESSION_PROFILE", "hagia")
            .env("SESSION_STARTUP", "terminal")
            .env("TRUECOLOR_PROOF", "false")
            .env("SOPHIA_DESKTOP_PROFILE", self.root.join("desktop.kdl"))
            .env("SOPHIA_TERMINAL_BIN", &self.terminal)
            .env("SOPHIA_HAGIA_BROWSER_BIN", &self.browser)
            .env("SOPHIA_FIREFOX_BIN", &self.browser)
            .env("SOPHIA_HAGIA_BIN", "")
            .env("SOPHIA_BIN", env!("CARGO_BIN_EXE_sophia"))
            .env("firefox_m10_profile_dir", self.root.join("proof-profile"));
        for name in [
            "ANY",
            "",
            "RENDERING",
            "DIALOG",
            "PRIMARY",
            "SELECTION",
            "LIFECYCLE",
        ] {
            command.env(
                if name.is_empty() {
                    "FIREFOX_M10_PROOF".into()
                } else {
                    format!("FIREFOX_M10_{name}_PROOF")
                },
                "false",
            );
        }
        for (name, value) in changes {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
        command.output().unwrap()
    }

    fn apps(&self, changes: &[(&str, Option<&str>)], extra: &[&str]) -> Vec<String> {
        let output = self.assemble(changes, extra);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.ends_with(&[0]));
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter_map(|value| {
                let value = String::from_utf8(value.to_vec()).unwrap();
                ["--session-app", "--session-action", "--session-start"]
                    .iter()
                    .any(|prefix| value.starts_with(prefix))
                    .then_some(value)
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn normal_defaults_are_literal_and_do_not_override_profile_startup() {
    let f = Fixture::new();
    for startup in ["terminal", "none"] {
        let values = f.apps(
            &[
                ("SESSION_STARTUP", Some(startup)),
                ("SOPHIA_SESSION_STARTUP", Some(startup)),
            ],
            &[],
        );
        let mut expected = vec![
            format!("--session-app-default=terminal={}", f.terminal.display()),
            "--session-action-default=terminal=terminal".into(),
        ];
        if startup == "terminal" {
            expected.push("--session-start-default=terminal".into());
        }
        expected.extend([
            format!("--session-app-default=browser={}", f.browser.display()),
            "--session-action-default=browser=browser".into(),
        ]);
        assert_eq!(values, expected);
    }
}

#[test]
fn profile_can_own_all_applications_without_discovered_fallbacks() {
    let f = Fixture::new();
    assert!(
        f.apps(
            &[
                ("SOPHIA_TERMINAL_BIN", None),
                ("SOPHIA_HAGIA_BROWSER_BIN", None)
            ],
            &[]
        )
        .is_empty()
    );
}

#[test]
fn explicit_missing_fallbacks_are_diagnosed() {
    let f = Fixture::new();
    for (variable, _role) in [
        ("SOPHIA_TERMINAL_BIN", "terminal"),
        ("SOPHIA_HAGIA_BROWSER_BIN", "browser"),
    ] {
        let output = f.assemble(&[(variable, Some("/nonexistent/sophia-app"))], &[]);
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(variable));
    }
}

#[test]
fn firefox_and_truecolor_proofs_retain_explicit_apps_and_private_profile() {
    let f = Fixture::new();
    let firefox = f.apps(
        &[
            ("FIREFOX_M10_ANY_PROOF", Some("true")),
            ("FIREFOX_M10_PROOF", Some("true")),
            ("SOPHIA_TERMINAL_KIND", Some("kitty")),
        ],
        &["--firefox-m10-proof"],
    );
    assert!(!firefox.iter().any(|arg| arg.contains("-default=")));
    for value in [
        format!("--session-app=terminal={}", f.terminal.display()),
        format!("--session-app=browser={}", f.browser.display()),
        "--session-start=terminal".into(),
        "--session-app-arg=terminal=linux_display_server=x11".into(),
        "--session-app-arg=browser=--profile".into(),
        format!(
            "--session-app-arg=browser={}/proof-profile",
            f.root.display()
        ),
    ] {
        assert!(firefox.contains(&value), "{value}");
    }
    let truecolor = f.apps(
        &[
            ("TRUECOLOR_PROOF", Some("true")),
            ("SOPHIA_TRUECOLOR_PROOF", Some("true")),
            ("SOPHIA_TERMINAL_KIND", Some("kitty")),
        ],
        &[],
    );
    assert!(!truecolor.iter().any(|arg| arg.contains("-default=")));
    assert!(truecolor.contains(&"--session-start=palette".into()));
    assert!(truecolor.contains(&"--session-start=terminal".into()));
    assert!(
        truecolor
            .iter()
            .any(|arg| arg.ends_with("/tools/fixtures/truecolor_kitty_probe.sh"))
    );
}

#[test]
fn native_xterm_uses_its_own_adapter() {
    let f = Fixture::new();
    assert_eq!(
        f.apps(
            &[
                ("SESSION_PROFILE", Some("native")),
                ("SOPHIA_TERMINAL_KIND", Some("xterm"))
            ],
            &[]
        ),
        vec![
            format!("--session-app=terminal={}", f.terminal.display()),
            "--session-app-arg=terminal=-cm".into(),
            "--session-app-arg=terminal=-dc".into(),
            "--session-start=terminal".into(),
            "--session-app-arg=terminal=-title".into(),
            "--session-app-arg=terminal=Sophia Native TTY3".into(),
            "--session-action-app=terminal=terminal".into()
        ]
    );
}
