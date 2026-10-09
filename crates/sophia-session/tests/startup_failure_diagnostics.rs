//! A login that ends at startup leaves its cause in the ordinary record.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sophia_config::DesktopOutputReconcileError as Refusal;
use sophia_session::diagnostics::{
    Capture, SessionFailurePhase, SessionRunStage, capture_line, reduced_record,
    session_failure_record, unrecorded_session_failure,
};

#[test]
fn an_output_profile_refusal_keeps_its_kind_and_not_the_connector() {
    let refusals = [
        Refusal::InvalidCandidate("DP-2".into()),
        Refusal::InvalidTopology("DP-2".into()),
        Refusal::InvalidReconciliation("DP-2".into()),
        Refusal::UnknownConnector("DP-2".into()),
        Refusal::AmbiguousConnector("DP-2".into()),
        Refusal::DisconnectedConnector("DP-2".into()),
        Refusal::PreferredModeUnavailable("DP-2".into()),
        Refusal::ModeUnavailable("DP-2".into()),
        Refusal::ModeAmbiguous("DP-2".into()),
        Refusal::ScaleUnsupported("DP-2".into()),
        Refusal::TransformUnsupported("DP-2".into()),
        Refusal::VrrUnsupported("DP-2".into()),
        Refusal::FocusedOutputDisabled("DP-2".into()),
        Refusal::OutputOverlap {
            first: "DP-1".into(),
            second: "DP-2".into(),
        },
        Refusal::MirrorConnectorClaimed {
            primary: "DP-1".into(),
            mirrored: "DP-2".into(),
        },
        Refusal::NoEnabledOutput,
    ];
    let mut codes = std::collections::BTreeSet::new();
    for refusal in &refusals {
        let record = session_failure_record(SessionFailurePhase::Startup, refusal);
        let code = record.rsplit_once("failure_code=").unwrap().1.to_owned();
        assert!(code.starts_with("output_profile_"), "{refusal:?}: {record}");
        assert_eq!(reduced_record(&record).unwrap(), record, "{refusal:?}");
        assert!(!record.contains("DP-"), "{record}");
        codes.insert(code);
    }
    assert_eq!(
        codes.len(),
        refusals.len(),
        "every refusal has its own kind"
    );
    assert_eq!(
        session_failure_record(
            SessionFailurePhase::Startup,
            &Refusal::UnknownConnector("DP-2".into())
        ),
        "sophia_session_failure schema=1 status=failed phase=startup failure_code=output_profile_unknown_connector"
    );
}

#[test]
fn only_a_failure_the_owner_loop_did_not_record_is_recorded_again() {
    let refusal: Box<dyn std::error::Error> = Box::new(Refusal::UnknownConnector("DP-2".into()));
    assert_eq!(
        unrecorded_session_failure(SessionRunStage::Startup, refusal.as_ref()).as_deref(),
        Some(
            "sophia_session_failure schema=1 status=failed phase=startup failure_code=output_profile_unknown_connector"
        )
    );
    assert_eq!(
        unrecorded_session_failure(SessionRunStage::OwnerLoop, refusal.as_ref()),
        None,
        "the owner loop wrote its own record with its own phase"
    );
    let cleanup: Box<dyn std::error::Error> = "native owner retirement failed".into();
    assert_eq!(
        unrecorded_session_failure(SessionRunStage::Finished, cleanup.as_ref()).as_deref(),
        Some(
            "sophia_session_failure schema=1 status=failed phase=cleanup failure_code=unclassified"
        )
    );
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-startup-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn events(&self) -> String {
        fs::read_to_string(self.0.join("events.0.log")).unwrap_or_default()
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for(path: &Path, what: &str, deadline: Duration) -> String {
    let until = Instant::now() + deadline;
    loop {
        let events = fs::read_to_string(path.join("events.0.log")).unwrap_or_default();
        if events.contains(what) {
            return events;
        }
        assert!(Instant::now() < until, "no {what:?} within {deadline:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The record that explains a failed login must outlive both ways the
/// ordinary history loses records: a flooding kind that spent its share of the
/// segment, and a producer burst that fills the queue.
#[test]
fn the_cause_survives_a_spent_present_share_and_a_full_queue() {
    const PRESENT: &str = "sophia_x_present_delivery schema=1";
    const FAILURE: &str = "sophia_session_failure schema=1 status=failed phase=startup failure_code=output_profile_unknown_connector";
    const RESULT: &str = "sophia_session_result schema=1 status=failed";
    let directory = Directory::new();
    let capture = Capture::start(&directory.0).unwrap();

    // Spend the Present share, as the daily desktop does within minutes.
    let spent = "status=share_spent name=sophia_x_present_delivery";
    let until = Instant::now() + Duration::from_secs(60);
    while !directory.events().contains(spent) {
        assert!(Instant::now() < until, "the Present share was never spent");
        for _ in 0..4096 {
            assert!(capture_line(PRESENT));
        }
        std::thread::yield_now();
    }

    // Hold the writer at the directory lock, then flood the ordinary queue
    // until it refuses records.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.0.join("lock"))
        .unwrap();
    lock.lock().unwrap();
    for _ in 0..4096 {
        assert!(capture_line(PRESENT));
    }
    assert!(capture_line(FAILURE));
    assert!(capture_line(RESULT));
    lock.unlock().unwrap();
    drop(lock);
    drop(capture);

    let events = wait_for(&directory.0, RESULT, Duration::from_secs(10));
    assert!(events.contains(FAILURE), "the cause was lost");
    let until = Instant::now() + Duration::from_secs(10);
    let health = loop {
        let health = fs::read_to_string(directory.0.join("health")).unwrap_or_default();
        if health.contains("recording=stopped") {
            break health;
        }
        assert!(Instant::now() < until, "the writer never stopped: {health}");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        !health.contains("discarded=0\n"),
        "the flood never filled the queue, so it controls nothing: {health}"
    );
}
