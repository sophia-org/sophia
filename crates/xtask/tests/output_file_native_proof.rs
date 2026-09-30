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
