//! Scripted bounded I/O around the real reducer/executor. Socket behavior is
//! covered independently by policy_transport's existing real IPC controls.
#![cfg(target_os = "linux")]
use sophia_protocol::{
    PolicyProfileCompletion, PolicyProfileIdentity, PolicyProfileOutcome, TransactionId,
};
use sophia_runtime::*;
use std::collections::VecDeque;

type Reply =
    Result<Option<(PolicyProfileHandoffKind, PolicyProfileCompletion)>, PolicyTransportError>;
#[derive(Debug, PartialEq)]
enum Trace {
    Send(PolicyProfileHandoffKind, u64),
    Receive,
}
struct Script {
    replies: VecDeque<Reply>,
    trace: Vec<Trace>,
    send_error: Option<PolicyTransportError>,
}
impl PolicyProfileHandoffIo for Script {
    fn send_profile_effect(
        &mut self,
        effect: PolicyProfileHandoffEffect,
    ) -> Result<(), PolicyTransportError> {
        assert_eq!(effect.command.identity, identity());
        self.trace
            .push(Trace::Send(effect.kind, effect.command.transaction.raw()));
        match self.send_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn receive_profile_completion(&mut self) -> Reply {
        self.trace.push(Trace::Receive);
        self.replies.pop_front().expect("no retry or extra receive")
    }
}
fn identity() -> PolicyProfileIdentity {
    PolicyProfileIdentity::new(9, 7, [0x5a; 32]).unwrap()
}
fn reply(kind: PolicyProfileHandoffKind, tx: u64, outcome: PolicyProfileOutcome) -> Reply {
    Ok(Some((
        kind,
        PolicyProfileCompletion {
            transaction: TransactionId::from_raw(tx),
            identity: identity(),
            outcome,
        },
    )))
}
fn script(replies: impl IntoIterator<Item = Reply>) -> Script {
    Script {
        replies: replies.into_iter().collect(),
        trace: Vec::new(),
        send_error: None,
    }
}

#[test]
fn shared_executor_orders_prepare_activate_and_uses_the_same_exact_reducer() {
    use PolicyProfileHandoffKind::{Activate, Prepare, Rollback};
    let mut io = script([
        reply(Prepare, 1, PolicyProfileOutcome::Accepted),
        reply(Activate, 2, PolicyProfileOutcome::Accepted),
        reply(Rollback, 3, PolicyProfileOutcome::Accepted),
    ]);
    let active = activate_policy_profile_handoff(
        &mut io,
        identity(),
        TransactionId::from_raw(1),
        TransactionId::from_raw(2),
    )
    .unwrap();
    assert_eq!(active.phase(), PolicyProfileHandoffPhase::Active);
    assert_eq!(
        io.trace,
        vec![
            Trace::Send(Prepare, 1),
            Trace::Receive,
            Trace::Send(Activate, 2),
            Trace::Receive
        ]
    );
    let rollback =
        execute_policy_profile_handoff_step(&mut io, &active, Rollback, TransactionId::from_raw(3))
            .unwrap();
    assert_eq!(
        rollback.model.phase(),
        PolicyProfileHandoffPhase::RolledBack
    );
    assert_eq!(
        rollback.completion,
        Some(PolicyProfileCompletionDisposition::Accepted)
    );
    assert_eq!(active.phase(), PolicyProfileHandoffPhase::Active); // caller-owned model is unchanged
}

#[test]
fn shared_executor_preserves_failure_order_without_retry_or_automatic_rollback() {
    use PolicyProfileHandoffKind::{Activate, Prepare};
    for (response, expected) in [
        (
            Err(PolicyTransportError::TimedOut),
            PolicyTransportError::TimedOut,
        ),
        (Ok(None), PolicyTransportError::ProfileCompletionOutOfPhase),
        (
            reply(Activate, 1, PolicyProfileOutcome::Accepted),
            PolicyTransportError::ProfileCompletionOutOfPhase,
        ),
        (
            reply(Prepare, 99, PolicyProfileOutcome::Accepted),
            PolicyTransportError::ProfileCompletionStale,
        ),
        (
            reply(Prepare, 1, PolicyProfileOutcome::RejectedIdentity),
            PolicyTransportError::ProfileRejected {
                kind: Prepare,
                outcome: PolicyProfileOutcome::RejectedIdentity,
            },
        ),
    ] {
        let mut io = script([response]);
        assert_eq!(
            activate_policy_profile_handoff(
                &mut io,
                identity(),
                TransactionId::from_raw(1),
                TransactionId::from_raw(2)
            ),
            Err(expected)
        );
        assert_eq!(io.trace, vec![Trace::Send(Prepare, 1), Trace::Receive]);
    }
    let mut io = script([]);
    io.send_error = Some(PolicyTransportError::Io("fixture send failure".into()));
    assert_eq!(
        activate_policy_profile_handoff(
            &mut io,
            identity(),
            TransactionId::from_raw(1),
            TransactionId::from_raw(2)
        ),
        Err(PolicyTransportError::Io("fixture send failure".into()))
    );
    assert_eq!(io.trace, vec![Trace::Send(Prepare, 1)]);
    let mut io = script([]);
    assert_eq!(
        activate_policy_profile_handoff(
            &mut io,
            identity(),
            TransactionId::INVALID,
            TransactionId::from_raw(2)
        ),
        Err(PolicyTransportError::ProfileHandoff(
            PolicyProfileHandoffError::InvalidTransaction
        ))
    );
    assert!(io.trace.is_empty());
}

#[test]
fn rejected_step_returns_candidate_for_explicit_rollback_and_reused_ids_never_send() {
    use PolicyProfileHandoffKind::{Prepare, Rollback};
    let ready = PolicyProfileHandoffModel::new(identity());
    let mut io = script([
        reply(Prepare, 1, PolicyProfileOutcome::RejectedState),
        reply(Rollback, 2, PolicyProfileOutcome::Accepted),
    ]);
    let rejected =
        execute_policy_profile_handoff_step(&mut io, &ready, Prepare, TransactionId::from_raw(1))
            .unwrap();
    assert_eq!(rejected.model.phase(), PolicyProfileHandoffPhase::Rejected);
    assert_eq!(
        rejected.completion,
        Some(PolicyProfileCompletionDisposition::Rejected(
            PolicyProfileOutcome::RejectedState
        ))
    );
    assert_eq!(
        execute_policy_profile_handoff_step(
            &mut io,
            &rejected.model,
            Rollback,
            TransactionId::from_raw(1)
        ),
        Err(PolicyTransportError::ProfileHandoff(
            PolicyProfileHandoffError::ReusedTransaction
        ))
    );
    assert_eq!(io.trace.len(), 2);
    let rolled = execute_policy_profile_handoff_step(
        &mut io,
        &rejected.model,
        Rollback,
        TransactionId::from_raw(2),
    )
    .unwrap();
    assert_eq!(rolled.model.phase(), PolicyProfileHandoffPhase::RolledBack);
    assert_eq!(ready.phase(), PolicyProfileHandoffPhase::Ready);
}
