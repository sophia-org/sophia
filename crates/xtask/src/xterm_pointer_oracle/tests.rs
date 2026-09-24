#![cfg(test)]

use super::judge;
use crate::headless_client_gate::ClientSession;

const XTEST: &str = "sophia_live_session_xtest schema=1 status=complete admitted=true injected_keys=0 injected_buttons=6 injected_motions=9 refused=0";
const DONE: &str = "sophia_session_result schema=1 status=bounded_complete";

fn session(exited_cleanly: Option<bool>, xtest: &str) -> ClientSession {
    ClientSession {
        exited_cleanly,
        text: format!("2026 INFO x: {xtest}\n{DONE}\n"),
    }
}

#[test]
fn a_complete_run_passes() {
    assert!(judge(&session(Some(true), XTEST)).is_ok());
}

#[test]
fn a_clean_exit_does_not_excuse_a_missing_obligation() {
    for case in [
        XTEST.replace("admitted=true", "admitted=false"),
        XTEST.replace("refused=0", "refused=2"),
        XTEST.replace("injected_buttons=6", "injected_buttons=4"),
        XTEST.replace("injected_motions=9", "injected_motions=2"),
        String::new(),
    ] {
        assert!(
            judge(&session(Some(true), &case)).is_err(),
            "passed: {case}"
        );
    }
    let mut unbounded = session(Some(true), XTEST);
    unbounded.text = unbounded.text.replace("bounded_complete", "failed");
    assert!(judge(&unbounded).is_err());
}

#[test]
fn an_unclean_or_unbounded_session_fails_whatever_it_logged() {
    assert!(judge(&session(Some(false), XTEST)).is_err());
    assert!(judge(&session(None, XTEST)).is_err());
}
