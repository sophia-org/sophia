#![cfg(feature = "native-session")]
//! The ordinary supervised login keeps why a session ended at startup.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-startup-failure-record-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }

    fn private_file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    /// One supervised `session run`, as the installed login starts it.
    fn supervised_run(&self, arguments: &[String]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_sophia"))
            .args(["session", "_supervise", "--profile=test", "--"])
            .arg(env!("CARGO_BIN_EXE_sophia"))
            .args(["session", "run", "--session-mode=normal", "--no-input"])
            .args(arguments)
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_CONFIG_HOME", &self.0)
            .env_remove("SOPHIA_DIAGNOSTIC_DIR")
            .env_remove("SOPHIA_DIAGNOSTIC_SESSION")
            .output()
            .unwrap()
    }

    fn only_session_events(&self) -> String {
        let sessions = self.0.join("state/sophia/sessions");
        let mut records = fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1, "{records:?}");
        events(&records.pop().unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn events(record: &Path) -> String {
    fs::read_to_string(record.join("events.0.log")).unwrap_or_default()
}

fn failures(events: &str) -> Vec<&str> {
    events
        .lines()
        .filter_map(|line| line.split('\t').nth(3))
        .filter(|record| record.starts_with("sophia_session_failure "))
        .collect()
}

#[test]
fn a_session_refused_at_startup_records_its_phase_in_the_supervised_record() {
    let fixture = Fixture::new();
    let core = fixture.private_file("core.kdl", "schema 2\n");
    let profile = fixture.private_file("desktop.kdl", "schema 99\n");
    let output = fixture.supervised_run(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", profile.display()),
        format!("--display=:{}", 20000 + std::process::id() % 10000),
        "--max-runtime-ms=100".into(),
    ]);
    assert!(!output.status.success());
    let events = fixture.only_session_events();
    assert!(
        events.contains("\tsophia_session_result schema=1 status=failed\n"),
        "{events}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        failures(&events),
        ["sophia_session_failure schema=1 status=failed phase=startup failure_code=unclassified"],
        "{events}"
    );
}

#[test]
fn a_failure_inside_the_owner_loop_is_recorded_once() {
    let fixture = Fixture::new();
    let core = fixture.private_file("core.kdl", "schema 2\n");
    let profile = fixture.private_file(
        "desktop.kdl",
        "schema 1\nshell { enabled #false; }\nsession { startup \"background\"; }\n",
    );
    let output = fixture.supervised_run(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", profile.display()),
        format!("--display=:{}", 30000 + std::process::id() % 10000),
        "--session-app=background=/usr/bin/sleep".into(),
        "--session-app-arg=background=20".into(),
        "--session-action-app=terminal=background".into(),
        "--max-runtime-ms=500".into(),
        "--startup-ready-timeout-ms=100".into(),
    ]);
    assert!(!output.status.success());
    let events = fixture.only_session_events();
    assert_eq!(failures(&events).len(), 1, "{events}");
}
