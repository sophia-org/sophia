use super::{identity, process, types::*};
use serde_json::json;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn options(arguments: &[String]) -> Result<Options, String> {
    let mut parsed = Options {
        output: PathBuf::new(),
        target: PathBuf::new(),
        registry: std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME missing")?
            .join(".cargo/registry"),
        self_test: false,
        timeout: 1800,
        build_timeout: 900,
        case_timeout: 60,
    };
    for argument in arguments {
        if argument == "--self-test" {
            parsed.self_test = true;
            continue;
        }
        let (name, value) = argument
            .split_once('=')
            .ok_or("options require --name=value; no test filters")?;
        match name {
            "--output" => parsed.output = value.into(),
            "--target-dir" => parsed.target = value.into(),
            "--registry" => parsed.registry = value.into(),
            "--timeout" => parsed.timeout = value.parse().map_err(|_| "invalid timeout")?,
            "--build-timeout" => {
                parsed.build_timeout = value.parse().map_err(|_| "invalid build timeout")?
            }
            "--case-timeout" => {
                parsed.case_timeout = value.parse().map_err(|_| "invalid case timeout")?
            }
            _ => {
                return Err(format!(
                    "unsupported option {name}; partial acceptance/filtering is forbidden"
                ));
            }
        }
    }
    if parsed.output.as_os_str().is_empty()
        || parsed.target.as_os_str().is_empty()
        || [parsed.timeout, parsed.build_timeout, parsed.case_timeout]
            .into_iter()
            .any(|n| n == 0 || n > 1800)
    {
        return Err("provide --output and --target-dir with timeouts in 1..=1800 seconds".into());
    }
    Ok(parsed)
}

pub(super) fn artifact_path(path: &Path, artifacts: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path.components().any(|part| part == Component::ParentDir)
        || !path.starts_with(artifacts)
        || path == artifacts
    {
        return Err(format!(
            "output and target must be disk-backed children of {}",
            artifacts.display()
        ));
    }
    let parent = path.parent().ok_or("artifact path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let actual = parent
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join(path.file_name().ok_or("no filename")?);
    if !actual.starts_with(artifacts) {
        return Err("artifact path escapes through a symlink".into());
    }
    Ok(actual)
}

pub(super) fn run(
    repo: &Path,
    arguments: &[String],
    suite: Option<&str>,
) -> Result<Vec<String>, String> {
    run_for(repo, arguments, suite, Gate::M3)
}

pub(super) fn run_for(
    repo: &Path,
    arguments: &[String],
    suite: Option<&str>,
    gate: Gate,
) -> Result<Vec<String>, String> {
    if gate != Gate::M3 && suite.is_some() {
        return Err("M4 and M5 acceptance do not accept an M3 component suite".into());
    }
    let mut opts = options(arguments)?;
    if gate == Gate::M4
        && !arguments
            .iter()
            .any(|arg| arg.starts_with("--case-timeout="))
    {
        // Integrity checks hash the actual Session, host and auxiliary debug
        // executables. Keep a bounded default that includes that work at the
        // reduced priority used alongside an interactive desktop.
        opts.case_timeout = 180;
    }
    if suite.is_some() && opts.self_test {
        return Err("component suites and harness self-tests are separate runs".into());
    }
    // The common repository path is read on the host, never mounted inside.
    let temporary = std::env::temp_dir().join(format!("m3-git-{}.log", std::process::id()));
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
    opts.output = artifact_path(&opts.output, &artifacts)?;
    opts.target = artifact_path(&opts.target, &artifacts)?;
    if opts.output.exists()
        || opts.output.starts_with(&opts.target)
        || opts.target.starts_with(&opts.output)
    {
        return Err("output must be new and cannot overlap the target".into());
    }
    std::fs::create_dir(&opts.output).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&opts.target).map_err(|e| e.to_string())?;
    let lock = std::fs::File::create(opts.target.join(".m3-acceptance.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|e| format!("target already owned by another harness: {e}"))?;
    execute(repo, &opts, suite, gate)
}

fn execute(
    repo: &Path,
    opts: &Options,
    suite: Option<&str>,
    gate: Gate,
) -> Result<Vec<String>, String> {
    let source = identity::snapshot(repo, &opts.output)?;
    let build_target_namespace = target_namespace(&source.content_sha256)?;
    let build_target = opts.target.join(&build_target_namespace);
    if std::fs::symlink_metadata(&build_target)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("source target namespace must not be a symlink".into());
    }
    std::fs::create_dir_all(&build_target).map_err(|e| e.to_string())?;
    let snapshot = opts.output.join("source");
    let harness = snapshot.join(gate.directory());
    let toolchain = identity::toolchain(repo, &opts.output)?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let wrapper = build_cache_wrapper()?;
    let config = Config {
        schema: 1,
        gate,
        run_id: format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ),
        self_test: opts.self_test,
        component_suite: suite.map(str::to_owned),
        component_tests: suite
            .map(|suite| super::components::suite(&snapshot, suite))
            .transpose()?
            .unwrap_or_default(),
        build_timeout: opts.build_timeout,
        case_timeout: opts.case_timeout,
        build_target_namespace,
        source,
        host_namespaces: identity::namespaces()?,
        inventory_sha256: identity::digest(&harness.join("inventory.json"))?,
        bindings_sha256: identity::digest(&harness.join("bindings.json"))?,
        xtask_sha256: identity::digest(&executable)?,
        toolchain_sha256: ["cargo", "rustc", "rustdoc"]
            .into_iter()
            .map(|name| {
                Ok((
                    name.to_owned(),
                    identity::digest(&toolchain.join("bin").join(name))?,
                ))
            })
            .collect::<Result<_, String>>()?,
        build_cache_sha256: wrapper.as_deref().map(identity::digest).transpose()?,
    };
    identity::json(&opts.output.join("config.json"), &config)?;
    let inventory = super::m4::inventory(gate, &harness.join("inventory.json"))?;
    let mut report = super::worker::initial(&config, &inventory, &opts.output.join("config.json"))?;
    identity::json(&opts.output.join("report.json"), &report)?;
    let outcome = launch(
        opts,
        &snapshot,
        &toolchain,
        &executable,
        &build_target,
        gate,
        wrapper.as_deref(),
    );
    match outcome {
        Ok(execution) => {
            let inner = opts.output.join("evidence/inner-report.json");
            match identity::read_json::<Report>(&inner) {
                Ok(received) => {
                    report = received;
                    report.launcher = Some(execution);
                }
                Err(error) => {
                    if report.components.is_none() {
                        report.overall = Verdict::Blocked;
                    }
                    report.harness_error = Some(error);
                }
            }
            if let Err(error) =
                super::worker::validate_report(&report, &config, &opts.output.join("config.json"))
            {
                if report.components.is_none() {
                    report.overall = Verdict::Fail;
                }
                report.harness_error = Some(error);
            }
        }
        Err(error) => {
            if report.components.is_none() {
                report.overall = Verdict::Blocked;
            }
            report.harness_error = Some(error);
        }
    }
    let success = super::worker::successful(&report, &config);
    if gate == Gate::M4
        && success
        && let Err(error) = super::m4::validate_binary(&report, &opts.output)
    {
        report.overall = Verdict::Fail;
        report.harness_error = Some(error);
    }
    if gate == Gate::M5
        && success
        && let Err(error) = super::m5::validate_binary(&report, &opts.output)
    {
        report.overall = Verdict::Fail;
        report.harness_error = Some(error);
    }
    let success = super::worker::successful(&report, &config);
    identity::json(&opts.output.join("report.json"), &report)?;
    if let Some(components) = &report.components {
        let summary = format!(
            "M3 components {}: {:?}; {}/{} exact controls passed; acceptance NOT_RUN; {}",
            components.suite,
            components.verdict,
            components
                .tests
                .iter()
                .filter(|row| row.status == Verdict::Pass)
                .count(),
            components.tests.len(),
            opts.output.join("report.json").display()
        );
        return if success {
            Ok(vec![summary])
        } else {
            Err(summary)
        };
    }
    let summary = format!(
        "{:?} acceptance: {:?}; {}/{} cases passed; {}",
        gate,
        report.overall,
        report
            .cases
            .iter()
            .filter(|row| row.status == Verdict::Pass)
            .count(),
        inventory.cases.len(),
        opts.output.join("report.json").display()
    );
    if success {
        Ok(vec![
            summary,
            "Harness self-tests are separate from acceptance".into(),
        ])
    } else {
        Err(format!("{summary}; acceptance not established"))
    }
}

/// Cargo sees every snapshot at the same contained pathname. Archive mtimes
/// can be older than a previous build, so its normal incremental timestamp
/// checks cannot distinguish those sources. Only identical snapshot contents
/// may share a target; commit names and timestamps are not content identity.
pub(super) fn target_namespace(content_sha256: &str) -> Result<String, String> {
    if content_sha256.len() != 64
        || !content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("build target requires the attested snapshot content SHA-256".into());
    }
    Ok(format!("source-{content_sha256}"))
}

/// The compiler wrapper a contained build runs through, when one is asked for.
///
/// Off unless requested. Putting a cache in an attestation path is a decision
/// someone makes, not something that switches itself on because a binary
/// happens to be installed on the host.
fn build_cache_wrapper() -> Result<Option<PathBuf>, String> {
    match std::env::var("SOPHIA_ACCEPTANCE_BUILD_CACHE").as_deref() {
        Ok("1" | "true" | "yes") => {}
        _ => return Ok(None),
    }
    let wrapper = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("kache"))
        .find(|path| path.is_file())
        .ok_or("SOPHIA_ACCEPTANCE_BUILD_CACHE is set but kache is not on PATH")?;
    wrapper.canonicalize().map(Some).map_err(|e| e.to_string())
}

fn launch(
    opts: &Options,
    source: &Path,
    toolchain: &Path,
    executable: &Path,
    build_target: &Path,
    gate: Gate,
    wrapper: Option<&Path>,
) -> Result<Execution, String> {
    let registry = opts.registry.canonicalize().map_err(|e| e.to_string())?;
    if ["cache", "index", "src"]
        .into_iter()
        .any(|part| !registry.join(part).is_dir())
    {
        return Err("offline registry must contain cache, index and src".into());
    }
    for name in ["evidence", "cargo"] {
        std::fs::create_dir(opts.output.join(name)).map_err(|e| e.to_string())?;
    }
    let mounts = [
        (source.to_owned(), "/work/source", false), (build_target.to_owned(), "/work/target", true),
        (opts.output.join("cargo"), "/work/cargo", true), (opts.output.join("evidence"), "/work/evidence", true),
        (opts.output.join("config.json"), "/work/config.json", false),
        (toolchain.to_owned(), "/work/toolchain", false), (registry, "/work/registry", false),
        (PathBuf::from("/usr/include"), "/work/include", false),
        (executable.to_owned(), "/work/xtask", false),
        (source.join("tools/probes/x11_conformance"), "/work/isolation", false),
        (source.join("tools/probes/m3_acceptance/containment.py"), "/work/containment.py", false),
    ].into_iter().map(|(source, destination, writable)| json!({"source":source,"destination":destination,"writable":writable})).collect::<Vec<_>>();
    // The wrapper's store outlives the run, so it sits beside the build target
    // namespaces under the same exclusive lock rather than inside the output
    // directory, which must be new for every run.
    let mut mounts = mounts;
    if let Some(wrapper) = wrapper {
        let store = opts.target.join("build-cache");
        std::fs::create_dir_all(&store).map_err(|e| e.to_string())?;
        mounts.push(
            json!({"source":wrapper,"destination":"/work/build-cache-wrapper","writable":false}),
        );
        mounts.push(json!({"source":store,"destination":"/work/build-cache","writable":true}));
    }
    let plan = opts.output.join("launch.json");
    let bwrap = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("bwrap"))
        .find(|path| path.is_file())
        .ok_or("bubblewrap unavailable; containment is required")?;
    identity::json(
        &plan,
        &json!({"mounts":mounts,"timeout":opts.timeout,
        "bwrap":bwrap,"gate":gate.command(),
        "isolation_directory":source.join("tools/probes/x11_conformance")}),
    )?;
    process::run(
        process::private_command("/usr/bin/python3")
            .arg("-B")
            .arg(source.join("tools/probes/m3_acceptance/containment.py"))
            .arg(plan),
        &opts.output.join("launcher.log"),
        Duration::from_secs(opts.timeout),
    )
}
