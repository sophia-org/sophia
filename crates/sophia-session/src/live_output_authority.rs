use sophia_backend_live::{
    LibdrmNativeOutputCapability, LiveLogicalOutputAllocator, LiveOutputAuthorityProjectionError,
    LiveResolvedOutputTopology, project_live_output_authority_candidate_snapshot,
    resolve_live_output_topology_candidate,
};
use sophia_engine::{
    OutputTopologyTransaction, OutputTopologyTransactionFailure, OutputTopologyTransactionPhase,
    OutputTopologyTransactionTransition, RenderHeadId,
};
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputTopologyIntent, OutputV1Outcome, OutputV1OutcomeKind,
    OutputV1Proposal, SOPHIA_OUTPUT_OUTCOME_REASON_APPLY,
    SOPHIA_OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION, SOPHIA_OUTPUT_OUTCOME_REASON_HEAD_LOST,
    SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT, SOPHIA_OUTPUT_OUTCOME_REASON_NONE,
    SOPHIA_OUTPUT_OUTCOME_REASON_PREPARATION, SOPHIA_OUTPUT_OUTCOME_REASON_ROLLBACK,
    SOPHIA_OUTPUT_OUTCOME_REASON_STALE, TransactionId,
};

pub const OUTPUT_OUTCOME_REASON_NONE: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_NONE;
pub const OUTPUT_OUTCOME_REASON_STALE: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_STALE;
pub const OUTPUT_OUTCOME_REASON_PREPARATION: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_PREPARATION;
pub const OUTPUT_OUTCOME_REASON_APPLY: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_APPLY;
pub const OUTPUT_OUTCOME_REASON_HEAD_LOST: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_HEAD_LOST;
pub const OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION: u16 =
    SOPHIA_OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION;
pub const OUTPUT_OUTCOME_REASON_ROLLBACK: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_ROLLBACK;
pub const OUTPUT_OUTCOME_REASON_INVARIANT: u16 = SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputTopologyPreparationWaitObservation {
    pub cancellation_requested: bool,
    pub ordinary_settlement_idle: bool,
    pub native_quiescent: bool,
    pub deadline_reached: bool,
    /// Whether the one-shot escalation is still unspent. A wait may force
    /// quiescence once; a second expiry rejects the candidate.
    pub escalation_available: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTopologyPreparationWaitDecision {
    Cancel,
    Begin,
    /// Force presentation quiescence and wait once more. The owners this wait
    /// blocks on can only advance while it waits, so a wait that merely
    /// expires reports a stall it never tried to clear.
    Escalate,
    TimedOut,
    Wait,
}

/// Reduces owner-observed readiness without granting the protocol client any
/// knowledge of renderer or KMS resource lifetime.
pub const fn reduce_output_topology_preparation_wait(
    observation: OutputTopologyPreparationWaitObservation,
) -> OutputTopologyPreparationWaitDecision {
    if observation.cancellation_requested {
        OutputTopologyPreparationWaitDecision::Cancel
    } else if observation.ordinary_settlement_idle && observation.native_quiescent {
        OutputTopologyPreparationWaitDecision::Begin
    } else if observation.deadline_reached && observation.escalation_available {
        OutputTopologyPreparationWaitDecision::Escalate
    } else if observation.deadline_reached {
        OutputTopologyPreparationWaitDecision::TimedOut
    } else {
        OutputTopologyPreparationWaitDecision::Wait
    }
}

#[derive(Debug)]
pub enum LiveOutputAuthorityOwnerError {
    InvalidConnectionEpoch,
    InvalidTransaction,
    ActiveCandidate,
    NoActiveCandidate,
    TopologyEpochExhausted,
    StalePublishedSnapshot,
    Projection(LiveOutputAuthorityProjectionError),
    TransactionInvariant,
    NotTerminal,
}

impl core::fmt::Display for LiveOutputAuthorityOwnerError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveOutputAuthorityOwnerError {}

impl From<LiveOutputAuthorityProjectionError> for LiveOutputAuthorityOwnerError {
    fn from(error: LiveOutputAuthorityProjectionError) -> Self {
        Self::Projection(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveOutputAuthoritySettlement {
    pub transaction: TransactionId,
    pub outcome: OutputV1Outcome,
    /// Present only when this settlement publishes a replacement topology.
    pub published_snapshot: Option<OutputAuthoritySnapshot>,
}

/// Immutable physical-effect contract handed from protocol admission to the
/// live session owner.
///
/// The authority keeps the candidate private while this value is in flight.
/// Owning or cloning this record does not publish topology and cannot advance
/// the reducer; only explicit owner observations below may do that.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveOutputAuthorityEffect {
    pub transaction: TransactionId,
    pub base_topology_epoch: u64,
    pub candidate_topology_epoch: u64,
    pub published_snapshot: OutputAuthoritySnapshot,
    /// Protocol-visible replacement retained privately until the physical
    /// first-presentation barrier completes.
    pub candidate_snapshot: OutputAuthoritySnapshot,
    pub resolved: LiveResolvedOutputTopology,
}

#[derive(Clone, Debug)]
struct ActiveOutputCandidate {
    transaction: TransactionId,
    candidate_snapshot: OutputAuthoritySnapshot,
    resolved: LiveResolvedOutputTopology,
    candidate_allocator: LiveLogicalOutputAllocator,
    transaction_state: OutputTopologyTransaction,
}

/// Session orchestration for the exclusive output topology owner.
///
/// This type has no DRM handles and performs no effects. It joins the output
/// protocol's complete proposal, backend-native resolution, and Engine's
/// prepare/apply/first-presentation reducer while retaining the last published
/// snapshot until the transaction commits. The live session remains the only
/// owner allowed to execute the resulting renderer and KMS effects.
#[derive(Clone, Debug)]
pub struct LiveOutputAuthorityOwner {
    connection_epoch: u64,
    published: OutputAuthoritySnapshot,
    allocator: LiveLogicalOutputAllocator,
    active: Option<ActiveOutputCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveOutputAuthorityAdmission {
    Validated(LiveOutputAuthoritySettlement),
    Prepared,
}

impl LiveOutputAuthorityOwner {
    pub fn new(
        connection_epoch: u64,
        published: OutputAuthoritySnapshot,
    ) -> Result<Self, LiveOutputAuthorityOwnerError> {
        if connection_epoch == 0 {
            return Err(LiveOutputAuthorityOwnerError::InvalidConnectionEpoch);
        }
        published
            .validate()
            .map_err(LiveOutputAuthorityProjectionError::InvalidSnapshot)?;
        let allocator =
            LiveLogicalOutputAllocator::after(published.groups.iter().map(|group| group.output))
                .ok_or(LiveOutputAuthorityOwnerError::TopologyEpochExhausted)?;
        Ok(Self {
            connection_epoch,
            published,
            allocator,
            active: None,
        })
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub const fn published(&self) -> &OutputAuthoritySnapshot {
        &self.published
    }

    /// Replaces the externally published baseline after a hardware-owned
    /// hotplug transaction has completed its first-presentation barrier.
    /// Protocol proposals cannot race this operation: an active candidate
    /// keeps the previous snapshot authoritative until it settles.
    pub fn replace_published_snapshot(
        &mut self,
        replacement: OutputAuthoritySnapshot,
    ) -> Result<(), LiveOutputAuthorityOwnerError> {
        if self.active.is_some() {
            return Err(LiveOutputAuthorityOwnerError::ActiveCandidate);
        }
        replacement
            .validate()
            .map_err(LiveOutputAuthorityProjectionError::InvalidSnapshot)?;
        if replacement.topology_epoch < self.published.topology_epoch {
            return Err(LiveOutputAuthorityOwnerError::StalePublishedSnapshot);
        }
        if replacement.topology_epoch == self.published.topology_epoch
            && replacement != self.published
        {
            return Err(LiveOutputAuthorityOwnerError::StalePublishedSnapshot);
        }
        let allocator =
            LiveLogicalOutputAllocator::after(replacement.groups.iter().map(|group| group.output))
                .ok_or(LiveOutputAuthorityOwnerError::TopologyEpochExhausted)?;
        self.published = replacement;
        self.allocator = allocator;
        Ok(())
    }

    pub fn replace_connection_epoch(
        &mut self,
        connection_epoch: u64,
    ) -> Result<(), LiveOutputAuthorityOwnerError> {
        if connection_epoch == 0 || connection_epoch <= self.connection_epoch {
            return Err(LiveOutputAuthorityOwnerError::InvalidConnectionEpoch);
        }
        if self.active.is_some() {
            return Err(LiveOutputAuthorityOwnerError::ActiveCandidate);
        }
        self.connection_epoch = connection_epoch;
        Ok(())
    }

    pub fn admit(
        &mut self,
        transaction: TransactionId,
        proposal: &OutputV1Proposal,
        capabilities: &[LibdrmNativeOutputCapability],
    ) -> Result<LiveOutputAuthorityAdmission, LiveOutputAuthorityOwnerError> {
        if !transaction.is_valid() {
            return Err(LiveOutputAuthorityOwnerError::InvalidTransaction);
        }
        if proposal.connection_epoch != self.connection_epoch {
            return Err(LiveOutputAuthorityOwnerError::InvalidConnectionEpoch);
        }
        if self.active.is_some() {
            return Err(LiveOutputAuthorityOwnerError::ActiveCandidate);
        }
        let candidate_epoch = self
            .published
            .topology_epoch
            .checked_add(1)
            .ok_or(LiveOutputAuthorityOwnerError::TopologyEpochExhausted)?;
        let mut candidate_allocator = self.allocator.clone();
        let resolved = resolve_live_output_topology_candidate(
            &self.published,
            capabilities,
            &proposal.candidate,
            &mut candidate_allocator,
        )?;
        let candidate_snapshot = project_live_output_authority_candidate_snapshot(
            &self.published,
            &proposal.candidate,
            &resolved,
            candidate_epoch,
        )?;

        if proposal.candidate.intent == OutputTopologyIntent::ValidateOnly {
            return Ok(LiveOutputAuthorityAdmission::Validated(
                LiveOutputAuthoritySettlement {
                    transaction,
                    outcome: OutputV1Outcome {
                        connection_epoch: self.connection_epoch,
                        topology_epoch: self.published.topology_epoch,
                        kind: OutputV1OutcomeKind::Validated,
                        reason: OUTPUT_OUTCOME_REASON_NONE,
                    },
                    published_snapshot: None,
                },
            ));
        }
        let transaction_state = OutputTopologyTransaction::new(
            self.published.topology_epoch,
            candidate_epoch,
            resolved.affected_heads(),
            resolved.outputs.iter().map(|output| output.id),
        )
        .ok_or(LiveOutputAuthorityOwnerError::TransactionInvariant)?;
        self.active = Some(ActiveOutputCandidate {
            transaction,
            candidate_snapshot,
            resolved,
            candidate_allocator,
            transaction_state,
        });
        Ok(LiveOutputAuthorityAdmission::Prepared)
    }

    pub fn active_transaction(&self) -> Option<TransactionId> {
        self.active.as_ref().map(|active| active.transaction)
    }

    pub fn active_resolved(&self) -> Option<&LiveResolvedOutputTopology> {
        self.active.as_ref().map(|active| &active.resolved)
    }

    pub fn active_candidate_snapshot(&self) -> Option<&OutputAuthoritySnapshot> {
        self.active
            .as_ref()
            .map(|active| &active.candidate_snapshot)
    }

    pub fn active_phase(&self) -> Option<OutputTopologyTransactionPhase> {
        self.active
            .as_ref()
            .map(|active| active.transaction_state.phase())
    }

    pub fn active_effect(&self) -> Option<LiveOutputAuthorityEffect> {
        let active = self.active.as_ref()?;
        Some(LiveOutputAuthorityEffect {
            transaction: active.transaction,
            base_topology_epoch: active.transaction_state.base_topology_epoch(),
            candidate_topology_epoch: active.transaction_state.candidate_topology_epoch(),
            published_snapshot: self.published.clone(),
            candidate_snapshot: active.candidate_snapshot.clone(),
            resolved: active.resolved.clone(),
        })
    }

    pub fn mark_prepared(
        &mut self,
        head: RenderHeadId,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.mark_prepared(head))
    }

    /// Applies a physical observation batch transactionally. A malformed card
    /// report must not advance a prefix and leave protocol authority ahead of
    /// the native resource owner.
    pub fn mark_prepared_batch(
        &mut self,
        heads: &[RenderHeadId],
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        self.observe_head_batch(heads, OutputTopologyTransaction::mark_prepared)
    }

    pub fn begin_apply(
        &mut self,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.begin_apply())
    }

    pub fn mark_applied(
        &mut self,
        head: RenderHeadId,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.mark_applied(head))
    }

    pub fn mark_applied_batch(
        &mut self,
        heads: &[RenderHeadId],
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        self.observe_head_batch(heads, OutputTopologyTransaction::mark_applied)
    }

    pub fn mark_first_presented(
        &mut self,
        output: sophia_protocol::OutputId,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self
            .active_mut()?
            .transaction_state
            .mark_first_presented(output))
    }

    pub fn mark_first_presented_batch(
        &mut self,
        outputs: &[sophia_protocol::OutputId],
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        let active = self.active_mut()?;
        let mut next = active.transaction_state.clone();
        let mut final_transition = OutputTopologyTransactionTransition::Accepted;
        for output in outputs.iter().copied() {
            let transition = next.mark_first_presented(output);
            if !valid_observation_transition(transition) {
                return Err(LiveOutputAuthorityOwnerError::TransactionInvariant);
            }
            final_transition = transition;
        }
        active.transaction_state = next;
        Ok(final_transition)
    }

    pub fn fail(
        &mut self,
        failure: OutputTopologyTransactionFailure,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.fail(failure))
    }

    pub fn mark_rolled_back(
        &mut self,
        head: RenderHeadId,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.mark_rolled_back(head))
    }

    pub fn mark_rolled_back_batch(
        &mut self,
        heads: &[RenderHeadId],
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        self.observe_head_batch(heads, OutputTopologyTransaction::mark_rolled_back)
    }

    pub fn rollback_failed(
        &mut self,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        Ok(self.active_mut()?.transaction_state.rollback_failed())
    }

    pub fn settle_terminal(
        &mut self,
    ) -> Result<LiveOutputAuthoritySettlement, LiveOutputAuthorityOwnerError> {
        let phase = self
            .active
            .as_ref()
            .ok_or(LiveOutputAuthorityOwnerError::NoActiveCandidate)?
            .transaction_state
            .phase();
        if !matches!(
            phase,
            OutputTopologyTransactionPhase::Committed
                | OutputTopologyTransactionPhase::RolledBack
                | OutputTopologyTransactionPhase::Failed
        ) {
            return Err(LiveOutputAuthorityOwnerError::NotTerminal);
        }
        let active = self
            .active
            .take()
            .expect("terminal candidate remains active until settlement");
        let failure = active.transaction_state.failure();
        let (kind, topology_epoch, published_snapshot) = match phase {
            OutputTopologyTransactionPhase::Committed => {
                self.published = active.candidate_snapshot;
                self.allocator = active.candidate_allocator;
                (
                    OutputV1OutcomeKind::Committed,
                    self.published.topology_epoch,
                    Some(self.published.clone()),
                )
            }
            OutputTopologyTransactionPhase::RolledBack => (
                OutputV1OutcomeKind::RolledBack,
                self.published.topology_epoch,
                None,
            ),
            OutputTopologyTransactionPhase::Failed => (
                if failure == Some(OutputTopologyTransactionFailure::Rollback) {
                    OutputV1OutcomeKind::Failed
                } else {
                    OutputV1OutcomeKind::Rejected
                },
                self.published.topology_epoch,
                None,
            ),
            _ => unreachable!("nonterminal phases were rejected"),
        };
        Ok(LiveOutputAuthoritySettlement {
            transaction: active.transaction,
            outcome: OutputV1Outcome {
                connection_epoch: self.connection_epoch,
                topology_epoch,
                kind,
                reason: failure
                    .map(failure_reason)
                    .unwrap_or(OUTPUT_OUTCOME_REASON_NONE),
            },
            published_snapshot,
        })
    }

    fn active_mut(&mut self) -> Result<&mut ActiveOutputCandidate, LiveOutputAuthorityOwnerError> {
        self.active
            .as_mut()
            .ok_or(LiveOutputAuthorityOwnerError::NoActiveCandidate)
    }

    fn observe_head_batch(
        &mut self,
        heads: &[RenderHeadId],
        observe: fn(
            &mut OutputTopologyTransaction,
            RenderHeadId,
        ) -> OutputTopologyTransactionTransition,
    ) -> Result<OutputTopologyTransactionTransition, LiveOutputAuthorityOwnerError> {
        let active = self.active_mut()?;
        let mut next = active.transaction_state.clone();
        let mut final_transition = OutputTopologyTransactionTransition::Accepted;
        for head in heads.iter().copied() {
            let transition = observe(&mut next, head);
            if !valid_observation_transition(transition) {
                return Err(LiveOutputAuthorityOwnerError::TransactionInvariant);
            }
            final_transition = transition;
        }
        active.transaction_state = next;
        Ok(final_transition)
    }
}

const fn valid_observation_transition(transition: OutputTopologyTransactionTransition) -> bool {
    matches!(
        transition,
        OutputTopologyTransactionTransition::Accepted
            | OutputTopologyTransactionTransition::PhaseReady
    )
}

const fn failure_reason(failure: OutputTopologyTransactionFailure) -> u16 {
    match failure {
        OutputTopologyTransactionFailure::Stale => OUTPUT_OUTCOME_REASON_STALE,
        OutputTopologyTransactionFailure::Preparation => OUTPUT_OUTCOME_REASON_PREPARATION,
        OutputTopologyTransactionFailure::Apply => OUTPUT_OUTCOME_REASON_APPLY,
        OutputTopologyTransactionFailure::HeadLost(_) => OUTPUT_OUTCOME_REASON_HEAD_LOST,
        OutputTopologyTransactionFailure::FirstPresentation => {
            OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION
        }
        OutputTopologyTransactionFailure::Rollback => OUTPUT_OUTCOME_REASON_ROLLBACK,
        OutputTopologyTransactionFailure::Invariant => OUTPUT_OUTCOME_REASON_INVARIANT,
    }
}
