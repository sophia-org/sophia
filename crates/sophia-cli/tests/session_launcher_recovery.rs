#![cfg(feature = "native-session")]
//! Run the real adapter on a disposable PTY, with supplied guard/TTY/session
//! effects. Only parser preparation delegates to the native-feature binary.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

fn on_pty(command: &mut Command) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Keep the pseudo-terminal's upstream input open until the adapter exits;
    // an immediate EOF can make script hang up its child before exec.
    let _input = child.stdin.take().unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn guard_death_and_early_recovery_prevent_graphics_takeover() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sophia-guard-regression-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin/pgrep"), "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(root.join("bin/pgrep"), fs::Permissions::from_mode(0o700)).unwrap();
    for (mode, expected) in [("die", 1), ("trigger", 130)] {
        let directory = root.join(mode);
        fs::create_dir_all(directory.join("runtime")).unwrap();
        let output = on_pty(
            Command::new("/usr/bin/timeout")
                .args(["--kill-after=2s", "20s", "script", "-qefc"])
                .arg(format!(
                    "exec bash '{}'",
                    source.join("tools/run_sophia_session.sh").display()
                ))
                .arg("/dev/null")
                .env_clear()
                .env(
                    "PATH",
                    format!("{}:/usr/bin:/bin", root.join("bin").display()),
                )
                .env("HOME", &directory)
                .env("XDG_STATE_HOME", directory.join("state"))
                .env("XDG_RUNTIME_DIR", directory.join("runtime"))
                .env(
                    "SOPHIA_BIN",
                    source.join("tools/fixtures/fake_sophia_session_watchdog.sh"),
                )
                .env("SOPHIA_TEST_PREPARER_BIN", env!("CARGO_BIN_EXE_sophia"))
                .env("SOPHIA_TEST_GUARD_MODE", mode)
                .env(
                    "SOPHIA_TTY_MODE_HELPER",
                    source.join("tools/fixtures/fake_sophia_tty_mode.py"),
                )
                .env("SOPHIA_NATIVE_WM_BIN", "/bin/true")
                .env("SOPHIA_STANDALONE_APP_BIN", "/bin/true")
                .env("SOPHIA_TTY_PROFILE", "standalone")
                .env("SOPHIA_BUILD_SESSION", "false")
                .env("SOPHIA_MANAGE_KEYD", "false"),
        );
        assert_eq!(output.status.code(), Some(expected), "{mode}: {output:?}");
        let lifecycle =
            fs::read_to_string(directory.join("state/sophia/standalone-session/lifecycle.log"))
                .unwrap();
        assert!(
            !lifecycle.contains("phase=graphics_takeover"),
            "{lifecycle}"
        );
        assert!(
            lifecycle.contains("status=returned phase=handoff"),
            "{lifecycle}"
        );
        assert!(
            !directory
                .join(format!(
                    "runtime/sophia-standalone-session-{}/wrapper.pid",
                    rustix::process::getuid().as_raw()
                ))
                .exists()
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tty_adapter_refuses_controls_before_queries_or_privileged_handoff() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sophia-handoff-refusal-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("tools/lib")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    let launcher = fs::read_to_string(source.join("tools/start_sophia_tty3.sh"))
        .unwrap()
        .replace(
            "LAUNCH_LOG=\"/tmp/sophia-${SESSION_PROFILE}-tty${TARGET_VT}-launch.log\"",
            &format!("LAUNCH_LOG=\"{}/launch.log\"", root.display()),
        );
    fs::write(root.join("tools/start_sophia_tty3.sh"), launcher).unwrap();
    fs::copy(
        source.join("tools/lib/session_preparation.sh"),
        root.join("tools/lib/session_preparation.sh"),
    )
    .unwrap();
    for (name, body) in [
        ("tty", "echo /dev/tty3"),
        (
            "prepare",
            "printf '%s\\n' \"$*\" > \"$HOME/preparation\"; exec \"$SOPHIA_TEST_PREPARER_BIN\" \"$@\"",
        ),
        ("python3", "echo forbidden >> \"$HOME/takeover\"; exit 99"),
        ("sudo", "echo forbidden >> \"$HOME/takeover\"; exit 99"),
    ] {
        let path = root.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let output = on_pty(
        Command::new("/usr/bin/timeout")
            .args(["--kill-after=2s", "20s", "script", "-qefc"])
            .arg(format!(
                "exec bash '{}'",
                root.join("tools/start_sophia_tty3.sh").display()
            ))
            .arg("/dev/null")
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.join("bin").display()),
            )
            .env("HOME", &root)
            .env("SOPHIA_TTY_PROFILE", "native")
            .env("SOPHIA_BUILD_SESSION", "false")
            .env("SOPHIA_BIN", root.join("bin/prepare"))
            .env("SOPHIA_TEST_PREPARER_BIN", env!("CARGO_BIN_EXE_sophia"))
            .env("SOPHIA_INPUT_GUARD_ARMING", "invalid"),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    // A fast refusal can exit before the launcher's process-substitution tee
    // drains. Observe the synchronous call, not that best-effort terminal log.
    assert_eq!(
        fs::read_to_string(root.join("preparation")).unwrap(),
        "session prepare-controls\n"
    );
    assert!(!root.join("takeover").exists());
    fs::remove_dir_all(root).unwrap();
}
