use super::{catalog, evidence, identity, process, types::*};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const SOURCE: &str = "/work/source";
const EVIDENCE: &str = "/work/evidence";

pub(super) fn initial(
    config: &Config,
    inventory: &Inventory,
    path: &Path,
) -> Result<Report, String> {
    Ok(Report {
        schema: 1,
        run_id: config.run_id.clone(),
        purpose: if config.component_suite.is_some() {
            "m3_components"
        } else if config.self_test {
            "harness_self_test"
        } else if config.gate == Gate::M4 {
            "m4_acceptance"
        } else {
            "m3_acceptance"
        }
        .into(),
        overall: Verdict::NotRun,
        cases: catalog::initial(inventory),
        source: config.source.clone(),
        build_target_namespace: config.build_target_namespace.clone(),
        config_sha256: identity::digest(path)?,
        containment: None,
        build: None,
        binary: None,
        self_tests: None,
        components: super::components::initial(config),
        launcher: None,
        source_attested_inside: false,
        source_unchanged_after: false,
        harness_error: None,
    })
}

fn validate_containment(record: &Containment, config: &Config) -> Result<(), String> {
    let actual = identity::namespaces()?;
    if !record.validated
        || record.delegated_descriptors != 0
        || record.render_devices_present
        || record.input_devices_present
        || actual != record.namespaces
        || actual.keys().ne(config.host_namespaces.keys())
        || actual
            .iter()
            .any(|(name, value)| config.host_namespaces.get(name) == Some(value))
        || Path::new("/dev/dri").exists()
        || Path::new("/dev/input").exists()
    {
        return Err("kernel-validated namespace/device/descriptor attestation is absent".into());
    }
    Ok(())
}

pub(super) fn validate_report(report: &Report, config: &Config, path: &Path) -> Result<(), String> {
    super::components::validate(report, config)?;
    if report.schema != 1
        || report.run_id != config.run_id
        || report.config_sha256 != identity::digest(path)?
        || report.source.commit != config.source.commit
        || report.source.content_sha256 != config.source.content_sha256
        || report.build_target_namespace != config.build_target_namespace
        || config.build_target_namespace
            != super::host::target_namespace(&config.source.content_sha256)?
        || !report.source_attested_inside
        || !report.source_unchanged_after
    {
        return Err(
            "source, configuration or completion identity differs from the requested run".into(),
        );
    }
    let record = report
        .containment
        .as_ref()
        .ok_or("no containment evidence")?;
    if !record.validated
        || record.delegated_descriptors != 0
        || record.render_devices_present
        || record.input_devices_present
        || record.namespaces.keys().ne(config.host_namespaces.keys())
        || record
            .namespaces
            .iter()
            .any(|(name, value)| config.host_namespaces.get(name) == Some(value))
    {
        return Err("invalid containment evidence".into());
    }
    let launcher = report
        .launcher
        .as_ref()
        .ok_or("launcher was not collected")?;
    if !evidence::launcher_collected(launcher)
        || ((report.overall == Verdict::Pass
            || report
                .components
                .as_ref()
                .is_some_and(|component| component.verdict == Verdict::Pass)
            || (config.self_test && report.self_tests.as_ref().is_some_and(Execution::clean)))
            && launcher.returncode != Some(0))
    {
        return Err("launcher timed out or left uncollected processes".into());
    }
    if !config.self_test && report.overall != super::m4::overall(config.gate, &report.cases)? {
        return Err("reported aggregate contradicts mandatory cases".into());
    }
    Ok(())
}

pub(super) fn save(report: &Report) -> Result<(), String> {
    identity::json(&Path::new(EVIDENCE).join("inner-report.json"), report)
}

pub(super) fn run() -> Result<(), String> {
    let config_path = Path::new("/work/config.json");
    let config: Config = identity::read_json(config_path)?;
    let containment: Containment =
        identity::read_json(&Path::new(EVIDENCE).join("containment.json"))?;
    validate_containment(&containment, &config)?;
    let inventory = super::m4::inventory(
        config.gate,
        &Path::new(SOURCE)
            .join(config.gate.directory())
            .join("inventory.json"),
    )?;
    let mut report = initial(&config, &inventory, config_path)?;
    report.containment = Some(containment);
    save(&report)?;
    if let Err(error) = execute(&config, &inventory, &mut report) {
        if let Some(component) = report.components.as_mut() {
            component.verdict = Verdict::Fail;
        } else {
            report.overall = Verdict::Fail;
        }
        report.harness_error = Some(error);
    }
    save(&report)?;
    if successful(&report, &config) {
        Ok(())
    } else {
        Err(format!(
            "contained {:?} result {:?}; no acceptance",
            report.purpose, report.overall
        ))
    }
}

pub(super) fn successful(report: &Report, config: &Config) -> bool {
    if report.harness_error.is_some() {
        return false;
    }
    if config.component_suite.is_some() {
        super::components::validate(report, config).is_ok()
            && report
                .components
                .as_ref()
                .is_some_and(|report| report.verdict == Verdict::Pass)
    } else if config.self_test {
        report.self_tests.as_ref().is_some_and(Execution::clean) && report.harness_error.is_none()
    } else {
        report.overall == Verdict::Pass
    }
}

fn execute(config: &Config, inventory: &Inventory, report: &mut Report) -> Result<(), String> {
    let harness = Path::new(SOURCE).join(config.gate.directory());
    if config.gate == Gate::M4 && config.component_suite.is_some() {
        return Err("M3 component evidence cannot qualify as M4".into());
    }
    if let Some(suite) = &config.component_suite {
        if config.self_test
            || super::components::suite(Path::new(SOURCE), suite)? != config.component_tests
        {
            return Err("component test inventory differs from attested source".into());
        }
    } else if !config.component_tests.is_empty() {
        return Err("component tests supplied to a non-component run".into());
    }
    for (name, expected) in &config.toolchain_sha256 {
        if identity::digest(&Path::new("/work/toolchain/bin").join(name))? != *expected {
            return Err("contained compiler differs from the attested toolchain".into());
        }
    }
    if config.schema != 1
        || !config.source.clean
        || config.build_target_namespace
            != super::host::target_namespace(&config.source.content_sha256)?
        || identity::contents(Path::new(SOURCE))? != config.source.content_sha256
        || identity::digest(Path::new("/work/xtask"))? != config.xtask_sha256
        || identity::digest(&harness.join("inventory.json"))? != config.inventory_sha256
        || identity::digest(&harness.join("bindings.json"))? != config.bindings_sha256
    {
        return Err("contained source or harness/configuration bytes differ from snapshot".into());
    }
    report.source_attested_inside = true;
    std::fs::create_dir_all("/etc").map_err(|e| e.to_string())?;
    std::fs::write("/etc/hosts", "127.0.0.1 localhost\n::1 localhost\n")
        .map_err(|e| e.to_string())?;
    std::os::unix::fs::symlink("/work/include", "/usr/include").map_err(|e| e.to_string())?;
    std::os::unix::fs::symlink("/work/registry", "/work/cargo/registry")
        .map_err(|e| e.to_string())?;
    let loader = process::run(
        process::private_command("/usr/bin/ldconfig").args([
            "-X",
            "-i",
            "-f",
            "/dev/null",
            "-C",
            "/etc/ld.so.cache",
            "/usr/lib",
        ]),
        &Path::new(EVIDENCE).join("loader.log"),
        Duration::from_secs(30),
    )?;
    if !loader.clean() {
        return Err("private runtime loader preparation failed".into());
    }
    let binary = build(config, report)?;
    if config.gate == Gate::M4 {
        build_activation_probe(config, report)?;
        if !config.self_test {
            build_private_host(config, report)?;
        }
    }
    let listed = process::capture(
        process::private_command(&binary).args(["--list", "--format=terse"]),
        &Path::new(EVIDENCE).join("tests-list.log"),
    )?;
    let available = listed
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect::<BTreeSet<_>>();
    if config.self_test {
        self_tests(config, report, &binary, &available)?;
    } else if config.component_suite.is_some() {
        super::components::execute(config, report, &binary, &available)?;
    } else {
        cases(config, inventory, report, &binary, &available)?;
    }
    if identity::contents(Path::new(SOURCE))? != config.source.content_sha256 {
        return Err("snapshot contents changed during contained execution".into());
    }
    report.source_unchanged_after = true;
    Ok(())
}

fn cargo() -> Command {
    let mut command = process::private_command("/work/toolchain/bin/cargo");
    command
        .current_dir(SOURCE)
        .env("PATH", "/work/toolchain/bin:/usr/bin:/bin")
        .env("CARGO", "/work/toolchain/bin/cargo")
        .env("RUSTC", "/work/toolchain/bin/rustc")
        .env("RUSTDOC", "/work/toolchain/bin/rustdoc")
        .env("CARGO_HOME", "/work/cargo")
        .env("CARGO_TARGET_DIR", "/work/target")
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_BUILD_JOBS", "2")
        .env("PWD", SOURCE);
    command
}

fn build_activation_probe(config: &Config, report: &mut Report) -> Result<(), String> {
    let mut command = cargo();
    command.args([
        "build",
        "--offline",
        "--locked",
        "-p",
        "sophia-conformance",
        "--example",
        "private_instance_probe",
    ]);
    let built = process::run(
        &mut command,
        &Path::new(EVIDENCE).join("activation-build.log"),
        Duration::from_secs(config.build_timeout),
    )?;
    if !built.clean() {
        return Err("activation probe build failed or leaked a process".into());
    }
    let path = Path::new(EVIDENCE).join("private-instance-probe");
    std::fs::copy("/work/target/debug/examples/private_instance_probe", &path)
        .map_err(|e| e.to_string())?;
    let binary = report
        .binary
        .as_mut()
        .ok_or("test binary identity missing")?;
    binary["activation_probe"] = serde_json::json!({
        "path":"evidence/private-instance-probe", "sha256":identity::digest(&path)?
    });
    save(report)
}

fn build_private_host(config: &Config, report: &mut Report) -> Result<(), String> {
    let built = process::run(
        cargo().args([
            "build",
            "--offline",
            "--locked",
            "-p",
            "sophia-session",
            "--example",
            "native_input_conformance_host",
        ]),
        &Path::new(EVIDENCE).join("private-host-build.log"),
        Duration::from_secs(config.build_timeout),
    )?;
    if !built.clean() {
        return Err("private Session host build failed or leaked a process".into());
    }
    let path = Path::new(EVIDENCE).join("native-input-conformance-host");
    std::fs::copy(
        "/work/target/debug/examples/native_input_conformance_host",
        &path,
    )
    .map_err(|e| e.to_string())?;
    let binary = report
        .binary
        .as_mut()
        .ok_or("test binary identity missing")?;
    binary["private_host"] = json!({
        "path":"evidence/native-input-conformance-host", "sha256":identity::digest(&path)?
    });
    save(report)
}

fn build(config: &Config, report: &mut Report) -> Result<PathBuf, String> {
    let package = if config.self_test {
        "xtask"
    } else if config.gate == Gate::M4 {
        "sophia-session"
    } else {
        "sophia-x-authority"
    };
    let target = if config.self_test {
        "xtask"
    } else if config.gate == Gate::M4 {
        "private_input_acceptance"
    } else {
        "sophia_x_authority"
    };
    let mut command = cargo();
    command.args([
        "test",
        "--offline",
        "--locked",
        "-p",
        package,
        "--no-run",
        "--message-format=json",
    ]);
    if config.self_test {
        command.args(["--bin", "xtask"]);
    } else if config.gate == Gate::M4 {
        command.args(["--test", "private_input_acceptance"]);
    } else {
        command.arg("--lib");
    }
    let log = Path::new(EVIDENCE).join("build.log");
    let built = process::run(
        &mut command,
        &log,
        Duration::from_secs(config.build_timeout),
    )?;
    let success = built.clean();
    report.build = Some(built);
    save(report)?;
    if !success {
        return Err("single contained build failed, timed out or leaked processes".into());
    }
    let artifacts = std::fs::read_to_string(log)
        .map_err(|e| e.to_string())?
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|row| {
            row["reason"] == "compiler-artifact"
                && row["target"]["name"] == target
                && row["profile"]["test"] == true
                && row["executable"].is_string()
        })
        .collect::<Vec<_>>();
    if artifacts.len() != 1 {
        return Err("build did not identify exactly one test binary".into());
    }
    let artifact = &artifacts[0];
    let binary = Path::new(EVIDENCE).join("test-binary");
    std::fs::copy(
        artifact["executable"]
            .as_str()
            .ok_or("missing executable")?,
        &binary,
    )
    .map_err(|e| e.to_string())?;
    report.binary = Some(
        json!({"path":"evidence/test-binary", "sha256":identity::digest(&binary)?,
        "features":artifact["features"],"profile":artifact["profile"],"target":artifact["target"],
        "default_features":true,"offline":true,"locked":true}),
    );
    save(report)?;
    Ok(binary)
}

fn self_tests(
    config: &Config,
    report: &mut Report,
    binary: &Path,
    available: &BTreeSet<&str>,
) -> Result<(), String> {
    let prefix = if config.gate == Gate::M4 {
        "m3_acceptance::m4_tests::"
    } else {
        "m3_acceptance::tests::"
    };
    let count = available
        .iter()
        .filter(|name| name.starts_with(prefix))
        .count();
    if count == 0 {
        return Err("no harness self-tests in the built binary".into());
    }
    let log = Path::new(EVIDENCE).join("self-tests.log");
    let mut command = process::private_command(binary);
    command.args([prefix, "--test-threads=1", "--show-output", "--color=never"]);
    if config.gate == Gate::M4 {
        // These controls require the probe built and attested above; ordinary
        // workspace tests cannot furnish that execution boundary.
        command.arg("--include-ignored");
    }
    let run = process::run(&mut command, &log, Duration::from_secs(config.case_timeout))?;
    let text = std::fs::read_to_string(log).map_err(|e| e.to_string())?;
    let passed = run.clean()
        && text.contains(&format!(
            "test result: ok. {count} passed; 0 failed; 0 ignored;"
        ));
    report.self_tests = Some(run);
    if passed {
        Ok(())
    } else {
        Err("harness self-tests failed, were ignored or were not collected".into())
    }
}

fn cases(
    config: &Config,
    inventory: &Inventory,
    report: &mut Report,
    binary: &Path,
    available: &BTreeSet<&str>,
) -> Result<(), String> {
    let bindings = super::m4::bindings(
        config.gate,
        &Path::new(SOURCE)
            .join(config.gate.directory())
            .join("bindings.json"),
    )?;
    let hash = identity::digest(binary)?;
    for (index, row) in inventory.cases.iter().enumerate() {
        let Some(exact) = bindings.cases.get(&row.case) else {
            continue;
        };
        let result = &mut report.cases[index];
        result.test = Some(exact.clone());
        if !available.contains(exact.as_str()) {
            result.reason = "bound integrated test absent from the built binary".into();
            continue;
        }
        if identity::digest(binary)? != hash {
            return Err("binary changed between cases".into());
        }
        let log = Path::new(EVIDENCE).join(format!("{}.log", row.case));
        let mut command = process::private_command(binary);
        command.args([
            exact,
            "--exact",
            "--test-threads=1",
            "--show-output",
            "--color=never",
        ]);
        if config.gate == Gate::M4 {
            command.arg("--include-ignored");
            if row.case == "M4.no_ambient_fallback" {
                // Only fabricated names inside the outer device-hidden scope.
                // The nested private host must clear these before entry.
                command
                    .env("DISPLAY", ":64999")
                    .env("WAYLAND_DISPLAY", "m4-fabricated-wayland")
                    .env(
                        "DBUS_SESSION_BUS_ADDRESS",
                        "unix:path=/tmp/m4-fabricated-bus",
                    )
                    .env("XAUTHORITY", "/tmp/m4-fabricated-authority");
            }
        }
        let run = process::run(&mut command, &log, Duration::from_secs(config.case_timeout))?;
        let text = std::fs::read_to_string(log).map_err(|e| e.to_string())?;
        match super::m4::validate_case(config.gate, row, exact, &run, &text) {
            Ok(evidence) => {
                result.status = Verdict::Pass;
                result.subcases = evidence.subcases.clone();
                result.evidence = Some(evidence);
                result.reason = "exact integrated case passed".into();
            }
            Err(error) => {
                result.status = Verdict::Fail;
                result.reason = error;
            }
        }
        result.execution = Some(run);
        report.overall = super::m4::overall(config.gate, &report.cases)?;
        save(report)?;
    }
    if identity::digest(binary)? != hash {
        return Err("binary changed during cases".into());
    }
    report.overall = super::m4::overall(config.gate, &report.cases)?;
    Ok(())
}
