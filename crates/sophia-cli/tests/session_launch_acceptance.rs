#![cfg(feature = "native-session")]
//! Generic parser acceptance and private diagnostic files. Kept separately
//! from the application-discovery and proof-staging tests that move externally.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sophia launch acceptance {} {nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path.join("state"))
            .unwrap();
        Self(path)
    }

    fn command(&self, subcommand: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .env_clear()
            .env("PATH", self.0.join("bin"))
            .args(["session", subcommand]);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "native-session")]
#[test]
fn exact_launch_acceptance_uses_prepared_environment_and_rejects_invalid_values() {
    let f = Fixture::new();
    let args = [
        "session",
        "run",
        "--session-mode=normal",
        "--display=:77",
        "--native-scanout",
        "--no-config",
        "--session-app=standalone=/usr/bin/true",
        "--session-start=standalone",
        "--exit-when-startup-exits",
    ];
    for (prepared_environment, invalid_flag, accepted) in [
        (true, false, true),
        (false, false, false),
        (true, true, false),
    ] {
        let mut command = f.command("check-launch");
        command
            .arg(format!("--state-dir={}/state", f.0.display()))
            .arg("--")
            .args(args);
        if prepared_environment {
            command.env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1");
        }
        if invalid_flag {
            command.arg("--max-runtime-ms=not-a-number");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.success(),
            accepted,
            "environment={prepared_environment} invalid={invalid_flag}: {output:?}"
        );
        if accepted {
            assert_eq!(
                output.stdout,
                b"sophia_session_launch schema=1 status=accepted\n"
            );
        } else {
            assert!(output.stdout.is_empty());
        }
        for name in ["session-args-check.log", "session-args-check.err"] {
            assert_eq!(
                fs::metadata(f.0.join("state").join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    // A linked diagnostic must not be truncated, even on a parser-only path.
    fs::remove_file(f.0.join("state/session-args-check.log")).unwrap();
    fs::write(f.0.join("keep"), "keep").unwrap();
    symlink(f.0.join("keep"), f.0.join("state/session-args-check.log")).unwrap();
    let result = f
        .command("check-launch")
        .arg(format!("--state-dir={}/state", f.0.display()))
        .arg("--")
        .args(args)
        .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(f.0.join("keep")).unwrap(), "keep");
}
