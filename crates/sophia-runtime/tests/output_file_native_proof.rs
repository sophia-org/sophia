//! Scripted stages of the generic native proof peer against the real output
//! file service and a test owner, without devices. Each test is ignored:
//! run with SOPHIA_OUTPUT_NATIVE_PROOF_PEER naming the peer built from the
//! pinned public C SDK. The native gate runs the same peer under Session.
#[path = "support/output_files_native_proof/mod.rs"]
mod proof;

use proof::{A_EPOCH, Input, argv, layout_a, layout_b, run, run_offline};
use sophia_protocol::*;
use sophia_runtime::OutputTransportServiceEvent as Event;

fn delivered(input: &Input<'_>) -> Option<(TransactionId, OutputTopologyCandidate)> {
    match input {
        Input::Event(Event::Proposal { proposal, .. }) => {
            Some((proposal.transaction, proposal.message.candidate.clone()))
        }
        _ => None,
    }
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn validate_stage_receives_exactly_one_validated_outcome() {
    let b = layout_b();
    let expected = b.candidate(A_EPOCH, OutputTopologyIntent::ValidateOnly);
    let run = run(
        "validate",
        "validate",
        &argv("validate", None, A_EPOCH, Some(&b)),
        layout_a().snapshot(A_EPOCH),
        |owner, input| {
            if let Some((transaction, candidate)) = delivered(&input) {
                if candidate != expected {
                    return Err(format!("unexpected candidate: {candidate:?}"));
                }
                owner.settle(transaction, A_EPOCH, OutputV1OutcomeKind::Validated)?;
            }
            Ok(())
        },
    );
    assert_eq!(run.code(), Some(0), "{run:#?}");
    assert_eq!(run.deliveries.len(), 1, "{run:#?}");
    assert_eq!(run.only("ready").get("topology_epoch"), Some("7"));
    assert_eq!(run.only("outcome").get("kind"), Some("validated"));
    assert_eq!(run.only("submitted").event, "submitted");
    assert_eq!(run.disconnected, 1);
}

/// Limit of the supplied owner: the real transport's admission already
/// refuses a mode absent from the head's published table
/// (`OutputTopologyCandidate::validate_against`, reason invariant), so the
/// owner never sees this candidate. Session's own owner-side refusal path is
/// not exercised here; this proves the peer's stage and the admission answer.
#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn reject_stage_unknown_mode_is_refused_at_transport_admission() {
    let run = run(
        "reject",
        "reject",
        &argv("reject", None, A_EPOCH, None),
        layout_a().snapshot(A_EPOCH),
        |_, input| match delivered(&input) {
            Some((_, candidate)) => Err(format!("admission delivered: {candidate:?}")),
            None => Ok(()),
        },
    );
    assert_eq!(run.code(), Some(0), "{run:#?}");
    assert!(run.deliveries.is_empty(), "{run:#?}");
    assert_eq!(run.admission_rejections.len(), 1, "{run:#?}");
    let candidate = run.only("reject-candidate");
    assert_eq!(candidate.get("head"), Some("1"));
    assert_eq!(candidate.get("absent_mode"), Some("3"));
    let outcome = run.only("outcome");
    assert_eq!(outcome.get("kind"), Some("rejected"));
    assert_eq!(outcome.get("reason"), Some("7"));
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn commit_restore_stage_commits_b_then_restores_a() {
    let (a, b) = (layout_a(), layout_b());
    let expected = [
        (
            b.candidate(A_EPOCH, OutputTopologyIntent::Apply),
            b.snapshot(A_EPOCH + 1),
        ),
        (
            a.candidate(A_EPOCH + 1, OutputTopologyIntent::Apply),
            a.snapshot(A_EPOCH + 2),
        ),
    ];
    let mut next = 0;
    let run = run(
        "commit-restore",
        "commit-restore",
        &argv("commit-restore", None, A_EPOCH, Some(&b)),
        a.snapshot(A_EPOCH),
        |owner, input| {
            if let Some((transaction, candidate)) = delivered(&input) {
                let (want, published) = expected
                    .get(next)
                    .ok_or("more than two deliveries")?
                    .clone();
                if candidate != want {
                    return Err(format!("delivery {next}: {candidate:?}"));
                }
                next += 1;
                // Session's order: the outcome, then the committed snapshot.
                owner.settle(
                    transaction,
                    published.topology_epoch,
                    OutputV1OutcomeKind::Committed,
                )?;
                owner.publish(published)?;
            }
            Ok(())
        },
    );
    assert_eq!(run.code(), Some(0), "{run:#?}");
    assert_eq!(run.deliveries.len(), 2, "{run:#?}");
    assert_eq!(
        run.published(),
        [("7", "a"), ("8", "b"), ("9", "a")],
        "{run:#?}"
    );
    let outcomes = run
        .lines
        .iter()
        .filter(|line| line.event == "outcome")
        .map(|line| line.get("kind").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes, ["committed", "committed"], "{run:#?}");
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn apply_await_termination_stage_is_ended_by_its_supervisor() {
    let b = layout_b();
    let expected = b.candidate(A_EPOCH, OutputTopologyIntent::Apply);
    let run = run(
        "await-termination",
        "apply-await-termination",
        &argv("apply-await-termination", None, A_EPOCH, Some(&b)),
        layout_a().snapshot(A_EPOCH),
        |owner, input| {
            if let Some((_, candidate)) = delivered(&input) {
                if candidate != expected {
                    return Err(format!("unexpected candidate: {candidate:?}"));
                }
                // Unanswered and still connected, as at native apply.
                owner.terminate_peer()?;
            }
            Ok(())
        },
    );
    assert_eq!(run.deliveries.len(), 1, "{run:#?}");
    assert_eq!(run.disconnected, 1, "{run:#?}");
    assert_eq!(
        run.only("awaiting-termination").event,
        "awaiting-termination"
    );
    assert!(!run.events().contains(&"outcome"), "{run:#?}");
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn baseline_waits_below_the_declared_epoch_then_proceeds() {
    let b = layout_b();
    let expected = b.candidate(A_EPOCH + 1, OutputTopologyIntent::ValidateOnly);
    let mut published = false;
    let run = run(
        "baseline-wait",
        "validate",
        &argv("validate", None, A_EPOCH + 1, Some(&b)),
        layout_a().snapshot(A_EPOCH),
        |owner, input| {
            match input {
                Input::Line(line)
                    if line.event == "topology" && line.get("topology_epoch") == Some("7") =>
                {
                    if published {
                        return Err("epoch 7 seen twice".into());
                    }
                    published = true;
                    owner.publish(layout_a().snapshot(A_EPOCH + 1))?;
                }
                input => {
                    if let Some((transaction, candidate)) = delivered(&input) {
                        if candidate != expected {
                            return Err(format!("unexpected candidate: {candidate:?}"));
                        }
                        owner.settle(transaction, A_EPOCH + 1, OutputV1OutcomeKind::Validated)?;
                    }
                }
            }
            Ok(())
        },
    );
    assert_eq!(run.code(), Some(0), "{run:#?}");
    assert_eq!(run.only("ready").get("topology_epoch"), Some("8"));
    assert_eq!(run.deliveries.len(), 1);
}

fn no_delivery(_: &mut proof::Owner<'_>, input: Input<'_>) -> Result<(), String> {
    match delivered(&input) {
        Some((_, candidate)) => Err(format!("unexpected delivery: {candidate:?}")),
        None => Ok(()),
    }
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn baseline_failures_exit_three_without_submitting() {
    let b = layout_b();
    // A passed epoch, A's layout at its own epoch but a different topology,
    // and a declared epoch that is never published.
    let cases = [
        (
            "epoch-passed",
            argv("validate", None, A_EPOCH - 1, Some(&b)),
            layout_a().snapshot(A_EPOCH),
            "baseline-epoch-passed",
        ),
        (
            "mismatch",
            argv("validate", None, A_EPOCH, Some(&b)),
            b.snapshot(A_EPOCH),
            "baseline-mismatch",
        ),
        (
            "deadline",
            argv("validate", Some(500), A_EPOCH + 1, Some(&b)),
            layout_a().snapshot(A_EPOCH),
            "baseline-deadline",
        ),
    ];
    for (label, args, snapshot, reason) in cases {
        let run = run(label, "validate", &args, snapshot, no_delivery);
        assert_eq!(run.code(), Some(3), "{label}: {run:#?}");
        assert_eq!(run.fail_reason(), Some(reason), "{label}: {run:#?}");
        assert!(!run.events().contains(&"submit"), "{label}: {run:#?}");
    }
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn outcome_before_termination_exits_five() {
    let b = layout_b();
    let run = run(
        "early-outcome",
        "apply-await-termination",
        &argv("apply-await-termination", None, A_EPOCH, Some(&b)),
        layout_a().snapshot(A_EPOCH),
        |owner, input| {
            if let Some((transaction, _)) = delivered(&input) {
                owner.settle(transaction, A_EPOCH + 1, OutputV1OutcomeKind::Committed)?;
            }
            Ok(())
        },
    );
    assert_eq!(run.code(), Some(5), "{run:#?}");
    assert_eq!(run.only("outcome").get("kind"), Some("committed"));
    assert_eq!(run.fail_reason(), Some("outcome-before-termination"));
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn unterminated_peer_exits_six_at_its_deadline() {
    let b = layout_b();
    let run = run(
        "not-terminated",
        "apply-await-termination",
        &argv("apply-await-termination", Some(500), A_EPOCH, Some(&b)),
        layout_a().snapshot(A_EPOCH),
        |_, _| Ok(()),
    );
    assert_eq!(run.code(), Some(6), "{run:#?}");
    assert_eq!(run.deliveries.len(), 1, "{run:#?}");
    assert_eq!(run.fail_reason(), Some("not-terminated"));
}

#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn refused_arguments_exit_two_before_connecting() {
    let a = layout_a().args("a");
    let b = layout_b().args("b");
    let base = |stage: &str| {
        let mut args = vec![
            format!("--stage={stage}"),
            "--a-topology-epoch=7".to_owned(),
        ];
        args.extend(a.clone());
        args
    };
    let with = |stage: &str, extra: &[&str]| {
        let mut args = base(stage);
        args.extend(extra.iter().map(|s| s.to_string()));
        args
    };
    let mut cases = vec![
        ("b equals a", {
            let mut args = base("validate");
            args.extend(layout_a().args("b"));
            args
        }),
        ("reject with b", {
            let mut args = base("reject");
            args.extend(b.clone());
            args
        }),
        ("validate without b", base("validate")),
        (
            "partial b",
            with("validate", &[b[0].as_str(), b[1].as_str()]),
        ),
        ("unknown stage", with("configure", &[])),
        ("zero epoch", {
            let mut args = base("reject");
            args[1] = "--a-topology-epoch=0".into();
            args
        }),
        ("leading zero", {
            let mut args = base("reject");
            args[1] = "--a-topology-epoch=07".into();
            args
        }),
        ("deadline zero", with("reject", &["--deadline-ms=0"])),
        (
            "deadline over cap",
            with("reject", &["--deadline-ms=120001"]),
        ),
        ("repeated", with("reject", &["--a-topology-epoch=7"])),
        (
            "repeated deadline",
            with("reject", &["--deadline-ms=1000", "--deadline-ms=2000"]),
        ),
        (
            "unknown flag",
            with("reject", &["--c-heads=1:1:normal:disabled"]),
        ),
    ];
    let mut replace = |label: &'static str, index: usize, value: &str| {
        let mut args = base("reject");
        args[index] = value.to_owned();
        cases.push((label, args));
    };
    replace(
        "bad transform",
        2,
        "--a-heads=1:1:45:disabled,2:1:normal:disabled",
    );
    replace("bad vrr", 2, "--a-heads=1:1:normal:on,2:1:normal:disabled");
    replace(
        "duplicate head",
        2,
        "--a-heads=1:1:normal:disabled,1:2:normal:disabled",
    );
    replace(
        "new output",
        3,
        "--a-groups=0@0,0,800x600=1/exact;2@800,0,800x600=2/exact",
    );
    replace(
        "bad mapping",
        3,
        "--a-groups=1@0,0,800x600=1/stretch;2@800,0,800x600=2/exact",
    );
    replace("ungrouped head", 3, "--a-groups=1@0,0,800x600=1/exact");
    replace(
        "five members",
        3,
        "--a-groups=1@0,0,800x600=1/fit+2/fit+3/fit+4/fit+5/fit",
    );
    replace("primary out of range", 4, "--a-primary=2");
    replace("primary leading zero", 4, "--a-primary=00");
    replace(
        "seventeen heads",
        2,
        &format!(
            "--a-heads={}",
            (1..=17)
                .map(|h| format!("{h}:1:normal:disabled"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    cases.push(("ten arguments", {
        let mut args = argv("validate", Some(1000), A_EPOCH, Some(&layout_b()));
        args.push("--stage=validate".into());
        args
    }));
    cases.push(("no arguments", Vec::new()));
    for (label, args) in cases {
        let run = run_offline(&args);
        assert_eq!(run.code(), Some(2), "{label}: {run:#?}");
        assert_eq!(run.fail_reason(), Some("usage"), "{label}: {run:#?}");
        assert!(!run.stderr.is_empty(), "{label}: {run:#?}");
    }
}

/// An outcome is checked after it is recorded: Validated must carry the
/// proposal's base epoch and Committed the next epoch.
#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn outcome_with_the_wrong_topology_epoch_exits_four() {
    let b = layout_b();
    for (label, stage, kind, epoch) in [
        (
            "validated-epoch",
            "validate",
            OutputV1OutcomeKind::Validated,
            A_EPOCH + 1,
        ),
        (
            "committed-epoch",
            "commit-restore",
            OutputV1OutcomeKind::Committed,
            A_EPOCH,
        ),
    ] {
        let run = run(
            label,
            stage,
            &argv(stage, None, A_EPOCH, Some(&b)),
            layout_a().snapshot(A_EPOCH),
            |owner, input| {
                if let Some((transaction, _)) = delivered(&input) {
                    owner.settle(transaction, epoch, kind)?;
                }
                Ok(())
            },
        );
        assert_eq!(run.code(), Some(4), "{label}: {run:#?}");
        assert_eq!(
            run.fail_reason(),
            Some("outcome-topology-epoch"),
            "{label}: {run:#?}"
        );
        let recorded = epoch.to_string();
        assert_eq!(
            run.only("outcome").get("topology_epoch"),
            Some(recorded.as_str())
        );
        assert!(
            run.published().iter().all(|(epoch, _)| *epoch == "7"),
            "{label}: {run:#?}"
        );
    }
}

/// Control for the scripted source: without reuse it carries the whole
/// commit-restore stage exactly as the real service does.
#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn scripted_source_without_reuse_carries_commit_restore() {
    let run = proof::scripted::commit_restore("control", false);
    assert_eq!(run.code(), Some(0), "{run:#?}");
    assert_eq!(run.deliveries.len(), 2, "{run:#?}");
    assert_eq!(
        run.published(),
        [("7", "a"), ("8", "b"), ("9", "a")],
        "{run:#?}"
    );
}

/// The real export never reuses a Qid, so the scripted source announces B's
/// publication under the baseline's Qid. The SDK reads it; the peer must
/// refuse it after recording it.
#[test]
#[ignore = "needs SOPHIA_OUTPUT_NATIVE_PROOF_PEER"]
fn reused_publication_qid_exits_four() {
    let run = proof::scripted::commit_restore("reuse", true);
    assert_eq!(run.code(), Some(4), "{run:#?}");
    assert_eq!(
        run.fail_reason(),
        Some("publication-qid-reused"),
        "{run:#?}"
    );
    assert_eq!(run.published(), [("7", "a"), ("8", "b")], "{run:#?}");
    let qids = run
        .lines
        .iter()
        .filter(|line| line.event == "topology")
        .map(|line| line.get("qid").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(qids[0], qids[1], "{run:#?}");
    assert_eq!(run.deliveries.len(), 1, "{run:#?}");
}
