//! Private supplied-stream startup join. This is not a complete runtime adapter:
//! atomic snapshot/Cycle publication and launch selection are separate joins.
use super::super::adapter::PolicyProfileAdmission;
use super::super::driver::{PolicyAdmissionPermit, PolicyProfilePermit};
use super::typed_codec::{TypedFileCodec, codec_error};
use super::*;
use sophia_protocol::PolicyProfileIdentity;
use sophia_runtime::{
    PolicyProfileHandoffEffect, PolicyProfileHandoffIo, PolicyProfileHandoffKind,
    PolicyTransportError, activate_policy_profile_handoff, select_policy_capabilities,
};

// Unlike legacy per-read socket timeouts these are absolute receive budgets.
// Fragments, event reads and ACKs do not renew them.
const OFFER_RESPONSE_BUDGET: Duration = super::super::POLICY_CLIENT_RESPONSE_DEADLINE;
const PROFILE_RESPONSE_BUDGET: Duration = super::super::POLICY_CLIENT_RESPONSE_DEADLINE;

pub(super) struct FileStartup {
    reactor: Option<NinePReactor<TypedFileCodec>>,
    epoch: u64,
    limits: WmFileLimits,
}
impl FileStartup {
    pub(super) fn adopt(
        stream: UnixStream,
        epoch: u64,
        limits: WmFileLimits,
        qids: WmQids,
    ) -> Result<Self, String> {
        let owner = WmFiles::awaiting_negotiation(epoch, limits, qids, TypedFileCodec)
            .map_err(|e| format!("WM file limits: {e:?}"))?;
        Ok(Self {
            reactor: Some(NinePReactor::adopt(stream, owner)?),
            epoch,
            limits,
        })
    }
    pub(super) fn stop_handle(&self) -> Box<dyn PolicyAdapterStop> {
        self.reactor.as_ref().expect("live startup").stop_handle()
    }
    pub(super) fn admit(
        &mut self,
        admission: PolicyAdmissionPermit,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        let result = self.admit_inner(admission, profile);
        if result.is_err() {
            // Drop the adopted socket as well as revoking all retained fids.
            if let Some(mut reactor) = self.reactor.take() {
                reactor.owner_mut().revoke();
            }
        }
        result
    }
    fn admit_inner(
        &mut self,
        admission: PolicyAdmissionPermit,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        if self.limits.profile_required != profile.is_some()
            || profile.is_some_and(|p| p.connection_epoch != self.epoch)
        {
            return Err("WM file supplied profile identity mismatch".into());
        }
        let reactor = self.reactor.as_mut().ok_or("WM file admission closed")?;
        if reactor.owner_mut().selected_capabilities().is_some() {
            return Err("WM file already negotiated".into());
        }
        let (offer_permit, profile_permit) = admission.negotiate();
        let Some(PolicyAdapterEvent::Negotiation(offer)) =
            reactor.receive(offer_permit, OFFER_RESPONSE_BUDGET)?
        else {
            return Err("WM file negotiation response deadline or kind".into());
        };
        let selected = select_policy_capabilities(
            offer.required_capabilities | offer.optional_capabilities,
            self.limits.capability_ceiling,
            profile.is_some(),
        );
        if offer.required_capabilities & !selected != 0 {
            return Err("WM file required capabilities unavailable".into());
        }
        reactor
            .owner_mut()
            .bind_selected(selected)
            .map_err(|e| format!("WM file selection: {e:?}"))?;
        // Use the canonical owner's selected value for every outgoing/incoming
        // codec after the one-time binding, not a separately cached selection.
        let selected = reactor
            .owner_mut()
            .selected_capabilities()
            .expect("bound above");
        reactor.send_encoded(WmFileKind::Negotiated, |header| {
            encode_wm_file_negotiated(header, selected).map_err(codec_error)
        })?;
        if let Some(profile) = profile {
            let identity = PolicyProfileIdentity::new(
                profile.connection_epoch,
                profile.generation,
                profile.digest,
            )
            .map_err(|e| format!("WM file profile: {e:?}"))?;
            let mut io = FileProfileIo {
                reactor,
                permission: profile_permit,
                pending: None,
            };
            activate_policy_profile_handoff(
                &mut io,
                identity,
                profile.prepare_transaction,
                profile.activate_transaction,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

struct FileProfileIo<'a> {
    reactor: &'a mut NinePReactor<TypedFileCodec>,
    permission: PolicyProfilePermit,
    // One in-flight reducer effect, not a second handoff state machine.
    pending: Option<PolicyReceivePermit>,
}
impl PolicyProfileHandoffIo for FileProfileIo<'_> {
    fn send_profile_effect(
        &mut self,
        effect: PolicyProfileHandoffEffect,
    ) -> Result<(), PolicyTransportError> {
        if self.pending.is_some() {
            return Err(PolicyTransportError::ProfileCompletionOutOfPhase);
        }
        let selected = self
            .reactor
            .owner_mut()
            .selected_capabilities()
            .ok_or(PolicyTransportError::ProfileCompletionOutOfPhase)?;
        let kind = match effect.kind {
            PolicyProfileHandoffKind::Prepare => WmFileKind::ProfilePrepare,
            PolicyProfileHandoffKind::Activate => WmFileKind::ProfileActivate,
            PolicyProfileHandoffKind::Rollback => WmFileKind::ProfileRollback,
        };
        self.reactor
            .send_encoded(kind, |header| {
                encode_wm_file_profile_command(header, effect.command, selected)
                    .map_err(codec_error)
            })
            .map_err(PolicyTransportError::Io)?;
        self.pending = Some(self.permission.completion(effect));
        Ok(())
    }
    fn receive_profile_completion(
        &mut self,
    ) -> Result<
        Option<(
            PolicyProfileHandoffKind,
            sophia_protocol::PolicyProfileCompletion,
        )>,
        PolicyTransportError,
    > {
        let permit = self
            .pending
            .take()
            .ok_or(PolicyTransportError::ProfileCompletionOutOfPhase)?;
        match self
            .reactor
            .receive(permit, PROFILE_RESPONSE_BUDGET)
            .map_err(PolicyTransportError::Io)?
        {
            Some(PolicyAdapterEvent::ProfileCompletion { kind, completion }) => {
                Ok(Some((kind, completion)))
            }
            _ => Err(PolicyTransportError::ProfileCompletionOutOfPhase),
        }
    }
}

#[path = "../../../../tests/support/policy_file_startup.rs"]
mod tests;
