//! `cargo xtask check xtest-selection`: drag-select in one real xterm and
//! middle-click paste into another, driven by XTEST through a headless
//! production session.
//!
//! WHAT IT PROVES. A real xterm takes PRIMARY when an XTEST drag lands on it,
//! and a second real xterm asks for it when an XTEST middle-click lands on it.
//! The verdict is read from the session's own wire counters -- `owner_changes`
//! is SetSelectionOwner arriving at the authority, `conversions` is
//! ConvertSelection -- together with the driver's aim checks, which read the
//! pointer back before any button so a silent selection cannot be mistaken for
//! a gesture that never landed. Two defects hid behind this path until it
//! could be run: XTEST buttons delivered at the screen origin (t155), and XTEST
//! pointer events targeted at the focus instead of the surface under them
//! (t156).
//!
//! WHAT IT DOES NOT. It runs a deterministic headless head with no window
//! manager and no physical input. It says nothing about a physical drag, a
//! window manager's session, or scanout; those are t124's and t147's other
//! rows.
//!
//! `--self-test` runs three mutations that must each fail -- a drag on a blank
//! row, a session that does not admit XTEST, and no middle-click -- and passes
//! only if all three do. A gate that cannot go red is not evidence.

use std::path::Path;

use crate::headless_client_gate::{
    ClientRun, ClientSession, admitted_xtest_record, build, evidence_directory, number, record,
    require_completed, run_client_session,
};

const EXAMPLE: &str = "xtest_selection_driver";

/// The driver's stdout on a full pass, compared byte for byte by the session.
const PASS_LINE: &str = "sophia_xtest_selection schema=1 status=pass owner_in_a=true matched=true pointer_in_a=true pointer_in_b=true";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Variant {
    Pass,
    BlankRow,
    NoAdmission,
    NoPaste,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::BlankRow => "blank-row",
            Self::NoAdmission => "no-admission",
            Self::NoPaste => "no-paste",
        }
    }
}

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let self_test = match arguments {
        [] => false,
        [flag] if flag == "--self-test" => true,
        [help] if help == "--help" => {
            return Ok(vec![
                "cargo xtask check xtest-selection [--self-test]".into(),
            ]);
        }
        _ => return Err("xtest-selection accepts only --self-test".into()),
    };
    build(repo, EXAMPLE)?;
    let output = evidence_directory(repo, "xtest-selection", self_test)?;
    let mut lines = vec![format!("evidence: {}", output.display())];
    if self_test {
        for variant in [Variant::BlankRow, Variant::NoAdmission, Variant::NoPaste] {
            match run_variant(repo, &output, variant)? {
                Ok(summary) => {
                    return Err(format!(
                        "mutation {} passed and must not: {summary}",
                        variant.name()
                    ));
                }
                Err(reason) => lines.push(format!(
                    "mutation {}: failed as required ({reason})",
                    variant.name()
                )),
            }
        }
        lines.push("xtest-selection self-test: every mutation failed".into());
    } else {
        match run_variant(repo, &output, Variant::Pass)? {
            Ok(summary) => lines.push(format!("xtest-selection: pass ({summary})")),
            Err(reason) => return Err(format!("xtest-selection failed: {reason}")),
        }
    }
    Ok(lines)
}

/// Ok(summary) when the run met every obligation; Err(reason) when it did
/// not. The outer Result is for the harness itself failing to run.
fn run_variant(
    repo: &Path,
    output: &Path,
    variant: Variant,
) -> Result<Result<String, String>, String> {
    let client_args: Vec<String> = match variant {
        Variant::BlankRow => vec!["--row=5".into()],
        Variant::NoPaste => vec!["--no-paste".into()],
        Variant::Pass | Variant::NoAdmission => Vec::new(),
    };
    let log_path = output.join(format!("{}.log", variant.name()));
    let session = run_client_session(
        repo,
        &ClientRun {
            example: EXAMPLE,
            pass_line: PASS_LINE,
            client_args: &client_args,
            admit_xtest: variant != Variant::NoAdmission,
            log_path: &log_path,
        },
    )?;
    Ok(judge(&session))
}

/// Every obligation, or the first one missed. An absent record is a failure,
/// never a pass.
fn judge(session: &ClientSession) -> Result<String, String> {
    require_completed(session, EXAMPLE)?;
    let text = &session.text;
    let selection =
        record(text, "sophia_live_selection").ok_or("no sophia_live_selection record")?;
    let owner_changes =
        number(&selection, "owner_changes").ok_or("selection record without owner_changes")?;
    let conversions =
        number(&selection, "conversions").ok_or("selection record without conversions")?;
    if owner_changes < 1 {
        return Err(format!(
            "owner_changes={owner_changes}: xterm never took PRIMARY"
        ));
    }
    // One is the driver reading the selection back; the second is xterm B
    // asking for it on the middle-click.
    if conversions < 2 {
        return Err(format!(
            "conversions={conversions}: the paste never asked for PRIMARY"
        ));
    }
    let xtest = admitted_xtest_record(text)?;
    let buttons = number(&xtest, "injected_buttons").unwrap_or(0);
    if buttons < 4 {
        return Err(format!(
            "injected_buttons={buttons}: the drag and the paste take four"
        ));
    }
    Ok(format!(
        "owner_changes={owner_changes} conversions={conversions} injected_buttons={buttons}"
    ))
}

#[cfg(test)]
mod tests {
    use super::judge;
    use crate::headless_client_gate::ClientSession;

    const SELECTION: &str = "sophia_live_selection schema=1 status=complete content=redacted owner_changes=1 conversions=2";
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
}
