//! The 96-check verdict covers production wire/content/focus behavior.
//! Catalog/indicator admission is scripted; Session launch policy is excluded.
use std::collections::BTreeSet;

const CHECKS: &str = "negotiation/api-accepted negotiation/policy-refused negotiation/second negotiation/allocation-before negotiation/candidate-before
objects/limits objects/outputs objects/announcement objects/getattr objects/second-pin objects/old-pin objects/fresh-qid objects/fresh-generation
allocation/granted allocation/rejected allocation/geometry
upload/admitted upload/split upload/msize upload/short upload/end upload/cancel upload/clunk upload/ended-stale upload/cancelled-stale upload/successor
candidate/permit candidate/custody candidate/prepared candidate/presented candidate/missing-revokes candidate/cancel candidate/cancelled-revokes
action/identity action/echo
custody/before-outcome custody/retry custody/watermark custody/eagain-empty custody/eagain-retry journal/offset journal/reread journal/floor journal/past-tail journal/pending custody/domain-id
malformed/length malformed/reserved malformed/count malformed/kind
stream/fragmented stream/staging stream/flush stream/no-late-reply
r6/profile r6/indicators r6/announcement r6/qid r6/second-pin r6/old-pin r6/fresh-generation r6/activation-custody r6/activation-echo r6/stale-activation
r7/profile r7/catalog r7/opening r7/allocation r7/permit r7/candidate-custody r7/prepared r7/no-focus-before-presented r7/presented r7/focus-binding r7/text-input r7/input-ack r7/query-disarms r7/repaint-focus r7/accept-input r7/activation-custody r7/activation-outcome r7/stale-input-ack r7/focus-revoked r7/closed
r8/profile r8/catalog-identities r8/catalog-old-pin r8/catalog-fresh-generation r8/allocation r8/permit r8/candidate-custody r8/presented r8/activation-custody r8/activation-echo r8/stale-generation r8/stale-slot";

pub fn validate(text: &str, success: bool) -> Result<(), String> {
    if !success {
        return Err("oracle process failed".into());
    }
    let mut lines = text.lines().collect::<Vec<_>>();
    if lines.pop() != Some("sophia_shell_files_oracle schema=1 status=pass checks=96 failed=0") {
        return Err("missing or invalid final verdict".into());
    }
    let expected = CHECKS.split_whitespace().collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 96);
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
        + "sophia_shell_files_oracle schema=1 status=pass checks=96 failed=0\n"
}
