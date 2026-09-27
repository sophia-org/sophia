use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("sophia-controls-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
    fn command(&self, verb: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["session", verb]);
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fields(output: &Output) -> Vec<&str> {
    assert!(output.status.success(), "{output:?}");
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .split('\0')
        .collect()
}

#[test]
fn invalid_controls_and_legacy_binaries_refuse_before_state_creation() {
    let f = Fixture::new();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/run_sophia_session.sh");
    for (name, value) in [
        ("SOPHIA_TTY_PROFILE", "../escape"),
        ("SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS", "301"),
        (
            "SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS",
            "18446744073709551616",
        ),
        ("SOPHIA_SESSION_HANDOFF", "other"),
        ("SOPHIA_INPUT_GUARD_ARMING", "other"),
    ] {
        let result = Command::new("/bin/bash")
            .arg(&source)
            .args(["--", "session", "run", "--input-seat=fixture"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &f.0)
            .env("XDG_RUNTIME_DIR", &f.0)
            .env("SOPHIA_BUILD_SESSION", "false")
            .env("SOPHIA_BIN", env!("CARGO_BIN_EXE_sophia"))
            .env("SOPHIA_TTY_PROFILE", "fixture")
            .env(name, value)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{name}: {result:?}");
        assert!(
            !f.0.join(format!(
                "sophia-fixture-session-{}",
                rustix::process::getuid().as_raw()
            ))
            .exists()
        );
    }
    let result = Command::new("/bin/bash")
        .arg(source)
        .args(["--", "session", "run", "--input-seat=fixture"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &f.0)
        .env("XDG_RUNTIME_DIR", &f.0)
        .env("SOPHIA_BUILD_SESSION", "false")
        .env("SOPHIA_BIN", "/bin/true")
        .env("SOPHIA_TTY_PROFILE", "fixture")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("does not support prepare-controls"));
    assert!(
        !f.0.join(format!(
            "sophia-fixture-session-{}",
            rustix::process::getuid().as_raw()
        ))
        .exists()
    );
}

#[test]
fn control_defaults_boundaries_and_identity_are_normalized() {
    let f = Fixture::new();
    for (seconds, ticks) in [("", "600"), ("1", "20"), ("300", "6000")] {
        let output = f
            .command("prepare-controls")
            .env("SOPHIA_TTY_PROFILE", "fixture")
            .env("SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS", seconds)
            .env("SOPHIA_INSTALLED_VERSION", "bad\nidentity")
            .output()
            .unwrap();
        let fields = fields(&output);
        assert_eq!(fields[4], ticks);
        assert_eq!(fields[5], "manual");
        assert_eq!(fields[6], "display_manager");
        assert_eq!(fields[7], "unknown");
    }
}
