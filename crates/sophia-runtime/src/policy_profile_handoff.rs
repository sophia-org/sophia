use std::collections::BTreeSet;

use sophia_protocol::{
    PolicyProfileCommand, PolicyProfileCompletion, PolicyProfileIdentity, PolicyProfileOutcome,
    TransactionId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProfileHandoffKind {
    Prepare,
    Activate,
    Rollback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProfileHandoffPhase {
    Ready,
    AwaitingPrepared,
    Prepared,
    AwaitingActive,
    Active,
    AwaitingRollback,
    RolledBack,
    Rejected,
    RollbackFailed,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfileOutstanding {
    pub kind: PolicyProfileHandoffKind,
    pub command: PolicyProfileCommand,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyProfileHandoffModel {
    identity: PolicyProfileIdentity,
    phase: PolicyProfileHandoffPhase,
    outstanding: Option<PolicyProfileOutstanding>,
    used_transactions: BTreeSet<TransactionId>,
}

impl PolicyProfileHandoffModel {
    pub fn new(identity: PolicyProfileIdentity) -> Self {
        Self {
            identity,
            phase: PolicyProfileHandoffPhase::Ready,
            outstanding: None,
            used_transactions: BTreeSet::new(),
        }
    }

    pub const fn identity(&self) -> PolicyProfileIdentity {
        self.identity
    }

    pub const fn phase(&self) -> PolicyProfileHandoffPhase {
        self.phase
    }

    pub const fn outstanding(&self) -> Option<PolicyProfileOutstanding> {
        self.outstanding
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProfileHandoffMsg {
    Begin {
        kind: PolicyProfileHandoffKind,
        transaction: TransactionId,
    },
    Completion {
        kind: PolicyProfileHandoffKind,
        completion: PolicyProfileCompletion,
    },
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyProfileHandoffEffect {
    pub kind: PolicyProfileHandoffKind,
    pub command: PolicyProfileCommand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProfileCompletionDisposition {
    Accepted,
    Rejected(PolicyProfileOutcome),
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyProfileHandoffUpdate {
    pub model: PolicyProfileHandoffModel,
    pub effect: Option<PolicyProfileHandoffEffect>,
    pub completion: Option<PolicyProfileCompletionDisposition>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyProfileHandoffError {
    InvalidTransaction,
    ReusedTransaction,
    InvalidPhase,
}

impl core::fmt::Display for PolicyProfileHandoffError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyProfileHandoffError {}

pub fn reduce_policy_profile_handoff(
    model: &PolicyProfileHandoffModel,
    message: PolicyProfileHandoffMsg,
) -> Result<PolicyProfileHandoffUpdate, PolicyProfileHandoffError> {
    let mut update = PolicyProfileHandoffUpdate {
        model: model.clone(),
        effect: None,
        completion: None,
    };
    match message {
        PolicyProfileHandoffMsg::Begin { kind, transaction } => {
            begin(&mut update, kind, transaction)?;
        }
        PolicyProfileHandoffMsg::Completion { kind, completion } => {
            settle(&mut update, kind, completion);
        }
        PolicyProfileHandoffMsg::Disconnected => {
            update.model.phase = PolicyProfileHandoffPhase::Disconnected;
            update.model.outstanding = None;
        }
    }
    Ok(update)
}

fn begin(
    update: &mut PolicyProfileHandoffUpdate,
    kind: PolicyProfileHandoffKind,
    transaction: TransactionId,
) -> Result<(), PolicyProfileHandoffError> {
    if !transaction.is_valid() {
        return Err(PolicyProfileHandoffError::InvalidTransaction);
    }
    if update.model.used_transactions.contains(&transaction) {
        return Err(PolicyProfileHandoffError::ReusedTransaction);
    }
    let next_phase = match kind {
        PolicyProfileHandoffKind::Prepare
            if update.model.phase == PolicyProfileHandoffPhase::Ready =>
        {
            PolicyProfileHandoffPhase::AwaitingPrepared
        }
        PolicyProfileHandoffKind::Activate
            if update.model.phase == PolicyProfileHandoffPhase::Prepared =>
        {
            PolicyProfileHandoffPhase::AwaitingActive
        }
        PolicyProfileHandoffKind::Rollback
            if matches!(
                update.model.phase,
                PolicyProfileHandoffPhase::Ready
                    | PolicyProfileHandoffPhase::AwaitingPrepared
                    | PolicyProfileHandoffPhase::Prepared
                    | PolicyProfileHandoffPhase::AwaitingActive
                    | PolicyProfileHandoffPhase::Active
                    | PolicyProfileHandoffPhase::Rejected
            ) =>
        {
            PolicyProfileHandoffPhase::AwaitingRollback
        }
        _ => return Err(PolicyProfileHandoffError::InvalidPhase),
    };
    let command = PolicyProfileCommand {
        transaction,
        identity: update.model.identity,
    };
    update.model.used_transactions.insert(transaction);
    update.model.phase = next_phase;
    update.model.outstanding = Some(PolicyProfileOutstanding { kind, command });
    update.effect = Some(PolicyProfileHandoffEffect { kind, command });
    Ok(())
}

fn settle(
    update: &mut PolicyProfileHandoffUpdate,
    kind: PolicyProfileHandoffKind,
    completion: PolicyProfileCompletion,
) {
    let Some(outstanding) = update.model.outstanding else {
        update.completion = Some(PolicyProfileCompletionDisposition::Stale);
        return;
    };
    let expected_phase = match kind {
        PolicyProfileHandoffKind::Prepare => PolicyProfileHandoffPhase::AwaitingPrepared,
        PolicyProfileHandoffKind::Activate => PolicyProfileHandoffPhase::AwaitingActive,
        PolicyProfileHandoffKind::Rollback => PolicyProfileHandoffPhase::AwaitingRollback,
    };
    if update.model.phase != expected_phase
        || outstanding.kind != kind
        || outstanding.command.transaction != completion.transaction
        || outstanding.command.identity != completion.identity
    {
        update.completion = Some(PolicyProfileCompletionDisposition::Stale);
        return;
    }

    update.model.outstanding = None;
    if completion.outcome == PolicyProfileOutcome::Accepted {
        update.model.phase = match kind {
            PolicyProfileHandoffKind::Prepare => PolicyProfileHandoffPhase::Prepared,
            PolicyProfileHandoffKind::Activate => PolicyProfileHandoffPhase::Active,
            PolicyProfileHandoffKind::Rollback => PolicyProfileHandoffPhase::RolledBack,
        };
        update.completion = Some(PolicyProfileCompletionDisposition::Accepted);
    } else {
        update.model.phase = match kind {
            PolicyProfileHandoffKind::Rollback => PolicyProfileHandoffPhase::RollbackFailed,
            PolicyProfileHandoffKind::Prepare | PolicyProfileHandoffKind::Activate => {
                PolicyProfileHandoffPhase::Rejected
            }
        };
        update.completion = Some(PolicyProfileCompletionDisposition::Rejected(
            completion.outcome,
        ));
    }
}
