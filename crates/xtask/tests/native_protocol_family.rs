#[allow(dead_code)]
#[path = "../src/native_protocol_family.rs"]
mod native_protocol_family;

#[test]
fn an_empty_or_ignored_only_target_is_not_lifecycle_evidence() {
    for log in [
        "",
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        "test result: ok. 0 passed; 0 failed; 9 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        "running 9 tests\ntest result: FAILED. 8 passed; 1 failed; 0 ignored\n",
    ] {
        assert!(
            native_protocol_family::require_tests_ran(log).is_err(),
            "{log}"
        );
    }
    assert!(native_protocol_family::require_tests_ran(
        "test result: ok. 0 passed; 0 failed; 0 ignored\ntest result: ok. 9 passed; 0 failed; 0 ignored\n"
    ).is_ok());
}
