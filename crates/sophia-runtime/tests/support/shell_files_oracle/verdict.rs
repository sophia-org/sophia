use std::collections::BTreeSet;

const CHECKS: &str = "negotiation/api-accepted negotiation/policy-refused negotiation/second negotiation/allocation-before negotiation/candidate-before
objects/limits objects/outputs objects/announcement objects/getattr objects/second-pin objects/old-pin objects/fresh-qid objects/fresh-generation
allocation/granted allocation/rejected allocation/geometry
upload/admitted upload/split upload/msize upload/short upload/end upload/cancel upload/clunk upload/ended-stale upload/cancelled-stale upload/successor
candidate/permit candidate/custody candidate/prepared candidate/presented candidate/missing-revokes candidate/cancel candidate/cancelled-revokes
action/identity action/echo
custody/before-outcome custody/retry custody/watermark custody/eagain-empty custody/eagain-retry journal/offset journal/reread journal/floor journal/past-tail journal/pending custody/domain-id
malformed/length malformed/reserved malformed/count malformed/kind
stream/fragmented stream/staging stream/flush stream/no-late-reply";

pub fn validate(text: &str, success: bool) -> Result<(), String> {
    if !success {
        return Err("oracle process failed".into());
    }
    let mut lines = text.lines().collect::<Vec<_>>();
    if lines.pop() != Some("sophia_shell_files_oracle schema=1 status=pass checks=54 failed=0") {
        return Err("missing or invalid final verdict".into());
    }
    let expected = CHECKS.split_whitespace().collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 54);
    let mut seen = BTreeSet::new();
    for line in lines {
        let Some(name) = line
            .strip_prefix("check ")
            .and_then(|s| s.strip_suffix(" ok"))
        else {
            return Err(format!("invalid check line: {line}"));
        };
        if !expected.contains(name) || !seen.insert(name) {
            return Err(format!("unexpected/duplicate check: {name}"));
        }
    }
    if seen != expected {
        return Err(format!(
            "missing checks: {:?}",
            expected.difference(&seen).collect::<Vec<_>>()
        ));
    }
    Ok(())
}
pub fn valid_transcript() -> String {
    CHECKS
        .split_whitespace()
        .map(|name| format!("check {name} ok\n"))
        .collect::<String>()
        + "sophia_shell_files_oracle schema=1 status=pass checks=54 failed=0\n"
}
