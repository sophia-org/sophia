use std::process::Command;

/// The paired t254 qualification needed the legacy transport. The files-only
/// diagnostic must not answer to its name.
#[test]
fn the_retired_qualify_phase_is_refused_by_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "check",
            "admin-file-performance",
            "qualify",
            "--output=/nonexistent/sophia-admin-qualify",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("qualify is retired"));
}
