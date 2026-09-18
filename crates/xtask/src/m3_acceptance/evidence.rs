use super::types::{Case, CaseEvidence, Execution, Verdict};
use std::collections::BTreeMap;

pub(super) fn validate_case(
    row: &Case,
    exact: &str,
    run: &Execution,
    text: &str,
) -> Result<CaseEvidence, String> {
    if !run.clean() {
        return Err("test failed, timed out or did not collect every process".into());
    }
    if text
        .lines()
        .filter(|line| *line == "running 1 test")
        .count()
        != 1
        || text
            .lines()
            .filter(|line| line.starts_with("running "))
            .count()
            != 1
        || !text
            .lines()
            .any(|line| line == format!("test {exact} ... ok"))
        || !text.lines().any(|line| {
            line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ")
        })
    {
        return Err(
            "exactly one non-ignored bound test must pass; filters/empty selections cannot qualify"
                .into(),
        );
    }
    let records = text
        .lines()
        .filter_map(|line| line.strip_prefix("sophia_m3_acceptance "))
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err("exactly one integrated evidence record is required".into());
    }
    let record: CaseEvidence = serde_json::from_str(records[0]).map_err(|e| e.to_string())?;
    let expected = row
        .subcases
        .iter()
        .map(|name| (name.clone(), Verdict::Pass))
        .collect::<BTreeMap<_, _>>();
    if record.schema != 1 || record.case != row.case || record.subcases != expected {
        return Err("wrong case or missing, extra, incomplete mandatory subcases".into());
    }
    let actors = &record.cleanup;
    if !actors.complete
        || actors.actors_started == 0
        || actors.actors_started != actors.actors_collected
        || actors.pending_actors != 0
        || record.observations.is_empty()
    {
        return Err(
            "actual observations and complete test-owned actor collection are required".into(),
        );
    }
    Ok(record)
}

impl Execution {
    pub(super) fn clean(&self) -> bool {
        self.returncode == Some(0)
            && !self.timed_out
            && self.collection.root_waited
            && self.collection.descendants_found == 0
            && self.collection.remaining.is_empty()
            && self.collection.error.is_none()
    }
}

pub(super) fn launcher_collected(run: &Execution) -> bool {
    !run.timed_out
        && run.collection.root_waited
        && run.collection.descendants_found == run.collection.descendants_reaped
        && run.collection.remaining.is_empty()
        && run.collection.error.is_none()
}
