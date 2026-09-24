#![cfg(test)]

use super::judge;
use crate::headless_client_gate::ClientSession;

const SELECTION: &str =
    "sophia_live_selection schema=1 status=complete content=redacted owner_changes=1 conversions=2";
const XTEST: &str = "sophia_live_session_xtest schema=1 status=complete admitted=true injected_keys=0 injected_buttons=4 injected_motions=14 refused=0";
const DONE: &str = "sophia_session_result schema=1 status=bounded_complete";

fn log(selection: &str, xtest: &str) -> String {
    format!("{selection}\n2026 INFO x: {xtest}\n{DONE}\n")
}

fn session(exited_cleanly: Option<bool>, text: String) -> ClientSession {
    ClientSession {
        exited_cleanly,
        text,
    }
}

#[test]
fn a_complete_run_passes() {
    assert!(judge(&session(Some(true), log(SELECTION, XTEST))).is_ok());
}

#[test]
fn a_clean_exit_does_not_excuse_a_missing_obligation() {
    // The driver's pass line is the session's to check; these are the
    // gate's own, and each must hold even when the session exits 0.
    let cases = [
        log(
            &SELECTION.replace("owner_changes=1", "owner_changes=0"),
            XTEST,
        ),
        log(&SELECTION.replace("conversions=2", "conversions=1"), XTEST),
        log("", XTEST),
        log(SELECTION, &XTEST.replace("admitted=true", "admitted=false")),
        log(SELECTION, &XTEST.replace("refused=0", "refused=1")),
        log(
            SELECTION,
            &XTEST.replace("injected_buttons=4", "injected_buttons=2"),
        ),
        log(SELECTION, ""),
        log(SELECTION, XTEST).replace("bounded_complete", "failed"),
    ];
    for case in cases {
        assert!(
            judge(&session(Some(true), case.clone())).is_err(),
            "passed:\n{case}"
        );
    }
}

#[test]
fn an_unclean_or_unbounded_session_fails_whatever_it_logged() {
    assert!(judge(&session(Some(false), log(SELECTION, XTEST))).is_err());
    assert!(judge(&session(None, log(SELECTION, XTEST))).is_err());
}
