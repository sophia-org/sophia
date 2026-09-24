//! x11bench in the X11 profile gate: an independent drawing suite as a pixel
//! oracle for the software fixture's raster.
//!
//! WHAT IT PROVES. x11bench (a separate C++ checkout) draws each of its
//! patterns through real Xlib, XRender and Xft, reads the result back with
//! GetImage and compares it with a reference image. The references are
//! generated in the same confined run, on Xvnc, whose drawing is the `fb`/`mi`
//! code the protocol's reference server uses, at the fixture host's own
//! screen geometry: Xft takes its DPI from the screen's millimetre size, so a
//! reference drawn on another geometry fails every text test for a reason that
//! is not the authority's. Both servers therefore share the machine's fonts
//! and DPI, and no reference image is committed. Each test the authority does
//! not pass must be declared in the manifest with a reason; a declared failure
//! that passes makes the manifest stale, which fails the run.
//!
//! WHAT IT DOES NOT. It certifies `x11_conformance_host`'s CPU raster, not GPU
//! composition or the Engine's scene, and nothing about input. Its stacking
//! tests read other clients' pixels through the root window, which the
//! authority's policy answers, not its raster.
//!
//! NEGATIVE CONTROL. Xvnc is run against its own fresh references before the
//! host is. Anything short of every test passing there is an unstable oracle,
//! and the run is FAIL without a word about the authority.
//!
//! CONFINEMENT. The gate re-enters itself inside bubblewrap
//! (`x11-profile --x11bench-contained`) with a private `/tmp`, no network and
//! no System V IPC, and a cleared environment; both servers and every client
//! run in there, so no display outside the namespace is reachable and none is
//! named. Both servers listen on sockets that exist only inside it.

use super::process;
use super::profiles::SuiteVerdict;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Inside the namespace: the fixture host's display and the oracle's.
const HOST_DISPLAY: u32 = 1;
const ORACLE_DISPLAY: u32 = 2;
/// How long a server may take to bind its socket.
const BIND_DEADLINE: Duration = Duration::from_secs(10);
/// The gate leaves the contained run this long, beyond its own deadline, to
/// stop its servers and write what it found.
pub(super) const GATE_MARGIN_SECS: u64 = 30;

/// One manifest row: a test, and what the authority is expected to do in it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ManifestRow {
    pub test: String,
    /// Absent means PASS. `FAIL` must carry a reason.
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Read a manifest: every row names a test once, and a declared failure says
/// why. A row that declares PASS explicitly is refused, so that there is one
/// spelling of "expected to pass".
pub(super) fn manifest(text: &str) -> Result<Vec<ManifestRow>, String> {
    let rows: Vec<ManifestRow> =
        serde_json::from_str(text).map_err(|error| format!("manifest is not valid: {error}"))?;
    if rows.is_empty() {
        return Err("manifest names no tests".into());
    }
    let mut seen = BTreeSet::new();
    for row in &rows {
        if !seen.insert(row.test.as_str()) {
            return Err(format!("manifest names {} twice", row.test));
        }
        match (row.expected.as_deref(), row.reason.as_deref()) {
            (None, None) => {}
            (Some("FAIL"), Some(reason)) if !reason.trim().is_empty() => {}
            (Some("FAIL"), _) => {
                return Err(format!("{}: a declared FAIL needs a reason", row.test));
            }
            (None, Some(_)) => {
                return Err(format!("{}: a reason without a declaration", row.test));
            }
            (Some(other), _) => {
                return Err(format!(
                    "{}: {other} is not a disposition; declare FAIL or nothing",
                    row.test
                ));
            }
        }
    }
    Ok(rows)
}

/// The names `x11bench --list` prints, in its order.
pub(super) fn inventory(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|line| {
            line.split_once(" - ")
                .map(|(name, _)| name.trim().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Each test's outcome from an x11bench run's standard output: the name,
/// padded, then `[PASS]`, `[FAIL]`, `[ERROR]` or `[GENERATED]` on the same
/// line. Colour codes are stripped first. A test named twice keeps its first
/// outcome and is reported by the caller as an inventory mismatch.
pub(super) fn outcomes(stdout: &str) -> BTreeMap<String, String> {
    let text = crate::headless_client_gate::strip_ansi(stdout);
    let mut found = BTreeMap::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(status) = rest
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
            .map(|(status, _)| status)
        else {
            continue;
        };
        if matches!(status, "PASS" | "FAIL" | "ERROR" | "GENERATED") {
            found
                .entry(name.to_owned())
                .or_insert_with(|| status.to_owned());
        }
    }
    found
}

/// What the contained run found, written by it and read by the gate.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct ContainedResult {
    /// Why the run stopped short, if it did. Set, nothing else is judged.
    pub error: Option<String>,
    pub inventory: Vec<String>,
    /// Xvnc against its own references: the negative control.
    pub control: BTreeMap<String, String>,
    /// The fixture host against the same references.
    pub host: BTreeMap<String, String>,
    pub oracle_version: String,
    /// Width and height in pixels, then in millimetres, as each server's
    /// setup reply gave them. The oracle's must equal the host's.
    pub host_geometry: Option<[u16; 4]>,
    pub oracle_geometry: Option<[u16; 4]>,
}

/// The gate's reading of a contained run against the manifest.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Judgement {
    pub status: String,
    pub passed: usize,
    pub declared: usize,
    pub failures: Vec<String>,
}

pub(super) fn judge(rows: &[ManifestRow], result: &ContainedResult) -> Judgement {
    let mut judgement = Judgement {
        status: "FAIL".into(),
        passed: 0,
        declared: 0,
        failures: Vec::new(),
    };
    if let Some(error) = &result.error {
        judgement
            .failures
            .push(format!("the contained run stopped: {error}"));
        return judgement;
    }
    if result.host_geometry.is_none() || result.host_geometry != result.oracle_geometry {
        judgement.failures.push(format!(
            "the oracle's screen {:?} is not the host's {:?}; its references would not apply",
            result.oracle_geometry, result.host_geometry
        ));
        return judgement;
    }
    let manifested: BTreeSet<&str> = rows.iter().map(|row| row.test.as_str()).collect();
    let listed: BTreeSet<&str> = result.inventory.iter().map(String::as_str).collect();
    if result.inventory.len() != listed.len() {
        judgement
            .failures
            .push("x11bench lists a test twice".into());
    }
    for missing in manifested.difference(&listed) {
        judgement.failures.push(format!(
            "{missing}: manifested, but x11bench has no such test"
        ));
    }
    for unknown in listed.difference(&manifested) {
        judgement.failures.push(format!(
            "{unknown}: x11bench runs it, but the manifest does not name it"
        ));
    }
    for name in &listed {
        match result.control.get(*name).map(String::as_str) {
            Some("PASS") => {}
            other => judgement.failures.push(format!(
                "{name}: the oracle against its own references gave {other:?}; the oracle is unstable"
            )),
        }
    }
    if !judgement.failures.is_empty() {
        return judgement;
    }
    for row in rows {
        let observed = result.host.get(&row.test).map(String::as_str);
        match (row.expected.as_deref(), observed) {
            (None, Some("PASS")) => judgement.passed += 1,
            (Some("FAIL"), Some("FAIL")) => judgement.declared += 1,
            (Some("FAIL"), Some("PASS")) => judgement.failures.push(format!(
                "{}: PASS but declared FAIL; the manifest is stale",
                row.test
            )),
            (expected, observed) => judgement.failures.push(format!(
                "{}: {} (expected {})",
                row.test,
                observed.unwrap_or("no result"),
                expected.unwrap_or("PASS")
            )),
        }
    }
    if judgement.failures.is_empty() {
        judgement.status = "PASS".into();
    }
    judgement
}

/// Search `PATH` for a program, as the contained run will.
fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(program))
            .find(|candidate| candidate.is_file())
    })
}

pub(super) fn blocked(reason: &str) -> SuiteVerdict {
    SuiteVerdict {
        status: "BLOCKED".into(),
        reason: reason.to_owned(),
        exit: None,
        report: None,
    }
}

/// Run x11bench against the fixture host the gate built, confined, and judge
/// it against the manifest.
pub(super) fn run(
    host: &Path,
    bench: &Path,
    expected: &Path,
    output: &Path,
    timeout: u64,
) -> Result<SuiteVerdict, String> {
    if !host.is_file() {
        return Ok(blocked(
            "the core host was not built in this target; x11bench runs against x11_conformance_host",
        ));
    }
    if !bench.is_file() {
        return Ok(blocked(&format!(
            "no x11bench binary at {}; it is a separate checkout, built with cmake",
            bench.display()
        )));
    }
    let (Some(bwrap), Some(_)) = (on_path("bwrap"), on_path("Xvnc")) else {
        return Ok(blocked(
            "bubblewrap and Xvnc (TigerVNC) are both required: one confines the run, the other draws the references",
        ));
    };
    let rows = match std::fs::read_to_string(expected)
        .map_err(|error| format!("could not read {}: {error}", expected.display()))
        .and_then(|text| manifest(&text))
    {
        Ok(rows) => rows,
        Err(reason) => {
            return Ok(SuiteVerdict {
                status: "FAIL".into(),
                reason,
                exit: None,
                report: None,
            });
        }
    };
    std::fs::create_dir(output).map_err(|error| error.to_string())?;
    let gate = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(bwrap);
    command
        .args(["--die-with-parent", "--new-session"])
        .args(["--unshare-net", "--unshare-ipc", "--unshare-pid"])
        .args(["--ro-bind", "/", "/"])
        .args(["--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"])
        .arg("--bind")
        .arg(output)
        .arg(output)
        .args([
            "--clearenv",
            "--setenv",
            "PATH",
            "/usr/local/bin:/usr/bin:/bin",
        ])
        .args(["--setenv", "HOME", "/tmp", "--setenv", "LANG", "C.UTF-8"])
        .arg("--")
        .arg(gate)
        .args(["check", "x11-profile", "--x11bench-contained"])
        .arg(format!("--host={}", host.display()))
        .arg(format!("--bench={}", bench.display()))
        .arg(format!("--output={}", output.display()))
        .arg(format!("--timeout={timeout}"));
    let execution = process::run(
        &mut command,
        &output.with_extension("log"),
        Duration::from_secs(timeout + GATE_MARGIN_SECS),
    )?;
    if execution.timed_out {
        return Ok(SuiteVerdict {
            status: "TIMEOUT".into(),
            reason: "the contained run's absolute deadline expired".into(),
            exit: None,
            report: None,
        });
    }
    let result: ContainedResult = match std::fs::read_to_string(output.join("result.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(result) => result,
        None => {
            return Ok(SuiteVerdict {
                status: "FAIL".into(),
                reason: format!(
                    "the contained run exited {:?} without a readable result",
                    execution.returncode
                ),
                exit: execution.returncode,
                report: None,
            });
        }
    };
    let judgement = judge(&rows, &result);
    let report = output.join("report.json");
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "judgement": judgement,
        "oracle_version": result.oracle_version,
        "host_geometry": result.host_geometry,
        "bench_sha256": super::identity::digest(bench)?,
        "host_sha256": super::identity::digest(host)?,
    }))
    .map_err(|error| error.to_string())?;
    std::fs::write(&report, text + "\n").map_err(|error| error.to_string())?;
    let reason = if judgement.status == "PASS" {
        format!(
            "every manifested test met its expectation ({} passed, {} declared)",
            judgement.passed, judgement.declared
        )
    } else {
        format!(
            "{} of the manifest's expectations not met: {}",
            judgement.failures.len(),
            judgement.failures.join("; ")
        )
    };
    Ok(SuiteVerdict {
        status: judgement.status,
        reason,
        exit: execution.returncode,
        report: Some(report),
    })
}

/// A server this run started, stopped when the run is done with it.
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn socket(display: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/.X11-unix/X{display}"))
}

fn start(command: &mut Command, display: u32, log: &Path) -> Result<Server, String> {
    let log = std::fs::File::create(log).map_err(|error| error.to_string())?;
    let child = command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|error| error.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|error| format!("could not start {command:?}: {error}"))?;
    let mut server = Server(child);
    let deadline = Instant::now() + BIND_DEADLINE;
    while !socket(display).exists() {
        if Instant::now() > deadline
            || server
                .0
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
        {
            return Err(format!("{command:?} did not bind display :{display}"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(server)
}

/// A screen's size in pixels and millimetres, from the server's own setup
/// reply. Framed here from the protocol specification, with nothing of the
/// authority's codec, so that the two servers are measured the same way.
fn geometry(display: u32) -> Result<[u16; 4], String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket(display))
        .map_err(|error| format!("could not connect to :{display}: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    // Little-endian, protocol 11.0, no authorization.
    stream
        .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .map_err(|error| error.to_string())?;
    let mut head = [0u8; 8];
    stream
        .read_exact(&mut head)
        .map_err(|error| error.to_string())?;
    if head[0] != 1 {
        return Err(format!(":{display} refused the connection setup"));
    }
    let mut body = vec![0u8; usize::from(u16::from_le_bytes([head[6], head[7]])) * 4];
    stream
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    let u16_at = |offset: usize| -> Result<u16, String> {
        body.get(offset..offset + 2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .ok_or_else(|| format!(":{display}'s setup reply is short"))
    };
    // Offsets are into the reply after its 8-byte head: the vendor string
    // follows 32 fixed bytes, padded to 4, then 8 bytes per pixmap format,
    // then the first screen, whose sizes sit 20 bytes in.
    let vendor = usize::from(u16_at(16)?);
    let formats = usize::from(*body.get(21).ok_or("short setup reply")?);
    let screen = 32 + vendor.div_ceil(4) * 4 + 8 * formats;
    Ok([
        u16_at(screen + 20)?,
        u16_at(screen + 22)?,
        u16_at(screen + 24)?,
        u16_at(screen + 26)?,
    ])
}

/// Run x11bench to completion or its deadline; its standard output, which
/// holds the one line per test the gate reads.
fn bench(
    bench: &Path,
    display: u32,
    arguments: &[&str],
    output: &Path,
    name: &str,
    deadline: Instant,
) -> Result<String, String> {
    let stdout = output.join(format!("{name}.stdout"));
    let stderr = output.join(format!("{name}.stderr"));
    let mut child = Command::new(bench)
        .arg("--display")
        .arg(format!(":{display}"))
        .args(arguments)
        .env("DISPLAY", format!(":{display}"))
        .env("XAUTHORITY", "/tmp/unused-Xauthority")
        .current_dir(output)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&stdout).map_err(|error| error.to_string())?)
        .stderr(std::fs::File::create(&stderr).map_err(|error| error.to_string())?)
        .spawn()
        .map_err(|error| format!("could not start x11bench: {error}"))?;
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("x11bench {name} passed the run's deadline"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::fs::read_to_string(&stdout).map_err(|error| error.to_string())
}

/// Inside the namespace: start both servers, generate the references, run
/// the control and the host, and write what was found.
pub(super) fn contained(arguments: &[String]) -> Result<(), String> {
    let mut host = None;
    let mut bench_path = None;
    let mut output = None;
    let mut timeout = None;
    for argument in arguments {
        match argument.split_once('=') {
            Some(("--host", value)) => host = Some(PathBuf::from(value)),
            Some(("--bench", value)) => bench_path = Some(PathBuf::from(value)),
            Some(("--output", value)) => output = Some(PathBuf::from(value)),
            Some(("--timeout", value)) => timeout = value.parse::<u64>().ok(),
            _ => return Err(format!("unexpected contained argument {argument}")),
        }
    }
    let (Some(host), Some(bench_path), Some(output), Some(timeout)) =
        (host, bench_path, output, timeout)
    else {
        return Err("the contained run needs --host, --bench, --output and --timeout".into());
    };
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut result = ContainedResult::default();
    if let Err(error) = contained_run(&host, &bench_path, &output, deadline, &mut result) {
        result.error = Some(error);
    }
    let text = serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?;
    std::fs::write(output.join("result.json"), text + "\n").map_err(|error| error.to_string())
}

fn contained_run(
    host: &Path,
    bench_path: &Path,
    output: &Path,
    deadline: Instant,
    result: &mut ContainedResult,
) -> Result<(), String> {
    std::fs::create_dir_all("/tmp/.X11-unix").map_err(|error| error.to_string())?;
    let listing = bench(
        bench_path,
        HOST_DISPLAY,
        &["--list"],
        output,
        "list",
        deadline,
    )?;
    result.inventory = inventory(&listing);
    if result.inventory.is_empty() {
        return Err("x11bench --list named no tests".into());
    }
    let _host = start(
        Command::new(host).arg(socket(HOST_DISPLAY)),
        HOST_DISPLAY,
        &output.join("host.log"),
    )?;
    let host_geometry = geometry(HOST_DISPLAY)?;
    result.host_geometry = Some(host_geometry);
    let [width, height, width_mm, _] = host_geometry;
    if width_mm == 0 {
        return Err("the host reports a screen with no width in millimetres".into());
    }
    // Xvnc takes a DPI and derives the millimetres from it; the geometry it
    // then reports is checked, not assumed.
    let dpi = (f64::from(width) * 25.4 / f64::from(width_mm)).round();
    let version = Command::new("Xvnc")
        .arg("-version")
        .output()
        .map_err(|error| format!("could not run Xvnc -version: {error}"))?;
    result.oracle_version = String::from_utf8_lossy(&version.stderr)
        .lines()
        .chain(String::from_utf8_lossy(&version.stdout).lines())
        .find(|line| line.contains("Xvnc"))
        .unwrap_or("unknown")
        .trim()
        .to_owned();
    let _oracle = start(
        Command::new("Xvnc")
            .arg(format!(":{ORACLE_DISPLAY}"))
            .arg("-geometry")
            .arg(format!("{width}x{height}"))
            .arg("-dpi")
            .arg(format!("{dpi}"))
            .args(["-depth", "24", "-SecurityTypes", "None", "-nolisten", "tcp"]),
        ORACLE_DISPLAY,
        &output.join("oracle.log"),
    )?;
    result.oracle_geometry = Some(geometry(ORACLE_DISPLAY)?);
    if result.oracle_geometry != result.host_geometry {
        return Ok(());
    }
    let references = output.join("references");
    std::fs::create_dir(&references).map_err(|error| error.to_string())?;
    let references = references.display().to_string();
    let generated = outcomes(&bench(
        bench_path,
        ORACLE_DISPLAY,
        &["--ref-dir", &references, "--regenerate"],
        output,
        "generate",
        deadline,
    )?);
    if generated.values().any(|status| status != "GENERATED") {
        return Err(format!(
            "the oracle did not generate every reference: {generated:?}"
        ));
    }
    result.control = outcomes(&bench(
        bench_path,
        ORACLE_DISPLAY,
        &["--ref-dir", &references],
        output,
        "control",
        deadline,
    )?);
    result.host = outcomes(&bench(
        bench_path,
        HOST_DISPLAY,
        &["--ref-dir", &references, "--save-failures"],
        output,
        "host",
        deadline,
    )?);
    Ok(())
}
