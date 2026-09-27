#![cfg(feature = "native-session")]
//! Real parser and host check on a disposable PTY; guard, TTY operations and
//! session execution are supplied effects. No input/display device is opened.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ARGS: &[&str] = &[
    "session",
    "run",
    "--session-mode=normal",
    "--display=:77",
    "--native-scanout",
    "--no-config",
    "--input-seat=fixture-seat",
    "--session-app=fixture=/usr/bin/true",
    "--session-start=fixture",
    "--exit-when-startup-exits",
    "--session-app-arg=fixture=literal space ; $(touch \"$HOME/injected\") ' \"\nlast line",
];

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-explicit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in ["bin", "runtime", "state"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        let f = Self(root);
        f.script("adapter", r#"
printf '%s\n' "$2" >> "$HOME/calls"
case "$2" in
    prepare-controls|prepare-environment|check-host)
        exec "$SOPHIA_TEST_PREPARER_BIN" "$@" ;;
    check-launch)
        state=""
        for arg; do
            shift
            case "$arg" in --state-dir=*) state="${arg#--state-dir=}" ;; --) break ;; esac
        done
        printf '%s\0' "$@" > "$HOME/checked"
        exec "$SOPHIA_TEST_PREPARER_BIN" session check-launch --state-dir="$state" -- "$@" ;;
    input-guard)
        printf '%s\0' "$@" > "$HOME/guard"
        for arg; do
            case "$arg" in --armed-file=*) printf 'armed\n' > "${arg#--armed-file=}" ;; esac
        done
        [ "${SOPHIA_TEST_GUARD_MODE:-ready}" != die ] || exit 1
        trap 'exit 0' TERM INT
        while :; do sleep 0.1; done ;;
    run)
        printf '%s\0' "$@" > "$HOME/executed"
        printf '%s\n' "$SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE" "$SOPHIA_SESSION_TTY" > "$HOME/environment"
        exit 0 ;;
    *) echo 'recipe preparation is forbidden' >&2; exit 99 ;;
esac
"#);
        f.script("host", "printf '%s\\n' host >> \"$HOME/order\"\nprintf 'sophia_session_preflight schema=1 status=clear tty=%s\\n' \"${1#--tty=}\"");
        f.script(
            "python3",
            r#"
printf '%s\n' "$2" >> "$HOME/tty"
case "$2" in get) echo 0 ;; get-keyboard) echo xlate ;; esac
"#,
        );
        for name in ["cargo", "sudo"] {
            f.script(name, "echo forbidden >> \"$HOME/forbidden\"; exit 99");
        }
        f
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn command(&self, args: &[&str]) -> Command {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let invocation = format!(
            "exec bash {} -- {}",
            quote(source.join("tools/run_sophia_session.sh").to_str().unwrap()),
            args.iter()
                .map(|arg| quote(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut command = Command::new("/usr/bin/timeout");
        command
            .args([
                "--kill-after=2s",
                "20s",
                "script",
                "-qefc",
                &invocation,
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
            .env("SOPHIA_BIN", self.0.join("bin/adapter"))
            .env("SOPHIA_TEST_PREPARER_BIN", env!("CARGO_BIN_EXE_sophia"))
            .env("SOPHIA_SESSION_PREFLIGHT", self.0.join("bin/host"))
            .env("SOPHIA_TTY_PROFILE", "custom.1")
            .env("SOPHIA_MANAGE_KEYD", "false")
            .env("SOPHIA_ISOLATE_SESSION_BUS", "1")
            // These recipe inputs must have no effect on the explicit path.
            .env("SOPHIA_SESSION_STARTUP", "none")
            .env("SOPHIA_TRUECOLOR_PROOF", "true")
            .env("SOPHIA_OPERATOR_INPUT_SEAT", "different-seat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, command: &mut Command) -> Output {
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
fn explicit_vector_reaches_validation_and_execution_unchanged() {
    let f = Fixture::new();
    let result = f.run(&mut f.command(ARGS));
    assert!(result.status.success(), "{result:?}");
    let expected = ARGS
        .iter()
        .flat_map(|arg| arg.bytes().chain([0]))
        .collect::<Vec<_>>();
    assert_eq!(fs::read(f.0.join("checked")).unwrap(), expected);
    assert_eq!(fs::read(f.0.join("executed")).unwrap(), expected);
    let guard = fs::read(f.0.join("guard")).unwrap();
    assert!(
        guard
            .split(|b| *b == 0)
            .any(|arg| arg == b"--input-seat=fixture-seat")
    );
    assert!(!String::from_utf8_lossy(&guard).contains("different-seat"));
    assert!(
        fs::read_to_string(f.0.join("environment"))
            .unwrap()
            .starts_with("1\n/dev/pts/")
    );
    let calls = fs::read_to_string(f.0.join("calls")).unwrap();
    let calls = calls.lines().collect::<Vec<_>>();
    assert_eq!(
        calls,
        [
            "prepare-controls",
            "check-host",
            "input-guard",
            "prepare-environment",
            "check-launch",
            "run"
        ]
    );
    assert!(!f.0.join("forbidden").exists());
    assert!(!f.0.join("injected").exists());
}

#[test]
fn explicit_path_rejects_missing_binary_bad_vector_and_ambiguous_input() {
    for (args, binary) in [
        (ARGS.to_vec(), ""),
        (ARGS.to_vec(), "relative"),
        (vec!["session", "inspect"], "/usr/bin/true"),
        (vec!["session", "run"], "/usr/bin/true"),
        (vec!["session", "run", "--input-seat="], "/usr/bin/true"),
        (
            vec![
                "session",
                "run",
                "--input-seat=a",
                "--input-devices=/dev/fixture",
            ],
            "/usr/bin/true",
        ),
    ] {
        let f = Fixture::new();
        let result = f.run(f.command(&args).env("SOPHIA_BIN", binary));
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(!f.0.join("calls").exists());
        assert!(!f.0.join("tty").exists());
    }
}

#[test]
fn explicit_path_refuses_invalid_labels_and_parser_values_before_takeover() {
    for (label, extra) in [("../escape", ""), ("safe", "--max-runtime-ms=bad")] {
        let f = Fixture::new();
        let mut args = ARGS.to_vec();
        if !extra.is_empty() {
            args.push(extra);
        }
        let result = f.run(f.command(&args).env("SOPHIA_TTY_PROFILE", label));
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(!f.0.join("executed").exists());
        assert_eq!(f.0.join("checked").exists(), !extra.is_empty());
        let tty = fs::read_to_string(f.0.join("tty")).unwrap_or_default();
        assert!(!tty.lines().any(|line| line == "graphics"), "{tty}");
    }
}

#[test]
fn explicit_path_keeps_guard_liveness_check() {
    let f = Fixture::new();
    let result = f.run(f.command(ARGS).env("SOPHIA_TEST_GUARD_MODE", "die"));
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(!f.0.join("executed").exists());
    let tty = fs::read_to_string(f.0.join("tty")).unwrap_or_default();
    assert!(!tty.lines().any(|line| line == "graphics"), "{tty}");
}

#[test]
fn controls_and_stop_accept_the_same_opaque_labels() {
    let f = Fixture::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (label, accepted) in [
        ("custom.1".to_owned(), true),
        ("a".repeat(64), true),
        (String::new(), false),
        ("..".to_owned(), false),
        ("../escape".to_owned(), false),
        ("bad/name".to_owned(), false),
        ("bad\nline".to_owned(), false),
        ("nonascii-é".to_owned(), false),
        ("a".repeat(65), false),
    ] {
        let controls = Command::new(env!("CARGO_BIN_EXE_sophia"))
            .env_clear()
            .args(["session", "prepare-controls"])
            .arg(format!("--profile={label}"))
            .output()
            .unwrap();
        assert_eq!(
            controls.status.success(),
            accepted,
            "{label:?}: {controls:?}"
        );
        let stop = Command::new("/bin/bash")
            .arg(source.join("tools/stop_sophia_session.sh"))
            .arg(&label)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_RUNTIME_DIR", f.0.join("runtime"))
            .output()
            .unwrap();
        assert_eq!(stop.status.success(), accepted, "{label:?}: {stop:?}");
        if accepted {
            assert!(String::from_utf8_lossy(&stop.stdout).starts_with("No Sophia "));
        } else {
            assert!(controls.stdout.is_empty());
        }
    }
}
