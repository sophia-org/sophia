use super::*;
use crate::application_catalog::PublishedApplicationCatalog;
use crate::session_actions::{NativeCatalogLaunchRefusal, SessionLaunchQueue};
use sophia_protocol::{NativeLauncherActivation, SessionApplicationId};
use sophia_runtime::{
    NativeLauncherActivationDecision as Decision,
    NativeLauncherActivationEligibility as Eligibility,
};

/// Native component service owns its own actual action ledger. It uses the same
/// issuance, ACK and cancellation transitions as the bar; it is not a second
/// pointer-grant map. The live role scheduler must visit each service boundedly.
#[derive(Default)]
pub struct NativeLauncherActionService {
    ledger: ContentActionLedger,
}
impl NativeLauncherActionService {
    pub fn issue(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        target: PresentedContentTarget,
        transaction: TransactionId,
        now_msec: u64,
    ) -> Result<Option<u64>, ShellTransportError> {
        let Some(binding) = transport.native_launcher_focus() else {
            return Ok(None);
        };
        let Ok(slot) = u16::try_from(target.action_id) else {
            return Ok(None);
        };
        if !transport.native_launcher_has_row(binding, slot)
            || target.grant != binding.grant
            || target.output != binding.output
            || target.allocation != binding.allocation
            || target.candidate_generation != binding.candidate_generation
            || target.presentation_epoch != binding.presentation_epoch
            || target.interaction_generation != binding.interaction_generation
        {
            return Ok(None);
        }
        let limits = transport
            .content_limits()
            .ok_or(ShellTransportError::MissingCapability)?
            .clone();
        self.ledger.issue_bound(
            target,
            now_msec,
            &limits,
            transaction,
            transport,
            ActionAuthority::Native(binding),
        )
    }
    pub fn service_acks(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        now_msec: u64,
        maximum: usize,
    ) -> Result<usize, ShellTransportError> {
        self.ledger
            .service_acks(transport, now_msec, maximum.min(32))
    }
    pub fn service_cancellation(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        presented: &[sophia_engine::PresentedContentBinding],
        transaction: TransactionId,
        now_msec: u64,
    ) -> Result<bool, ShellTransportError> {
        let Some(index) = self.ledger.next_cancellation(presented, now_msec) else {
            return Ok(false);
        };
        self.ledger
            .queue_cancellation(index, transaction, transport)?;
        Ok(true)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn service_request(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
        now_mono_usec: u64,
        ledger_now_msec: u64,
    ) -> Result<bool, ShellTransportError> {
        self.ledger.service_native_launcher_request(
            transport,
            publication,
            launches,
            application,
            active_children,
            now_mono_usec,
            ledger_now_msec,
        )
    }
    /// Shared connected visit: exact ACKs and stale-target cancellation precede
    /// bounded activation intake. Admission means queue insertion, not execution.
    #[allow(clippy::too_many_arguments)]
    pub fn service_connected(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        presented: &[sophia_engine::PresentedContentBinding],
        cancellation_transaction: TransactionId,
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
        now_mono_usec: u64,
        ledger_now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.service_acks(transport, ledger_now_msec, 32)?;
        self.service_cancellation(
            transport,
            presented,
            cancellation_transaction,
            ledger_now_msec,
        )?;
        let mut processed = 0;
        for _ in 0..32 {
            if !self.service_request(
                transport,
                publication,
                launches,
                application,
                active_children,
                now_mono_usec,
                ledger_now_msec,
            )? {
                break;
            }
            processed += 1;
        }
        Ok(processed)
    }

    pub fn reset_disconnected(
        &mut self,
        transport: &ShellTransportConnection<'_>,
    ) -> Result<(), ShellTransportError> {
        if transport.content_grant().is_some() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        self.ledger.reset();
        Ok(())
    }
}

impl ContentActionLedger {
    fn native_pointer_eligible(
        &self,
        activation: &NativeLauncherActivation,
        now_msec: u64,
    ) -> bool {
        if activation.cause != 2 {
            return false;
        }
        self.live.iter().any(|pending| {
            let a = &pending.action;
            let b = activation.event.binding;
            pending.activation == ActivationState::Awaiting
                && pending.authority == ActionAuthority::Native(b)
                && now_msec <= pending.deadline_msec
                && !pending.cancel_sent
                && a.kind == ACTION_ACTIVATE
                && a.event_id == activation.event.event_id
                && a.grant == b.grant
                && a.output == b.output
                && a.allocation == b.allocation
                && a.candidate_generation == b.candidate_generation
                && a.presentation_epoch == b.presentation_epoch
                && a.interaction_generation == b.interaction_generation
                && a.action_id == u64::from(activation.slot)
        })
    }

    /// Shared decision sequence for native launchers. Transport intake owns the
    /// reply before this can insert into the actual Session queue. Queue receipt
    /// is not worker verification, process startup, or first-window admission.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session) fn service_native_launcher_request(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
        now_mono_usec: u64,
        ledger_now_msec: u64,
    ) -> Result<bool, ShellTransportError> {
        let Some((transaction, activation)) = transport.poll_native_launcher_activation()? else {
            return Ok(false);
        };
        let eligibility = transport.native_launcher_activation_eligibility(
            transaction,
            &activation,
            publication.wire(),
            now_mono_usec,
        )?;
        let eligible = match eligibility {
            Eligibility::Keyboard => true,
            Eligibility::Pointer => self.native_pointer_eligible(&activation, ledger_now_msec),
            Eligibility::Rejected(_) => false,
        };
        let decision = if eligible {
            if let Some(entry) = publication.entry(activation.slot) {
                match launches.enqueue_native_catalog(
                    activation,
                    entry,
                    application,
                    active_children,
                ) {
                    Ok(_) => Decision::Admitted,
                    Err(NativeCatalogLaunchRefusal::Capacity) => Decision::Capacity,
                    Err(NativeCatalogLaunchRefusal::Stale) => Decision::Stale,
                    Err(NativeCatalogLaunchRefusal::Unauthorized) => Decision::Unauthorized,
                    Err(NativeCatalogLaunchRefusal::Exhausted) => {
                        return Err(ShellTransportError::InvalidConnectionEpoch);
                    }
                }
            } else {
                Decision::Unauthorized
            }
        } else {
            match eligibility {
                Eligibility::Rejected(reason) => reason,
                _ => Decision::Stale,
            }
        };
        if activation.cause == 2 && eligible {
            // Mark only the exact checked event. ACK remains independent. No
            // externally supplied callback or allocating operation occurs here.
            let pending = self
                .live
                .iter_mut()
                .find(|p| p.action.event_id == activation.event.event_id)
                .expect("eligible pointer event remains owned");
            pending.activation = if decision == Decision::Admitted {
                ActivationState::EffectAdmitted
            } else {
                ActivationState::Rejected
            };
        }
        transport.finish_native_launcher_activation(transaction, &activation, decision)?;
        Ok(true)
    }
}
