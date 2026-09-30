#[allow(dead_code)]
#[path = "../src/c_desktop_sdk.rs"]
mod c_desktop_sdk;
#[path = "../src/git_tree.rs"]
mod git_tree;
#[allow(dead_code)]
#[path = "../src/output_file_performance/mod.rs"]
mod performance;

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn cargo_must_name_exactly_one_test_executable() {
    let record = r#"{"reason":"compiler-artifact","target":{"name":"output_file_performance"},"profile":{"test":true},"executable":"/private/harness"}"#;
    assert_eq!(
        performance::artifact(record).unwrap(),
        PathBuf::from("/private/harness")
    );
    for bad in [
        String::new(),
        "not json".into(),
        record.replace("true", "false"),
        record.replace("output_file_performance", "some_other_test"),
        format!("{record}\n{record}"),
    ] {
        assert!(performance::artifact(&bad).is_err());
    }
}

#[test]
fn stalled_child_is_terminated_by_the_outer_deadline() {
    let root = std::env::temp_dir().join(format!("sophia-perf-deadline-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let started = Instant::now();
    let result = performance::process::run(
        Command::new("sleep").arg("60"),
        &root,
        "timeout",
        Duration::from_millis(100),
    );
    assert!(result.unwrap_err().contains("deadline expired"));
    assert!(started.elapsed() < Duration::from_secs(5));
    std::fs::remove_dir_all(root).unwrap();
}
