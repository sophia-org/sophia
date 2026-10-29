use std::process::{Command, Stdio};

#[test]
fn diagnostic_capture_never_writes_binary_to_an_ordinary_stdout_pipe() {
    let output = Command::new(env!("CARGO_BIN_EXE_sophia"))
        .args(["capture", "7", "--diagnostic-stream"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    if cfg!(feature = "diagnostic-capture-delivery") {
        assert!(error.contains("private socket stdout"));
    } else {
        assert!(error.contains("usage: sophia capture"));
    }
}
