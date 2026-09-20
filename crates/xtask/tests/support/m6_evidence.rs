#![cfg(test)]
//! The composition's own rules: absence is never a pass, snapshots must be
//! byte-identical, and a citation is checked and labelled, never stood
//! behind. No gate runs here.
use super::m6::{COMPONENTS, cite_canonical, cite_core, judge_component, options, overall};
use super::types::SourceIdentity;
use std::collections::BTreeMap;
use std::path::Path;

fn arguments(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn source() -> SourceIdentity {
    SourceIdentity {
        commit: "abc".into(),
        tree: "tree".into(),
        clean: true,
        archive_sha256: "arch".into(),
        content_sha256: "cont".into(),
    }
}

fn gate_report(overall: &str, commit: &str, archive: &str, content: &str) -> serde_json::Value {
    serde_json::json!({
        "overall": overall,
        "source": {"commit": commit, "tree": "tree", "clean": true,
                   "archive_sha256": archive, "content_sha256": content},
    })
}

#[test]
fn options_take_the_two_paths_and_only_named_citations() {
    let parsed = options(&arguments(&["--output=/o", "--target-dir=/t"])).unwrap();
    assert_eq!(parsed.timeout, 1800);
    assert!(parsed.core_report.is_none() && parsed.canonical_report.is_none());
    let cited = options(&arguments(&[
        "--output=/o",
        "--target-dir=/t",
        "--core-report=/c.json",
        "--canonical-report=/w.json",
    ]))
    .unwrap();
    assert_eq!(cited.core_report.as_deref(), Some(Path::new("/c.json")));
    for refused in [
        arguments(&["--output=/o"]),
        arguments(&["--output=/o", "--target-dir=/t", "--component=m3"]),
        arguments(&["--output=/o", "--target-dir=/t", "--xts-root=/x"]),
        arguments(&["--output=/o", "--target-dir=/t", "--timeout=1801"]),
    ] {
        assert!(options(&refused).is_err(), "{refused:?}");
    }
}

#[test]
fn a_component_counts_only_on_this_source_byte_for_byte() {
    let src = source();
    let same = judge_component(
        Some(&gate_report("PASS", "abc", "arch", "cont")),
        Some(0),
        false,
        &src,
    );
    assert_eq!(
        (same.verdict.as_str(), same.identical_source),
        ("PASS", true)
    );
    let failed = judge_component(
        Some(&gate_report("FAIL", "abc", "arch", "cont")),
        Some(1),
        false,
        &src,
    );
    assert_eq!(failed.verdict, "FAIL");
    // The same commit name on different bytes is not this source.
    let other_bytes = judge_component(
        Some(&gate_report("PASS", "abc", "arch2", "cont")),
        Some(0),
        false,
        &src,
    );
    assert_eq!(
        (other_bytes.verdict.as_str(), other_bytes.identical_source),
        ("NORESULT", false)
    );
    let other_content = judge_component(
        Some(&gate_report("PASS", "abc", "arch", "cont2")),
        Some(0),
        false,
        &src,
    );
    assert_eq!(other_content.verdict, "NORESULT");
    let other_commit = judge_component(
        Some(&gate_report("PASS", "def", "arch", "cont")),
        Some(0),
        false,
        &src,
    );
    assert_eq!(other_commit.verdict, "NORESULT");
    // Absent, timed out, not a result, or a PASS the gate did not exit on.
    assert_eq!(
        judge_component(None, Some(0), false, &src).verdict,
        "NORESULT"
    );
    assert_eq!(
        judge_component(
            Some(&gate_report("PASS", "abc", "arch", "cont")),
            None,
            true,
            &src
        )
        .verdict,
        "NORESULT"
    );
    assert_eq!(
        judge_component(
            Some(&gate_report("NOT_RUN", "abc", "arch", "cont")),
            Some(1),
            false,
            &src
        )
        .verdict,
        "NORESULT"
    );
    assert_eq!(
        judge_component(
            Some(&gate_report("PASS", "abc", "arch", "cont")),
            Some(1),
            false,
            &src
        )
        .verdict,
        "NORESULT"
    );
}

fn core_report(
    status: &str,
    commit: &str,
    dirty: bool,
    required: u64,
    executed: u64,
) -> serde_json::Value {
    serde_json::json!({
        "status": status, "required": required, "executed": executed, "failures": [],
        "identity": {"source_commit": commit, "source_dirty": dirty},
    })
}

#[test]
fn a_citation_is_checked_and_labelled_and_never_passes_by_absence() {
    let none = cite_core(None, None, "abc");
    assert_eq!(
        (none.verdict.as_str(), none.identity_verified),
        ("NORESULT", false)
    );
    assert_eq!(none.owner, "t057");
    let unreadable = cite_core(None, Some(Path::new("/nowhere.json")), "abc");
    assert_eq!(unreadable.verdict, "NORESULT");
    let good = cite_core(
        Some(&core_report("PASS", "abc", false, 100, 100)),
        Some(Path::new("/c")),
        "abc",
    );
    assert_eq!(
        (good.verdict.as_str(), good.identity_verified),
        ("PASS", true)
    );
    assert_eq!(good.cited_status.as_deref(), Some("PASS"));
    for (label, report) in [
        (
            "another commit",
            core_report("PASS", "def", false, 100, 100),
        ),
        ("a dirty tree", core_report("PASS", "abc", true, 100, 100)),
    ] {
        let cited = cite_core(Some(&report), Some(Path::new("/c")), "abc");
        assert_eq!(
            (cited.verdict.as_str(), cited.identity_verified),
            ("NORESULT", false),
            "{label}"
        );
    }
    assert_eq!(
        cite_core(
            Some(&core_report("PASS", "abc", false, 100, 99)),
            None,
            "abc"
        )
        .verdict,
        "FAIL"
    );
    assert_eq!(
        cite_core(
            Some(&core_report("FAIL", "abc", false, 100, 100)),
            None,
            "abc"
        )
        .verdict,
        "FAIL"
    );
    let canonical = |status: &str, commit: &str, executed: bool| {
        serde_json::json!({"status": status, "full_check_executed": executed,
                           "provenance": {"commit": commit, "dirty": false}})
    };
    assert_eq!(cite_canonical(None, None, "abc").verdict, "NORESULT");
    assert_eq!(
        cite_canonical(Some(&canonical("PASS", "abc", true)), None, "abc").verdict,
        "PASS"
    );
    assert_eq!(
        cite_canonical(Some(&canonical("PASS", "abc", false)), None, "abc").verdict,
        "NORESULT"
    );
    assert_eq!(
        cite_canonical(Some(&canonical("PASS", "def", true)), None, "abc").verdict,
        "NORESULT"
    );
    assert_eq!(
        cite_canonical(Some(&canonical("FAIL", "abc", true)), None, "abc").verdict,
        "FAIL"
    );
    assert_eq!(
        cite_canonical(Some(&canonical("BLOCKED", "abc", false)), None, "abc").verdict,
        "NORESULT"
    );
}

#[test]
fn the_overall_is_a_pass_only_when_everything_is() {
    let src = source();
    let pass = |name: &str| {
        (
            name.to_owned(),
            judge_component(
                Some(&gate_report("PASS", "abc", "arch", "cont")),
                Some(0),
                false,
                &src,
            ),
        )
    };
    let core = cite_core(
        Some(&core_report("PASS", "abc", false, 100, 100)),
        None,
        "abc",
    );
    let canonical = cite_canonical(
        Some(
            &serde_json::json!({"status": "PASS", "full_check_executed": true, "provenance": {"commit": "abc", "dirty": false}}),
        ),
        None,
        "abc",
    );
    let mut components: BTreeMap<_, _> = COMPONENTS.iter().map(|name| pass(name)).collect();
    assert_eq!(overall(&components, &[&core, &canonical]), "PASS");
    // A missing component is not a pass, whatever the rest say.
    components.remove("m4-acceptance");
    assert_eq!(overall(&components, &[&core, &canonical]), "NORESULT");
    let mut components: BTreeMap<_, _> = COMPONENTS.iter().map(|name| pass(name)).collect();
    components.insert(
        "m5-acceptance".into(),
        judge_component(
            Some(&gate_report("FAIL", "abc", "arch", "cont")),
            Some(1),
            false,
            &src,
        ),
    );
    assert_eq!(overall(&components, &[&core, &canonical]), "FAIL");
    components.insert(
        "m5-acceptance".into(),
        judge_component(None, Some(0), false, &src),
    );
    assert_eq!(overall(&components, &[&core, &canonical]), "NORESULT");
    // A citation that is absent holds the whole to NORESULT; one that failed, to FAIL.
    let components: BTreeMap<_, _> = COMPONENTS.iter().map(|name| pass(name)).collect();
    assert_eq!(
        overall(&components, &[&cite_core(None, None, "abc"), &canonical]),
        "NORESULT"
    );
    let failed_core = cite_core(
        Some(&core_report("FAIL", "abc", false, 100, 100)),
        None,
        "abc",
    );
    assert_eq!(overall(&components, &[&failed_core, &canonical]), "FAIL");
}
