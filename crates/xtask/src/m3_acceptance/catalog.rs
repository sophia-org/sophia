use super::types::{Bindings, CaseResult, Inventory, Verdict};
use std::collections::BTreeSet;
use std::path::Path;

pub(super) const CASES: [&str; 20] = [
    "A.press_release_repress",
    "A.recipient_disconnect",
    "A.partial_proof",
    "B.applied_focus",
    "B.keyboard_history",
    "B.ordered_input",
    "B.shared_hold",
    "C.capacity",
    "C.interrupted_ownership",
    "C.indeterminate_send",
    "C.poison",
    "C.control_cleanup",
    "C.exact_origin",
    "D.ready_before_exposure",
    "D.start_failures",
    "D.worker_exit",
    "D.registration_destruction",
    "D.service_exit",
    "D.namespace_reuse",
    "D.scheduler",
];
pub(super) const PREFIX: &str = "x11_socket::routing_tests::m3_acceptance::";

pub(super) fn inventory(path: &Path) -> Result<Inventory, String> {
    let value: Inventory = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    validate(&value)?;
    Ok(value)
}

pub(super) fn validate(value: &Inventory) -> Result<(), String> {
    if value.schema != 1 || value.cases.iter().map(|row| row.case.as_str()).ne(CASES) {
        return Err("inventory must contain the exact twenty mandatory cases in order".into());
    }
    for row in &value.cases {
        if !row.required
            || row.gate != row.case[..1]
            || row.requirement.is_empty()
            || row.subcases.is_empty()
            || row.subcases.iter().any(String::is_empty)
            || row.subcases.iter().collect::<BTreeSet<_>>().len() != row.subcases.len()
        {
            return Err(format!("invalid mandatory case {}", row.case));
        }
    }
    Ok(())
}

pub(super) fn bindings(path: &Path) -> Result<Bindings, String> {
    let value: Bindings = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut unique = BTreeSet::new();
    for (case, test) in &value.cases {
        let suffix = test.strip_prefix(PREFIX).unwrap_or_default();
        if value.schema != 1
            || !CASES.contains(&case.as_str())
            || suffix.is_empty()
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !unique.insert(test)
        {
            return Err("bindings require unique exact dedicated integrated test names".into());
        }
    }
    if value.schema != 1 {
        return Err("invalid bindings schema".into());
    }
    Ok(value)
}

pub(super) fn initial(value: &Inventory) -> Vec<CaseResult> {
    value
        .cases
        .iter()
        .map(|row| CaseResult {
            case: row.case.clone(),
            status: Verdict::NotRun,
            reason: "integrated case has not run; component tests do not qualify".into(),
            subcases: row
                .subcases
                .iter()
                .map(|name| (name.clone(), Verdict::NotRun))
                .collect(),
            test: None,
            execution: None,
            evidence: None,
        })
        .collect()
}

pub(super) fn overall(results: &[CaseResult]) -> Result<Verdict, String> {
    if results.iter().map(|row| row.case.as_str()).ne(CASES) {
        return Err("aggregate requires all twenty case results in order".into());
    }
    if results.iter().any(|row| row.status == Verdict::Fail) {
        return Ok(Verdict::Fail);
    }
    Ok(if results.iter().all(|row| row.status == Verdict::Pass) {
        Verdict::Pass
    } else {
        Verdict::NotRun
    })
}
