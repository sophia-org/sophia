//! Durable capture of renderer-image handoff evidence (t322): captured,
//! retained and discarded handoffs keep their count and path, a failed export
//! keeps its bounded cause, an unknown status keeps only the record name, and
//! free text is never copied.

use std::fs;
use std::path::{Path, PathBuf};

use sophia_session::diagnostics::reduced_record;

fn kept(record: &str) {
    assert_eq!(reduced_record(record).as_deref(), Some(record), "{record}");
}

#[test]
fn every_settlement_keeps_its_count_and_path() {
    for record in [
        "sophia_live_renderer_handoff schema=1 status=captured images=4 source=terminal_switch",
        "sophia_live_renderer_handoff schema=1 status=retained images=4 source=terminal_switch",
        "sophia_live_renderer_handoff schema=1 status=retained images=0 source=switch_rejected",
        "sophia_live_renderer_handoff schema=1 status=retained images=2 source=disable_timeout",
        "sophia_live_renderer_handoff schema=1 status=discarded images=3 source=forced_detach",
        "sophia_live_renderer_handoff schema=1 status=retained images=2 source=seat_resume",
        "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code=renderer_worker_disconnected retained_count=5",
    ] {
        kept(record);
    }
}

#[test]
fn unknown_statuses_keep_only_the_name_and_unbounded_values_drop_alone() {
    for record in [
        "sophia_live_renderer_handoff schema=1 status=kept images=4",
        "sophia_live_renderer_handoff schema=1 images=4 source=terminal_switch",
        "sophia_live_renderer_handoff failure_code=private_document",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            Some("sophia_live_renderer_handoff"),
            "{record}"
        );
    }
    assert_eq!(
        reduced_record(
            "sophia_live_renderer_handoff schema=1 status=retained images=-1 source=/tmp/x error=private"
        )
        .as_deref(),
        Some("sophia_live_renderer_handoff schema=1 status=retained")
    );
    assert_eq!(
        reduced_record(
            "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code=made_up retained_count=5"
        )
        .as_deref(),
        Some("sophia_live_renderer_handoff schema=1 status=failed phase=export_images retained_count=5")
    );
}

fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn every_handoff_status_and_source_the_session_prints_is_admitted() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut checked = 0;
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        for (at, _) in source.match_indices("\"sophia_live_renderer_handoff schema=1 ") {
            let line = &source[at + 1..];
            let line = &line[..line.find('"').unwrap()];
            // Substitute every placeholder with a value its field admits.
            let mut record = Vec::new();
            for field in line.split_whitespace() {
                let field = match field.split_once('=') {
                    Some(("status", "{}")) => "status=retained".to_owned(),
                    Some(("failure_code", "{}")) => {
                        "failure_code=renderer_worker_disconnected".to_owned()
                    }
                    Some((key, value)) if value.starts_with('{') => format!("{key}=1"),
                    _ => field.to_owned(),
                };
                record.push(field);
            }
            kept(&record.join(" "));
            checked += 1;
        }
    }
    assert!(checked >= 6, "{checked}");
}
