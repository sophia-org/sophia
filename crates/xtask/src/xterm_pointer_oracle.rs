//! `cargo xtask check xterm-pointer-oracle`: xterm reports what pointer
//! events its text widget received, and the frontend is judged on them.
//!
//! WHAT IT PROVES. With SGR any-event mouse tracking on, a real xterm writes
//! an escape sequence for every motion, press, drag and release its widget
//! receives, naming the button and the cell. The driver
//! (`examples/xterm_pointer_oracle.rs`) injects XTEST events at chosen cells
//! and reads those reports back, so the verdict is xterm's own account of
//! what the frontend delivered: on which window, at which position, with
//! which button state. It would have caught t155, t156, t162 and t158.
//!
//! WHAT IT DOES NOT. It runs headless with no window manager and no physical
//! input, and it reads cells, not pixels: it cannot say the highlight was
//! drawn, only that xterm was told what it needed to draw it.
//!
//! `--self-test` runs two mutations that must fail: a session that does not
//! admit XTEST, and an xterm with tracking left off so nothing is ever
//! reported. The mutation that matters most -- the frontend delivering drag
//! motion to the shell instead of the widget -- is a source change, made
//! once by hand when the gate was written (t162's mask reverted), and its
//! red is recorded in t163's note.

use std::path::Path;

use crate::headless_client_gate::{
    ClientRun, ClientSession, admitted_xtest_record, build, evidence_directory, number,
    require_completed, run_client_session,
};

const EXAMPLE: &str = "xterm_pointer_oracle";

/// The driver's stdout on a full pass, compared byte for byte by the session.
const PASS_LINE: &str = "sophia_xterm_pointer_oracle schema=1 status=pass motion=true press=true drag=true release=true buttons=3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Variant {
    Pass,
    NoAdmission,
    NoTracking,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::NoAdmission => "no-admission",
            Self::NoTracking => "no-tracking",
        }
    }
}

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let self_test = match arguments {
        [] => false,
        [flag] if flag == "--self-test" => true,
        [help] if help == "--help" => {
            return Ok(vec![
                "cargo xtask check xterm-pointer-oracle [--self-test]".into(),
            ]);
        }
        _ => return Err("xterm-pointer-oracle accepts only --self-test".into()),
    };
    build(repo, EXAMPLE)?;
    let output = evidence_directory(repo, "xterm-pointer-oracle", self_test)?;
    let mut lines = vec![format!("evidence: {}", output.display())];
    if self_test {
        for variant in [Variant::NoAdmission, Variant::NoTracking] {
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
        lines.push("xterm-pointer-oracle self-test: every mutation failed".into());
    } else {
        match run_variant(repo, &output, Variant::Pass)? {
            Ok(summary) => lines.push(format!("xterm-pointer-oracle: pass ({summary})")),
            Err(reason) => return Err(format!("xterm-pointer-oracle failed: {reason}")),
        }
    }
    Ok(lines)
}

fn run_variant(
    repo: &Path,
    output: &Path,
    variant: Variant,
) -> Result<Result<String, String>, String> {
    let client_args: Vec<String> = match variant {
        Variant::NoTracking => vec!["--no-tracking".into()],
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

/// The session completed with the driver's verdict matched, and the XTEST
/// record accounts for what the driver injected: the settle motion, the
/// first motion, three drag motions and one more after the release are six
/// motions at least; button 1 down and up and buttons 2 and 3 down and up
/// are six buttons.
fn judge(session: &ClientSession) -> Result<String, String> {
    require_completed(session, EXAMPLE)?;
    let xtest = admitted_xtest_record(&session.text)?;
    let motions = number(&xtest, "injected_motions").unwrap_or(0);
    let buttons = number(&xtest, "injected_buttons").unwrap_or(0);
    if motions < 6 {
        return Err(format!(
            "injected_motions={motions}: the oracle moves at least six times"
        ));
    }
    if buttons < 6 {
        return Err(format!(
            "injected_buttons={buttons}: three buttons down and up are six"
        ));
    }
    Ok(format!(
        "injected_motions={motions} injected_buttons={buttons}"
    ))
}

mod tests;
