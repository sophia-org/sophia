//! A failed run keeps its cause beside its records, bounded and escaped, and
//! private: `keep` leaves it out unless asked. Its own binary, because a
//! session capture is installed once per process.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use sophia_session::diagnostics::{Capture, FAILURE_CAUSE, Retention, Store, failure_code};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "sophia-failure-cause-{}-{nonce}",
            std::process::id()
        )))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_failed_run_keeps_its_cause_bounded_escaped_and_private() {
    let fixture = Fixture::new();
    let store = Store::open(&fixture.0, Retention::default()).unwrap();
    let record = store.begin("capture", std::process::id(), "").unwrap();
    let capture = Capture::start(&record.path).unwrap();
    let cause = format!(
        "renderer-image handoff head coverage changed during replacement\n\x1b[31m{}",
        "x".repeat(2000)
    );
    capture.record_failure_cause(&cause).unwrap();
    drop(capture);

    let path = record.path.join(FAILURE_CAUSE);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(
        "renderer-image handoff head coverage changed during replacement\\n\\x1b[31mxxx"
    ));
    // One line: the newline and the escape byte are escaped, the cut marked.
    assert_eq!(text.matches('\n').count(), 1);
    assert!(text.ends_with(" [cut]\n"));
    assert!(text.len() < 1024 * 4 + 16, "bounded: {} bytes", text.len());
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );

    store.finish(&record.id, Some(1)).unwrap();
    assert_eq!(
        store.failure_cause(&record.id).unwrap().as_deref(),
        Some(text.as_str())
    );
    let kept = store.keep(&record.id).unwrap();
    assert!(!kept.join(FAILURE_CAUSE).exists(), "kept without asking");
    let asked = store
        .keep_with_application_stderr(&record.id, true)
        .unwrap();
    assert_eq!(fs::read_to_string(asked.join(FAILURE_CAUSE)).unwrap(), text);
    // A run that kept no cause has none to show.
    let clean = store.begin("capture", std::process::id(), "").unwrap();
    store.finish(&clean.id, Some(0)).unwrap();
    assert_eq!(store.failure_cause(&clean.id).unwrap(), None);
}

#[test]
fn the_renderer_image_handoff_refusals_have_their_own_failure_codes() {
    for (message, code) in [
        (
            "renderer-image handoff head coverage changed during replacement",
            "handoff_head_coverage_changed",
        ),
        (
            "renderer-image handoff names an unavailable connector",
            "handoff_connector_unavailable",
        ),
    ] {
        let error: Box<dyn Error> = message.into();
        assert_eq!(failure_code(error.as_ref()), code, "{message}");
    }
}
