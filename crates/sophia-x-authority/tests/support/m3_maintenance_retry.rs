use super::*;

fn report(phase: PrivateMaintenancePhase, charged: bool, delay: Option<Duration>) -> Maintained {
    Maintained {
        phase,
        status: PrivateMaintenanceStatus::Yielded,
        allowance_refusal: None,
        output_refusal: None,
        terminal_visit: None,
        terminal_refusal: None,
        supervision_ok: charged,
        settled: None,
        charged,
        budget_retry_after: delay,
        modifiers: Some(1),
        instance: 17,
        detail: String::new(),
    }
}

#[test]
fn budget_retry_waits_for_output_after_the_intervening_terminal_visit() {
    use PrivateMaintenancePhase::{Output, Terminal};
    let delay = Duration::from_millis(1);
    let mut steps = [
        report(Output, false, Some(delay)),
        report(Terminal, true, None),
        report(Output, true, None),
    ]
    .into_iter();
    let mut waits = Vec::new();
    let output = charged_phase(Output, || steps.next().unwrap(), |delay| waits.push(delay));
    assert_eq!(output.phase, Output);
    assert!(output.charged);
    assert_eq!(output.instance, 17);
    assert_eq!(output.modifiers, Some(1));
    assert!(steps.next().is_none());
    assert_eq!(waits, [delay, Duration::ZERO]);
}

#[test]
fn a_non_budget_refusal_is_returned_to_the_asserting_caller() {
    let output = charged_phase(
        PrivateMaintenancePhase::Output,
        || report(PrivateMaintenancePhase::Output, false, None),
        |_| panic!("a non-budget refusal must not be retried"),
    );
    assert!(!output.charged);
}

#[test]
#[should_panic(expected = "exceeded the visit bound")]
fn repeated_yields_cannot_spin_forever() {
    charged_phase(
        PrivateMaintenancePhase::Output,
        || report(PrivateMaintenancePhase::Output, false, Some(Duration::ZERO)),
        |_| {},
    );
}
