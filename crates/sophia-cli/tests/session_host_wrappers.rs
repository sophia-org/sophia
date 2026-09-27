//! Exercise only the host-check boundary on a disposable PTY. No probe, input
//! guard, service control or display command is allowed to run.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-host-wrapper-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for name in ["bin", "runtime", "state"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let fixture = Self(root);
        fixture.script(
            "prepare",
            "printf '%s\\n' \"$*\" >> \"$HOME/calls\"\nexec \"$SOPHIA_TEST_PREPARER_BIN\" \"$@\"",
        );
        for name in ["sudo", "python3"] {
            fixture.script(name, "echo forbidden >> \"$HOME/privileged\"\nexit 99");
        }
        fixture
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn run(&self, script: &str, checker: Option<&Path>, force: bool) -> Output {
        let mut command = Command::new("/usr/bin/timeout");
        command
            .args([
                "--kill-after=2s",
                "20s",
                "script",
                "-qefc",
                script,
                "/dev/null",
            ])
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.0.join("bin").display()),
            )
            .env("HOME", &self.0)
            .env("XDG_RUNTIME_DIR", self.0.join("runtime"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("SOPHIA_BIN", self.0.join("bin/prepare"))
            .env("SOPHIA_TEST_PREPARER_BIN", env!("CARGO_BIN_EXE_sophia"))
            .env("SOPHIA_TTY_PROFILE", "native")
            .env("SOPHIA_BUILD_SESSION", "false")
            .env("SOPHIA_MANAGE_KEYD", "true")
            .env("SOPHIA_DRM_MASTER_FORCE", if force { "1" } else { "0" })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(checker) = checker {
            command.env("SOPHIA_SESSION_PREFLIGHT", checker);
        }
        let mut child = command.spawn().unwrap();
        let _input = child.stdin.take().unwrap();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn session_host_refusal_stops_before_preparing_inputs_or_service_changes() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for body in [None, Some("exit 1"), Some("echo malformed; exit 0")] {
        let f = Fixture::new();
        let checker = body.map(|body| f.script("host", body));
        let output = f.run(
            &format!(
                "exec bash '{}'",
                source.join("tools/run_sophia_session.sh").display()
            ),
            checker.as_deref(),
            false,
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let calls = fs::read_to_string(f.0.join("calls")).unwrap();
        let calls = calls.lines().collect::<Vec<_>>();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert_eq!(calls[0], "session prepare-controls");
        assert!(
            calls[1].starts_with("session check-host --tty=/dev/pts/"),
            "{calls:?}"
        );
        assert!(!f.0.join("privileged").exists());
        let lifecycle =
            fs::read_to_string(f.0.join("state/sophia/native-session/lifecycle.log")).unwrap();
        assert!(lifecycle.contains("phase=preflight"), "{lifecycle}");
        assert!(
            !lifecycle.contains("phase=graphics_takeover"),
            "{lifecycle}"
        );
        assert!(
            !f.0.join(format!(
                "runtime/sophia-native-session-{}/input-guard.armed",
                rustix::process::getuid().as_raw()
            ))
            .exists()
        );
    }
}

#[test]
fn drm_force_still_requires_a_valid_host_checker() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for body in [None, Some("exit 2"), Some("echo malformed; exit 1")] {
        let f = Fixture::new();
        let checker = body.map(|body| f.script("host", body));
        // This invokes only the guard function. It never runs a DRM probe,
        // regardless of whether /dev/dri exists on the host.
        let output = f.run(
            &format!(
                "bash -c '. \"$1\"; sophia_require_drm_master_available' guard '{}'",
                source.join("tools/lib/drm_master_guard.sh").display()
            ),
            checker.as_deref(),
            true,
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let calls = fs::read_to_string(f.0.join("calls")).unwrap();
        assert!(
            calls.starts_with("session check-host --tty=/dev/pts/"),
            "{calls}"
        );
        assert!(calls.ends_with(" --allow-active=true\n"), "{calls}");
        assert!(!f.0.join("privileged").exists());
    }
}
