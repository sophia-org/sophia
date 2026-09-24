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
        // Root and manifest without a scenario used to parse, and the
        // adapter then blocked on the missing scenario every time.
        arguments(&[
            "--profile=xtest",
            "--output=/o",
            "--target-dir=/t",
            "--xts-root=/x",
            "--xts-expected=/p.json",
        ]),
        arguments(&[
            "--profile=xtest",
            "--output=/o",
            "--target-dir=/t",
            "--xts-scenario=selected-core",
        ]),
        arguments(&[
            "--profile=xtest",
            "--output=/o",
            "--target-dir=/t",
            "--xts-root=/x",
            "--xts-expected=/p.json",
            "--xts-scenario=selected-core",
            "--xts-timeout=1786",
        ]),
        // The adapter's deadline must leave the gate its own margin.
        arguments(&[
            "--profile=xtest",
            "--output=/o",
            "--target-dir=/t",
            "--timeout=600",
            "--xts-root=/x",
            "--xts-expected=/p.json",
            "--xts-scenario=selected-core",
            "--xts-timeout=600",
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
        "--xts-scenario=selected-core",
    ]))
    .unwrap();
    assert_eq!(xts.xts_root, Some(PathBuf::from("/x")));
    assert_eq!(xts.xts_scenario.as_deref(), Some("selected-core"));
    assert_eq!(xts.xts_timeout, 600);
    let slow = options(&arguments(&[
        "--profile=xtest",
        "--output=/o",
        "--target-dir=/t",
        "--xts-root=/x",
        "--xts-expected=/p.json",
        "--xts-scenario=selected-core",
        "--xts-timeout=1500",
    ]))
    .unwrap();
    assert_eq!(slow.xts_timeout, 1500);
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
    // run.py keeps its source under `identity`; it is read there too.
    let wire = serde_json::json!({"status": "PASS", "required": 40, "executed": 40, "failures": [],
                                  "identity": {"source_commit": "def", "source_dirty": false}});
    assert_eq!(judge(Some(&wire), Some(0), false, "abc").status, "NORESULT");
    assert_eq!(judge(Some(&wire), Some(0), false, "def").status, "PASS");
    let mut wire_dirty = wire.clone();
    wire_dirty["identity"]["source_dirty"] = serde_json::Value::Bool(true);
    assert_eq!(
        judge(Some(&wire_dirty), Some(0), false, "def").status,
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
        descendants_found: 0,
        descendants_reaped: 0,
        remaining: Vec::new(),
        collection_error: None,
    }
}

#[test]
fn a_reaped_orphan_is_recorded_and_a_lingering_one_is_not_a_result() {
    use super::profiles::collected;
    use super::types::Collection;
    let clean = Collection {
        root_waited: true,
        descendants_found: 0,
        descendants_reaped: 0,
        remaining: Vec::new(),
        error: None,
    };
    assert!(collected(&clean));
    // A child that outlived the entry by a moment and was reaped here is
    // teardown, not a leak.
    let reaped = Collection {
        descendants_found: 2,
        descendants_reaped: 2,
        ..clean.clone()
    };
    assert!(collected(&reaped));
    let lingering = Collection {
        descendants_found: 1,
        remaining: vec![4242],
        error: Some("descendant collection deadline elapsed".into()),
        ..clean.clone()
    };
    assert!(!collected(&lingering));
    let unread = Collection {
        error: Some("invalid child pid".into()),
        ..clean.clone()
    };
    assert!(!collected(&unread));
    let unwaited = Collection {
        root_waited: false,
        ..clean
    };
    assert!(!collected(&unwaited));
}

#[test]
fn the_gate_passes_only_when_every_profile_does_and_xts_never_by_absence() {
    let blocked = xts_blocked("no checkout");
    let mut profiles = BTreeMap::new();
    assert_eq!(overall(&profiles, &[&blocked, &blocked]), "NORESULT");
    profiles.insert("xtest".to_owned(), verdict("PASS"));
    assert_eq!(overall(&profiles, &[&blocked, &blocked]), "PASS");
    profiles.insert("native-input".to_owned(), verdict("FAIL"));
    assert_eq!(overall(&profiles, &[&blocked, &blocked]), "FAIL");
    profiles.insert("native-input".to_owned(), verdict("NORESULT"));
    assert_eq!(overall(&profiles, &[&blocked, &blocked]), "NORESULT");
    profiles.insert("native-input".to_owned(), verdict("PASS"));
    let mut failed = xts_blocked("ran");
    failed.status = "FAIL".into();
    assert_eq!(overall(&profiles, &[&failed, &blocked]), "FAIL");
    let mut passed = xts_blocked("ran");
    passed.status = "PASS".into();
    assert_eq!(overall(&profiles, &[&passed, &blocked]), "PASS");
    // Either suite failing fails the gate; neither passes it by absence.
    assert_eq!(overall(&profiles, &[&passed, &failed]), "FAIL");
    assert_eq!(overall(&profiles, &[&blocked, &failed]), "FAIL");
}
