#![cfg(test)]

use super::{Verdict, check_pin, judge};
use std::path::Path;

const PASS: &str = "check client/version ok\n\
sophia_9p_oracle schema=1 status=pass checks=42 failed=0\n";

#[test]
fn a_full_run_with_no_failure_passes() {
    assert_eq!(judge(PASS), Verdict::Pass("checks=42".into()));
}

#[test]
fn a_pass_with_too_few_checks_is_not_a_full_run() {
    let short = "sophia_9p_oracle schema=1 status=pass checks=3 failed=0\n";
    assert!(matches!(judge(short), Verdict::Unreadable(_)));
}

#[test]
fn a_failure_names_its_checks() {
    let failing = "check raw/flush FAIL: timeout\ncheck raw/x ok\n\
sophia_9p_oracle schema=1 status=fail checks=42 failed=1\n";
    assert_eq!(
        judge(failing),
        Verdict::Fail("1 of 42 failed: raw/flush".into())
    );
}

#[test]
fn a_verdict_that_disagrees_with_its_checks_is_unreadable() {
    let lying = "check raw/flush FAIL: timeout\n\
sophia_9p_oracle schema=1 status=pass checks=42 failed=0\n";
    assert!(matches!(judge(lying), Verdict::Unreadable(_)));
}

#[test]
fn a_crash_is_unreadable_not_a_failure() {
    assert!(matches!(judge(""), Verdict::Unreadable(_)));
    assert!(matches!(
        judge("panic: runtime error\n"),
        Verdict::Unreadable(_)
    ));
}

#[test]
fn the_committed_oracle_module_is_pinned() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    check_pin(&repo).unwrap();
}
