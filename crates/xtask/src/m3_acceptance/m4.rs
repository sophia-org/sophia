//! M4 shares process/source custody with M3, never its acceptance inventory.
use super::{catalog, evidence, identity, types::*};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod binaries;
pub(super) use binaries::{AuxiliaryBinary, build_auxiliary};

pub(super) const CASES: [&str; 8] = [
    "M4.construction",
    "M4.authorization",
    "M4.connection_identity",
    "M4.committed_routing",
    "M4.lifetime",
    "M4.containment",
    "M4.no_ambient_fallback",
    "M4.evidence_integrity",
];

pub(super) fn inventory(gate: Gate, path: &Path) -> Result<Inventory, String> {
    if gate == Gate::M3 {
        return catalog::inventory(path);
    }
    let value: Inventory = identity::read_json(path)?;
    let frozen: Inventory = serde_json::from_str(include_str!(
        "../../../../tools/probes/m4_acceptance/inventory.json"
    ))
    .map_err(|e| e.to_string())?;
    if value.schema != 1
        || value.cases.iter().map(|row| row.case.as_str()).ne(CASES)
        || value
            .cases
            .iter()
            .zip(&frozen.cases)
            .any(|(actual, expected)| {
                !actual.required
                    || actual.gate != "M4"
                    || actual.requirement != expected.requirement
                    || actual.subcases != expected.subcases
            })
    {
        return Err("M4 requires the exact eight mandatory cases and their full contracts".into());
    }
    Ok(value)
}

pub(super) fn bindings(gate: Gate, path: &Path) -> Result<Bindings, String> {
    if gate == Gate::M3 {
        return catalog::bindings(path);
    }
    let value: Bindings = identity::read_json(path)?;
    if value.schema != 1
        || value.cases.iter().any(|(case, test)| {
            !CASES.contains(&case.as_str()) || expected_test(case) != Some(test.as_str())
        })
    {
        return Err("M4 bindings require their exact dedicated Session or gate tests".into());
    }
    Ok(value)
}

fn expected_test(case: &str) -> Option<&str> {
    match case {
        "M4.lifetime" => Some("private_input::tests::lifetime"),
        "M4.evidence_integrity" => Some("m3_acceptance::m4_integrity::evidence_integrity"),
        _ => case.strip_prefix("M4."),
    }
}

pub(super) fn overall(gate: Gate, rows: &[CaseResult]) -> Result<Verdict, String> {
    if gate == Gate::M3 {
        return catalog::overall(rows);
    }
    if rows.iter().map(|row| row.case.as_str()).ne(CASES) {
        return Err("M4 aggregate requires all eight rows in order".into());
    }
    Ok(if rows.iter().any(|row| row.status == Verdict::Fail) {
        Verdict::Fail
    } else if rows.iter().all(|row| row.status == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::NotRun
    })
}

pub(super) fn validate_binary(report: &Report, output: &Path) -> Result<(), String> {
    let binary = report.binary.as_ref().ok_or("M4 binary evidence absent")?;
    if binary["path"] != "evidence/test-binary"
        || binary["sha256"].as_str()
            != Some(&identity::digest(&output.join("evidence/test-binary"))?)
    {
        return Err("M4 binary identity changed after contained execution".into());
    }
    if binary["activation_probe"]["path"] != "evidence/private-instance-probe"
        || binary["activation_probe"]["sha256"].as_str()
            != Some(&identity::digest(
                &output.join("evidence/private-instance-probe"),
            )?)
    {
        return Err("M4 activation probe identity changed after contained execution".into());
    }
    if report.purpose == "m4_acceptance"
        && (binary["private_host"]["path"] != "evidence/native-input-conformance-host"
            || binary["private_host"]["sha256"].as_str()
                != Some(&identity::digest(
                    &output.join("evidence/native-input-conformance-host"),
                )?))
    {
        return Err("M4 private host identity changed after contained execution".into());
    }
    binaries::validate_auxiliary(report, output)?;
    Ok(())
}

pub(super) fn validate_case(
    gate: Gate,
    row: &Case,
    exact: &str,
    run: &Execution,
    text: &str,
) -> Result<CaseEvidence, String> {
    if gate == Gate::M3 {
        return evidence::validate_case(row, exact, run, text);
    }
    evidence::validate_exact_test(exact, run, text)?;
    let records = text
        .lines()
        .filter_map(|line| line.strip_prefix("sophia_m4_acceptance "))
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err("exactly one M4 evidence record is required".into());
    }
    let record: CaseEvidence = serde_json::from_str(records[0]).map_err(|e| e.to_string())?;
    let required = row
        .subcases
        .iter()
        .map(|name| (name.clone(), Verdict::Pass))
        .collect::<BTreeMap<_, _>>();
    if record.schema != 1
        || record.case != row.case
        || record.subcases != required
        || record.cleanup.actors_started == 0
        || record.cleanup.actors_started != record.cleanup.actors_collected
        || record.cleanup.pending_actors != 0
        || !record.cleanup.complete
        || record.observations.is_empty()
    {
        return Err(
            "M4 needs every subcase, observed behavior and complete actor collection".into(),
        );
    }
    let names = record
        .observations
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if names.contains("component_only") {
        return Err("component diagnostics cannot qualify as M4 acceptance".into());
    }
    Ok(record)
}
