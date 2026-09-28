use sophia_protocol::{
    POLICY_PROFILE_DIGEST_BYTES, PolicyProfileCompletion, PolicyProfileIdentity,
    PolicyProfileOutcome, TransactionId,
};
use sophia_runtime::{
    PolicyProfileCompletionDisposition, PolicyProfileHandoffError, PolicyProfileHandoffKind,
    PolicyProfileHandoffModel, PolicyProfileHandoffMsg, PolicyProfileHandoffPhase,
    reduce_policy_profile_handoff,
};

fn profile_identity(epoch: u64, generation: u64, digest: u8) -> PolicyProfileIdentity {
    PolicyProfileIdentity::new(epoch, generation, [digest; POLICY_PROFILE_DIGEST_BYTES]).unwrap()
}

fn transaction(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

fn completion(
    transaction: u64,
    identity: PolicyProfileIdentity,
    outcome: PolicyProfileOutcome,
) -> PolicyProfileCompletion {
    PolicyProfileCompletion {
        transaction: TransactionId::from_raw(transaction),
        identity,
        outcome,
    }
}

fn begin(
    model: &PolicyProfileHandoffModel,
    kind: PolicyProfileHandoffKind,
    transaction: u64,
) -> PolicyProfileHandoffModel {
    reduce_policy_profile_handoff(
        model,
        PolicyProfileHandoffMsg::Begin {
            kind,
            transaction: TransactionId::from_raw(transaction),
        },
    )
    .unwrap()
    .model
}

fn settle(
    model: &PolicyProfileHandoffModel,
    kind: PolicyProfileHandoffKind,
    completion: PolicyProfileCompletion,
) -> (
    PolicyProfileHandoffModel,
    PolicyProfileCompletionDisposition,
) {
    let update = reduce_policy_profile_handoff(
        model,
        PolicyProfileHandoffMsg::Completion { kind, completion },
    )
    .unwrap();
    (update.model, update.completion.unwrap())
}

#[test]
fn exact_prepare_and_activate_completions_advance_the_candidate() {
    let identity = profile_identity(9, 7, 0x5a);
    let ready = PolicyProfileHandoffModel::new(identity);

    let prepare = reduce_policy_profile_handoff(
        &ready,
        PolicyProfileHandoffMsg::Begin {
            kind: PolicyProfileHandoffKind::Prepare,
            transaction: transaction(1),
        },
    )
    .unwrap();
    assert_eq!(
        prepare.model.phase(),
        PolicyProfileHandoffPhase::AwaitingPrepared
    );
    assert_eq!(prepare.effect.unwrap().command.identity, identity);

    let (prepared, disposition) = settle(
        &prepare.model,
        PolicyProfileHandoffKind::Prepare,
        completion(1, identity, PolicyProfileOutcome::Accepted),
    );
    assert_eq!(disposition, PolicyProfileCompletionDisposition::Accepted);
    assert_eq!(prepared.phase(), PolicyProfileHandoffPhase::Prepared);

    let activating = begin(&prepared, PolicyProfileHandoffKind::Activate, 2);
    let (active, disposition) = settle(
        &activating,
        PolicyProfileHandoffKind::Activate,
        completion(2, identity, PolicyProfileOutcome::Accepted),
    );
    assert_eq!(disposition, PolicyProfileCompletionDisposition::Accepted);
    assert_eq!(active.phase(), PolicyProfileHandoffPhase::Active);
}

#[test]
fn stale_completion_is_inert_across_epoch_transaction_identity_and_phase() {
    let identity = profile_identity(9, 7, 0x5a);
    let awaiting = begin(
        &PolicyProfileHandoffModel::new(identity),
        PolicyProfileHandoffKind::Prepare,
        1,
    );
    for stale in [
        completion(
            1,
            profile_identity(8, 7, 0x5a),
            PolicyProfileOutcome::Accepted,
        ),
        completion(2, identity, PolicyProfileOutcome::Accepted),
        completion(
            1,
            profile_identity(9, 8, 0x5a),
            PolicyProfileOutcome::Accepted,
        ),
    ] {
        let (unchanged, disposition) = settle(&awaiting, PolicyProfileHandoffKind::Prepare, stale);
        assert_eq!(disposition, PolicyProfileCompletionDisposition::Stale);
        assert_eq!(unchanged, awaiting);
    }
    let (unchanged, disposition) = settle(
        &awaiting,
        PolicyProfileHandoffKind::Activate,
        completion(1, identity, PolicyProfileOutcome::Accepted),
    );
    assert_eq!(disposition, PolicyProfileCompletionDisposition::Stale);
    assert_eq!(unchanged, awaiting);
}

#[test]
fn rejection_requires_explicit_rollback_and_never_promotes() {
    let identity = profile_identity(3, 11, 0xa5);
    let awaiting = begin(
        &PolicyProfileHandoffModel::new(identity),
        PolicyProfileHandoffKind::Prepare,
        4,
    );
    let (rejected, disposition) = settle(
        &awaiting,
        PolicyProfileHandoffKind::Prepare,
        completion(4, identity, PolicyProfileOutcome::RejectedIdentity),
    );
    assert_eq!(
        disposition,
        PolicyProfileCompletionDisposition::Rejected(PolicyProfileOutcome::RejectedIdentity)
    );
    assert_eq!(rejected.phase(), PolicyProfileHandoffPhase::Rejected);

    let rolling_back = begin(&rejected, PolicyProfileHandoffKind::Rollback, 5);
    let (rolled_back, disposition) = settle(
        &rolling_back,
        PolicyProfileHandoffKind::Rollback,
        completion(5, identity, PolicyProfileOutcome::Accepted),
    );
    assert_eq!(disposition, PolicyProfileCompletionDisposition::Accepted);
    assert_eq!(rolled_back.phase(), PolicyProfileHandoffPhase::RolledBack);
}

#[test]
fn transaction_reuse_and_out_of_phase_commands_fail_closed() {
    let model = PolicyProfileHandoffModel::new(profile_identity(1, 1, 1));
    assert_eq!(
        reduce_policy_profile_handoff(
            &model,
            PolicyProfileHandoffMsg::Begin {
                kind: PolicyProfileHandoffKind::Prepare,
                transaction: TransactionId::INVALID,
            },
        ),
        Err(PolicyProfileHandoffError::InvalidTransaction)
    );
    assert_eq!(
        reduce_policy_profile_handoff(
            &model,
            PolicyProfileHandoffMsg::Begin {
                kind: PolicyProfileHandoffKind::Activate,
                transaction: transaction(1),
            },
        ),
        Err(PolicyProfileHandoffError::InvalidPhase)
    );
    let awaiting = begin(&model, PolicyProfileHandoffKind::Prepare, 1);
    assert_eq!(
        reduce_policy_profile_handoff(
            &awaiting,
            PolicyProfileHandoffMsg::Begin {
                kind: PolicyProfileHandoffKind::Rollback,
                transaction: transaction(1),
            },
        ),
        Err(PolicyProfileHandoffError::ReusedTransaction)
    );
}

#[test]
fn disconnect_discards_the_outstanding_operation_and_old_ack_is_stale() {
    let identity = profile_identity(14, 22, 0xcc);
    let awaiting = begin(
        &PolicyProfileHandoffModel::new(identity),
        PolicyProfileHandoffKind::Prepare,
        6,
    );
    let disconnected =
        reduce_policy_profile_handoff(&awaiting, PolicyProfileHandoffMsg::Disconnected)
            .unwrap()
            .model;
    assert_eq!(
        disconnected.phase(),
        PolicyProfileHandoffPhase::Disconnected
    );
    assert_eq!(disconnected.outstanding(), None);

    let (unchanged, disposition) = settle(
        &disconnected,
        PolicyProfileHandoffKind::Prepare,
        completion(6, identity, PolicyProfileOutcome::Accepted),
    );
    assert_eq!(disposition, PolicyProfileCompletionDisposition::Stale);
    assert_eq!(unchanged, disconnected);
}
