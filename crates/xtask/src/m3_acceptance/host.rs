use super::{catalog, identity, process, types::*};
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

fn artifact_path(path: &Path, artifacts: &Path) -> Result<PathBuf, String> {
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
    let mut opts = options(arguments)?;
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
    execute(repo, &opts, suite)
}

fn execute(repo: &Path, opts: &Options, suite: Option<&str>) -> Result<Vec<String>, String> {
    let source = identity::snapshot(repo, &opts.output)?;
    let snapshot = opts.output.join("source");
    let harness = snapshot.join("tools/probes/m3_acceptance");
    let toolchain = identity::toolchain(repo, &opts.output)?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let config = Config {
        schema: 1,
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
    };
    identity::json(&opts.output.join("config.json"), &config)?;
    let inventory = catalog::inventory(&harness.join("inventory.json"))?;
    let mut report = super::worker::initial(&config, &inventory, &opts.output.join("config.json"))?;
    identity::json(&opts.output.join("report.json"), &report)?;
    let outcome = launch(opts, &snapshot, &toolchain, &executable);
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
        "M3 acceptance: {:?}; {}/20 cases passed; {}",
        report.overall,
        report
            .cases
            .iter()
            .filter(|row| row.status == Verdict::Pass)
            .count(),
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

fn launch(
    opts: &Options,
    source: &Path,
    toolchain: &Path,
    executable: &Path,
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
        (source.to_owned(), "/work/source", false), (opts.target.clone(), "/work/target", true),
        (opts.output.join("cargo"), "/work/cargo", true), (opts.output.join("evidence"), "/work/evidence", true),
        (opts.output.join("config.json"), "/work/config.json", false),
        (toolchain.to_owned(), "/work/toolchain", false), (registry, "/work/registry", false),
        (PathBuf::from("/usr/include"), "/work/include", false),
        (executable.to_owned(), "/work/xtask", false),
        (source.join("tools/probes/x11_conformance"), "/work/isolation", false),
        (source.join("tools/probes/m3_acceptance/containment.py"), "/work/containment.py", false),
    ].into_iter().map(|(source, destination, writable)| json!({"source":source,"destination":destination,"writable":writable})).collect::<Vec<_>>();
    let plan = opts.output.join("launch.json");
    let bwrap = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("bwrap"))
        .find(|path| path.is_file())
        .ok_or("bubblewrap unavailable; containment is required")?;
    identity::json(
        &plan,
        &json!({"mounts":mounts,"timeout":opts.timeout,
        "bwrap":bwrap,
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
