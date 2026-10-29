use super::*;

#[test]
fn private_capture_directory_requires_a_build_gate_and_one_absolute_path() {
    let valid =
        isolated_session_config(
            &["--diagnostic-capture-directory=/tmp/new-private-capture".into()],
        );
    if cfg!(feature = "diagnostic-capture-delivery") {
        assert_eq!(
            valid.unwrap().diagnostic_capture_directory.unwrap(),
            std::path::Path::new("/tmp/new-private-capture")
        );
    } else {
        assert!(
            valid
                .unwrap_err()
                .to_string()
                .contains("diagnostic-capture-delivery")
        );
    }
    for invalid in [
        "--diagnostic-capture-directory",
        "--diagnostic-capture-directory=relative",
        "--diagnostic-capture-directory=/tmp/../capture",
        "--diagnostic-capture-directory=/tmp/capture\n",
    ] {
        assert!(isolated_session_config(&[invalid.into()]).is_err());
    }
    assert!(
        isolated_session_config(&[
            "--diagnostic-capture-directory=/tmp/a".into(),
            "--diagnostic-capture-directory=/tmp/b".into()
        ])
        .is_err()
    );
    assert!(
        isolated_session_config(&[])
            .unwrap()
            .diagnostic_capture_directory
            .is_none()
    );
}
