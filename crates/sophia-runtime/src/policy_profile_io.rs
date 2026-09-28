//! Transport operations around the existing profile reducer. There is no
//! additional phase state here, nor a retry/deadline renewal policy.
use crate::{
    PolicyProfileCompletionDisposition, PolicyProfileHandoffEffect, PolicyProfileHandoffError,
    PolicyProfileHandoffKind, PolicyProfileHandoffModel, PolicyProfileHandoffMsg,
    PolicyProfileHandoffUpdate, reduce_policy_profile_handoff,
};
use sophia_protocol::{
    PolicyProfileCompletion, PolicyProfileIdentity, PolicyProfileOutcome, TransactionId,
};

/// Handoff and bounded I/O failures shared by the role's transports. Wire
/// framing errors remain the responsibility of each adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyProfileIoError {
    Io(String),
    TimedOut,
    ProfileHandoff(PolicyProfileHandoffError),
    ProfileCompletionOutOfPhase,
    ProfileCompletionStale,
    ProfileRejected {
        kind: PolicyProfileHandoffKind,
        outcome: PolicyProfileOutcome,
    },
}

impl core::fmt::Display for PolicyProfileIoError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyProfileIoError {}

impl From<PolicyProfileHandoffError> for PolicyProfileIoError {
    fn from(error: PolicyProfileHandoffError) -> Self {
        Self::ProfileHandoff(error)
    }
}

pub trait PolicyProfileHandoffIo {
    type Error: From<PolicyProfileIoError>;
    /// Send the exact reducer effect under the adapter's existing bounded I/O
    /// budget. Successful transport custody is not profile acceptance.
    fn send_profile_effect(
        &mut self,
        effect: PolicyProfileHandoffEffect,
    ) -> Result<(), Self::Error>;

    /// One bounded receive, not a loop that skips stale or out-of-phase input.
    /// None denotes a different, successfully decoded semantic message.
    fn receive_profile_completion(
        &mut self,
    ) -> Result<Option<(PolicyProfileHandoffKind, PolicyProfileCompletion)>, Self::Error>;
}

/// Returns the candidate model even on a peer rejection, so its existing
/// coordinator can request exact rollback. Nothing is committed by this helper.
pub fn execute_policy_profile_handoff_step<I: PolicyProfileHandoffIo>(
    io: &mut I,
    model: &PolicyProfileHandoffModel,
    kind: PolicyProfileHandoffKind,
    transaction: TransactionId,
) -> Result<PolicyProfileHandoffUpdate, I::Error> {
    let update =
        reduce_policy_profile_handoff(model, PolicyProfileHandoffMsg::Begin { kind, transaction })
            .map_err(PolicyProfileIoError::from)?;
    io.send_profile_effect(
        update
            .effect
            .expect("a valid profile begin always emits one effect"),
    )?;
    let Some((completion_kind, completion)) = io.receive_profile_completion()? else {
        return Err(PolicyProfileIoError::ProfileCompletionOutOfPhase.into());
    };
    if completion_kind != kind {
        return Err(PolicyProfileIoError::ProfileCompletionOutOfPhase.into());
    }
    reduce_policy_profile_handoff(
        &update.model,
        PolicyProfileHandoffMsg::Completion { kind, completion },
    )
    .map_err(|error| PolicyProfileIoError::from(error).into())
}

pub fn activate_policy_profile_handoff<I: PolicyProfileHandoffIo>(
    io: &mut I,
    identity: PolicyProfileIdentity,
    prepare_transaction: TransactionId,
    activate_transaction: TransactionId,
) -> Result<PolicyProfileHandoffModel, I::Error> {
    let mut model = PolicyProfileHandoffModel::new(identity);
    for (kind, transaction) in [
        (PolicyProfileHandoffKind::Prepare, prepare_transaction),
        (PolicyProfileHandoffKind::Activate, activate_transaction),
    ] {
        let settled = execute_policy_profile_handoff_step(io, &model, kind, transaction)?;
        match settled.completion {
            Some(PolicyProfileCompletionDisposition::Accepted) => {
                model = settled.model;
            }
            Some(PolicyProfileCompletionDisposition::Rejected(outcome)) => {
                return Err(PolicyProfileIoError::ProfileRejected { kind, outcome }.into());
            }
            Some(PolicyProfileCompletionDisposition::Stale) | None => {
                return Err(PolicyProfileIoError::ProfileCompletionStale.into());
            }
        }
    }
    Ok(model)
}
