//! Every exceptional test target is named, built, listed and hashed separately.
use super::super::{identity, process, types::*, worker};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(in crate::m3_acceptance) struct AuxiliaryBinary {
    pub path: PathBuf,
    pub sha256: String,
    pub tests: BTreeSet<String>,
}

struct Target {
    case: &'static str,
    package: &'static str,
    selector: &'static str,
    name: &'static str,
    key: &'static str,
}

const TARGETS: [Target; 2] = [
    Target {
        case: "M4.lifetime",
        package: "sophia-session",
        selector: "--lib",
        name: "sophia_session",
        key: "session-unit-tests",
    },
    Target {
        case: "M4.evidence_integrity",
        package: "xtask",
        selector: "--bin",
        name: "xtask",
        key: "evidence-integrity-tests",
    },
];

pub(in crate::m3_acceptance) fn build_auxiliary(
    config: &Config,
    report: &mut Report,
) -> Result<BTreeMap<String, AuxiliaryBinary>, String> {
    let mut result = BTreeMap::new();
    if config.gate != Gate::M4 {
        return Ok(result);
    }
    let bindings = super::bindings(
        Gate::M4,
        Path::new("/work/source/tools/probes/m4_acceptance/bindings.json"),
    )?;
    for target in &TARGETS {
        if !bindings.cases.contains_key(target.case) {
            continue;
        }
        let mut command = worker::cargo(config);
        command.args([
            "test",
            "--offline",
            "--locked",
            "-p",
            target.package,
            "--no-run",
            "--message-format=json",
            target.selector,
        ]);
        if target.selector == "--bin" {
            command.arg(target.name);
        }
        let log = Path::new("/work/evidence").join(format!("{}-build.log", target.key));
        let execution = process::run(
            &mut command,
            &log,
            Duration::from_secs(config.build_timeout),
        )?;
        if !execution.clean() {
            return Err(format!("{} did not build cleanly", target.key));
        }
        let artifacts = std::fs::read_to_string(&log)
            .map_err(|e| e.to_string())?
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|row| {
                row["reason"] == "compiler-artifact"
                    && row["target"]["name"] == target.name
                    && row["profile"]["test"] == true
                    && row["executable"].is_string()
            })
            .collect::<Vec<_>>();
        let [artifact] = &artifacts[..] else {
            return Err(format!("{} requires one exact artifact", target.key));
        };
        let path = Path::new("/work/evidence").join(target.key);
        std::fs::copy(artifact["executable"].as_str().unwrap(), &path)
            .map_err(|e| e.to_string())?;
        let sha256 = identity::digest(&path)?;
        let listed = process::capture(
            process::private_command(&path).args(["--list", "--format=terse"]),
            &Path::new("/work/evidence").join(format!("{}-list.log", target.key)),
        )?;
        let tests = listed
            .lines()
            .filter_map(|line| line.strip_suffix(": test").map(str::to_owned))
            .collect();
        report.binary.as_mut().ok_or("primary binary absent")?[target.key] = serde_json::json!({
            "path":format!("evidence/{}", target.key), "sha256":sha256, "target":artifact["target"], "build":execution
        });
        worker::save(report)?;
        result.insert(
            target.case.into(),
            AuxiliaryBinary {
                path,
                sha256,
                tests,
            },
        );
    }
    Ok(result)
}

pub(super) fn validate_auxiliary(report: &Report, output: &Path) -> Result<(), String> {
    let binary = report.binary.as_ref().ok_or("binary absent")?;
    for target in &TARGETS {
        let required = report
            .cases
            .iter()
            .any(|row| row.case == target.case && row.test.is_some());
        if !required && binary.get(target.key).is_none() {
            continue;
        }
        let path = format!("evidence/{}", target.key);
        if binary[target.key]["path"] != path
            || binary[target.key]["sha256"].as_str()
                != Some(&identity::digest(&output.join(&path))?)
        {
            return Err(format!("{} binary identity changed", target.key));
        }
    }
    Ok(())
}
