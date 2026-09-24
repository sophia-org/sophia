//! The X11 conformance profiles as a gate.
//!
//! `check.py` is the probe's own entry and stays the thing that runs a
//! profile; what this adds is that a run is produced by a gate on a committed
//! source rather than by whoever remembered, with the verdict, the evidence
//! and the source identity kept together. Nothing here relaxes the probe's
//! own accounting: a profile is judged by the report it wrote, and a report
//! that names another source, or none, cannot pass.

use super::{host, identity, process, types::SourceIdentity, x11bench};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The profiles this gate runs. `core` stays with its own gate.
pub(super) const PROFILES: [&str; 2] = ["xtest", "native-input"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProfileOptions {
    pub profiles: Vec<String>,
    pub output: PathBuf,
    pub target: PathBuf,
    pub timeout: u64,
    pub xts_root: Option<PathBuf>,
    pub xts_expected: Option<PathBuf>,
    pub xts_scenario: Option<String>,
    /// The adapter's own deadline in seconds, handed to it as `--timeout`.
    /// A whole TET scenario is slower than a probe case, and the adapter's
    /// default of two minutes read as TIMEOUT before anything had run.
    pub xts_timeout: u64,
    /// x11bench, built from its own checkout, and the manifest it is judged
    /// by; both or neither.
    pub x11bench_bin: Option<PathBuf>,
    pub x11bench_expected: Option<PathBuf>,
    /// The contained run's own deadline in seconds.
    pub x11bench_timeout: u64,
}

/// The adapter leaves itself 15 seconds for contained host cleanup; the
/// gate leaves the adapter twice that before its own deadline applies.
const XTS_GATE_MARGIN_SECS: u64 = 30;

pub(super) fn options(arguments: &[String]) -> Result<ProfileOptions, String> {
    let mut parsed = ProfileOptions {
        profiles: Vec::new(),
        output: PathBuf::new(),
        target: PathBuf::new(),
        timeout: 1800,
        xts_root: None,
        xts_expected: None,
        xts_scenario: None,
        xts_timeout: 600,
        x11bench_bin: None,
        x11bench_expected: None,
        x11bench_timeout: 600,
    };
    let mut xts_timeout_given = false;
    let mut x11bench_timeout_given = false;
    for argument in arguments {
        let (name, value) = argument
            .split_once('=')
            .ok_or("options require --name=value; no test filters")?;
        match name {
            "--profile" => {
                parsed.profiles = match value {
                    "all" => PROFILES.iter().map(|name| (*name).to_owned()).collect(),
                    name if PROFILES.contains(&name) => vec![name.to_owned()],
                    other => {
                        return Err(format!(
                            "unknown profile {other:?}; this gate runs xtest, native-input or all"
                        ));
                    }
                }
            }
            "--output" => parsed.output = value.into(),
            "--target-dir" => parsed.target = value.into(),
            "--timeout" => parsed.timeout = value.parse().map_err(|_| "invalid timeout")?,
            "--xts-root" => parsed.xts_root = Some(value.into()),
            "--xts-expected" => parsed.xts_expected = Some(value.into()),
            "--xts-scenario" => parsed.xts_scenario = Some(value.to_owned()),
            "--xts-timeout" => {
                parsed.xts_timeout = value.parse().map_err(|_| "invalid XTS timeout")?;
                xts_timeout_given = true;
            }
            "--x11bench-bin" => parsed.x11bench_bin = Some(value.into()),
            "--x11bench-expected" => parsed.x11bench_expected = Some(value.into()),
            "--x11bench-timeout" => {
                parsed.x11bench_timeout = value.parse().map_err(|_| "invalid x11bench timeout")?;
                x11bench_timeout_given = true;
            }
            _ => {
                return Err(format!(
                    "unsupported option {name}; partial acceptance/filtering is forbidden"
                ));
            }
        }
    }
    if parsed.profiles.is_empty() {
        return Err("provide --profile=xtest, --profile=native-input or --profile=all".into());
    }
    if parsed.output.as_os_str().is_empty()
        || parsed.target.as_os_str().is_empty()
        || parsed.timeout == 0
        || parsed.timeout > 1800
    {
        return Err("provide --output and --target-dir with a timeout in 1..=1800 seconds".into());
    }
    let xts_given = [
        parsed.xts_root.is_some(),
        parsed.xts_expected.is_some(),
        parsed.xts_scenario.is_some(),
    ];
    let xts_all = xts_given.iter().all(|given| *given);
    if (xts_given.iter().any(|given| *given) || xts_timeout_given) && !xts_all {
        return Err(
            "XTS needs --xts-root, --xts-expected and --xts-scenario together, or none; --xts-timeout only with them".into(),
        );
    }
    if xts_all
        && (parsed.xts_timeout == 0
            || parsed.xts_timeout > 1785
            || parsed.xts_timeout + XTS_GATE_MARGIN_SECS > parsed.timeout)
    {
        return Err(
            "--xts-timeout must be 1..=1785 seconds and leave the gate 30 seconds of its own timeout"
                .into(),
        );
    }
    let x11bench_all = parsed.x11bench_bin.is_some() && parsed.x11bench_expected.is_some();
    if (parsed.x11bench_bin.is_some()
        || parsed.x11bench_expected.is_some()
        || x11bench_timeout_given)
        && !x11bench_all
    {
        return Err(
            "x11bench needs --x11bench-bin and --x11bench-expected together, or neither; --x11bench-timeout only with them".into(),
        );
    }
    if x11bench_all
        && (parsed.x11bench_timeout == 0
            || parsed.x11bench_timeout + x11bench::GATE_MARGIN_SECS > parsed.timeout)
    {
        return Err(
            "--x11bench-timeout must be at least 1 second and leave the gate 30 seconds of its own timeout"
                .into(),
        );
    }
    Ok(parsed)
}

/// What one profile's run established.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ProfileVerdict {
    pub status: String,
    pub detail: Option<String>,
    pub required: u64,
    pub executed: u64,
    pub failures: Vec<String>,
    pub source_commit: Option<String>,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub report: Option<PathBuf>,
    /// What the gate found still running when the profile's entry returned,
    /// and what it did about it: orphans the subreaper reaped are recorded,
    /// not counted against the profile; anything that could not be reaped
    /// is, and the verdict says so.
    pub descendants_found: usize,
    pub descendants_reaped: usize,
    pub remaining: Vec<u32>,
    pub collection_error: Option<String>,
}

/// Whether the profile's processes were all collected once its entry
/// returned.
///
/// A child of the entry that outlives it by a moment is reparented to this
/// gate, which is the subreaper, and reaped there; that is teardown, not a
/// leak, and the numbers are kept in the report for whoever wants to see
/// them. What cannot be reaped within the collection deadline, or a
/// collection that could not be read, is a process the profile genuinely
/// left behind, and a PASS beside it is not a result.
///
/// This is one of three collection predicates in this gate family, and they
/// differ on purpose. `Execution::clean` in `evidence.rs` requires
/// `descendants_found == 0`: it governs a build or an exact single-test run,
/// where any descendant at all is anomalous. `launcher_collected` requires
/// found to equal reaped: the launcher's own children are its to account
/// for. A profile's entry is a runner that starts hosts and clients of its
/// own, and a child of it still dying when it returns is reaped here rather
/// than leaked, which is why this one asks only that nothing remained.
/// Unifying them would reinstate a demotion of a passing profile that was
/// measured to be teardown, so keep them apart.
pub(super) fn collected(collection: &super::types::Collection) -> bool {
    collection.root_waited && collection.remaining.is_empty() && collection.error.is_none()
}

/// Judge a profile by the report it wrote, against the source that was run.
///
/// THE PROBE'S OWN RULES FIRST, THEN PROVENANCE. `check.py` refuses a report
/// with no required cases, with fewer executed than required, or with any
/// failure, and so does this; on top of that a report that names a different
/// commit, or a dirty tree, is evidence about something else and is
/// NORESULT rather than a pass or a failure of this candidate.
pub(super) fn judge(
    report: Option<&serde_json::Value>,
    exit: Option<i32>,
    timed_out: bool,
    commit: &str,
) -> ProfileVerdict {
    let mut verdict = ProfileVerdict {
        status: "NORESULT".into(),
        detail: None,
        required: 0,
        executed: 0,
        failures: Vec::new(),
        source_commit: None,
        exit,
        timed_out,
        report: None,
        descendants_found: 0,
        descendants_reaped: 0,
        remaining: Vec::new(),
        collection_error: None,
    };
    if timed_out {
        verdict.status = "TIMEOUT".into();
        verdict.detail = Some("the profile's absolute process deadline expired".into());
        return verdict;
    }
    let Some(report) = report else {
        verdict.detail = Some(format!("profile exit {exit:?}; missing or invalid report"));
        return verdict;
    };
    let status = report["status"].as_str().unwrap_or("");
    verdict.required = report["required"].as_u64().unwrap_or(0);
    verdict.executed = report["executed"].as_u64().unwrap_or(0);
    verdict.failures = report["failures"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| match item.as_str() {
                    Some(text) => text.to_owned(),
                    None => item.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    // The two probes record their source in different places: native.py at
    // the top of its report, run.py under `identity`. Both are read, and a
    // report that names no commit at all is judged on its own rules alone.
    let commit_field = if report["source_commit"].is_string() {
        &report["source_commit"]
    } else {
        &report["identity"]["source_commit"]
    };
    let dirty_field = if report["source_dirty"].is_boolean() {
        &report["source_dirty"]
    } else {
        &report["identity"]["source_dirty"]
    };
    verdict.source_commit = commit_field.as_str().map(str::to_owned);
    if let Some(named) = verdict.source_commit.as_deref()
        && named != commit
    {
        verdict.detail = Some(format!(
            "the report names commit {named}, not the candidate {commit}"
        ));
        return verdict;
    }
    if dirty_field.as_bool() == Some(true) {
        verdict.detail = Some("the report says the source it ran was dirty".into());
        return verdict;
    }
    if status.is_empty() || !report["required"].is_u64() || !report["executed"].is_u64() {
        verdict.detail = Some(format!(
            "profile exit {exit:?}; report lacks a status or counts"
        ));
        return verdict;
    }
    if exit != Some(0)
        || status != "PASS"
        || verdict.required == 0
        || verdict.executed != verdict.required
        || !verdict.failures.is_empty()
    {
        verdict.status = "FAIL".into();
        verdict.detail = Some(format!(
            "profile exit {exit:?}, status {status}, {} of {} executed, {} failures",
            verdict.executed,
            verdict.required,
            verdict.failures.len()
        ));
        return verdict;
    }
    verdict.status = "PASS".into();
    verdict
}

/// An external suite -- XTS5 or x11bench -- reported as itself: unrun and
/// BLOCKED without its separate checkout and manifest, never a pass by
/// absence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct SuiteVerdict {
    pub status: String,
    pub reason: String,
    pub exit: Option<i32>,
    pub report: Option<PathBuf>,
}

pub(super) fn xts_blocked(reason: &str) -> SuiteVerdict {
    SuiteVerdict {
        status: "BLOCKED".into(),
        reason: reason.to_owned(),
        exit: None,
        report: None,
    }
}

/// The gate's one verdict over its profiles and external suites.
///
/// Every selected profile must PASS. A suite that was not run is BLOCKED and
/// changes nothing; a suite that was run and did not pass fails the gate.
pub(super) fn overall(
    profiles: &BTreeMap<String, ProfileVerdict>,
    suites: &[&SuiteVerdict],
) -> String {
    if profiles.is_empty() {
        return "NORESULT".into();
    }
    if profiles
        .values()
        .any(|verdict| verdict.status == "NORESULT" || verdict.status == "TIMEOUT")
    {
        return "NORESULT".into();
    }
    if profiles.values().any(|verdict| verdict.status != "PASS") {
        return "FAIL".into();
    }
    if suites
        .iter()
        .all(|suite| matches!(suite.status.as_str(), "PASS" | "BLOCKED"))
    {
        "PASS".into()
    } else {
        "FAIL".into()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ProfileReport {
    pub schema: u32,
    pub gate: String,
    pub run_id: String,
    pub overall: String,
    pub source: SourceIdentity,
    pub build_target_namespace: String,
    pub harness_sha256: BTreeMap<String, String>,
    pub profiles: BTreeMap<String, ProfileVerdict>,
    pub xts5: SuiteVerdict,
    pub x11bench: SuiteVerdict,
    pub source_unchanged_after: bool,
}

pub(super) fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    // The gate's own re-entry inside bubblewrap, never an operator's command.
    if let [flag, rest @ ..] = arguments
        && flag == "--x11bench-contained"
    {
        return x11bench::contained(rest).map(|()| Vec::new());
    }
    process::arm_subreaper()?;
    if arguments.iter().any(|argument| argument == "--help") {
        return Ok(vec![
            "cargo xtask check x11-profile --profile=xtest|native-input|all --output=/NEW/DIR --target-dir=/OWNED/TARGET [--timeout=SECONDS] [--xts-root=/XTS --xts-expected=/PURPOSES.json --xts-scenario=NAME [--xts-timeout=SECONDS]] [--x11bench-bin=/X11BENCH --x11bench-expected=/TESTS.json [--x11bench-timeout=SECONDS]]".into(),
        ]);
    }
    let mut opts = options(arguments)?;
    let temporary =
        std::env::temp_dir().join(format!("x11-profile-git-{}.log", std::process::id()));
    let common = identity::git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        &temporary,
    )?;
    let _ = std::fs::remove_file(&temporary);
    let artifacts = Path::new(&common)
        .parent()
        .ok_or("no repository parent")?
        .join(".artifacts");
    std::fs::create_dir_all(&artifacts).map_err(|e| e.to_string())?;
    let artifacts = artifacts.canonicalize().map_err(|e| e.to_string())?;
    opts.output = host::artifact_path(&opts.output, &artifacts)?;
    opts.target = host::artifact_path(&opts.target, &artifacts)?;
    if opts.output.exists()
        || opts.output.starts_with(&opts.target)
        || opts.target.starts_with(&opts.output)
    {
        return Err("output must be new and cannot overlap the target".into());
    }
    std::fs::create_dir(&opts.output).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&opts.target).map_err(|e| e.to_string())?;
    let lock =
        std::fs::File::create(opts.target.join(".x11-profile.lock")).map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("target already owned by another harness: {e}"))?;
    execute(repo, &opts)
}

fn execute(repo: &Path, opts: &ProfileOptions) -> Result<Vec<String>, String> {
    // THE CANDIDATE IS PINNED BEFORE ANYTHING RUNS, and the profiles run
    // from the repository itself rather than from the snapshot: the probe
    // records the commit it ran from with git, which an extracted archive
    // cannot answer. The snapshot is the retained provenance; the identity
    // is checked again after the run, and the report the probe wrote must
    // name the same commit, or the run is evidence about something else.
    let source = identity::snapshot(repo, &opts.output)?;
    let namespace = host::target_namespace(&source.content_sha256)?;
    let build_target = opts.target.join(&namespace);
    std::fs::create_dir_all(&build_target).map_err(|e| e.to_string())?;
    let probe = repo.join("tools/probes/x11_conformance");
    let mut harness_sha256 = BTreeMap::new();
    for name in [
        "check.py",
        "run.py",
        "native.py",
        "xts.py",
        "isolation.py",
        "xtest_manifest.json",
        "native_manifest.json",
        "x11bench_expected.json",
    ] {
        harness_sha256.insert(name.to_owned(), identity::digest(&probe.join(name))?);
    }
    let run_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let mut profiles = BTreeMap::new();
    for profile in &opts.profiles {
        let output = opts.output.join(profile);
        let log = opts.output.join(format!("{profile}.log"));
        let mut command = probe_command(repo, &probe.join("check.py"));
        command
            .arg("--profile")
            .arg(profile)
            .arg("--output")
            .arg(&output)
            .arg("--target-dir")
            .arg(&build_target);
        let execution = process::run(&mut command, &log, Duration::from_secs(opts.timeout))?;
        let report_path = output.join("report.json");
        let report: Option<serde_json::Value> = std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        let mut verdict = judge(
            report.as_ref(),
            execution.returncode,
            execution.timed_out,
            &source.commit,
        );
        verdict.descendants_found = execution.collection.descendants_found;
        verdict.descendants_reaped = execution.collection.descendants_reaped;
        verdict.remaining = execution.collection.remaining.clone();
        verdict.collection_error = execution.collection.error.clone();
        if !collected(&execution.collection) && verdict.status == "PASS" {
            verdict.status = "NORESULT".into();
            verdict.detail = Some(format!(
                "the profile left processes that could not be collected: {:?} remaining, {:?}",
                execution.collection.remaining, execution.collection.error
            ));
        }
        verdict.report = report.is_some().then_some(report_path);
        profiles.insert(profile.clone(), verdict);
    }
    let xts5 = match (&opts.xts_root, &opts.xts_expected) {
        (Some(root), Some(expected)) => {
            // XTS runs against the core fixture host, which none of this
            // gate's profiles builds; without this the verdict was BLOCKED
            // on every run that asked for XTS, for want of a binary the
            // gate could have built itself.
            build_xts_host(repo, opts, &build_target)?;
            xts(repo, opts, &probe, &build_target, root, expected)?
        }
        _ => xts_blocked(
            "XTS is a separate checkout and a selected-purpose manifest; neither was supplied, so nothing was run",
        ),
    };
    let x11bench = match (&opts.x11bench_bin, &opts.x11bench_expected) {
        (Some(bench), Some(expected)) => {
            // The same core host XTS runs against; built once if both are.
            if opts.xts_root.is_none() {
                build_xts_host(repo, opts, &build_target)?;
            }
            x11bench::run(
                &build_target.join("debug/examples/x11_conformance_host"),
                bench,
                expected,
                &opts.output.join("x11bench"),
                opts.x11bench_timeout,
            )?
        }
        _ => x11bench::blocked(
            "x11bench is a separate checkout and a test manifest; neither was supplied, so nothing was run",
        ),
    };
    let temporary = opts.output.join("git-after.log");
    let source_unchanged_after = identity::git(repo, &["rev-parse", "HEAD"], &temporary)?
        == source.commit
        && identity::git(
            repo,
            &["status", "--porcelain=v1", "--untracked-files=all"],
            &temporary,
        )?
        .is_empty();
    let mut overall = overall(&profiles, &[&xts5, &x11bench]);
    if !source_unchanged_after {
        overall = "NORESULT".into();
    }
    let report = ProfileReport {
        schema: 1,
        gate: "x11-profile".into(),
        run_id,
        overall: overall.clone(),
        source,
        build_target_namespace: namespace,
        harness_sha256,
        profiles,
        xts5,
        x11bench,
        source_unchanged_after,
    };
    let path = opts.output.join("report.json");
    identity::json(&path, &report)?;
    let summary = report
        .profiles
        .iter()
        .map(|(name, verdict)| {
            format!(
                "{name} {} ({}/{} executed)",
                verdict.status, verdict.executed, verdict.required
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let line = format!(
        "xtask: X11 profiles: {overall}; {summary}; XTS5 {} ({}); x11bench {} ({}); {}",
        report.xts5.status,
        report.xts5.reason,
        report.x11bench.status,
        report.x11bench.reason,
        path.display()
    );
    if overall == "PASS" {
        Ok(vec![line])
    } else {
        Err(format!("{line}; acceptance not established"))
    }
}

/// The probe's entry, run with this process's environment less every
/// live-session opt-in, exactly as `check.py` clears it for its children.
fn probe_command(repo: &Path, script: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("python3");
    command.current_dir(repo).arg("-B").arg(script);
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("SOPHIA_")
            || name.starts_with("HAGIA_")
            || name.starts_with("DBUS_")
            || name.starts_with("XDG_")
            || matches!(
                name.as_str(),
                "DISPLAY" | "XAUTHORITY" | "WAYLAND_DISPLAY" | "WAYLAND_SOCKET" | "PYTHONOPTIMIZE"
            )
        {
            command.env_remove(&name);
        }
    }
    command
}

/// Build `x11_conformance_host` into the gate's own target namespace, from
/// the repository the profiles run from, with the same environment hygiene.
fn build_xts_host(repo: &Path, opts: &ProfileOptions, build_target: &Path) -> Result<(), String> {
    let mut command = std::process::Command::new("cargo");
    command
        .current_dir(repo)
        .args([
            "build",
            "--offline",
            "-p",
            "sophia-x-authority",
            "--example",
            "x11_conformance_host",
        ])
        .env("CARGO_TARGET_DIR", build_target);
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("SOPHIA_") || name.starts_with("HAGIA_") {
            command.env_remove(&name);
        }
    }
    let execution = process::run(
        &mut command,
        &opts.output.join("xts5-host-build.log"),
        Duration::from_secs(opts.timeout.min(1800)),
    )?;
    if execution.timed_out || execution.returncode != Some(0) {
        return Err(format!(
            "building x11_conformance_host for XTS failed: exit {:?}, timed_out={}",
            execution.returncode, execution.timed_out
        ));
    }
    Ok(())
}

fn xts(
    repo: &Path,
    opts: &ProfileOptions,
    probe: &Path,
    build_target: &Path,
    root: &Path,
    expected: &Path,
) -> Result<SuiteVerdict, String> {
    let host = build_target.join("debug/examples/x11_conformance_host");
    if !host.is_file() {
        return Ok(xts_blocked(
            "the core host was not built in this target; XTS runs against x11_conformance_host, which the core profile builds",
        ));
    }
    let output = opts.output.join("xts5");
    let mut command = probe_command(repo, &probe.join("xts.py"));
    command
        .arg("--host")
        .arg(&host)
        .arg("--xts-root")
        .arg(root)
        .arg("--expected")
        .arg(expected)
        .arg("--output")
        .arg(&output)
        .arg("--timeout")
        .arg(opts.xts_timeout.to_string());
    if let Some(scenario) = &opts.xts_scenario {
        command.arg("--scenario").arg(scenario);
    }
    let execution = process::run(
        &mut command,
        &opts.output.join("xts5.log"),
        Duration::from_secs(opts.xts_timeout + XTS_GATE_MARGIN_SECS),
    )?;
    let report_path = output.join("report.json");
    let report = std::fs::read_to_string(&report_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let status = report
        .as_ref()
        .and_then(|report| report["status"].as_str().map(str::to_owned));
    // How much of a PASS is declaration, in the verdict itself: a manifest
    // may declare dispositions the suite or the authority cannot pass today,
    // each with a reason, and a PASS that met them must say so.
    let accounting = report.as_ref().map(|report| {
        let passed = report["passed"].as_u64().unwrap_or(0);
        let declared: u64 = report["declared"]
            .as_object()
            .map(|d| d.values().filter_map(serde_json::Value::as_u64).sum())
            .unwrap_or(0);
        format!("{passed} passed, {declared} declared")
    });
    Ok(
        match (execution.timed_out, execution.returncode, status.as_deref()) {
            (true, _, _) => SuiteVerdict {
                status: "TIMEOUT".into(),
                reason: "the adapter's absolute process deadline expired".into(),
                exit: None,
                report: None,
            },
            (false, Some(0), Some("PASS")) => SuiteVerdict {
                status: "PASS".into(),
                reason: format!(
                    "every manifested purpose started and met its expectation ({})",
                    accounting.as_deref().unwrap_or("unaccounted")
                ),
                exit: Some(0),
                report: Some(report_path),
            },
            (false, Some(2), _) | (false, _, Some("BLOCKED")) => SuiteVerdict {
                status: "BLOCKED".into(),
                reason: "the adapter reported missing dependencies; see its report".into(),
                exit: execution.returncode,
                report: report_path.is_file().then_some(report_path),
            },
            (false, exit, status) => SuiteVerdict {
                status: "FAIL".into(),
                reason: format!("adapter exit {exit:?} with status {status:?}"),
                exit,
                report: report_path.is_file().then_some(report_path),
            },
        },
    )
}
