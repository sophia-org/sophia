#![cfg(any(
    feature = "comparator-allocation-counting",
    feature = "comparator-cpu-counting"
))]
#[path = "../src/comparator_counters.rs"]
mod guard;

#[test]
fn instrumented_session_requires_one_explicit_bounded_lifetime() {
    for value in ["1", "60000"] {
        guard::require_bounded_run(&[format!("--max-runtime-ms={value}")]).unwrap();
    }
    for args in [
        vec![],
        vec!["--max-runtime-ms=0"],
        vec!["--max-runtime-ms=60001"],
        vec!["--max-runtime-ms=+1"],
        vec!["--max-runtime-ms=60000", "--max-runtime-ms=60000"],
    ] {
        assert!(
            guard::require_bounded_run(&args.into_iter().map(str::to_owned).collect::<Vec<_>>())
                .is_err()
        );
    }
}

#[test]
fn instrumented_binary_refuses_unbounded_session_before_diagnostics() {
    let directory = std::env::temp_dir().join(format!("counter-guard-{}", std::process::id()));
    assert!(!directory.exists());
    let output = std::process::Command::new("timeout")
        .args([
            "5",
            env!("CARGO_BIN_EXE_sophia"),
            "session",
            "run",
            "--session-mode=normal",
        ])
        .env("SOPHIA_DIAGNOSTIC_DIR", &directory)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("comparison counters require exactly one")
    );
    assert!(!directory.exists());
}
