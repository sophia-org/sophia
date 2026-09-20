//! M5 shares process and source custody with M3 and M4, never their
//! inventories. Its eight rows are obligation groups, so a failure names a
//! behaviour rather than a request.
use super::{evidence, identity, types::*};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) const CASES: [&str; 8] = [
    "M5.registration_admission",
    "M5.version_negotiation",
    "M5.cursor_comparison",
    "M5.fake_input_encoding",
    "M5.fake_input_effects",
    "M5.processing_barrier",
    "M5.cancellation_half_close",
    "M5.grab_control",
];

/// The inventory compiled into the gate. The copy in the attested source must
/// match it row for row, including the plan revision the contract came from.
fn frozen() -> Result<Inventory, String> {
    serde_json::from_str(include_str!(
        "../../../../tools/probes/m5_acceptance/inventory.json"
    ))
    .map_err(|e| e.to_string())
}

pub(super) fn inventory(path: &Path) -> Result<Inventory, String> {
    let value: Inventory = identity::read_json(path)?;
    let frozen = frozen()?;
    if value.schema != 1
        || value.plan_source_commit != frozen.plan_source_commit
        || value.cases.iter().map(|row| row.case.as_str()).ne(CASES)
        || value
            .cases
            .iter()
            .zip(&frozen.cases)
            .any(|(actual, expected)| {
                !actual.required
                    || actual.gate != "M5"
                    || actual.requirement != expected.requirement
                    || actual.subcases != expected.subcases
            })
    {
        return Err(
            "M5 requires the exact eight mandatory groups, their full contracts and the pinned plan revision"
                .into(),
        );
    }
    Ok(value)
}

pub(super) fn bindings(path: &Path) -> Result<Bindings, String> {
    let value: Bindings = identity::read_json(path)?;
    if value.schema != 1
        || value.cases.iter().any(|(case, test)| {
            !CASES.contains(&case.as_str()) || case.strip_prefix("M5.") != Some(test.as_str())
        })
    {
        return Err("M5 bindings require the exact dedicated Session group tests".into());
    }
    Ok(value)
}

pub(super) fn overall(rows: &[CaseResult]) -> Result<Verdict, String> {
    if rows.iter().map(|row| row.case.as_str()).ne(CASES) {
        return Err("M5 aggregate requires all eight rows in order".into());
    }
    Ok(if rows.iter().any(|row| row.status == Verdict::Fail) {
        Verdict::Fail
    } else if rows.iter().all(|row| row.status == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::NotRun
    })
}

/// The one executable M5 runs is the Session test target; nothing else is
/// attested, so nothing else may change under the run.
pub(super) fn validate_binary(report: &Report, output: &Path) -> Result<(), String> {
    let binary = report.binary.as_ref().ok_or("M5 binary evidence absent")?;
    if binary["path"] != "evidence/test-binary"
        || binary["sha256"].as_str()
            != Some(&identity::digest(&output.join("evidence/test-binary"))?)
    {
        return Err("M5 binary identity changed after contained execution".into());
    }
    Ok(())
}

pub(super) fn validate_case(
    row: &Case,
    exact: &str,
    run: &Execution,
    text: &str,
) -> Result<CaseEvidence, String> {
    evidence::validate_exact_test(exact, run, text)?;
    let records = text
        .lines()
        .filter_map(|line| line.strip_prefix("sophia_m5_acceptance "))
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err("exactly one M5 evidence record is required".into());
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
            "M5 needs every subcase, observed behaviour and complete actor collection".into(),
        );
    }
    let names = record
        .observations
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if names.contains("component_only") {
        return Err("component diagnostics cannot qualify as M5 acceptance".into());
    }
    Ok(record)
}
