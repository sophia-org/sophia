//! Bind each authority update once to the Engine state it may replace.

use std::collections::{BTreeMap, BTreeSet};

use sophia_engine::AuthorityTransactionIntake;
use sophia_protocol::{
    ClientAdmissionId, CommittedSurfaceState, SurfaceId, TransactionCommit, TransactionId,
    TransactionOutcome,
};
use sophia_x_authority::{PrivateAdmittedConnection, XAuthorityObservedTransactionBatch};

#[derive(Clone, Copy)]
struct SourceGeneration {
    admission: ClientAdmissionId,
    previous: u64,
    removed: bool,
    committed: Option<TransactionId>,
}

#[derive(Default)]
pub(super) struct GenerationLedger {
    sources: BTreeMap<SurfaceId, SourceGeneration>,
}

impl GenerationLedger {
    pub(super) fn committed_transaction(&self, surface: SurfaceId) -> Option<TransactionId> {
        self.sources
            .get(&surface)
            .filter(|source| !source.removed)
            .and_then(|source| source.committed)
    }

    pub(super) fn prepare(
        &self,
        batch: &XAuthorityObservedTransactionBatch,
        committed: &[CommittedSurfaceState],
        live: &[PrivateAdmittedConnection],
    ) -> Result<AuthorityTransactionIntake, TransactionOutcome> {
        let mut transactions = batch.transactions.clone();
        let mut surfaces = BTreeSet::new();
        let mut new_sources = 0;
        for transaction in &mut transactions {
            if transaction.transaction != batch.transaction || !surfaces.insert(transaction.surface)
            {
                return Err(TransactionOutcome::RejectedStaleSurface);
            }
            let route = batch
                .surface_routes
                .iter()
                .find(|route| route.surface == transaction.surface)
                .ok_or(TransactionOutcome::RejectedStaleSurface)?;
            let admission = route
                .admission
                .ok_or(TransactionOutcome::RejectedStaleSurface)?;
            if !live.iter().any(|row| {
                row.client == route.client
                    && row.admission == admission.client_id
                    && Some(row.namespace) == transaction.namespace
                    && !row.closed
                    && row.lifecycle_open
            }) {
                return Err(TransactionOutcome::RejectedStaleSurface);
            }
            if let Some(prior) = self.sources.get(&transaction.surface) {
                if prior.admission != admission.client_id
                    || transaction.previous_committed_generation <= prior.previous
                    || prior.removed
                {
                    return Err(TransactionOutcome::RejectedStaleSurface);
                }
            } else {
                new_sources += 1;
                if self.sources.len() + new_sources > super::PRIVATE_INPUT_BRIDGE_BOUND {
                    return Err(TransactionOutcome::RejectedInvalidSurface);
                }
            }
            // X's raster generation advances when it publishes an update;
            // Engine advances only when it commits one. The FIFO owner binds
            // this new update to the current Engine predecessor once. A
            // retained control retries downstream without preparing it again.
            transaction.previous_committed_generation = committed
                .iter()
                .find(|state| state.surface == transaction.surface)
                .map_or(0, |state| state.committed_generation);
        }
        Ok(
            AuthorityTransactionIntake::new(batch.transaction, transactions)
                .with_surface_removals(batch.removed_surfaces.clone()),
        )
    }

    pub(super) fn record(
        &mut self,
        batch: &XAuthorityObservedTransactionBatch,
        commits: &[TransactionCommit],
    ) {
        // A rejected candidate was still attempted. Reoffering its source
        // generation must not turn that answer into a new commit.
        for transaction in &batch.transactions {
            if let Some(admission) = batch
                .surface_routes
                .iter()
                .find(|route| route.surface == transaction.surface)
                .and_then(|route| route.admission)
            {
                let committed = if commits.iter().any(|commit| {
                    commit.outcome == TransactionOutcome::Committed
                        && commit.applied_surfaces.contains(&transaction.surface)
                }) {
                    Some(batch.transaction)
                } else {
                    self.committed_transaction(transaction.surface)
                };
                self.sources.insert(
                    transaction.surface,
                    SourceGeneration {
                        admission: admission.client_id,
                        previous: transaction.previous_committed_generation,
                        removed: false,
                        committed,
                    },
                );
            }
        }
        if commits
            .iter()
            .any(|commit| commit.outcome == TransactionOutcome::Committed)
        {
            for surface in &batch.removed_surfaces {
                if let Some(source) = self.sources.get_mut(surface) {
                    source.removed = true;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/private_input_generations.rs"]
mod tests;
