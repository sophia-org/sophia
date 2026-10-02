use crate::EngineError;
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct WmTransactionUpdate {
    pub commit: TransactionCommit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastCommittedLayout {
    layers: Vec<LayerSnapshot>,
}

impl LastCommittedLayout {
    pub fn new(layers: Vec<LayerSnapshot>) -> Self {
        Self { layers }
    }

    pub fn layers(&self) -> &[LayerSnapshot] {
        &self.layers
    }

    pub fn replace(&mut self, layers: &[LayerSnapshot]) {
        self.layers.clear();
        self.layers.extend_from_slice(layers);
    }

    pub fn restore_into(&self, layers: &mut Vec<LayerSnapshot>) {
        layers.clear();
        layers.extend_from_slice(&self.layers);
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceVisualStateEntry {
    pub surface: SurfaceId,
    pub committed: Option<CommittedSurfaceState>,
    pub pending: Option<SurfaceTransaction>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SurfaceVisualStateTable {
    entries: BTreeMap<SurfaceId, SurfaceVisualStateEntry>,
}

/// A validated visual-state candidate that has not yet become authoritative.
///
/// Presentation backends may render this candidate and retain it across an
/// asynchronous KMS submission. Applying it revalidates the committed baseline
/// for every surface touched by the transaction, so a stale page-flip callback
/// cannot overwrite newer state while unrelated surfaces may continue to
/// commit. Dropping the value is the explicit discard operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedSurfaceCommit {
    pub(crate) damage_identity: crate::SurfaceDamageIdentity,
    pub(crate) commit: TransactionCommit,
    pub(crate) baseline: Vec<CommittedSurfaceState>,
    pub(crate) candidate: Vec<CommittedSurfaceState>,
}

impl PreparedSurfaceCommit {
    pub(crate) fn new(
        commit: TransactionCommit,
        baseline: Vec<CommittedSurfaceState>,
        candidate: Vec<CommittedSurfaceState>,
    ) -> Self {
        Self {
            damage_identity: Default::default(),
            commit,
            baseline,
            candidate,
        }
    }

    /// Identifies this exact candidate's pixels, even if another candidate
    /// later reuses its generation and client buffer handle.
    pub fn damage_identity(&self) -> &crate::SurfaceDamageIdentity {
        &self.damage_identity
    }

    pub const fn transaction(&self) -> TransactionId {
        self.commit.transaction
    }

    pub const fn commit(&self) -> &TransactionCommit {
        &self.commit
    }

    pub fn baseline(&self) -> &[CommittedSurfaceState] {
        &self.baseline
    }

    pub fn candidate(&self) -> &[CommittedSurfaceState] {
        &self.candidate
    }

    pub const fn is_ready(&self) -> bool {
        matches!(self.commit.outcome, TransactionOutcome::Committed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceTransactionCommitReadiness {
    Ready,
    InvalidSurface,
    EmptyGeometry,
    MissingBuffer,
    NotReady(SurfaceTransactionReadiness),
    StaleGeneration { current: u64, expected: u64 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SurfaceTimeoutPolicy {
    #[default]
    PreserveCommitted,
    ReplanAtCommittedExtent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlowClientVisualDecision {
    PreserveCommitted {
        surface: SurfaceId,
        committed: Option<CommittedSurfaceState>,
    },
    ReplanAtCommittedExtent {
        surface: SurfaceId,
        committed: Option<CommittedSurfaceState>,
        extent: Option<Size>,
    },
    NotTimedOut {
        surface: SurfaceId,
        readiness: SurfaceTransactionCommitReadiness,
    },
}

impl SurfaceVisualStateTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_committed_states(
        committed: impl IntoIterator<Item = CommittedSurfaceState>,
    ) -> Self {
        let mut table = Self::new();
        for state in committed {
            table.upsert_committed(state);
        }
        table
    }

    pub fn entry(&self, surface: SurfaceId) -> Option<&SurfaceVisualStateEntry> {
        self.entries.get(&surface)
    }

    pub fn committed(&self, surface: SurfaceId) -> Option<&CommittedSurfaceState> {
        self.entries
            .get(&surface)
            .and_then(|entry| entry.committed.as_ref())
    }

    pub fn pending(&self, surface: SurfaceId) -> Option<&SurfaceTransaction> {
        self.entries
            .get(&surface)
            .and_then(|entry| entry.pending.as_ref())
    }

    pub fn committed_states(&self) -> Vec<CommittedSurfaceState> {
        self.entries
            .values()
            .filter_map(|entry| entry.committed.clone())
            .collect()
    }

    pub fn pending_transactions(&self) -> Vec<SurfaceTransaction> {
        self.entries
            .values()
            .filter_map(|entry| entry.pending.clone())
            .collect()
    }

    pub fn transaction_commit_readiness(
        &self,
        transaction: &SurfaceTransaction,
    ) -> SurfaceTransactionCommitReadiness {
        if !transaction.surface.is_valid() {
            return SurfaceTransactionCommitReadiness::InvalidSurface;
        }
        if transaction.target_geometry.is_empty() {
            return SurfaceTransactionCommitReadiness::EmptyGeometry;
        }
        if matches!(transaction.target_buffer(), BufferSource::None)
            && self.committed(transaction.surface).is_none()
        {
            return SurfaceTransactionCommitReadiness::MissingBuffer;
        }
        if transaction.readiness != SurfaceTransactionReadiness::Ready {
            return SurfaceTransactionCommitReadiness::NotReady(transaction.readiness);
        }

        let current = self
            .committed(transaction.surface)
            .map(|state| state.committed_generation)
            .unwrap_or(0);
        let expected = transaction.previous_committed_generation;
        if current != expected {
            return SurfaceTransactionCommitReadiness::StaleGeneration { current, expected };
        }

        SurfaceTransactionCommitReadiness::Ready
    }

    pub fn slow_client_timeout_decision(
        &self,
        transaction: &SurfaceTransaction,
        policy: SurfaceTimeoutPolicy,
    ) -> SlowClientVisualDecision {
        let readiness = self.transaction_commit_readiness(transaction);
        if !matches!(
            readiness,
            SurfaceTransactionCommitReadiness::NotReady(SurfaceTransactionReadiness::TimedOut)
        ) {
            return SlowClientVisualDecision::NotTimedOut {
                surface: transaction.surface,
                readiness,
            };
        }

        match policy {
            SurfaceTimeoutPolicy::PreserveCommitted => {
                SlowClientVisualDecision::PreserveCommitted {
                    surface: transaction.surface,
                    committed: self.committed(transaction.surface).cloned(),
                }
            }
            SurfaceTimeoutPolicy::ReplanAtCommittedExtent => {
                let committed = self.committed(transaction.surface).cloned();
                let extent = committed.as_ref().map(|state| Size {
                    width: state.geometry.width,
                    height: state.geometry.height,
                });
                SlowClientVisualDecision::ReplanAtCommittedExtent {
                    surface: transaction.surface,
                    committed,
                    extent,
                }
            }
        }
    }

    pub fn upsert_committed(&mut self, committed: CommittedSurfaceState) {
        self.entries
            .entry(committed.surface)
            .and_modify(|entry| {
                entry.committed = Some(committed.clone());
            })
            .or_insert_with(|| SurfaceVisualStateEntry {
                surface: committed.surface,
                committed: Some(committed),
                pending: None,
            });
    }

    pub fn stage_pending(&mut self, pending: SurfaceTransaction) -> Result<(), EngineError> {
        if !pending.surface.is_valid() {
            return Err(EngineError::InvalidSurface);
        }

        self.entries
            .entry(pending.surface)
            .and_modify(|entry| {
                entry.pending = Some(pending.clone());
            })
            .or_insert_with(|| SurfaceVisualStateEntry {
                surface: pending.surface,
                committed: None,
                pending: Some(pending),
            });
        Ok(())
    }

    pub fn clear_pending(&mut self, surface: SurfaceId) -> Option<SurfaceTransaction> {
        self.entries
            .get_mut(&surface)
            .and_then(|entry| entry.pending.take())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
