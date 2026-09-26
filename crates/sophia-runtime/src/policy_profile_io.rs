//! Transport operations around the existing profile reducer. There is no
//! additional phase state here, nor a retry/deadline renewal policy.
use crate::{
    PolicyProfileCompletionDisposition, PolicyProfileHandoffEffect, PolicyProfileHandoffKind,
    PolicyProfileHandoffModel, PolicyProfileHandoffMsg, PolicyProfileHandoffUpdate,
    PolicyTransportError, reduce_policy_profile_handoff,
};
use sophia_protocol::{PolicyProfileCompletion, PolicyProfileIdentity, TransactionId};

pub trait PolicyProfileHandoffIo {
    /// Send the exact reducer effect under the adapter's existing bounded I/O
    /// budget. Successful transport custody is not profile acceptance.
    fn send_profile_effect(
        &mut self,
        effect: PolicyProfileHandoffEffect,
    ) -> Result<(), PolicyTransportError>;

    /// One bounded receive, not a loop that skips stale or out-of-phase input.
    /// None denotes a different, successfully decoded semantic message.
    fn receive_profile_completion(
        &mut self,
    ) -> Result<Option<(PolicyProfileHandoffKind, PolicyProfileCompletion)>, PolicyTransportError>;
}

/// Returns the candidate model even on a peer rejection, so its existing
/// coordinator can request exact rollback. Nothing is committed by this helper.
pub fn execute_policy_profile_handoff_step(
    io: &mut impl PolicyProfileHandoffIo,
    model: &PolicyProfileHandoffModel,
    kind: PolicyProfileHandoffKind,
    transaction: TransactionId,
) -> Result<PolicyProfileHandoffUpdate, PolicyTransportError> {
    let update =
        reduce_policy_profile_handoff(model, PolicyProfileHandoffMsg::Begin { kind, transaction })?;
    io.send_profile_effect(
        update
            .effect
            .expect("a valid profile begin always emits one effect"),
    )?;
    let Some((completion_kind, completion)) = io.receive_profile_completion()? else {
        return Err(PolicyTransportError::ProfileCompletionOutOfPhase);
    };
    if completion_kind != kind {
        return Err(PolicyTransportError::ProfileCompletionOutOfPhase);
    }
    reduce_policy_profile_handoff(
        &update.model,
        PolicyProfileHandoffMsg::Completion { kind, completion },
    )
    .map_err(Into::into)
}

pub fn activate_policy_profile_handoff(
    io: &mut impl PolicyProfileHandoffIo,
    identity: PolicyProfileIdentity,
    prepare_transaction: TransactionId,
    activate_transaction: TransactionId,
) -> Result<PolicyProfileHandoffModel, PolicyTransportError> {
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
                return Err(PolicyTransportError::ProfileRejected { kind, outcome });
            }
            Some(PolicyProfileCompletionDisposition::Stale) | None => {
                return Err(PolicyTransportError::ProfileCompletionStale);
            }
        }
    }
    Ok(model)
}
