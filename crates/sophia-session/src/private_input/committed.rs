//! Driving one coordinator step, and routing exactly what it committed.

use sophia_x_authority::XAuthorityObservedTransactionBatch;
use std::time::Duration;

use super::control::{PrivateInputCommitted, PrivateInputControlError, PrivateInputSubmitted};
use super::handle::{PrivateInputHandle, PrivateInputUnavailable};

impl PrivateInputHandle {
    /// Take the transaction batches the frontend observed, commit them through
    /// this Session's own coordinator, and submit exactly the effects that
    /// commit called for.
    ///
    /// THE COMMIT IS THE AUTHORITY FOR THE EFFECT. Batches are converted to
    /// authority intakes and put through `ProductionSessionCoordinator`; only a
    /// `TransactionCommit` whose outcome is `Committed` produces an effect, and
    /// each effect's geometry is read from the coordinator's committed surface
    /// state. Nothing here accepts a geometry from a caller, and a caller
    /// cannot construct a committed effect at all.
    ///
    /// Reported as four numbers that agree only when nothing was rejected or
    /// refused: batches observed, commits returned, commits that committed,
    /// and effects the order took. Counting observed batches would establish
    /// nothing about commitment, which is why the committed count is separate.
    pub fn apply_committed(
        &self,
        within: Duration,
    ) -> Result<PrivateInputCommitted, PrivateInputUnavailable> {
        let batches = self.drain_transactions_within(within);
        let mut report = PrivateInputCommitted {
            batches_observed: batches.len(),
            ..PrivateInputCommitted::default()
        };
        if batches.is_empty() {
            return Ok(report);
        }

        // MAPPING EDGES COME FROM THE BATCH, not from committed presence. A
        // surface is admissible when the batch says it is mapped, or when a
        // policy-managed deferred map has raised a Request that this admission
        // is what satisfies.
        let mut wants_admission = std::collections::BTreeSet::new();
        let mut withdrawn = std::collections::BTreeSet::new();
        let mut intakes = Vec::with_capacity(batches.len());
        for batch in &batches {
            for seen in &batch.surface_presentations {
                if seen.mapped {
                    wants_admission.insert(seen.surface);
                }
            }
            for intent in &batch.presentation_intents {
                match intent.kind {
                    sophia_protocol::SurfacePresentationIntentKind::Request => {
                        wants_admission.insert(intent.surface);
                    }
                    sophia_protocol::SurfacePresentationIntentKind::Withdraw => {
                        withdrawn.insert(intent.surface);
                    }
                }
            }
            withdrawn.extend(batch.removed_surfaces.iter().copied());
            intakes.push(
                sophia_engine::AuthorityTransactionIntake::new(
                    batch.transaction,
                    batch.transactions.clone(),
                )
                .with_surface_removals(batch.removed_surfaces.clone()),
            );
        }

        let mut coordinator = self
            .runtime
            .coordinator
            .lock()
            .map_err(|_| PrivateInputUnavailable)?;
        let commits = coordinator.commit_authority_batches(&intakes);
        report.commits = commits.len();

        // ONLY A COMMIT WHOSE OUTCOME IS Committed PRODUCES AN EFFECT. A
        // rejected or timed out transaction is a commit result too, and
        // treating it as committed is the failure this separation exposes.
        let mut effects = Vec::new();
        for commit in &commits {
            if commit.outcome != sophia_protocol::TransactionOutcome::Committed {
                continue;
            }
            report.committed += 1;
            for surface in &commit.applied_surfaces {
                if withdrawn.contains(surface) {
                    effects.push((
                        commit.transaction,
                        *surface,
                        sophia_x_authority::XAuthorityControlKind::WithdrawSurface,
                        None,
                    ));
                    continue;
                }
                if !wants_admission.contains(surface) {
                    continue;
                }
                // GEOMETRY FROM THE COMMITTED STATE, never from a caller.
                let Some(geometry) = coordinator
                    .committed_surfaces()
                    .iter()
                    .find(|held| held.surface == *surface)
                    .map(|held| held.geometry)
                else {
                    continue;
                };
                let first = self
                    .runtime
                    .admitted_surfaces
                    .lock()
                    .map(|held| !held.contains(surface))
                    .map_err(|_| PrivateInputUnavailable)?;
                let kind = if first {
                    sophia_x_authority::XAuthorityControlKind::AdmitSurface
                } else {
                    sophia_x_authority::XAuthorityControlKind::ConfigureSurface
                };
                effects.push((commit.transaction, *surface, kind, Some(geometry)));
            }
        }
        drop(coordinator);

        for (committed_transaction, surface, kind, geometry) in effects {
            let submitted = self.route_committed(&batches, surface, kind, geometry);
            match submitted {
                Ok(submitted) => {
                    if let Ok(mut held) = self.runtime.admitted_surfaces.lock() {
                        match kind {
                            sophia_x_authority::XAuthorityControlKind::WithdrawSurface => {
                                held.remove(&surface);
                            }
                            _ => {
                                held.insert(surface);
                            }
                        }
                    }
                    report.effects.push(super::PrivateInputCommittedEffect::new(
                        committed_transaction,
                        surface,
                        kind,
                        geometry,
                        Some(submitted),
                    ));
                }
                Err(refusal) => {
                    report.effects.push(super::PrivateInputCommittedEffect::new(
                        committed_transaction,
                        surface,
                        kind,
                        geometry,
                        None,
                    ));
                    report.refused.push(refusal);
                }
            }
        }
        Ok(report)
    }

    /// Submit one committed effect to the connection that owns its surface.
    fn route_committed(
        &self,
        batches: &[XAuthorityObservedTransactionBatch],
        surface: sophia_protocol::SurfaceId,
        kind: sophia_x_authority::XAuthorityControlKind,
        geometry: Option<sophia_protocol::Rect>,
    ) -> Result<PrivateInputSubmitted, PrivateInputControlError> {
        let client = batches
            .iter()
            .flat_map(|batch| batch.surface_routes.iter())
            .find(|route| route.surface == surface)
            .map(|route| route.client)
            .ok_or(PrivateInputControlError::ConnectionGone)?;
        let transaction = self
            .runtime
            .next_transaction()
            .ok_or(PrivateInputControlError::Exhausted)?;
        let command = match (kind, geometry) {
            (sophia_x_authority::XAuthorityControlKind::AdmitSurface, Some(geometry)) => {
                sophia_x_authority::XAuthorityControlCommand::AdmitSurface {
                    transaction,
                    surface,
                    geometry,
                }
            }
            (sophia_x_authority::XAuthorityControlKind::ConfigureSurface, Some(geometry)) => {
                sophia_x_authority::XAuthorityControlCommand::ConfigureSurface {
                    transaction,
                    surface,
                    geometry,
                }
            }
            (sophia_x_authority::XAuthorityControlKind::WithdrawSurface, _) => {
                sophia_x_authority::XAuthorityControlCommand::WithdrawSurface {
                    transaction,
                    surface,
                }
            }
            _ => return Err(PrivateInputControlError::ConnectionGone),
        };
        let producer = self
            .runtime
            .access
            .control_producer(&self.runtime.owner.lease())
            .map_err(|_| PrivateInputControlError::Ended)?;
        producer
            .submit(
                &self.runtime.owner.lease(),
                sophia_x_authority::XAuthorityClientControlCommand { client, command },
            )
            .map(|_sequence| PrivateInputSubmitted {
                transaction,
                surface,
                kind,
            })
            .map_err(|(refusal, command)| PrivateInputControlError::Refused(refusal, command))
    }
}
