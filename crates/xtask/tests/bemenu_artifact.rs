//! The artifact preparer refuses ambiguous input and bounds process cleanup.
#[allow(dead_code)]
#[path = "../src/bemenu_artifact.rs"]
mod bemenu_artifact;
#[allow(dead_code)]
#[path = "../src/c_desktop_sdk.rs"]
mod c_desktop_sdk;
#[path = "../src/git_tree.rs"]
mod git_tree;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn ambiguous_revision_and_existing_destination_are_refused() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        bemenu_artifact::run(root, &[])
            .unwrap_err()
            .contains("usage")
    );
    let args = [root.display().to_string(), "HEAD".into(), "unused".into()];
    assert!(
        bemenu_artifact::run(root, &args)
            .unwrap_err()
            .contains("40 lowercase hex")
    );
    let args = [
        root.display().to_string(),
        "0".repeat(40),
        root.display().to_string(),
    ];
    assert!(
        bemenu_artifact::run(root, &args)
            .unwrap_err()
            .contains("already exists")
    );
}

#[test]
fn excess_output_is_refused_instead_of_truncated_success() {
    let error = bemenu_artifact::bounded(
        Command::new("head").args(["-c", "1048577", "/dev/zero"]),
        Duration::from_secs(5),
        "oversized fixture",
    )
    .unwrap_err();
    assert!(error.contains("output exceeds"), "{error}");
}

#[test]
fn timeout_stops_the_private_process_group() {
    let start = Instant::now();
    let error = bemenu_artifact::bounded(
        Command::new("sh").args(["-c", "sleep 30 & wait"]),
        Duration::from_millis(50),
        "timeout fixture",
    )
    .unwrap_err();
    assert!(error.contains("exceeded"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(6));
}

#[test]
fn inherited_pipe_cannot_keep_collection_waiting_after_leader_exit() {
    let start = Instant::now();
    let error = bemenu_artifact::bounded(
        Command::new("sh").args(["-c", "sleep 30 & exit 0"]),
        Duration::from_secs(5),
        "inherited pipe fixture",
    )
    .unwrap_err();
    assert!(error.contains("descendant kept its output open"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(7));
}
