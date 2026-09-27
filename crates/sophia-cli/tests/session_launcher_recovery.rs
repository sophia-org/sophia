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
    fs::write(root.join("bin/check-host"), "#!/bin/sh\nprintf 'sophia_session_preflight schema=1 status=clear tty=%s\\n' \"${1#--tty=}\"\n").unwrap();
    fs::set_permissions(
        root.join("bin/check-host"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    for (mode, expected) in [("die", 1), ("trigger", 130)] {
        let directory = root.join(mode);
        fs::create_dir_all(directory.join("runtime")).unwrap();
        let output = on_pty(
            Command::new("/usr/bin/timeout")
                .args(["--kill-after=2s", "20s", "script", "-qefc"])
                .arg(format!(
                    "exec bash '{}' -- session run --input-seat=fixture",
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
                .env("SOPHIA_SESSION_PREFLIGHT", root.join("bin/check-host"))
                .env("SOPHIA_TEST_GUARD_MODE", mode)
                .env(
                    "SOPHIA_TTY_MODE_HELPER",
                    source.join("tools/fixtures/fake_sophia_tty_mode.py"),
                )
                .env("SOPHIA_TTY_PROFILE", "fixture")
                .env("SOPHIA_BUILD_SESSION", "false")
                .env("SOPHIA_MANAGE_KEYD", "false"),
        );
        assert_eq!(output.status.code(), Some(expected), "{mode}: {output:?}");
        let lifecycle =
            fs::read_to_string(directory.join("state/sophia/fixture-session/lifecycle.log"))
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
                    "runtime/sophia-fixture-session-{}/wrapper.pid",
                    rustix::process::getuid().as_raw()
                ))
                .exists()
        );
    }
    fs::remove_dir_all(root).unwrap();
}
