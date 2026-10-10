//! Durable capture of session lock evidence (t297): each admitted record keeps
//! exactly its bounded fields, an unknown status drops the record whole, and
//! free text is never copied.

use std::fs;
use std::path::{Path, PathBuf};

use sophia_session::diagnostics::reduced_record;

fn kept(record: &str) {
    assert_eq!(reduced_record(record).as_deref(), Some(record), "{record}");
}

fn reduced(record: &str, expected: &str) {
    assert_eq!(
        reduced_record(record).as_deref(),
        Some(expected),
        "{record}"
    );
}

#[test]
fn every_producer_form_keeps_its_bounded_fields() {
    for record in [
        "sophia_live_session_lock schema=1 status=locking source=shortcut epoch=2 input_epoch=5 revoked_leases=1",
        "sophia_live_session_lock schema=1 status=already_locked source=proof epoch=2",
        "sophia_live_session_lock schema=1 status=locked epoch=2",
        "sophia_live_session_lock schema=1 status=covered epoch=2 topology_epoch=3 outputs=2 heads=2",
        "sophia_live_session_lock schema=1 status=key_held epoch=2 device=262",
        "sophia_live_session_lock schema=1 status=checking epoch=2 attempt=1",
        "sophia_live_session_lock schema=1 status=failed epoch=2 attempt=1 verdict=Rejected",
        "sophia_live_session_lock schema=1 status=failed epoch=2 attempt=2 verdict=Unavailable",
        "sophia_live_session_lock schema=1 status=stale_verdict epoch=2 attempt=3",
        "sophia_live_session_lock schema=1 status=unlocking epoch=2 input_epoch=6 revoked_leases=0",
        "sophia_live_session_lock schema=1 status=unlocked epoch=2",
        "sophia_live_session_lock schema=1 status=refused reason=no_authenticator source=shortcut",
        "sophia_live_session_lock schema=1 status=refused reason=no_native_presentation source=proof",
        "sophia_live_session_lock schema=1 status=refused reason=EpochExhausted source=shortcut",
        "sophia_live_session_lock schema=1 status=authenticator_ready",
    ] {
        kept(record);
    }
}

#[test]
fn errors_and_debug_dumps_are_never_copied() {
    reduced(
        "sophia_live_session_lock schema=1 status=refused reason=lock_input source=shortcut error=/run/user/1000/lock",
        "sophia_live_session_lock schema=1 status=refused reason=lock_input source=shortcut",
    );
    reduced(
        "sophia_live_session_lock schema=1 status=repaint_deferred epoch=4 error=renderer said no",
        "sophia_live_session_lock schema=1 status=repaint_deferred epoch=4",
    );
    reduced(
        "sophia_live_session_lock schema=1 status=unlock_repaint_failed epoch=4 error=x",
        "sophia_live_session_lock schema=1 status=unlock_repaint_failed epoch=4",
    );
    reduced(
        "sophia_live_session_lock schema=1 status=unavailable outcome=Failed(SessionUnlockAttempt { epoch: SessionLockEpoch(4), serial: 2 }, Unavailable)",
        "sophia_live_session_lock schema=1 status=unavailable",
    );
    for status in ["authenticator_unavailable", "authenticator_failed"] {
        reduced(
            &format!("sophia_live_session_lock schema=1 status={status} error=agent_exited"),
            &format!("sophia_live_session_lock schema=1 status={status}"),
        );
    }
}

#[test]
fn an_unknown_or_missing_status_drops_the_record_whole() {
    for record in [
        "sophia_live_session_lock schema=1 status=lockd epoch=1",
        "sophia_live_session_lock schema=1 status= epoch=1",
        "sophia_live_session_lock schema=1 epoch=1 topology_epoch=3",
        "sophia_live_session_lock schema=1",
    ] {
        assert_eq!(reduced_record(record), None, "{record}");
    }
}

#[test]
fn unbounded_values_are_dropped_field_by_field_and_repeats_keep_the_first() {
    reduced(
        "sophia_live_session_lock schema=2 status=locking source=/home/user epoch=-1 input_epoch=5x revoked_leases=99999999999999999999999 device=262",
        "sophia_live_session_lock status=locking device=262",
    );
    reduced(
        "sophia_live_session_lock schema=1 status=refused reason=because source=shortcut",
        "sophia_live_session_lock schema=1 status=refused source=shortcut",
    );
    reduced(
        "sophia_live_session_lock schema=1 status=locked status=bogus epoch=2 epoch=9",
        "sophia_live_session_lock schema=1 status=locked epoch=2",
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

/// The words that follow `needle` in Session's own source.
fn words_after(needle: &str) -> Vec<String> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut words = files
        .iter()
        .flat_map(|path| {
            let source = fs::read_to_string(path).unwrap();
            source
                .match_indices(needle)
                .map(|(at, _)| {
                    source[at + needle.len()..]
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    words.sort();
    words.dedup();
    words
}

#[test]
fn every_status_and_source_the_session_prints_is_admitted() {
    // A new producer status that the reducer does not know would vanish from
    // daily capture; this keeps the vocabulary equal to the producers.
    let statuses = words_after("sophia_live_session_lock schema=1 status=");
    assert!(statuses.len() >= 17, "{statuses:?}");
    for status in &statuses {
        kept(&format!(
            "sophia_live_session_lock schema=1 status={status}"
        ));
    }
    let lock_sources = words_after("begin_session_lock!(\"");
    assert!(!lock_sources.is_empty());
    for source in &lock_sources {
        kept(&format!(
            "sophia_live_session_lock schema=1 status=already_locked source={source} epoch=1"
        ));
    }
}
