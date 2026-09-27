//! Host-checker invocation only: no TTY, input device or display is opened.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CLEAR: &str = "test \"$#\" = 1\ntest \"$1\" = --tty=/dev/tty7\nprintf 'sophia_session_preflight schema=1 status=clear tty=/dev/tty7\\n'";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-host-check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }
    fn script(&self, body: &str) -> PathBuf {
        let path = self.0.join("host checker");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["session", "check-host", "--tty=/dev/tty7"])
            .env("SOPHIA_SESSION_PREFLIGHT", self.0.join("host checker"))
            .env("SOPHIA_TEST_PID", self.0.join("pid"));
        command
    }
    fn run(&self, body: &str, allow: bool) -> Output {
        self.script(body);
        let mut command = self.command();
        if allow {
            command.arg("--allow-active=true");
        }
        command.output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn refused(output: Output) {
    assert!(!output.status.success(), "unexpected success: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "refusal published a verdict: {output:?}"
    );
}

#[test]
fn exact_clear_verdict_and_literal_tty_argument_are_required() {
    let f = Fixture::new();
    let output = f.run(CLEAR, false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"sophia_session_preflight schema=1 status=clear tty=/dev/tty7\n"
    );
    for body in [
        "exit 0".to_owned(),
        CLEAR.replace("status=clear", "status=ready"),
        CLEAR.replace("tty=/dev/tty7\\n", "tty=/dev/tty8\\n"),
        format!("{CLEAR}\n{CLEAR}"),
        format!("{CLEAR}\nexit 2"),
    ] {
        refused(f.run(&body, false));
    }
}

#[test]
fn checker_must_be_explicit_absolute_regular_and_executable() {
    let f = Fixture::new();
    refused(
        f.command()
            .env_remove("SOPHIA_SESSION_PREFLIGHT")
            .output()
            .unwrap(),
    );
    for path in [
        PathBuf::new(),
        PathBuf::from("relative"),
        f.0.clone(),
        f.0.join("missing"),
    ] {
        refused(
            f.command()
                .env("SOPHIA_SESSION_PREFLIGHT", path)
                .output()
                .unwrap(),
        );
    }
    let path = f.script(CLEAR);
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    refused(f.command().output().unwrap());
}

#[test]
fn executable_symlink_is_accepted() {
    let f = Fixture::new();
    let path = f.script(CLEAR);
    let link = f.0.join("checker-link");
    std::os::unix::fs::symlink(path, &link).unwrap();
    let output = f
        .command()
        .env("SOPHIA_SESSION_PREFLIGHT", link)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn diagnostics_escape_terminal_controls_on_every_verdict() {
    let f = Fixture::new();
    for (verdict, allow, success) in [
        (CLEAR, false, true),
        ("exit 1", true, true),
        ("exit 2", false, false),
    ] {
        let output = f.run(
            &format!("printf '\\033[31mwarning\\007\\r\\tmessage\\n' >&2\n{verdict}"),
            allow,
        );
        assert_eq!(output.status.success(), success, "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let escaped = "\\u{1b}[31mwarning\\u{7}\\r\tmessage\n";
        // The CLI's Result termination prints errors through Debug, escaping
        // the diagnostic a second time. Success and override print it directly.
        let expected = if success {
            escaped.to_owned()
        } else {
            format!("{escaped:?}").trim_matches('"').to_owned()
        };
        assert!(stderr.contains(&expected), "{stderr:?}");
        assert!(
            !stderr
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
        );
    }
}

#[test]
fn only_explicit_active_session_refusal_can_be_overridden() {
    let f = Fixture::new();
    refused(f.run("exit 1", false));
    let output = f.run("echo 'fixture:123 active' >&2\nexit 1", true);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"sophia_session_preflight schema=1 status=overridden tty=/dev/tty7\n"
    );
    for body in ["exit 2", "exit 3", "exit 0", "echo malformed; exit 1"] {
        refused(f.run(body, true));
    }
    refused(
        f.command()
            .env_remove("SOPHIA_SESSION_PREFLIGHT")
            .arg("--allow-active=true")
            .output()
            .unwrap(),
    );
}

#[test]
fn output_floods_refuse_even_with_an_active_session_override() {
    let f = Fixture::new();
    for redirect in ["", " >&2"] {
        let output = f.run(&format!("head -c 20000 /dev/zero{redirect}\nexit 1"), true);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("exceeds"),
            "{output:?}"
        );
        refused(output);
    }
}

#[test]
fn malformed_arguments_never_execute_the_checker() {
    let f = Fixture::new();
    f.script("touch \"$SOPHIA_TEST_PID\"; exit 0");
    for arguments in [
        vec!["--tty=relative"],
        vec!["--tty=/dev/tty7\nextra"],
        vec!["--tty=/dev/pts/../tty7"],
        vec!["--tty=/dev/tty7", "--profile=managed"],
        vec!["--tty=/dev/tty7", "--allow-active=1"],
        vec!["--tty=/dev/tty7", "--", "extra"],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        refused(
            command
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("SOPHIA_SESSION_PREFLIGHT", f.0.join("host checker"))
                .env("SOPHIA_TEST_PID", f.0.join("pid"))
                .args(["session", "check-host"])
                .args(arguments)
                .output()
                .unwrap(),
        );
        assert!(!f.0.join("pid").exists());
    }
}

#[test]
fn timeout_terminates_the_checker_process_group() {
    let f = Fixture::new();
    let started = Instant::now();
    let output = f.run("trap 'kill \"$child\"; wait \"$child\"; exit 1' TERM\nsleep 30 &\nchild=$!\nprintf '%s' \"$child\" > \"$SOPHIA_TEST_PID\"\nwait \"$child\"", true);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exceeded ten seconds"),
        "{output:?}"
    );
    refused(output);
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert!(started.elapsed() < Duration::from_secs(15));
    let pid = fs::read_to_string(f.0.join("pid")).unwrap();
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "checker descendant survived timeout"
    );
}

#[test]
fn successful_leader_does_not_leave_a_background_child_without_pipes() {
    let f = Fixture::new();
    let output = f.run(&format!(
        "sleep 30 </dev/null >/dev/null 2>&1 &\nprintf '%s' \"$!\" > \"$SOPHIA_TEST_PID\"\n{CLEAR}"
    ), false);
    assert!(output.status.success(), "{output:?}");
    let pid = fs::read_to_string(f.0.join("pid")).unwrap();
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "checker descendant survived successful exit"
    );
}
