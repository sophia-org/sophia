//! M5 acceptance: the XTEST adapter's eight obligation groups, run through
//! `cargo xtask check m5-acceptance`. Every group test carries `#[ignore]` so
//! an ordinary `cargo test` cannot claim acceptance; the gate includes them.
//!
//! The groups land with the mechanism they prove. Until a group's test exists
//! here, its inventory row reports NOT_RUN as a bound test absent from the
//! built binary, which is the honest state and not a pass.
#![cfg(unix)]

#[path = "support/xtest_acceptance/mod.rs"]
mod support;

/// The record shape the gate parses, pinned where it is produced. Not a
/// group test: it makes no claim about XTEST behaviour.
#[test]
fn evidence_record_names_the_group_and_every_subcase() {
    let line = support::record(
        "grab_control",
        &["strict_boolean", "impervious_during_server_grab"],
        3,
        3,
        2,
    );
    let body = line
        .strip_prefix("sophia_m5_acceptance ")
        .expect("the gate keys the record by this prefix");
    assert!(body.starts_with("{\"schema\":1,\"case\":\"M5.grab_control\","));
    assert!(body.contains(
        "\"subcases\":{\"strict_boolean\":\"PASS\",\"impervious_during_server_grab\":\"PASS\"}"
    ));
    assert!(body.contains(
        "\"cleanup\":{\"actors_started\":3,\"actors_collected\":3,\"pending_actors\":0,\"complete\":true}"
    ));
    assert!(body.contains("\"real_session_invocations\":2"));
    assert!(body.ends_with('}'));
    let mut evidence = support::Evidence::default();
    // Nothing was started, so nothing may be claimed: emitting is refused.
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        std::mem::take(&mut evidence).emit("grab_control", &["strict_boolean"])
    }));
    assert!(refused.is_err());
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn processing_barrier() {
    support::adapter::processing_barrier();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn cancellation_half_close() {
    support::adapter::cancellation_half_close();
}

// This lane's groups are appended here and the top of the file is left for
// the adapter lane's, so the two never share a hunk.

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn registration_admission() {
    support::groups::registration_admission();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn version_negotiation() {
    support::groups::version_negotiation();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn fake_input_encoding() {
    support::groups::fake_input_encoding();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn fake_input_effects() {
    support::groups::fake_input_effects();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn cursor_comparison() {
    support::groups::cursor_comparison();
}

#[test]
#[ignore = "run through cargo xtask check m5-acceptance"]
fn grab_control() {
    support::groups::grab_control();
}
