#[allow(dead_code)]
#[path = "../src/c_desktop_sdk.rs"]
mod c_desktop_sdk;
#[path = "../src/git_tree.rs"]
mod git_tree;
#[allow(dead_code)]
#[path = "../src/output_file_native_proof.rs"]
mod native;
#[allow(dead_code)]
#[path = "../src/output_file_performance/mod.rs"]
mod output_file_performance;

#[test]
fn preparation_requires_every_named_export_case_once() {
    let tests = native::EXPORT_TESTS
        .iter()
        .map(|name| format!("test {name} ... ok\n"))
        .collect::<String>();
    let summary = format!(
        "test result: ok. {} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2s\n",
        native::EXPORT_TESTS.len()
    );
    let valid = format!("{tests}{summary}");
    assert!(native::check_export_result(&valid).is_ok());
    for invalid in [
        valid.replace(native::EXPORT_TESTS[0], "renamed_case"),
        format!("{valid}test {} ... ok\n", native::EXPORT_TESTS[0]),
        valid.replace("0 ignored", "1 ignored"),
        summary,
        tests,
    ] {
        assert!(native::check_export_result(&invalid).is_err());
    }
}

#[test]
fn cargo_artifact_must_match_the_requested_harness() {
    let record = r#"{"reason":"compiler-artifact","target":{"name":"sophia_session"},"profile":{"test":true},"executable":"/private/session-harness"}"#;
    assert!(output_file_performance::artifact_for(record, "sophia_session").is_ok());
    assert!(output_file_performance::artifact_for(record, "output_file_native_proof").is_err());
    assert!(
        output_file_performance::artifact_for(&format!("{record}\n{record}"), "sophia_session")
            .is_err()
    );
}

#[test]
fn build_jobs_default_to_every_cpu_and_honor_a_positive_override() {
    use output_file_performance::process::jobs;
    use std::ffi::OsStr;
    assert_eq!(jobs(None, 12), Ok(12));
    assert_eq!(jobs(None, 0), Ok(1));
    assert_eq!(jobs(Some(OsStr::new("3")), 12), Ok(3));
    assert_eq!(jobs(Some(OsStr::new("64")), 2), Ok(64));
    for refused in ["0", "01", "-1", "+2", "", "two", "1.5", " 2"] {
        assert!(jobs(Some(OsStr::new(refused)), 12).is_err(), "{refused:?}");
    }
}

#[test]
fn recorded_nice_is_the_callers_and_parses_hostile_command_names() {
    use output_file_performance::process::{nice_field, niceness};
    // Field 19 counts from the last ')', past a name holding ") (" and spaces.
    let stat = |nice: &str| {
        format!(
            "4242 (a) (b c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 {nice} 1 0 99 1 2 3\n"
        )
    };
    assert_eq!(nice_field(&stat("-5")), Some(-5));
    assert_eq!(nice_field(&stat("19")), Some(19));
    assert_eq!(nice_field(&stat("0")), Some(0));
    assert_eq!(nice_field("4242 (truncated) S 1 2"), None);
    // nice(1) without arguments prints the niceness it inherited from us.
    let inherited = std::process::Command::new("nice").output().unwrap();
    assert!(inherited.status.success());
    let inherited: i32 = String::from_utf8(inherited.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(niceness(), Ok(inherited));
}

#[test]
fn sandboxed_programs_run_unwrapped_with_the_resolved_jobs_and_serial_tests() {
    let sandbox = output_file_performance::process::Sandbox {
        repo: "/src".into(),
        output: "/out".into(),
        toolchain: "/rust".into(),
        registry: "/registry".into(),
        jobs: 7,
    };
    let command = sandbox.command("cargo");
    let args = command
        .get_args()
        .map(|a| a.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(command.get_program(), "bwrap");
    assert!(!args.iter().any(|a| a.contains("nice")), "{args:?}");
    let env = |name: &str| {
        args.windows(3)
            .filter(|w| w[0] == "--setenv" && w[1] == name)
            .map(|w| w[2])
            .collect::<Vec<_>>()
    };
    assert_eq!(env("CARGO_BUILD_JOBS"), ["7"]);
    // Test-thread serialization orders the harnesses; it is not a throttle.
    assert_eq!(env("RUST_TEST_THREADS"), ["1"]);
    assert_eq!(env("CARGO_NET_OFFLINE"), ["true"]);
    assert_eq!(args[args.len() - 3..], ["--chdir", "/src", "cargo"]);
}
