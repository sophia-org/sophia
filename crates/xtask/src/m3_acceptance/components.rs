//! Exact component controls. Their result cannot change acceptance verdicts.
use super::{catalog, evidence, identity, process, types::*, worker};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suites {
    schema: u32,
    suites: BTreeMap<String, Vec<String>>,
}

pub(super) fn options(arguments: &[String]) -> Result<(String, Vec<String>), String> {
    let mut suite = None;
    let mut remaining = Vec::new();
    for argument in arguments {
        if let Some(name) = argument.strip_prefix("--suite=") {
            if name.is_empty() || suite.replace(name.to_owned()).is_some() {
                return Err("exactly one nonempty component suite is required".into());
            }
        } else {
            remaining.push(argument.clone());
        }
    }
    Ok((
        suite.ok_or("provide --suite=NAME; filters are not suites")?,
        remaining,
    ))
}

pub(super) fn suite(source: &Path, name: &str) -> Result<Vec<String>, String> {
    let mut manifest: Suites =
        identity::read_json(&source.join("tools/probes/m3_components/suites.json"))?;
    if manifest.schema != 1 {
        return Err("unknown component suite schema".into());
    }
    let tests = manifest
        .suites
        .remove(name)
        .ok_or("unknown component suite")?;
    validate_names(&tests)?;
    Ok(tests)
}

pub(super) fn validate_names(tests: &[String]) -> Result<(), String> {
    if tests.is_empty()
        || tests.len() > 256
        || tests.iter().collect::<BTreeSet<_>>().len() != tests.len()
        || tests.iter().any(|test| !component_name(test))
    {
        return Err("component suite must contain 1..=256 unique exact component names".into());
    }
    Ok(())
}

fn component_name(test: &str) -> bool {
    let Some(path) = test.strip_prefix("x11_socket::routing_tests::") else {
        return false;
    };
    // A diagnostic can borrow private acceptance fixtures without becoming
    // an acceptance case. Only this separate module is allowed; every test
    // still has to appear by exact name in the source-attested suite.
    if path == "m3_acceptance"
        || (test.starts_with(catalog::PREFIX) && !test.starts_with(catalog::DIAGNOSTIC_PREFIX))
    {
        return false;
    }
    path.split("::").all(|segment| {
        let mut bytes = segment.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

pub(super) fn initial(config: &Config) -> Option<ComponentReport> {
    Some(ComponentReport {
        suite: config.component_suite.as_ref()?.clone(),
        verdict: Verdict::NotRun,
        tests: config
            .component_tests
            .iter()
            .map(|test| ComponentResult {
                test: test.clone(),
                status: Verdict::NotRun,
                reason: "not executed".into(),
                execution: None,
            })
            .collect(),
    })
}

fn verdict(rows: &[ComponentResult]) -> Verdict {
    if rows.is_empty() || rows.iter().any(|row| row.status == Verdict::Fail) {
        Verdict::Fail
    } else if rows.iter().all(|row| row.status == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::NotRun
    }
}

pub(super) fn validate(report: &Report, config: &Config) -> Result<(), String> {
    let Some(suite) = config.component_suite.as_ref() else {
        return if report.components.is_none() && config.component_tests.is_empty() {
            Ok(())
        } else {
            Err("component data cannot alter an acceptance run".into())
        };
    };
    validate_names(&config.component_tests)?;
    let component = report
        .components
        .as_ref()
        .ok_or("missing component results")?;
    if config.self_test
        || report.purpose != "m3_components"
        || report.overall != Verdict::NotRun
        || report.cases.iter().any(|case| {
            case.status != Verdict::NotRun || case.execution.is_some() || case.evidence.is_some()
        })
        || &component.suite != suite
        || component
            .tests
            .iter()
            .map(|row| &row.test)
            .ne(config.component_tests.iter())
        || component.verdict != verdict(&component.tests)
        || component.tests.iter().any(|row| {
            row.status == Verdict::Pass && !row.execution.as_ref().is_some_and(Execution::clean)
        })
    {
        return Err("component report differs from its suite or makes an acceptance claim".into());
    }
    Ok(())
}

pub(super) fn execute(
    config: &Config,
    report: &mut Report,
    binary: &Path,
    available: &BTreeSet<&str>,
) -> Result<(), String> {
    let hash = identity::digest(binary)?;
    for index in 0..config.component_tests.len() {
        let exact = &config.component_tests[index];
        if identity::digest(binary)? != hash {
            return Err("component binary changed between controls".into());
        }
        let row = &mut report
            .components
            .as_mut()
            .ok_or("missing component report")?
            .tests[index];
        if !available.contains(exact.as_str()) {
            row.reason = "exact component test absent from attested binary".into();
        } else {
            let log = Path::new("/work/evidence").join(format!("component-{index}.log"));
            let run = process::run(
                process::private_command(binary).args([
                    exact,
                    "--exact",
                    "--test-threads=1",
                    "--show-output",
                    "--color=never",
                ]),
                &log,
                Duration::from_secs(config.case_timeout),
            )?;
            let text = std::fs::read_to_string(log).map_err(|e| e.to_string())?;
            match evidence::validate_exact_test(exact, &run, &text) {
                Ok(()) => {
                    row.status = Verdict::Pass;
                    row.reason = "exact component control passed; not integrated acceptance".into();
                }
                Err(error) => {
                    row.status = Verdict::Fail;
                    row.reason = error;
                }
            }
            row.execution = Some(run);
        }
        let component = report
            .components
            .as_mut()
            .ok_or("missing component report")?;
        component.verdict = verdict(&component.tests);
        worker::save(report)?;
    }
    if identity::digest(binary)? != hash {
        return Err("component binary changed during controls".into());
    }
    validate(report, config)
}
