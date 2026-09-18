//! Driving coordinator steps, and routing exactly what each one committed.
//!
//! ONE BATCH AT A TIME, IN ORDER. An earlier version unioned the mapping facts
//! across every batch, committed them all, and then read geometry from the
//! final snapshot. That let an earlier commit borrow authority a later batch
//! raised, and gave an earlier surface a later batch's geometry. Each batch is
//! now committed on its own and its committed geometry read immediately, so
//! what an effect carries is what that commit actually decided.

use sophia_protocol::{Rect, SurfaceId, TransactionOutcome};
use sophia_x_authority::{XAuthorityControlKind, XAuthorityObservedTransactionBatch};
use std::time::Duration;

use super::control::{PrivateInputCommitted, PrivateInputControlError, PrivateInputSubmitted};
use super::handle::{PrivateInputHandle, PrivateInputUnavailable};
use super::submission::PrivateInputConnection;

/// One effect a commit called for, with everything needed to submit it.
///
/// RETAINED WHOLE WHEN THE ORDER REFUSES IT. A refusal must not cost the
/// effect: recommitting to rebuild it would put the same transaction through
/// the coordinator twice, and dropping it would lose committed state that was
/// never applied. This is what a later call retries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PrivateInputPendingEffect {
    pub(super) committed_transaction: sophia_protocol::TransactionId,
    pub(super) surface: SurfaceId,
    pub(super) kind: XAuthorityControlKind,
    pub(super) geometry: Option<Rect>,
    /// The exact connection this effect belongs to, captured from the batch
    /// that produced it. Kept so a withdrawal still reaches the connection
    /// that was admitted, whose route the withdrawing batch no longer carries.
    pub(super) connection: PrivateInputConnection,
}

impl PrivateInputHandle {
    /// Take the transaction batches the frontend observed, commit each through
    /// this Session's own coordinator, and submit exactly the effects those
    /// commits called for.
    ///
    /// THE COMMIT IS THE AUTHORITY FOR THE EFFECT. Only a commit whose outcome
    /// is `Committed` produces one, and its geometry is read from the
    /// coordinator's committed surface state at that moment. Nothing accepts a
    /// geometry from a caller.
    ///
    /// Effects the order refused last time are retried first, from what was
    /// retained, without putting anything through the coordinator again.
    pub fn apply_committed(
        &self,
        within: Duration,
    ) -> Result<PrivateInputCommitted, PrivateInputUnavailable> {
        let mut report = PrivateInputCommitted::default();

        // RETRIES FIRST, AND THEY ARE NOT RECOMMITTED. These were committed
        // once already; what failed was the submission.
        let retries = self
            .runtime
            .pending_effects
            .lock()
            .map(|mut held| std::mem::take(&mut *held))
            .map_err(|_| PrivateInputUnavailable)?;
        for pending in retries {
            self.apply_one(pending, &mut report)?;
        }

        let batches = self.drain_transactions_within(within);
        report.batches_observed = batches.len();
        for batch in &batches {
            self.commit_one_batch(batch, &mut report)?;
        }
        Ok(report)
    }

    /// Commit one batch and apply what it decided.
    fn commit_one_batch(
        &self,
        batch: &XAuthorityObservedTransactionBatch,
        report: &mut PrivateInputCommitted,
    ) -> Result<(), PrivateInputUnavailable> {
        // THIS BATCH'S OWN MAPPING FACTS, not a union across batches. A map in
        // a later batch must not authorise an earlier commit.
        let mut mapped = std::collections::BTreeSet::new();
        for seen in &batch.surface_presentations {
            if seen.mapped {
                mapped.insert(seen.surface);
            }
        }
        let mut withdrawn = std::collections::BTreeSet::new();
        for intent in &batch.presentation_intents {
            match intent.kind {
                sophia_protocol::SurfacePresentationIntentKind::Request => {
                    mapped.insert(intent.surface);
                }
                sophia_protocol::SurfacePresentationIntentKind::Withdraw => {
                    withdrawn.insert(intent.surface);
                }
            }
        }
        withdrawn.extend(batch.removed_surfaces.iter().copied());

        let intake = sophia_engine::AuthorityTransactionIntake::new(
            batch.transaction,
            batch.transactions.clone(),
        )
        .with_surface_removals(batch.removed_surfaces.clone());

        let mut coordinator = self
            .runtime
            .coordinator
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let commits = coordinator.commit_authority_batches(std::slice::from_ref(&intake));
        report.commits += commits.len();

        let mut decided = Vec::new();
        for commit in &commits {
            if commit.outcome != TransactionOutcome::Committed {
                continue;
            }
            report.committed += 1;
            for surface in &commit.applied_surfaces {
                if !mapped.contains(surface) {
                    continue;
                }
                // CAPTURED NOW, from this commit's own state.
                let Some(geometry) = coordinator
                    .committed_surfaces()
                    .iter()
                    .find(|held| held.surface == *surface)
                    .map(|held| held.geometry)
                else {
                    continue;
                };
                decided.push((commit.transaction, *surface, Some(geometry), false));
            }
            // REMOVALS TRAVEL SEPARATELY. A removal-only batch applies no
            // surface, so a withdrawal read out of `applied_surfaces` would
            // never be seen at all.
            for surface in &withdrawn {
                decided.push((commit.transaction, *surface, None, true));
            }
        }
        drop(coordinator);

        for (committed_transaction, surface, geometry, withdrawal) in decided {
            let Some((kind, connection)) = self.classify(batch, surface, withdrawal)? else {
                continue;
            };
            self.apply_one(
                PrivateInputPendingEffect {
                    committed_transaction,
                    surface,
                    kind,
                    geometry,
                    connection,
                },
                report,
            )?;
        }
        Ok(())
    }

    /// Decide which effect this surface calls for and which connection owns it.
    ///
    /// THE LEDGER MOVES HERE, one effect at a time, so two commits of the same
    /// surface in one call cannot both be admissions. An earlier version
    /// classified everything before updating anything, which allowed exactly
    /// that.
    fn classify(
        &self,
        batch: &XAuthorityObservedTransactionBatch,
        surface: SurfaceId,
        withdrawal: bool,
    ) -> Result<Option<(XAuthorityControlKind, PrivateInputConnection)>, PrivateInputUnavailable>
    {
        let mut admitted = self
            .runtime
            .admitted_surfaces
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        if withdrawal {
            // THE ROUTE THAT ADMITTED IT, not one read from the withdrawing
            // batch, which no longer carries it.
            return Ok(admitted
                .remove(&surface)
                .map(|connection| (XAuthorityControlKind::WithdrawSurface, connection)));
        }
        // THIS BATCH'S OWN ROUTE, WITH ITS ADMISSION. A client number alone
        // names a connection a successor may since have taken.
        let Some(route) = batch
            .surface_routes
            .iter()
            .find(|route| route.surface == surface)
        else {
            return Ok(None);
        };
        let Some(admission) = route.admission else {
            return Ok(None);
        };
        let connection = PrivateInputConnection {
            client: route.client,
            admission: admission.client_id,
            connection_generation: admission.auth_provenance.session_generation,
        };
        let kind = match admitted.get(&surface) {
            // A surface whose admission belongs to another connection is a new
            // admission, not a configure of somebody else's window.
            Some(held) if held.admission == connection.admission => {
                XAuthorityControlKind::ConfigureSurface
            }
            _ => XAuthorityControlKind::AdmitSurface,
        };
        admitted.insert(surface, connection);
        Ok(Some((kind, connection)))
    }

    /// Submit one decided effect, retaining it whole if the order refuses.
    fn apply_one(
        &self,
        pending: PrivateInputPendingEffect,
        report: &mut PrivateInputCommitted,
    ) -> Result<(), PrivateInputUnavailable> {
        match self.route_committed(pending) {
            Ok(submitted) => {
                report.effects.push(super::PrivateInputCommittedEffect::new(
                    pending.committed_transaction,
                    pending.surface,
                    pending.kind,
                    pending.geometry,
                    Some(submitted),
                ));
            }
            Err(refusal) => {
                self.runtime
                    .pending_effects
                    .lock()
                    .map_err(|_| PrivateInputUnavailable)?
                    .push(pending);
                report.effects.push(super::PrivateInputCommittedEffect::new(
                    pending.committed_transaction,
                    pending.surface,
                    pending.kind,
                    pending.geometry,
                    None,
                ));
                report.refused.push(refusal);
            }
        }
        Ok(())
    }

    /// Submit one committed effect to the connection that owns its surface.
    fn route_committed(
        &self,
        pending: PrivateInputPendingEffect,
    ) -> Result<PrivateInputSubmitted, PrivateInputControlError> {
        let transaction = self
            .runtime
            .next_transaction()
            .ok_or(PrivateInputControlError::Exhausted)?;
        let surface = pending.surface;
        let command = match (pending.kind, pending.geometry) {
            (XAuthorityControlKind::AdmitSurface, Some(geometry)) => {
                sophia_x_authority::XAuthorityControlCommand::AdmitSurface {
                    transaction,
                    surface,
                    geometry,
                }
            }
            (XAuthorityControlKind::ConfigureSurface, Some(geometry)) => {
                sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
                    transaction,
                    surface,
                    geometry,
                }
            }
            (XAuthorityControlKind::WithdrawSurface, _) => {
                sophia_x_authority::XAuthorityControlCommand::WithdrawSurface {
                    transaction,
                    surface,
                }
            }
            _ => return Err(PrivateInputControlError::ConnectionGone),
        };
        // THE ADMISSION IS RECHECKED AGAINST CURRENT STATE. A connection that
        // ended between the commit and here is refused rather than served
        // through a number a successor now holds.
        let live = self
            .runtime
            .participant
            .admitted()
            .map_err(|_| PrivateInputControlError::Unavailable)?;
        if !live.iter().any(|seen| {
            seen.client == pending.connection.client
                && seen.admission == pending.connection.admission
                && !seen.closed
        }) {
            return Err(PrivateInputControlError::ConnectionGone);
        }
        let producer = self
            .runtime
            .access
            .control_producer(&self.runtime.owner.lease())
            .map_err(|_| PrivateInputControlError::Ended)?;
        producer
            .submit(
                &self.runtime.owner.lease(),
                sophia_x_authority::XAuthorityClientControlCommand {
                    client: pending.connection.client,
                    command,
                },
            )
            .map(|_sequence| PrivateInputSubmitted {
                transaction,
                surface,
                kind: pending.kind,
            })
            .map_err(|(refusal, command)| PrivateInputControlError::Refused(refusal, command))
    }
}
