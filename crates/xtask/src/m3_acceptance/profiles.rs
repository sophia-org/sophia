//! The X11 conformance profiles as a gate.
//!
//! `check.py` is the probe's own entry and stays the thing that runs a
//! profile; what this adds is that a run is produced by a gate on a committed
//! source rather than by whoever remembered, with the verdict, the evidence
//! and the source identity kept together. Nothing here relaxes the probe's
//! own accounting: a profile is judged by the report it wrote, and a report
//! that names another source, or none, cannot pass.

use super::{host, identity, process, types::SourceIdentity};
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
}

pub(super) fn options(arguments: &[String]) -> Result<ProfileOptions, String> {
    let mut parsed = ProfileOptions {
        profiles: Vec::new(),
        output: PathBuf::new(),
        target: PathBuf::new(),
        timeout: 1800,
        xts_root: None,
        xts_expected: None,
        xts_scenario: None,
    };
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
    if parsed.xts_root.is_some() != parsed.xts_expected.is_some() {
        return Err("XTS needs both --xts-root and --xts-expected, or neither".into());
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

/// XTS5, reported as itself: unrun and BLOCKED without the separate
/// checkout and the exact selected purposes, never a pass by absence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct XtsVerdict {
    pub status: String,
    pub reason: String,
    pub exit: Option<i32>,
    pub report: Option<PathBuf>,
}

pub(super) fn xts_blocked(reason: &str) -> XtsVerdict {
    XtsVerdict {
        status: "BLOCKED".into(),
        reason: reason.to_owned(),
        exit: None,
        report: None,
    }
}

/// The gate's one verdict over its profiles and XTS.
///
/// Every selected profile must PASS. XTS that was not run is BLOCKED and
/// changes nothing; XTS that was run and did not pass fails the gate.
pub(super) fn overall(profiles: &BTreeMap<String, ProfileVerdict>, xts: &XtsVerdict) -> String {
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
    match xts.status.as_str() {
        "PASS" | "BLOCKED" => "PASS".into(),
        _ => "FAIL".into(),
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
    pub xts5: XtsVerdict,
    pub source_unchanged_after: bool,
}

pub(super) fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    if arguments.iter().any(|argument| argument == "--help") {
        return Ok(vec![
            "cargo xtask check x11-profile --profile=xtest|native-input|all --output=/NEW/DIR --target-dir=/OWNED/TARGET [--timeout=SECONDS] [--xts-root=/XTS --xts-expected=/PURPOSES.json [--xts-scenario=NAME]]".into(),
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
        let collected = execution.collection.root_waited
            && execution.collection.descendants_found == 0
            && execution.collection.remaining.is_empty()
            && execution.collection.error.is_none();
        if !collected && verdict.status == "PASS" {
            verdict.status = "NORESULT".into();
            verdict.detail = Some("the profile left processes uncollected".into());
        }
        verdict.report = report.is_some().then_some(report_path);
        profiles.insert(profile.clone(), verdict);
    }
    let xts5 = match (&opts.xts_root, &opts.xts_expected) {
        (Some(root), Some(expected)) => xts(repo, opts, &probe, &build_target, root, expected)?,
        _ => xts_blocked(
            "XTS is a separate checkout and a selected-purpose manifest; neither was supplied, so nothing was run",
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
    let mut overall = overall(&profiles, &xts5);
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
        "xtask: X11 profiles: {overall}; {summary}; XTS5 {}; {}",
        report.xts5.status,
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

fn xts(
    repo: &Path,
    opts: &ProfileOptions,
    probe: &Path,
    build_target: &Path,
    root: &Path,
    expected: &Path,
) -> Result<XtsVerdict, String> {
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
        .arg(&output);
    if let Some(scenario) = &opts.xts_scenario {
        command.arg("--scenario").arg(scenario);
    }
    let execution = process::run(
        &mut command,
        &opts.output.join("xts5.log"),
        Duration::from_secs(opts.timeout),
    )?;
    let report_path = output.join("report.json");
    let status = std::fs::read_to_string(&report_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|report| report["status"].as_str().map(str::to_owned));
    Ok(
        match (execution.timed_out, execution.returncode, status.as_deref()) {
            (true, _, _) => XtsVerdict {
                status: "TIMEOUT".into(),
                reason: "the adapter's absolute process deadline expired".into(),
                exit: None,
                report: None,
            },
            (false, Some(0), Some("PASS")) => XtsVerdict {
                status: "PASS".into(),
                reason: "every declared purpose started and finished PASS".into(),
                exit: Some(0),
                report: Some(report_path),
            },
            (false, Some(2), _) | (false, _, Some("BLOCKED")) => XtsVerdict {
                status: "BLOCKED".into(),
                reason: "the adapter reported missing dependencies; see its report".into(),
                exit: execution.returncode,
                report: report_path.is_file().then_some(report_path),
            },
            (false, exit, status) => XtsVerdict {
                status: "FAIL".into(),
                reason: format!("adapter exit {exit:?} with status {status:?}"),
                exit,
                report: report_path.is_file().then_some(report_path),
            },
        },
    )
}
