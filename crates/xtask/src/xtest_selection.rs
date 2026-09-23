//! `cargo xtask check xtest-selection`: drag-select in one real xterm and
//! middle-click paste into another, driven by XTEST through a headless
//! production session.
//!
//! WHAT IT PROVES. A real xterm takes PRIMARY when an XTEST drag lands on it,
//! and a second real xterm asks for it when an XTEST middle-click lands on it.
//! The verdict is read from the session's own wire counters -- `owner_changes`
//! is SetSelectionOwner arriving at the authority, `conversions` is
//! ConvertSelection -- together with the driver's aim checks, which read the
//! pointer back before any button so a silent selection cannot be mistaken for
//! a gesture that never landed. Two defects hid behind this path until it
//! could be run: XTEST buttons delivered at the screen origin (t155), and XTEST
//! pointer events targeted at the focus instead of the surface under them
//! (t156).
//!
//! WHAT IT DOES NOT. It runs a deterministic headless head with no window
//! manager and no physical input. It says nothing about a physical drag, a
//! window manager's session, or scanout; those are t124's and t147's other
//! rows.
//!
//! `--self-test` runs three mutations that must each fail -- a drag on a blank
//! row, a session that does not admit XTEST, and no middle-click -- and passes
//! only if all three do. A gate that cannot go red is not evidence.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The driver's stdout on a full pass, compared byte for byte by the session.
const PASS_LINE: &str = "sophia_xtest_selection schema=1 status=pass owner_in_a=true matched=true pointer_in_a=true pointer_in_b=true";

/// The session bounds itself at this; the outer deadline allows for startup
/// and teardown beyond it.
const SESSION_RUNTIME_MSEC: u64 = 120_000;
const OUTER_DEADLINE: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Variant {
    Pass,
    BlankRow,
    NoAdmission,
    NoPaste,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::BlankRow => "blank-row",
            Self::NoAdmission => "no-admission",
            Self::NoPaste => "no-paste",
        }
    }
}

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let self_test = match arguments {
        [] => false,
        [flag] if flag == "--self-test" => true,
        [help] if help == "--help" => {
            return Ok(vec![
                "cargo xtask check xtest-selection [--self-test]".into(),
            ]);
        }
        _ => return Err("xtest-selection accepts only --self-test".into()),
    };
    build(repo)?;
    let output = evidence_directory(repo, self_test)?;
    let mut lines = vec![format!("evidence: {}", output.display())];
    if self_test {
        for variant in [Variant::BlankRow, Variant::NoAdmission, Variant::NoPaste] {
            match run_variant(repo, &output, variant)? {
                Ok(summary) => {
                    return Err(format!(
                        "mutation {} passed and must not: {summary}",
                        variant.name()
                    ));
                }
                Err(reason) => lines.push(format!(
                    "mutation {}: failed as required ({reason})",
                    variant.name()
                )),
            }
        }
        lines.push("xtest-selection self-test: every mutation failed".into());
    } else {
        match run_variant(repo, &output, Variant::Pass)? {
            Ok(summary) => lines.push(format!("xtest-selection: pass ({summary})")),
            Err(reason) => return Err(format!("xtest-selection failed: {reason}")),
        }
    }
    Ok(lines)
}

fn build(repo: &Path) -> Result<(), String> {
    for args in [
        &[
            "build",
            "--offline",
            "-p",
            "sophia-cli",
            "--features",
            "native-session",
        ][..],
        &[
            "build",
            "--offline",
            "-p",
            "sophia-session",
            "--all-features",
            "--example",
            "xtest_selection_driver",
        ][..],
    ] {
        let status = Command::new("cargo")
            .current_dir(repo)
            .args(args)
            .status()
            .map_err(|error| format!("could not run cargo {args:?}: {error}"))?;
        if !status.success() {
            return Err(format!("cargo {args:?} exited with {status}"));
        }
    }
    Ok(())
}

fn target_directory(repo: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| repo.join("target"), PathBuf::from)
}

fn evidence_directory(repo: &Path, self_test: bool) -> Result<PathBuf, String> {
    let commit = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    let dirty = Command::new("git")
        .current_dir(repo)
        .args(["status", "--porcelain"])
        .output()
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(true);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let name = format!(
        "{commit}{}{}-{stamp}",
        if dirty { "-dirty" } else { "" },
        if self_test { "-self-test" } else { "" }
    );
    let directory = repo.join(".artifacts/xtest-selection").join(name);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
    Ok(directory)
}

/// A display number whose socket does not exist, never the operator's. The
/// session creates the socket itself; nothing here binds it.
fn free_display() -> Result<u32, String> {
    (90..100)
        .find(|number| !Path::new(&format!("/tmp/.X11-unix/X{number}")).exists())
        .ok_or_else(|| "no free private display in :90..:99".into())
}

/// Ok(summary) when the run met every obligation; Err(reason) when it did
/// not. The outer Result is for the harness itself failing to run.
fn run_variant(
    repo: &Path,
    output: &Path,
    variant: Variant,
) -> Result<Result<String, String>, String> {
    let target = target_directory(repo);
    let session = target.join("debug/sophia");
    let driver = target.join("debug/examples/xtest_selection_driver");
    let display = free_display()?;
    let config = tempfile_directory()?;
    let mut args = vec![
        "session".to_owned(),
        "run".into(),
        format!("--display=:{display}"),
        "--no-input".into(),
        format!("--client={}", driver.display()),
        format!("--expect-client-stdout={PASS_LINE}"),
        "--require-client-normal-exit".into(),
        format!("--max-runtime-ms={SESSION_RUNTIME_MSEC}"),
    ];
    if variant != Variant::NoAdmission {
        args.push("--admit-xtest".into());
    }
    match variant {
        Variant::BlankRow => args.push("--client-arg=--row=5".into()),
        Variant::NoPaste => args.push("--client-arg=--no-paste".into()),
        Variant::Pass | Variant::NoAdmission => {}
    }
    let log_path = output.join(format!("{}.log", variant.name()));
    let log = std::fs::File::create(&log_path)
        .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|error| format!("could not share the log: {error}"))?;
    // The gate's isolation, as `workspace_tests` sets it up: compiled
    // defaults, not the developer's desktop, and no route to a live display.
    let mut child = Command::new(&session)
        .current_dir(repo)
        .args(&args)
        .env("XDG_CONFIG_HOME", &config)
        .env_remove("SOPHIA_SHELL_CONFIG")
        .env_remove("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE")
        .env_remove("DISPLAY")
        .env_remove("XAUTHORITY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err)
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", session.display()))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not wait for the session: {error}"))?
        {
            break Some(status);
        }
        if started.elapsed() >= OUTER_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let _ = std::fs::remove_dir_all(&config);
    // A killed session may leave its socket; it is ours, chosen free above.
    if status.is_none() {
        let _ = std::fs::remove_file(format!("/tmp/.X11-unix/X{display}"));
    }
    let mut text = String::new();
    std::fs::File::open(&log_path)
        .and_then(|mut file| file.read_to_string(&mut text))
        .map_err(|error| format!("could not read {}: {error}", log_path.display()))?;
    Ok(judge(
        status.map(|status| status.success()),
        &strip_ansi(&text),
    ))
}

fn tempfile_directory() -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "sophia-xtest-selection-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not restrict {}: {error}", path.display()))?;
    Ok(path)
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The last record named `name`, as its space-separated `key=value` fields.
fn record<'a>(text: &'a str, name: &str) -> Option<Vec<(&'a str, &'a str)>> {
    text.lines()
        .filter_map(|line| line.find(name).map(|at| &line[at..]))
        .rfind(|rest| rest.starts_with(&format!("{name} ")))
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|field| field.split_once('='))
                .collect()
        })
}

fn number(fields: &[(&str, &str)], key: &str) -> Option<u64> {
    fields
        .iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse().ok())
}

fn value<'a>(fields: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Every obligation, or the first one missed. An absent record is a failure,
/// never a pass.
fn judge(exited_cleanly: Option<bool>, text: &str) -> Result<String, String> {
    match exited_cleanly {
        None => return Err("session exceeded the outer deadline".into()),
        Some(false) => {
            let driver = text
                .lines()
                .filter_map(|line| {
                    line.find("xtest_selection_driver: status=")
                        .map(|at| &line[at..])
                })
                .next_back()
                .unwrap_or("no driver verdict");
            return Err(format!("session exited unsuccessfully; {driver}"));
        }
        Some(true) => {}
    }
    if !text.contains("status=bounded_complete") {
        return Err("no bounded_complete session record".into());
    }
    let selection =
        record(text, "sophia_live_selection").ok_or("no sophia_live_selection record")?;
    let owner_changes =
        number(&selection, "owner_changes").ok_or("selection record without owner_changes")?;
    let conversions =
        number(&selection, "conversions").ok_or("selection record without conversions")?;
    if owner_changes < 1 {
        return Err(format!(
            "owner_changes={owner_changes}: xterm never took PRIMARY"
        ));
    }
    // One is the driver reading the selection back; the second is xterm B
    // asking for it on the middle-click.
    if conversions < 2 {
        return Err(format!(
            "conversions={conversions}: the paste never asked for PRIMARY"
        ));
    }
    let xtest = text
        .lines()
        .rfind(|line| line.contains("sophia_live_session_xtest schema=1 status=complete"))
        .map(|line| {
            let at = line.find("sophia_live_session_xtest").unwrap_or(0);
            line[at..]
                .split_whitespace()
                .filter_map(|f| f.split_once('='))
                .collect::<Vec<_>>()
        })
        .ok_or("no completed sophia_live_session_xtest record")?;
    if value(&xtest, "admitted") != Some("true") {
        return Err("XTEST was not admitted".into());
    }
    if number(&xtest, "refused") != Some(0) {
        return Err("an injection was refused".into());
    }
    let buttons = number(&xtest, "injected_buttons").unwrap_or(0);
    if buttons < 4 {
        return Err(format!(
            "injected_buttons={buttons}: the drag and the paste take four"
        ));
    }
    Ok(format!(
        "owner_changes={owner_changes} conversions={conversions} injected_buttons={buttons}"
    ))
}

#[cfg(test)]
mod tests {
    use super::judge;

    const SELECTION: &str = "sophia_live_selection schema=1 status=complete content=redacted owner_changes=1 conversions=2";
    const XTEST: &str = "sophia_live_session_xtest schema=1 status=complete admitted=true injected_keys=0 injected_buttons=4 injected_motions=14 refused=0";
    const DONE: &str = "sophia_session_result schema=1 status=bounded_complete";

    fn log(selection: &str, xtest: &str) -> String {
        format!("{selection}\n2026 INFO x: {xtest}\n{DONE}\n")
    }

    #[test]
    fn a_complete_run_passes() {
        assert!(judge(Some(true), &log(SELECTION, XTEST)).is_ok());
    }

    #[test]
    fn a_clean_exit_does_not_excuse_a_missing_obligation() {
        // The driver's pass line is the session's to check; these are the
        // gate's own, and each must hold even when the session exits 0.
        let cases = [
            log(
                &SELECTION.replace("owner_changes=1", "owner_changes=0"),
                XTEST,
            ),
            log(&SELECTION.replace("conversions=2", "conversions=1"), XTEST),
            log("", XTEST),
            log(SELECTION, &XTEST.replace("admitted=true", "admitted=false")),
            log(SELECTION, &XTEST.replace("refused=0", "refused=1")),
            log(
                SELECTION,
                &XTEST.replace("injected_buttons=4", "injected_buttons=2"),
            ),
            log(SELECTION, ""),
            log(SELECTION, XTEST).replace("bounded_complete", "failed"),
        ];
        for case in cases {
            assert!(judge(Some(true), &case).is_err(), "passed:\n{case}");
        }
    }

    #[test]
    fn an_unclean_or_unbounded_session_fails_whatever_it_logged() {
        assert!(judge(Some(false), &log(SELECTION, XTEST)).is_err());
        assert!(judge(None, &log(SELECTION, XTEST)).is_err());
    }
}
