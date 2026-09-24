#![cfg(test)]
//! The x11bench suite's own rules: what a manifest may say, how x11bench's
//! output is read, and how a contained run is judged. Nothing runs here.
use super::profiles::options;
use super::x11bench::{ContainedResult, ManifestRow, inventory, judge, manifest, outcomes};
use std::collections::BTreeMap;

const GEOMETRY: [u16; 4] = [1280, 720, 339, 191];

fn row(test: &str, expected: Option<&str>) -> ManifestRow {
    ManifestRow {
        test: test.into(),
        expected: expected.map(str::to_owned),
        reason: expected.map(|_| "a reason".to_owned()),
    }
}

fn statuses(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, status)| ((*name).to_owned(), (*status).to_owned()))
        .collect()
}

/// Two tests, both passing on the oracle; the host's outcomes as given.
fn run(host: &[(&str, &str)]) -> ContainedResult {
    ContainedResult {
        inventory: vec!["circle".into(), "solid_red".into()],
        control: statuses(&[("circle", "PASS"), ("solid_red", "PASS")]),
        host: statuses(host),
        host_geometry: Some(GEOMETRY),
        oracle_geometry: Some(GEOMETRY),
        ..ContainedResult::default()
    }
}

#[test]
fn a_manifest_names_each_test_once_and_every_declaration_has_a_reason() {
    let rows =
        manifest(r#"[{"test": "a"}, {"test": "b", "expected": "FAIL", "reason": "why"}]"#).unwrap();
    assert_eq!(
        rows,
        vec![row("a", None), {
            let mut declared = row("b", Some("FAIL"));
            declared.reason = Some("why".into());
            declared
        }]
    );
    for refused in [
        "[]",
        r#"[{"test": "a"}, {"test": "a"}]"#,
        r#"[{"test": "a", "expected": "FAIL"}]"#,
        r#"[{"test": "a", "expected": "FAIL", "reason": "  "}]"#,
        r#"[{"test": "a", "reason": "why"}]"#,
        r#"[{"test": "a", "expected": "PASS"}]"#,
        r#"[{"test": "a", "expected": "UNTESTED", "reason": "why"}]"#,
        r#"[{"test": "a", "status": "FAIL"}]"#,
    ] {
        assert!(manifest(refused).is_err(), "{refused} must be refused");
    }
}

#[test]
fn the_inventory_is_what_list_prints() {
    let listing = "Available tests (2):\n  alpha_blend - Alpha blending with specific values\n  win_raise - XRaiseWindow brings window to front\n";
    assert_eq!(inventory(listing), vec!["alpha_blend", "win_raise"]);
}

#[test]
fn outcomes_are_read_through_colour_and_padding() {
    let stdout = "Connected to X display\n\n\u{1b}[1mRunning X11 visual tests\u{1b}[0m\n\
        alpha_blend                         \u{1b}[32m[PASS]\u{1b}[0m\n\
        colored_text                        \u{1b}[32m[PASS]\u{1b}[0m (810 pixels within tolerance)\n\
        circle                              \u{1b}[31m[FAIL]\u{1b}[0m 226 pixels differ\n\
        gc_set                              \u{1b}[31m[ERROR]\u{1b}[0m Failed to load reference\n\
        solid_red                           \u{1b}[34m[GENERATED]\u{1b}[0m (regenerated)\n\
        Summary:\n  Passed:  2\n";
    assert_eq!(
        outcomes(stdout),
        statuses(&[
            ("alpha_blend", "PASS"),
            ("colored_text", "PASS"),
            ("circle", "FAIL"),
            ("gc_set", "ERROR"),
            ("solid_red", "GENERATED"),
        ])
    );
}

#[test]
fn a_run_passes_when_every_test_meets_its_expectation() {
    let rows = [row("circle", Some("FAIL")), row("solid_red", None)];
    let judged = judge(&rows, &run(&[("circle", "FAIL"), ("solid_red", "PASS")]));
    assert_eq!(judged.status, "PASS", "{judged:?}");
    assert_eq!((judged.passed, judged.declared), (1, 1));
}

#[test]
fn an_undeclared_failure_and_a_stale_declaration_both_fail() {
    let rows = [row("circle", Some("FAIL")), row("solid_red", None)];
    let judged = judge(&rows, &run(&[("circle", "FAIL"), ("solid_red", "FAIL")]));
    assert_eq!(judged.status, "FAIL");
    assert!(judged.failures[0].contains("solid_red: FAIL"), "{judged:?}");
    // A fix must take its declaration out of the manifest with it.
    let judged = judge(&rows, &run(&[("circle", "PASS"), ("solid_red", "PASS")]));
    assert_eq!(judged.status, "FAIL");
    assert!(
        judged.failures[0].contains("the manifest is stale"),
        "{judged:?}"
    );
    // A test the host never reported is not a pass.
    let judged = judge(&rows, &run(&[("circle", "FAIL")]));
    assert!(judged.failures[0].contains("no result"), "{judged:?}");
}

#[test]
fn the_manifest_and_the_suite_must_name_the_same_tests() {
    let host = [("circle", "FAIL"), ("solid_red", "PASS")];
    let short = [row("solid_red", None)];
    let judged = judge(&short, &run(&host));
    assert!(
        judged
            .failures
            .iter()
            .any(|failure| failure.contains("circle: x11bench runs it"))
    );
    let long = [
        row("circle", Some("FAIL")),
        row("solid_red", None),
        row("gone", None),
    ];
    let judged = judge(&long, &run(&host));
    assert!(
        judged
            .failures
            .iter()
            .any(|failure| failure.contains("gone: manifested"))
    );
}

#[test]
fn an_unstable_oracle_or_a_mismatched_screen_says_nothing_about_the_host() {
    let rows = [row("circle", Some("FAIL")), row("solid_red", None)];
    let host = [("circle", "FAIL"), ("solid_red", "PASS")];
    let mut unstable = run(&host);
    unstable.control.insert("circle".into(), "FAIL".into());
    let judged = judge(&rows, &unstable);
    assert_eq!(judged.status, "FAIL");
    assert!(
        judged.failures[0].contains("the oracle is unstable"),
        "{judged:?}"
    );
    assert_eq!(judged.passed, 0, "nothing about the host is judged");
    let mut elsewhere = run(&host);
    elsewhere.oracle_geometry = Some([1024, 768, 271, 203]);
    assert!(judge(&rows, &elsewhere).failures[0].contains("would not apply"));
    let mut stopped = run(&host);
    stopped.error = Some("did not bind".into());
    assert!(judge(&rows, &stopped).failures[0].contains("did not bind"));
}

#[test]
fn x11bench_options_come_together_and_leave_the_gate_its_margin() {
    let base = [
        "--profile=xtest",
        "--output=/tmp/out",
        "--target-dir=/tmp/target",
    ];
    let with = |extra: &[&str]| {
        options(
            &base
                .iter()
                .chain(extra)
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>(),
        )
    };
    let parsed = with(&["--x11bench-bin=/b", "--x11bench-expected=/e.json"]).unwrap();
    assert_eq!(
        parsed.x11bench_bin.as_deref(),
        Some(std::path::Path::new("/b"))
    );
    assert_eq!(parsed.x11bench_timeout, 600);
    assert!(with(&["--x11bench-bin=/b"]).is_err());
    assert!(with(&["--x11bench-timeout=60"]).is_err());
    assert!(
        with(&[
            "--x11bench-bin=/b",
            "--x11bench-expected=/e.json",
            "--x11bench-timeout=1780",
        ])
        .is_err()
    );
    assert!(
        with(&[
            "--x11bench-bin=/b",
            "--x11bench-expected=/e.json",
            "--x11bench-timeout=0"
        ])
        .is_err()
    );
}
