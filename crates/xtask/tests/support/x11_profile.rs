#![cfg(test)]
//! The X11 profile gate's own rules: how options are read, how a profile's
//! report is judged, and how the verdicts combine. No profile runs here.
use super::profiles::{PROFILES, ProfileVerdict, judge, options, overall, xts_blocked};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn arguments(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn a_profile_is_required_and_all_means_both() {
    let parsed = options(&arguments(&[
        "--profile=all",
        "--output=/tmp/out",
        "--target-dir=/tmp/target",
    ]))
    .unwrap();
    assert_eq!(parsed.profiles, PROFILES.map(str::to_owned).to_vec());
    assert_eq!(parsed.timeout, 1800);
    let one = options(&arguments(&[
        "--profile=xtest",
        "--output=/tmp/out",
        "--target-dir=/tmp/target",
        "--timeout=60",
    ]))
    .unwrap();
    assert_eq!(one.profiles, vec!["xtest".to_owned()]);
    assert_eq!(one.timeout, 60);
    for missing in [
        arguments(&["--output=/tmp/out", "--target-dir=/tmp/target"]),
        arguments(&[
            "--profile=core",
            "--output=/tmp/out",
            "--target-dir=/tmp/target",
        ]),
        arguments(&["--profile=xtest", "--output=/tmp/out"]),
        arguments(&[
            "--profile=xtest",
            "--output=/tmp/out",
            "--target-dir=/t",
            "--case=one",
        ]),
        arguments(&[
            "--profile=xtest",
            "--output=/tmp/out",
            "--target-dir=/t",
            "--timeout=0",
        ]),
        arguments(&[
            "--profile=xtest",
            "--output=/o",
            "--target-dir=/t",
            "--xts-root=/x",
        ]),
    ] {
        assert!(options(&missing).is_err(), "{missing:?}");
    }
    let xts = options(&arguments(&[
        "--profile=xtest",
        "--output=/o",
        "--target-dir=/t",
        "--xts-root=/x",
        "--xts-expected=/p.json",
    ]))
    .unwrap();
    assert_eq!(xts.xts_root, Some(PathBuf::from("/x")));
}

fn report(status: &str, required: u64, executed: u64, failures: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "status": status,
        "required": required,
        "executed": executed,
        "failures": failures,
        "source_commit": "abc",
        "source_dirty": false,
    })
}

#[test]
fn a_profile_passes_only_on_its_own_complete_report_for_this_source() {
    let passing = judge(Some(&report("PASS", 4, 4, &[])), Some(0), false, "abc");
    assert_eq!(passing.status, "PASS");
    assert_eq!((passing.required, passing.executed), (4, 4));
    assert_eq!(judge(None, Some(0), false, "abc").status, "NORESULT");
    assert_eq!(
        judge(Some(&report("PASS", 4, 4, &[])), Some(0), true, "abc").status,
        "TIMEOUT"
    );
    for (label, value, exit) in [
        ("nonzero exit", report("PASS", 4, 4, &[]), Some(1)),
        ("failed status", report("FAIL", 4, 4, &[]), Some(0)),
        ("nothing required", report("PASS", 0, 0, &[]), Some(0)),
        ("short of required", report("PASS", 4, 3, &[]), Some(0)),
        (
            "a failure listed",
            report("PASS", 4, 4, &["x: MISSING"]),
            Some(0),
        ),
    ] {
        assert_eq!(
            judge(Some(&value), exit, false, "abc").status,
            "FAIL",
            "{label}"
        );
    }
    // Evidence about another source is not evidence about this one.
    let foreign = judge(Some(&report("PASS", 4, 4, &[])), Some(0), false, "def");
    assert_eq!(foreign.status, "NORESULT");
    let mut dirty = report("PASS", 4, 4, &[]);
    dirty["source_dirty"] = serde_json::Value::Bool(true);
    assert_eq!(
        judge(Some(&dirty), Some(0), false, "abc").status,
        "NORESULT"
    );
    // A report without counts cannot be judged, whatever its status says.
    let bare = serde_json::json!({"status": "PASS"});
    assert_eq!(judge(Some(&bare), Some(0), false, "abc").status, "NORESULT");
}

fn verdict(status: &str) -> ProfileVerdict {
    ProfileVerdict {
        status: status.into(),
        detail: None,
        required: 1,
        executed: 1,
        failures: Vec::new(),
        source_commit: None,
        exit: Some(0),
        timed_out: false,
        report: None,
    }
}

#[test]
fn the_gate_passes_only_when_every_profile_does_and_xts_never_by_absence() {
    let blocked = xts_blocked("no checkout");
    let mut profiles = BTreeMap::new();
    assert_eq!(overall(&profiles, &blocked), "NORESULT");
    profiles.insert("xtest".to_owned(), verdict("PASS"));
    assert_eq!(overall(&profiles, &blocked), "PASS");
    profiles.insert("native-input".to_owned(), verdict("FAIL"));
    assert_eq!(overall(&profiles, &blocked), "FAIL");
    profiles.insert("native-input".to_owned(), verdict("NORESULT"));
    assert_eq!(overall(&profiles, &blocked), "NORESULT");
    profiles.insert("native-input".to_owned(), verdict("PASS"));
    let mut failed = xts_blocked("ran");
    failed.status = "FAIL".into();
    assert_eq!(overall(&profiles, &failed), "FAIL");
    let mut passed = xts_blocked("ran");
    passed.status = "PASS".into();
    assert_eq!(overall(&profiles, &passed), "PASS");
}
