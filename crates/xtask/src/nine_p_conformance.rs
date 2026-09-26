//! `cargo xtask check 9p-conformance`: an independent client judges the
//! `sophia-9p` core.
//!
//! WHAT IT PROVES. The oracle (`tools/9p-oracle`) shares no code with the
//! core. It drives the pinned third-party Go client, github.com/hugelgupf/p9
//! v0.4.1 (Apache-2.0), through version, attach, walk, open, read, readdir,
//! write, getattr and clunk, and sends frames written from the 9P2000.L
//! specification for what that client never sends: flush, malformed and
//! oversize frames, tag rules and version edge cases. The server is the
//! crate's `static_export_server` example serving the C1 static test export
//! over a Unix socket.
//!
//! WHAT IT DOES NOT. It is headless and exercises no role: no WM, shell or
//! application contract, no Session admission, no v9fs mount, and no
//! performance. Revocation, reply reservation and the driver's socket
//! handling are covered by the crate's own tests, not here.
//!
//! The oracle builds offline from the module cache; its pin is checked
//! before it is built. Populating the cache is setup, done once with
//! `go mod download` in `tools/9p-oracle`.
//!
//! `--self-test` runs the server with each harness mutation, and each must
//! make the oracle fail, including every check the mutation is named for.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::headless_client_gate::evidence_directory;

const ORACLE: &str = "tools/9p-oracle";
const EXAMPLE: &str = "static_export_server";
/// The client module and the go.sum line that pins its content.
const PINNED_MODULE: &str = "github.com/hugelgupf/p9 v0.4.1";
const PINNED_SUM: &str =
    "github.com/hugelgupf/p9 v0.4.1 h1:04RUBWSYlvP38QX8At5VXnMTg9qNEnbP/548aMLtfsk=";
/// Fewer checks than this is an oracle that skipped some, not a pass.
const MINIMUM_CHECKS: usize = 50;
/// Each harness mutation, and the checks that must be among those it fails:
/// a mutation proves only the checks it is required to break.
const MUTATIONS: [(&str, &[&str]); 10] = [
    (
        "permissive-check",
        &["client/owner-refuses-hidden", "client/write-sink"],
    ),
    (
        "corrupt-read",
        &["client/walk-open-read", "client/large-read"],
    ),
    (
        "pending-as-empty",
        &[
            "raw/flushed-read-is-never-answered",
            "raw/clunk-answers-waiting-reads-first",
            "raw/disconnect-releases-held-fids-and-waiting-read",
        ],
    ),
    ("version-suffix", &["raw/version-exact"]),
    (
        "drop-flush",
        &[
            "raw/flushed-read-is-never-answered",
            "raw/flush-of-unknown-tag-answered",
        ],
    ),
    (
        "list-hidden",
        &[
            "client/readdir-lists-root",
            "client/readdir-resumes-at-an-offset",
        ],
    ),
    (
        "errno-as-eio",
        &[
            "raw/attach-string-past-frame-eproto",
            "raw/walk-name-past-frame-eproto",
            "raw/open-access-mode-3-einval",
            "raw/open-unknown-flag-einval",
            "raw/walk-to-used-newfid-ebadf",
            "raw/fid-limit-emfile",
            "raw/waiting-read-limit-eagain",
        ],
    ),
    ("unbounded-fids", &["raw/fid-limit-emfile"]),
    ("unbounded-pending", &["raw/waiting-read-limit-eagain"]),
    (
        "leak-on-release",
        &["raw/disconnect-releases-held-fids-and-waiting-read"],
    ),
];
const RUN_LIMIT: Duration = Duration::from_secs(180);

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let self_test = match arguments {
        [] => false,
        [flag] if flag == "--self-test" => true,
        [help] if help == "--help" => {
            return Ok(vec![
                "cargo xtask check 9p-conformance [--self-test]".into(),
            ]);
        }
        _ => return Err("9p-conformance accepts only --self-test".into()),
    };
    check_pin(repo)?;
    let output = evidence_directory(repo, "9p-conformance", self_test)?;
    let oracle = build_oracle(repo, &output)?;
    let server = build_server(repo)?;
    let mut lines = vec![format!("evidence: {}", output.display())];
    if self_test {
        for (mutation, required) in MUTATIONS {
            let transcript = run_once(&server, &oracle, &output, Some(mutation))?;
            match judge(&transcript) {
                Verdict::Fail(summary) => {
                    let missed = unbroken(&transcript, required);
                    if !missed.is_empty() {
                        return Err(format!(
                            "mutation {mutation} left required checks passing: {}",
                            missed.join(" ")
                        ));
                    }
                    lines.push(format!(
                        "mutation {mutation}: failed as required ({summary})"
                    ));
                }
                Verdict::Pass(summary) => {
                    return Err(format!(
                        "mutation {mutation} passed and must not: {summary}"
                    ));
                }
                Verdict::Unreadable(reason) => {
                    return Err(format!(
                        "mutation {mutation} gave no verdict, which is not a failure the \
                         oracle detected: {reason}"
                    ));
                }
            }
        }
        lines.push("9p-conformance self-test: every mutation failed".into());
    } else {
        let transcript = run_once(&server, &oracle, &output, None)?;
        match judge(&transcript) {
            Verdict::Pass(summary) => lines.push(format!("9p-conformance: pass ({summary})")),
            Verdict::Fail(summary) | Verdict::Unreadable(summary) => {
                return Err(format!("9p-conformance failed: {summary}"));
            }
        }
    }
    Ok(lines)
}

/// The oracle's module must require exactly the pinned client, and go.sum
/// must hold its hash, so an offline build cannot substitute another.
pub fn check_pin(repo: &Path) -> Result<(), String> {
    let read = |name: &str| {
        let path = repo.join(ORACLE).join(name);
        std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))
    };
    let module = read("go.mod")?;
    // Direct requirements, from single-line and block forms; indirect ones
    // are the pinned client's own dependencies, held by go.sum.
    let mut required = Vec::new();
    let mut in_block = false;
    for line in module.lines().map(str::trim) {
        let entry = match line {
            "require (" => {
                in_block = true;
                continue;
            }
            ")" => {
                in_block = false;
                continue;
            }
            _ if in_block => line,
            _ => match line.strip_prefix("require ") {
                Some(entry) => entry,
                None => continue,
            },
        };
        if !entry.ends_with("// indirect") {
            required.push(entry);
        }
    }
    if required != [PINNED_MODULE] {
        return Err(format!(
            "{ORACLE}/go.mod must require exactly {PINNED_MODULE}, found {required:?}"
        ));
    }
    if !read("go.sum")?.lines().any(|line| line == PINNED_SUM) {
        return Err(format!("{ORACLE}/go.sum does not pin {PINNED_MODULE}"));
    }
    Ok(())
}

fn build_oracle(repo: &Path, output: &Path) -> Result<PathBuf, String> {
    let binary = output.join("9p-oracle");
    let status = Command::new("go")
        .current_dir(repo.join(ORACLE))
        .env("GOFLAGS", "-mod=readonly")
        .env("GOPROXY", "off")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .status()
        .map_err(|error| format!("could not run go: {error}"))?;
    if !status.success() {
        return Err(format!(
            "the oracle did not build offline; populate the module cache once with \
             `go mod download` in {ORACLE}"
        ));
    }
    Ok(binary)
}

fn build_server(repo: &Path) -> Result<PathBuf, String> {
    let status = Command::new("cargo")
        .current_dir(repo)
        .args([
            "build",
            "--offline",
            "-p",
            "sophia-9p",
            "--example",
            EXAMPLE,
        ])
        .status()
        .map_err(|error| format!("could not run cargo: {error}"))?;
    if !status.success() {
        return Err(format!("could not build the {EXAMPLE} example"));
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo.join(path)
            }
        })
        .unwrap_or_else(|| repo.join("target"));
    Ok(target.join("debug/examples").join(EXAMPLE))
}

/// One server, one oracle run against it. Returns the oracle's output, also
/// kept in the evidence directory with the server's.
fn run_once(
    server: &Path,
    oracle: &Path,
    output: &Path,
    mutation: Option<&str>,
) -> Result<String, String> {
    let name = mutation.unwrap_or("pass");
    // A socket address holds about a hundred bytes, which an evidence path
    // can exceed; the sockets live in a short private directory instead.
    let sockets = std::env::temp_dir().join(format!("s9p-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&sockets)
        .map_err(|error| format!("could not create {}: {error}", sockets.display()))?;
    let socket = sockets.join("9p.sock");
    let result = serve_and_judge(server, oracle, output, mutation, name, &socket);
    let _ = std::fs::remove_dir_all(&sockets);
    result
}

fn serve_and_judge(
    server: &Path,
    oracle: &Path,
    output: &Path,
    mutation: Option<&str>,
    name: &str,
    socket: &Path,
) -> Result<String, String> {
    let mut command = Command::new(server);
    command.arg("--socket").arg(socket);
    if let Some(mutation) = mutation {
        command.args(["--mutation", mutation]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", server.display()))?;
    let ready = child
        .stdout
        .take()
        .and_then(|stdout| BufReader::new(stdout).lines().next())
        .and_then(Result::ok)
        .unwrap_or_default();
    let result = if ready.starts_with("sophia_9p_static_export schema=1 status=ready ") {
        run_oracle(oracle, socket)
    } else {
        Err(format!("the server did not report ready: {ready:?}"))
    };
    // Closing standard input stops the server.
    drop(child.stdin.take());
    let stopped = wait(&mut child, "the server");
    let transcript = result?;
    stopped?;
    let log = output.join(format!("{name}.log"));
    std::fs::File::create(&log)
        .and_then(|mut file| write!(file, "{ready}\n{transcript}"))
        .map_err(|error| format!("could not write {}: {error}", log.display()))?;
    Ok(transcript)
}

fn run_oracle(oracle: &Path, socket: &Path) -> Result<String, String> {
    let child = Command::new(oracle)
        .arg("-socket")
        .arg(socket)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start the oracle: {error}"))?;
    let started = Instant::now();
    let mut child = child;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > RUN_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("the oracle ran past {RUN_LIMIT:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("could not wait for the oracle: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("could not read the oracle: {error}"))?;
    String::from_utf8(output.stdout).map_err(|_| "the oracle wrote non-UTF-8".into())
}

fn wait(child: &mut Child, what: &str) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{what} exited with {status}")),
            Ok(None) if started.elapsed() > RUN_LIMIT => {
                let _ = child.kill();
                return Err(format!("{what} did not stop"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("could not wait for {what}: {error}")),
        }
    }
}

/// The required checks that did not fail in this transcript.
pub fn unbroken<'a>(transcript: &str, required: &[&'a str]) -> Vec<&'a str> {
    required
        .iter()
        .copied()
        .filter(|name| {
            !transcript.lines().any(|line| {
                line.strip_prefix("check ")
                    .and_then(|rest| rest.split_once(" FAIL: "))
                    .is_some_and(|(failed, _)| failed == *name)
            })
        })
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
pub enum Verdict {
    Pass(String),
    Fail(String),
    Unreadable(String),
}

/// The oracle's last line is its verdict. A pass needs no failed check and
/// at least [`MINIMUM_CHECKS`]; a failure names the checks that failed.
pub fn judge(transcript: &str) -> Verdict {
    let Some(last) = transcript.lines().last() else {
        return Verdict::Unreadable("no output".into());
    };
    let Some(fields) = last.strip_prefix("sophia_9p_oracle schema=1 ") else {
        return Verdict::Unreadable(format!("no verdict line: {last:?}"));
    };
    let field = |name: &str| {
        fields
            .split(' ')
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
    };
    let number = |name: &str| field(name).and_then(|value| value.parse::<usize>().ok());
    let (Some(status), Some(checks), Some(failed)) =
        (field("status"), number("checks"), number("failed"))
    else {
        return Verdict::Unreadable(format!("malformed verdict line: {last:?}"));
    };
    let failures = transcript
        .lines()
        .filter(|line| line.starts_with("check ") && line.contains(" FAIL: "))
        .count();
    if failures != failed {
        return Verdict::Unreadable(format!(
            "the verdict says {failed} failed but {failures} checks report failure"
        ));
    }
    match status {
        "pass" if failed == 0 && checks >= MINIMUM_CHECKS => {
            Verdict::Pass(format!("checks={checks}"))
        }
        "pass" => Verdict::Unreadable(format!(
            "a pass with checks={checks} failed={failed} is not a full run"
        )),
        "fail" if failed > 0 => {
            let names = transcript
                .lines()
                .filter_map(|line| line.strip_prefix("check ")?.split_once(" FAIL: "))
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            Verdict::Fail(format!("{failed} of {checks} failed: {}", names.join(" ")))
        }
        _ => Verdict::Unreadable(format!("unrecognised verdict line: {last:?}")),
    }
}

mod tests;
