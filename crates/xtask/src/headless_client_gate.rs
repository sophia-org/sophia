//! What the headless client gates share: a production session on a private
//! display with a workspace example as its `--client`, run under the gate's
//! isolation, judged from its own log.
//!
//! The session compares the client's stdout with a fixed pass line and exits
//! 0 only when it matches; a finding is a verdict the client prints and exits
//! 0 on, so the session's completion records are written and the gate can
//! read the counters that are the real evidence. Anything here is a
//! mechanism; what each gate asserts stays in its own module.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The session bounds itself at this; the outer deadline allows for startup
/// and teardown beyond it.
pub const SESSION_RUNTIME_MSEC: u64 = 120_000;
const OUTER_DEADLINE: Duration = Duration::from_secs(180);

/// One session run to make.
pub struct ClientRun<'a> {
    /// The workspace example that is the session's client.
    pub example: &'a str,
    /// The exact stdout the session requires of it.
    pub pass_line: &'a str,
    pub client_args: &'a [String],
    pub admit_xtest: bool,
    /// Where the session's stdout and stderr go.
    pub log_path: &'a Path,
}

/// What a run left behind.
pub struct ClientSession {
    /// None when the outer deadline killed the session.
    pub exited_cleanly: Option<bool>,
    /// The log, ANSI colour stripped.
    pub text: String,
}

/// Build the session binary and the client example, offline.
pub fn build(repo: &Path, example: &str) -> Result<(), String> {
    for args in [
        vec![
            "build",
            "--offline",
            "-p",
            "sophia-cli",
            "--features",
            "native-session",
        ],
        vec![
            "build",
            "--offline",
            "-p",
            "sophia-session",
            "--all-features",
            "--example",
            example,
        ],
    ] {
        let status = Command::new("cargo")
            .current_dir(repo)
            .args(&args)
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

/// `.artifacts/<subject>/<commit>[-dirty][-self-test]-<secs>/`, created.
pub fn evidence_directory(repo: &Path, subject: &str, self_test: bool) -> Result<PathBuf, String> {
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
    let directory = repo.join(".artifacts").join(subject).join(name);
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

fn isolated_config_directory() -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let path = std::env::temp_dir().join(format!(
        "sophia-headless-gate-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir(&path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not restrict {}: {error}", path.display()))?;
    Ok(path)
}

/// Run one session with the client, to completion or the outer deadline.
pub fn run_client_session(repo: &Path, run: &ClientRun<'_>) -> Result<ClientSession, String> {
    let target = target_directory(repo);
    let session = target.join("debug/sophia");
    let client = target.join("debug/examples").join(run.example);
    let display = free_display()?;
    let config = isolated_config_directory()?;
    let mut args = vec![
        "session".to_owned(),
        "run".into(),
        format!("--display=:{display}"),
        "--no-input".into(),
        format!("--client={}", client.display()),
        format!("--expect-client-stdout={}", run.pass_line),
        "--require-client-normal-exit".into(),
        format!("--max-runtime-ms={SESSION_RUNTIME_MSEC}"),
    ];
    if run.admit_xtest {
        args.push("--admit-xtest".into());
    }
    args.extend(
        run.client_args
            .iter()
            .map(|argument| format!("--client-arg={argument}")),
    );
    let log = std::fs::File::create(run.log_path)
        .map_err(|error| format!("could not create {}: {error}", run.log_path.display()))?;
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
    std::fs::File::open(run.log_path)
        .and_then(|mut file| file.read_to_string(&mut text))
        .map_err(|error| format!("could not read {}: {error}", run.log_path.display()))?;
    Ok(ClientSession {
        exited_cleanly: status.map(|status| status.success()),
        text: strip_ansi(&text),
    })
}

pub fn strip_ansi(text: &str) -> String {
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
pub fn record<'a>(text: &'a str, name: &str) -> Option<Vec<(&'a str, &'a str)>> {
    text.lines()
        .filter_map(|line| line.find(name).map(|at| &line[at..]))
        .rfind(|rest| rest.starts_with(&format!("{name} ")))
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|field| field.split_once('='))
                .collect()
        })
}

pub fn number(fields: &[(&str, &str)], key: &str) -> Option<u64> {
    fields
        .iter()
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse().ok())
}

pub fn value<'a>(fields: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The client's last `status=` line, for a failure message.
pub fn client_verdict<'a>(text: &'a str, client: &str) -> &'a str {
    let marker = format!("{client}: status=");
    text.lines()
        .filter_map(|line| line.find(&marker).map(|at| &line[at..]))
        .next_back()
        .unwrap_or("no client verdict")
}

/// The session ran to a bounded completion and the client matched; otherwise
/// why not.
pub fn require_completed(session: &ClientSession, client: &str) -> Result<(), String> {
    match session.exited_cleanly {
        None => return Err("session exceeded the outer deadline".into()),
        Some(false) => {
            return Err(format!(
                "session exited unsuccessfully; {}",
                client_verdict(&session.text, client)
            ));
        }
        Some(true) => {}
    }
    if !session.text.contains("status=bounded_complete") {
        return Err("no bounded_complete session record".into());
    }
    Ok(())
}

/// The completed XTEST record's fields, admitted with nothing refused.
pub fn admitted_xtest_record(text: &str) -> Result<Vec<(&str, &str)>, String> {
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
    Ok(xtest)
}
