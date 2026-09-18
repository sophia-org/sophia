//! Persistent catalog effects consume the existing issued-action ledger. A
//! catalog slot or client event number alone is never launch authority.
use super::*;
use crate::application_catalog::PublishedApplicationCatalog;
use crate::session_actions::{NativeCatalogLaunchRefusal, SessionLaunchQueue};
use sophia_engine::PresentedContentBinding;
use sophia_protocol::{CatalogActivation, SessionApplicationId, ShellApplicationCatalog};

impl ContentActionLedger {
    pub(super) fn catalog_eligible(
        &self,
        activation: &CatalogActivation,
        current: &ShellApplicationCatalog,
        presented: &[PresentedContentBinding],
        now: u64,
    ) -> Option<usize> {
        if activation.action.event_id == 0
            || activation.action.event_id > self.issued_high_water
            || activation.catalog_generation != current.generation
            || activation.action.grant.connection_epoch != current.connection_epoch
        {
            return None;
        }
        self.live.iter().position(|pending| {
            pending.authority == ActionAuthority::Catalog(current.generation)
                && pending.action == activation.action
                && pending.action.kind == ACTION_ACTIVATE
                && pending.activation == ActivationState::Awaiting
                && !pending.cancel_sent
                && now < pending.deadline_msec
                && presented.iter().any(|binding| {
                    binding.authority_current
                        && binding.grant == pending.action.grant
                        && binding.output == pending.action.output
                        && binding.targets.iter().any(|target| {
                            sophia_engine::content_target_continues(target, &pending.target)
                        })
                })
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn service_catalog_request(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        presented: &[PresentedContentBinding],
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
        now: u64,
    ) -> Result<bool, ShellTransportError> {
        // Typed intake owns the outcome credit before any launch effect.
        let Some((transaction, activation)) = transport.poll_catalog_activation()? else {
            return Ok(false);
        };
        let eligible = self.catalog_eligible(&activation, publication.wire(), presented, now);
        let status = if let Some(index) = eligible {
            let status = match u16::try_from(activation.action.action_id)
                .ok()
                .and_then(|slot| publication.entry(slot))
            {
                None => 4,
                Some(entry) => match launches.enqueue_persistent_catalog(
                    activation.clone(),
                    entry,
                    application,
                    active_children,
                ) {
                    Ok(_) => 1,
                    Err(NativeCatalogLaunchRefusal::Stale) => 2,
                    Err(NativeCatalogLaunchRefusal::Unauthorized) => 4,
                    Err(NativeCatalogLaunchRefusal::Capacity) => 5,
                    Err(NativeCatalogLaunchRefusal::Exhausted) => {
                        return Err(ShellTransportError::InvalidConnectionEpoch);
                    }
                },
            };
            // No callback or fallible transfer intervenes between actual queue
            // insertion and consuming this exact ledger event. ACK is separate.
            self.live[index].activation = if status == 1 {
                ActivationState::EffectAdmitted
            } else {
                ActivationState::Rejected
            };
            status
        } else {
            2
        };
        // Failure here cannot replay the completed queue effect. The transport
        // retains this exact outcome and retries only its FIFO transfer.
        transport.finish_catalog_activation(transaction, &activation, status)?;
        Ok(true)
    }
}

impl super::super::LiveContentSession {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session::metadata_shell) fn service_catalog_requests(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        presented: &[PresentedContentBinding],
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
    ) -> Result<usize, ShellTransportError> {
        if !transport.supports_persistent_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        let now = self.now_msec();
        let mut processed = 0;
        for _ in 0..32 {
            if !self.actions.service_catalog_request(
                transport,
                publication,
                presented,
                launches,
                application,
                active_children,
                now,
            )? {
                break;
            }
            processed += 1;
        }
        Ok(processed)
    }
}
