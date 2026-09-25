use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sophia_runtime::{TraceLevel, init_tracing_with_layer};
use sophia_session::diagnostics::{Capture, DIAGNOSTIC_RECORD_MAX_BYTES, Retention, Store};

#[path = "../src/scanout_diagnostics.rs"]
mod scanout_diagnostics;

const TEST_NAME: &str = "scanout_records_reach_capture_independently_of_console_logging";
const CHILD_MODE: &str = "SOPHIA_SCANOUT_DIAGNOSTICS_TEST_CHILD";
const CHILD_STORE: &str = "SOPHIA_SCANOUT_DIAGNOSTICS_TEST_STORE";
const EXPORTER_TARGET: &str = "sophia_scanout_evidence";
const ATOMIC_REJECTED: &str = "sophia_live_atomic_test schema=1 output=2 scene_generation=10 status=Rejected errno=22 request_scope=PageFlip nonblocking=true allow_modeset=false";
const ATOMIC_ACCEPTED: &str = "sophia_live_atomic_test schema=1 output=2 scene_generation=11 status=Submitted errno=none request_scope=PageFlip nonblocking=true allow_modeset=false";
const LAYOUT_TESTED: &str = "sophia_live_layout_probe schema=1 output=2 scene_generation=10 source_image=41 status=Tested original_status=Rejected alternative_status=Submitted original_errno=22 alternative_errno=none format=875713112 original_modifier=144115188757872388 alternative_modifier=0";
const SHELL_BINDING: &str = "sophia_shell_native_binding schema=1 connection_epoch=1 content_grant_epoch=2 output=3 candidate_generation=4 native_owner=5 native_frame=6 head=7 target_generation=8 heads=1";
const SHELL_COMPLETION: &str = "sophia_shell_native_completion schema=1 output=3 native_owner=5 native_frame=6 heads=1 monotonic_usec=12345 timestamp_source=kernel missing_kernel_timestamp=0";
const FORMATTER_MARKER: &str = "unrelated formatter output remains visible";
const PRESENT_WRITTEN: &str = "sophia_x_present_delivery schema=1 client=2 transaction=0 sequence=7 window_token=11 subscription_token=12 pixmap_token=13 serial=4 kind=idle status=written";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "sophia-scanout-diagnostics-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct MustNotFormat;

impl fmt::Debug for MustNotFormat {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        panic!("the diagnostic layer must not format private fields or unrelated events")
    }
}

fn child_command(mode: &str, filter: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
        .env(CHILD_MODE, mode)
        .env("RUST_LOG", filter);
    command
}

fn capture_child(path: &Path) {
    init_tracing_with_layer(TraceLevel::Info, scanout_diagnostics::layer()).unwrap();
    let store = Store::open(path, Retention::default()).unwrap();
    let record = store.begin("scanout-test", std::process::id(), "").unwrap();
    let capture = Capture::start(&record.path).unwrap();

    tracing::info!(target: EXPORTER_TARGET, "{}", ATOMIC_REJECTED);
    tracing::info!(target: EXPORTER_TARGET, private = ?MustNotFormat, "{}", ATOMIC_ACCEPTED);
    tracing::info!(
        target: EXPORTER_TARGET,
        "{} cookie=private-cookie title=private-title path=/private/account",
        LAYOUT_TESTED
    );

    tracing::info!(target: EXPORTER_TARGET, "{} payload=private", SHELL_BINDING);
    tracing::info!(target: EXPORTER_TARGET, "{}", SHELL_COMPLETION);
    tracing::debug!(target: "sophia_application_evidence", "{} title=secret xid=123", PRESENT_WRITTEN);
    tracing::debug!(target: EXPORTER_TARGET, "{}", PRESENT_WRITTEN);
    tracing::debug!(target: "sophia_application_evidence", "{}", SHELL_COMPLETION);
    tracing::info!(target: "another_backend", "{}", SHELL_COMPLETION);
    tracing::info!(target: EXPORTER_TARGET, "sophia_shell_native_completion_extra native_frame=6");

    // Both target identity and the complete record token are required.
    tracing::info!(target: "another_backend", "{}", ATOMIC_REJECTED);
    tracing::info!(
        target: EXPORTER_TARGET,
        "sophia_live_direct_scanout schema=1 status=exported scene_generation=20"
    );
    tracing::info!(
        target: EXPORTER_TARGET,
        "sophia_live_atomic_test_extra schema=1 status=Rejected errno=22"
    );
    tracing::info!(
        target: EXPORTER_TARGET,
        "sophia_live_layout_probe_extra schema=1 status=Tested"
    );
    tracing::info!(target: EXPORTER_TARGET, "prefix {}", LAYOUT_TESTED);
    tracing::info!(target: EXPORTER_TARGET, private = ?MustNotFormat);
    {
        let span = tracing::info_span!(
            target: EXPORTER_TARGET,
            "sophia_live_layout_probe",
            private = ?MustNotFormat
        );
        let _entered = span.enter();
    }
    tracing::debug!(target: "unrelated_debug_target", private = ?MustNotFormat);
    assert!(
        !tracing::enabled!(target: "unrelated_debug_target", tracing::Level::DEBUG),
        "the evidence layer must not enable unrelated debug callsites"
    );

    // The limit applies to the original message, before private fields shrink.
    let prefix =
        "sophia_live_atomic_test schema=1 scene_generation=12 status=Submitted errno=none payload=";
    let suffix = " output=2";
    let boundary = format!(
        "{prefix}{}{suffix}",
        "x".repeat(DIAGNOSTIC_RECORD_MAX_BYTES - prefix.len() - suffix.len())
    );
    assert_eq!(boundary.len(), DIAGNOSTIC_RECORD_MAX_BYTES);
    tracing::info!(target: EXPORTER_TARGET, "{}", boundary);

    let prefix =
        "sophia_live_atomic_test schema=1 scene_generation=13 status=Rejected errno=22 payload=";
    let oversized = format!(
        "{prefix}{}",
        "y".repeat(DIAGNOSTIC_RECORD_MAX_BYTES + 1 - prefix.len())
    );
    assert_eq!(oversized.len(), DIAGNOSTIC_RECORD_MAX_BYTES + 1);
    tracing::info!(target: EXPORTER_TARGET, "{}", oversized);

    let oversized_unicode = format!("{prefix}{}", "🦀".repeat(DIAGNOSTIC_RECORD_MAX_BYTES / 4));
    tracing::info!(target: EXPORTER_TARGET, "{}", oversized_unicode);

    drop(capture);
    wait_for_stopped_capture(&record.path);
}

fn wait_for_stopped_capture(path: &Path) {
    // Drop bounds recovery latency, not durable completion. Keep this child
    // alive until the worker publishes its actual synchronized health record.
    let started = Instant::now();
    loop {
        let health = fs::read_to_string(path.join("health"));
        if health
            .as_ref()
            .is_ok_and(|text| text.lines().any(|line| line == "recording=stopped"))
        {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "capture did not stop before child exit: pid={} path={} elapsed={:?} last_health={health:?}",
            std::process::id(),
            path.display(),
            started.elapsed(),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn scanout_records_reach_capture_independently_of_console_logging() {
    match std::env::var(CHILD_MODE).as_deref() {
        Ok("capture") => {
            capture_child(&PathBuf::from(std::env::var_os(CHILD_STORE).unwrap()));
            return;
        }
        Ok("formatter") => {
            init_tracing_with_layer(TraceLevel::Info, scanout_diagnostics::layer()).unwrap();
            tracing::info!(target: "unrelated_formatter_target", "{}", FORMATTER_MARKER);
            return;
        }
        Err(std::env::VarError::NotPresent) => {}
        mode => panic!("unexpected subprocess mode: {mode:?}"),
    }

    let fixture = Fixture::new();
    let output = child_command("capture", "off")
        .env(CHILD_STORE, &fixture.0)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "capture subprocess failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let store = Store::open(&fixture.0, Retention::default()).unwrap();
    let record = store.select(Some("latest")).unwrap();
    let inspection = store.inspect(&record.id, None).unwrap();
    let records = inspection
        .events
        .iter()
        .map(|line| {
            line.split_once("sophia_")
                .map(|(_, record)| format!("sophia_{record}"))
                .unwrap_or_else(|| panic!("unexpected captured record: {line}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records,
        [
            ATOMIC_REJECTED,
            ATOMIC_ACCEPTED,
            LAYOUT_TESTED,
            SHELL_BINDING,
            SHELL_COMPLETION,
            PRESENT_WRITTEN,
            "sophia_live_atomic_test schema=1 scene_generation=12 status=Submitted errno=none output=2",
        ],
        "only approved messages should persist, once each and without private fields"
    );
    assert!(inspection.health.contains("recording=stopped\n"));
    assert!(inspection.health.contains("storage_errors=0\n"));
    assert!(
        inspection.health.lines().any(|line| line == "discarded=2"),
        "both oversized events must contribute to drop accounting: {}",
        inspection.health
    );

    let output = child_command("formatter", "info").output().unwrap();
    assert!(
        output.status.success(),
        "formatter subprocess failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(FORMATTER_MARKER),
        "the diagnostic layer must not veto ordinary console output"
    );
}
