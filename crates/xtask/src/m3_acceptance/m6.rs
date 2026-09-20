//! M6: the whole of t093's headless evidence on one exact source.
//!
//! Nothing here asserts a behaviour. It runs the gates that already exist,
//! each with its own snapshot and its own report, on the one committed source
//! this run pins, and composes their verdicts into one. Two results it does
//! not produce, the core baseline owned by t057 and the contained canonical
//! workspace run, are cited by report path and labelled as citations: the
//! composition checks that each names this source and reports its own
//! verdict, and stands behind neither.
//!
//! THE RULE THAT HOLDS THROUGHOUT: a component that did not run, errored,
//! wrote no report, or ran on any other bytes is NORESULT, and NORESULT is
//! not PASS. Absence can never make the overall read better.

use super::{host, identity, process, types::SourceIdentity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The gates this run produces, in the order they run.
pub(super) const COMPONENTS: [&str; 4] = [
    "m3-acceptance",
    "m4-acceptance",
    "m5-acceptance",
    "x11-profile",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EvidenceOptions {
    pub output: PathBuf,
    pub target: PathBuf,
    pub timeout: u64,
    pub core_report: Option<PathBuf>,
    pub canonical_report: Option<PathBuf>,
    pub xts_root: Option<PathBuf>,
    pub xts_expected: Option<PathBuf>,
}

pub(super) fn options(arguments: &[String]) -> Result<EvidenceOptions, String> {
    let mut parsed = EvidenceOptions {
        output: PathBuf::new(),
        target: PathBuf::new(),
        timeout: 1800,
        core_report: None,
        canonical_report: None,
        xts_root: None,
        xts_expected: None,
    };
    for argument in arguments {
        let (name, value) = argument
            .split_once('=')
            .ok_or("options require --name=value; no test filters")?;
        match name {
            "--output" => parsed.output = value.into(),
            "--target-dir" => parsed.target = value.into(),
            "--timeout" => parsed.timeout = value.parse().map_err(|_| "invalid timeout")?,
            "--core-report" => parsed.core_report = Some(value.into()),
            "--canonical-report" => parsed.canonical_report = Some(value.into()),
            "--xts-root" => parsed.xts_root = Some(value.into()),
            "--xts-expected" => parsed.xts_expected = Some(value.into()),
            _ => {
                return Err(format!(
                    "unsupported option {name}; partial evidence/filtering is forbidden"
                ));
            }
        }
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

/// One produced component: the gate's own verdict, and whether it counts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ComponentVerdict {
    /// PASS, FAIL or NORESULT, as this composition counts it.
    pub verdict: String,
    /// What the gate's own report said, verbatim, when there was one.
    pub gate_overall: Option<String>,
    pub detail: Option<String>,
    pub report: Option<PathBuf>,
    pub source_commit: Option<String>,
    pub archive_sha256: Option<String>,
    pub content_sha256: Option<String>,
    /// Whether the gate's snapshot is byte-identical to this run's.
    pub identical_source: bool,
    pub exit: Option<i32>,
    pub timed_out: bool,
}

/// Judge a component by the report its gate wrote, against this run's source.
///
/// DIGESTS, NOT NAMES. Every gate refuses a dirty tree and snapshots its own
/// source, so the snapshots have to be byte-identical and that is checked
/// rather than trusted: the same commit name is satisfied by a dirty tree
/// too, and the digests are the cheap check that is not.
pub(super) fn judge_component(
    report: Option<&serde_json::Value>,
    exit: Option<i32>,
    timed_out: bool,
    source: &SourceIdentity,
) -> ComponentVerdict {
    let mut verdict = ComponentVerdict {
        verdict: "NORESULT".into(),
        gate_overall: None,
        detail: None,
        report: None,
        source_commit: None,
        archive_sha256: None,
        content_sha256: None,
        identical_source: false,
        exit,
        timed_out,
    };
    if timed_out {
        verdict.detail = Some("the gate's absolute process deadline expired".into());
        return verdict;
    }
    let Some(report) = report else {
        verdict.detail = Some(format!("gate exit {exit:?}; missing or invalid report"));
        return verdict;
    };
    verdict.gate_overall = report["overall"].as_str().map(str::to_owned);
    verdict.source_commit = report["source"]["commit"].as_str().map(str::to_owned);
    verdict.archive_sha256 = report["source"]["archive_sha256"]
        .as_str()
        .map(str::to_owned);
    verdict.content_sha256 = report["source"]["content_sha256"]
        .as_str()
        .map(str::to_owned);
    verdict.identical_source = verdict.source_commit.as_deref() == Some(source.commit.as_str())
        && verdict.archive_sha256.as_deref() == Some(source.archive_sha256.as_str())
        && verdict.content_sha256.as_deref() == Some(source.content_sha256.as_str())
        && report["source"]["clean"].as_bool() == Some(true);
    if !verdict.identical_source {
        verdict.detail = Some("the gate's snapshot is not this run's source, byte for byte".into());
        return verdict;
    }
    match verdict.gate_overall.as_deref() {
        Some("PASS") if exit == Some(0) => verdict.verdict = "PASS".into(),
        Some("PASS") => {
            verdict.detail = Some(format!("the gate reported PASS but exited {exit:?}"));
        }
        Some("FAIL") => verdict.verdict = "FAIL".into(),
        other => {
            verdict.detail = Some(format!(
                "the gate reported {other:?}, which is not a result"
            ));
        }
    }
    verdict
}

/// A result this run did not produce: read from the path given, checked
/// against this source, and labelled for what it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Citation {
    pub kind: String,
    pub owner: String,
    pub path: Option<PathBuf>,
    /// PASS, FAIL or NORESULT, as this composition counts it.
    pub verdict: String,
    /// The cited report's own status, verbatim.
    pub cited_status: Option<String>,
    pub cited_commit: Option<String>,
    pub identity_verified: bool,
    pub detail: Option<String>,
}

fn citation(kind: &str, owner: &str, path: Option<&Path>) -> Citation {
    Citation {
        kind: kind.to_owned(),
        owner: owner.to_owned(),
        path: path.map(Path::to_path_buf),
        verdict: "NORESULT".into(),
        cited_status: None,
        cited_commit: None,
        identity_verified: false,
        detail: None,
    }
}

/// The core baseline, owned by t057: `check.py --profile core` on this
/// commit, with the probe's own accounting.
pub(super) fn cite_core(
    report: Option<&serde_json::Value>,
    path: Option<&Path>,
    commit: &str,
) -> Citation {
    let mut cited = citation("core X11 conformance baseline", "t057", path);
    let Some(report) = report else {
        cited.detail = Some(match path {
            Some(_) => "the cited report could not be read".into(),
            None => "no --core-report was supplied; the baseline was not cited".into(),
        });
        return cited;
    };
    cited.cited_status = report["status"].as_str().map(str::to_owned);
    cited.cited_commit = report["identity"]["source_commit"]
        .as_str()
        .map(str::to_owned);
    cited.identity_verified = cited.cited_commit.as_deref() == Some(commit)
        && report["identity"]["source_dirty"].as_bool() == Some(false);
    if !cited.identity_verified {
        cited.detail = Some("the cited report does not name this commit on a clean tree".into());
        return cited;
    }
    let required = report["required"].as_u64().unwrap_or(0);
    let executed = report["executed"].as_u64().unwrap_or(0);
    let failures = report["failures"].as_array().map_or(0, Vec::len);
    match cited.cited_status.as_deref() {
        Some("PASS") if required > 0 && executed == required && failures == 0 => {
            cited.verdict = "PASS".into();
        }
        Some("PASS") => {
            cited.verdict = "FAIL".into();
            cited.detail = Some(format!(
                "cited PASS with {executed} of {required} executed and {failures} failures"
            ));
        }
        Some("FAIL") => cited.verdict = "FAIL".into(),
        other => cited.detail = Some(format!("the cited report says {other:?}")),
    }
    cited
}

/// The contained canonical workspace run, from `offline_check.py`, on this
/// commit and with the full check actually executed.
pub(super) fn cite_canonical(
    report: Option<&serde_json::Value>,
    path: Option<&Path>,
    commit: &str,
) -> Citation {
    let mut cited = citation(
        "contained canonical workspace check",
        "offline_check.py",
        path,
    );
    let Some(report) = report else {
        cited.detail = Some(match path {
            Some(_) => "the cited report could not be read".into(),
            None => "no --canonical-report was supplied; the workspace run was not cited".into(),
        });
        return cited;
    };
    cited.cited_status = report["status"].as_str().map(str::to_owned);
    cited.cited_commit = report["provenance"]["commit"].as_str().map(str::to_owned);
    cited.identity_verified = cited.cited_commit.as_deref() == Some(commit)
        && report["provenance"]["dirty"].as_bool() == Some(false);
    if !cited.identity_verified {
        cited.detail = Some("the cited report does not name this commit on a clean tree".into());
        return cited;
    }
    match (
        cited.cited_status.as_deref(),
        report["full_check_executed"].as_bool(),
    ) {
        (Some("PASS"), Some(true)) => cited.verdict = "PASS".into(),
        (Some("PASS"), _) => {
            cited.detail = Some("cited PASS without the full check executed".into());
        }
        (Some("FAIL"), _) => cited.verdict = "FAIL".into(),
        (other, _) => cited.detail = Some(format!("the cited report says {other:?}")),
    }
    cited
}

/// One verdict over everything: NORESULT if anything is not a result,
/// FAIL if anything failed, PASS only when every component and citation
/// passed on this source.
pub(super) fn overall(
    components: &BTreeMap<String, ComponentVerdict>,
    citations: &[&Citation],
) -> String {
    let verdicts = components
        .values()
        .map(|component| component.verdict.as_str())
        .chain(citations.iter().map(|cited| cited.verdict.as_str()))
        .collect::<Vec<_>>();
    if verdicts.len() < COMPONENTS.len() + 2
        || verdicts.iter().any(|v| *v != "PASS" && *v != "FAIL")
    {
        return "NORESULT".into();
    }
    if verdicts.contains(&"FAIL") {
        return "FAIL".into();
    }
    "PASS".into()
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct EvidenceReport {
    pub schema: u32,
    pub gate: String,
    pub run_id: String,
    pub overall: String,
    pub source: SourceIdentity,
    pub components: BTreeMap<String, ComponentVerdict>,
    pub core: Citation,
    pub canonical: Citation,
    pub source_unchanged_after: bool,
}

pub(super) fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    if arguments.iter().any(|argument| argument == "--help") {
        return Ok(vec![
            "cargo xtask check m6-evidence --output=/NEW/DIR --target-dir=/OWNED/TARGET [--timeout=SECONDS] [--core-report=/PATH/report.json] [--canonical-report=/PATH/report.json] [--xts-root=/XTS --xts-expected=/PURPOSES.json]".into(),
        ]);
    }
    let mut opts = options(arguments)?;
    let temporary = std::env::temp_dir().join(format!("m6-git-{}.log", std::process::id()));
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
        std::fs::File::create(opts.target.join(".m6-evidence.lock")).map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("target already owned by another harness: {e}"))?;
    execute(repo, &opts)
}

fn read_report(path: &Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn execute(repo: &Path, opts: &EvidenceOptions) -> Result<Vec<String>, String> {
    let source = identity::snapshot(repo, &opts.output)?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let run_id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let mut components = BTreeMap::new();
    for name in COMPONENTS {
        let output = opts.output.join(name);
        let mut command = std::process::Command::new(&executable);
        command
            .current_dir(repo)
            .arg("check")
            .arg(name)
            .arg(format!("--output={}", output.display()))
            .arg(format!("--target-dir={}", opts.target.display()));
        if name == "x11-profile" {
            command.arg("--profile=all");
            if let (Some(root), Some(expected)) = (&opts.xts_root, &opts.xts_expected) {
                command
                    .arg(format!("--xts-root={}", root.display()))
                    .arg(format!("--xts-expected={}", expected.display()));
            }
        }
        let execution = process::run(
            &mut command,
            &opts.output.join(format!("{name}.log")),
            Duration::from_secs(opts.timeout),
        )?;
        let report_path = output.join("report.json");
        let report = read_report(&report_path);
        let mut verdict = judge_component(
            report.as_ref(),
            execution.returncode,
            execution.timed_out,
            &source,
        );
        verdict.report = report.is_some().then_some(report_path);
        components.insert(name.to_owned(), verdict);
    }
    let core = cite_core(
        opts.core_report.as_deref().and_then(read_report).as_ref(),
        opts.core_report.as_deref(),
        &source.commit,
    );
    let canonical = cite_canonical(
        opts.canonical_report
            .as_deref()
            .and_then(read_report)
            .as_ref(),
        opts.canonical_report.as_deref(),
        &source.commit,
    );
    let temporary = opts.output.join("git-after.log");
    let source_unchanged_after = identity::git(repo, &["rev-parse", "HEAD"], &temporary)?
        == source.commit
        && identity::git(
            repo,
            &["status", "--porcelain=v1", "--untracked-files=all"],
            &temporary,
        )?
        .is_empty();
    let mut verdict = overall(&components, &[&core, &canonical]);
    if !source_unchanged_after {
        verdict = "NORESULT".into();
    }
    let report = EvidenceReport {
        schema: 1,
        gate: "m6-evidence".into(),
        run_id,
        overall: verdict.clone(),
        source,
        components,
        core,
        canonical,
        source_unchanged_after,
    };
    let path = opts.output.join("report.json");
    identity::json(&path, &report)?;
    let summary = report
        .components
        .iter()
        .map(|(name, component)| format!("{name} {}", component.verdict))
        .chain([
            format!("core (cited) {}", report.core.verdict),
            format!("canonical (cited) {}", report.canonical.verdict),
        ])
        .collect::<Vec<_>>()
        .join(", ");
    let line = format!(
        "xtask: M6 evidence: {verdict}; {summary}; {}",
        path.display()
    );
    if verdict == "PASS" {
        Ok(vec![line])
    } else {
        Err(format!("{line}; not established"))
    }
}
