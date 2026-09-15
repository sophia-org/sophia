use super::*;

/// Candidate resources split into the ones to enable and the ones to disable.
type CandidateResourceSplit<Enabled, Disabled> = (
    Vec<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>>,
    Vec<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionSemanticStartupBarrier {
    Waiting,
    Ready,
    Invalid,
}

/// Pure admission boundary for the first semantic modeset.
///
/// A prepared framebuffer is meaningful only for a required head whose worker
/// was established first. KMS may mutate only when both sets exactly cover the
/// unique required-head set.
pub fn reduce_live_production_semantic_startup_barrier(
    required: &[sophia_engine::RenderHeadId],
    workers: &BTreeSet<sophia_engine::RenderHeadId>,
    prepared: &BTreeSet<sophia_engine::RenderHeadId>,
) -> LiveProductionSemanticStartupBarrier {
    let required_set = required.iter().copied().collect::<BTreeSet<_>>();
    if required.is_empty()
        || required_set.len() != required.len()
        || required_set.iter().any(|head| !head.is_valid())
        || !workers.is_subset(&required_set)
        || !prepared.is_subset(&required_set)
        || !prepared.is_subset(workers)
    {
        return LiveProductionSemanticStartupBarrier::Invalid;
    }
    if workers == &required_set && prepared == &required_set {
        LiveProductionSemanticStartupBarrier::Ready
    } else {
        LiveProductionSemanticStartupBarrier::Waiting
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyCurrentHead {
    pub head: sophia_engine::RenderHeadId,
    pub enabled: bool,
    pub card_index: usize,
    pub output: OutputId,
    pub selection: crate::LibdrmNativePrimaryPlaneSelection,
    pub target_generation: u64,
    pub scale: u32,
    pub refresh_millihz: u32,
    pub transform: sophia_protocol::OutputTransform,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub vrr: sophia_protocol::OutputVrrPolicy,
}

impl LiveProductionNativeTopologyCurrentHead {
    pub const fn new(
        head: sophia_engine::RenderHeadId,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
    ) -> Self {
        Self::new_with_target(
            head,
            true,
            card_index,
            output,
            selection,
            target_generation,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            sophia_protocol::OutputHeadMapping::Fit,
            sophia_protocol::OutputVrrPolicy::Disabled,
        )
    }

    pub const fn new_with_enabled(
        head: sophia_engine::RenderHeadId,
        enabled: bool,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
    ) -> Self {
        Self::new_with_target(
            head,
            enabled,
            card_index,
            output,
            selection,
            target_generation,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            sophia_protocol::OutputHeadMapping::Fit,
            sophia_protocol::OutputVrrPolicy::Disabled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub const fn new_with_target(
        head: sophia_engine::RenderHeadId,
        enabled: bool,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
        scale: u32,
        refresh_millihz: u32,
        transform: sophia_protocol::OutputTransform,
        mapping: sophia_protocol::OutputHeadMapping,
        vrr: sophia_protocol::OutputVrrPolicy,
    ) -> Self {
        Self {
            head,
            enabled,
            card_index,
            output,
            selection,
            target_generation,
            scale,
            refresh_millihz,
            transform,
            mapping,
            vrr,
        }
    }
}

/// Reduces one backend-private current head into the exact passive target that
/// Engine may plan against. Disabled heads are not render targets.
///
/// Keeping this reduction pure prevents composition from recovering a stale
/// session-global mapping or hard-coded target generation after an IPC topology
/// commit.
pub fn reduce_live_production_head_render_target(
    head: LiveProductionNativeTopologyCurrentHead,
) -> Option<sophia_engine::HeadRenderTarget> {
    head.enabled.then_some(sophia_engine::HeadRenderTarget {
        head: head.head,
        output: head.output,
        target_generation: head.target_generation,
        native_size: head.selection.size(),
        scale: head.scale,
        refresh_millihz: head.refresh_millihz,
        transform: head.transform,
        mapping: head.mapping,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyDisposition {
    Enabled {
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        scale: u32,
        refresh_millihz: u32,
        transform: sophia_protocol::OutputTransform,
        mapping: sophia_protocol::OutputHeadMapping,
        vrr: sophia_protocol::OutputVrrPolicy,
    },
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyHeadPlan {
    pub head: sophia_engine::RenderHeadId,
    pub card_index: usize,
    pub previous_output: OutputId,
    pub previous_enabled: bool,
    pub previous_selection: crate::LibdrmNativePrimaryPlaneSelection,
    pub previous_target_generation: u64,
    pub previous_scale: u32,
    pub previous_refresh_millihz: u32,
    pub previous_transform: sophia_protocol::OutputTransform,
    pub previous_mapping: sophia_protocol::OutputHeadMapping,
    pub previous_vrr: sophia_protocol::OutputVrrPolicy,
    pub candidate_target_generation: u64,
    pub disposition: LiveProductionNativeTopologyDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyPlan {
    pub primary_output: OutputId,
    pub primary_heads: BTreeMap<OutputId, sophia_engine::RenderHeadId>,
    pub outputs: Vec<sophia_engine::HeadlessOutput>,
    pub logical_viewports: Vec<crate::LiveOutputAuthorityLogicalViewport>,
    pub heads: Vec<LiveProductionNativeTopologyHeadPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyApplyPhase {
    Prepared,
    Applying,
    RollingBack,
    Applied,
    RolledBack,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveProductionNativeTopologyCard {
    card_index: usize,
    heads: Vec<sophia_engine::RenderHeadId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyApplyTransition {
    Accepted,
    Retry,
    CardApplied {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    Applied {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    RollbackRequired {
        failed_card_index: usize,
    },
    CardRolledBack {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    RolledBack {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    FailedWithoutMutation {
        card_index: usize,
    },
    RollbackFailed {
        card_index: usize,
    },
    OutOfOrder,
    Terminal,
}

/// Orders blocking card commits and reverses the accepted prefix on failure.
///
/// KMS is atomic only within one DRM card. This reducer supplies the missing
/// userspace transaction across cards: cards apply in stable index order, and a
/// later rejection rolls the accepted prefix back in reverse order. It owns no
/// DRM handles; the live executor keeps candidate and rollback resource owners
/// beside it and consumes the card named by `current_card_index()`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyApplyCoordinator {
    cards: Vec<LiveProductionNativeTopologyCard>,
    phase: LiveProductionNativeTopologyApplyPhase,
    next_apply: usize,
    applied: usize,
    rollback_remaining: usize,
}

#[derive(Debug)]
pub enum LiveProductionNativeTopologyCandidateResource<Enabled, Disabled> {
    Enabled(Enabled),
    Disabled(Disabled),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyResourceTransition {
    Accepted,
    Ready,
    Duplicate,
    UnknownHead,
    WrongDisposition,
}

#[derive(Debug)]
pub struct LiveProductionNativeTopologyResourceRejection<Owner> {
    pub transition: LiveProductionNativeTopologyResourceTransition,
    pub owner: Owner,
}

/// Affine prepare-all owner for a topology transaction.
///
/// Every affected head needs a candidate resource (an enabled framebuffer or
/// explicit disabled-head property set) and an enabled rollback framebuffer.
/// `ready()` becomes true only after both complete sets exist. This is the
/// safety boundary that prevents the cross-card coordinator from beginning an
/// irreversible prefix with no resource capable of restoring it.
#[derive(Debug)]
pub struct LiveProductionNativeTopologyResourceCohort<Enabled, Disabled> {
    expected: BTreeMap<
        sophia_engine::RenderHeadId,
        (usize, LiveProductionNativeTopologyDisposition, bool),
    >,
    candidate: BTreeMap<
        sophia_engine::RenderHeadId,
        LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>,
    >,
    rollback: BTreeMap<
        sophia_engine::RenderHeadId,
        LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>,
    >,
}

type LiveProductionPreparedTopologyHead =
    crate::LivePreparedRenderedTopologyHead<crate::NativeGbmRenderedScanoutOwner>;

type LiveProductionNativeTopologyResources = LiveProductionNativeTopologyResourceCohort<
    LiveProductionPreparedTopologyHead,
    crate::LibdrmNativePreparedDisabledTopologyHead,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyPreparationPhase {
    PreparingCandidate,
    PreparingRollback,
    Prepared,
    Applying,
    RollingBack,
    Applied,
    CandidateInstalled,
    FirstFramesQueued,
    RolledBack,
    Aborting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyPreparationReport {
    pub phase: LiveProductionNativeTopologyPreparationPhase,
    pub candidate_prepared: usize,
    pub rollback_prepared: usize,
    pub affected_heads: usize,
}

#[derive(Debug)]
pub(super) struct LiveProductionNativeTopologyPreparation {
    plan: LiveProductionNativeTopologyPlan,
    rollback: crate::LiveResolvedOutputTopology,
    resources: LiveProductionNativeTopologyResources,
    rollback_frames:
        BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    apply: LiveProductionNativeTopologyApplyCoordinator,
    phase: LiveProductionNativeTopologyPreparationPhase,
    failure: Option<String>,
}

struct LiveProductionNativeInstalledHead {
    index: usize,
    enabled: bool,
    output: OutputId,
    selection: crate::LibdrmNativePrimaryPlaneSelection,
    target_generation: u64,
    scale: u32,
    refresh_millihz: u32,
    transform: sophia_protocol::OutputTransform,
    mapping: sophia_protocol::OutputHeadMapping,
    vrr: sophia_protocol::OutputVrrPolicy,
    output_frames: OutputFramePresentationState,
}

impl<Enabled, Disabled> LiveProductionNativeTopologyResourceCohort<Enabled, Disabled> {
    pub fn new(plan: &LiveProductionNativeTopologyPlan) -> Option<Self> {
        let expected = plan
            .heads
            .iter()
            .map(|head| {
                (
                    head.head,
                    (head.card_index, head.disposition, head.previous_enabled),
                )
            })
            .collect::<BTreeMap<_, _>>();
        (expected.len() == plan.heads.len() && !expected.is_empty()).then_some(Self {
            expected,
            candidate: BTreeMap::new(),
            rollback: BTreeMap::new(),
        })
    }

    pub fn prepare_candidate_enabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        let Some((_, disposition, _)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !matches!(
            disposition,
            LiveProductionNativeTopologyDisposition::Enabled { .. }
        ) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.candidate.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.candidate.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Enabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_candidate_disabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Disabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Disabled>,
    > {
        let Some((_, disposition, _)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !matches!(
            disposition,
            LiveProductionNativeTopologyDisposition::Disabled
        ) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.candidate.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.candidate.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Disabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_rollback(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        self.prepare_rollback_enabled(head, owner)
    }

    pub fn prepare_rollback_enabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Enabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Enabled>,
    > {
        let Some((_, _, previous_enabled)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if !previous_enabled {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.rollback.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.rollback.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Enabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn prepare_rollback_disabled(
        &mut self,
        head: sophia_engine::RenderHeadId,
        owner: Disabled,
    ) -> Result<
        LiveProductionNativeTopologyResourceTransition,
        LiveProductionNativeTopologyResourceRejection<Disabled>,
    > {
        let Some((_, _, previous_enabled)) = self.expected.get(&head) else {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::UnknownHead,
                owner,
            });
        };
        if *previous_enabled {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::WrongDisposition,
                owner,
            });
        }
        if self.rollback.contains_key(&head) {
            return Err(LiveProductionNativeTopologyResourceRejection {
                transition: LiveProductionNativeTopologyResourceTransition::Duplicate,
                owner,
            });
        }
        self.rollback.insert(
            head,
            LiveProductionNativeTopologyCandidateResource::Disabled(owner),
        );
        Ok(self.accepted_transition())
    }

    pub fn ready(&self) -> bool {
        self.candidate.len() == self.expected.len() && self.rollback.len() == self.expected.len()
    }

    pub fn candidate_count(&self) -> usize {
        self.candidate.len()
    }

    pub fn rollback_count(&self) -> usize {
        self.rollback.len()
    }

    pub fn card_heads(&self, card_index: usize) -> Vec<sophia_engine::RenderHeadId> {
        self.expected
            .iter()
            .filter_map(|(head, (card, _, _))| (*card == card_index).then_some(*head))
            .collect()
    }

    pub fn candidate(
        &self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<&LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.candidate.get(&head)
    }

    pub fn rollback(
        &self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<&LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.rollback.get(&head)
    }

    pub fn take_candidate(
        &mut self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.candidate.remove(&head)
    }

    pub fn take_rollback(
        &mut self,
        head: sophia_engine::RenderHeadId,
    ) -> Option<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>> {
        self.rollback.remove(&head)
    }

    pub fn into_remaining(self) -> CandidateResourceSplit<Enabled, Disabled> {
        (
            self.candidate.into_values().collect(),
            self.rollback.into_values().collect(),
        )
    }

    fn accepted_transition(&self) -> LiveProductionNativeTopologyResourceTransition {
        if self.ready() {
            LiveProductionNativeTopologyResourceTransition::Ready
        } else {
            LiveProductionNativeTopologyResourceTransition::Accepted
        }
    }
}

impl LiveProductionNativeTopologyApplyCoordinator {
    pub fn new(plan: &LiveProductionNativeTopologyPlan) -> Option<Self> {
        let mut by_card = BTreeMap::<usize, Vec<sophia_engine::RenderHeadId>>::new();
        for head in &plan.heads {
            by_card.entry(head.card_index).or_default().push(head.head);
        }
        if by_card.is_empty() {
            return None;
        }
        let cards = by_card
            .into_iter()
            .map(|(card_index, mut heads)| {
                heads.sort();
                heads.dedup();
                LiveProductionNativeTopologyCard { card_index, heads }
            })
            .collect::<Vec<_>>();
        if cards.iter().any(|card| card.heads.is_empty())
            || cards.iter().map(|card| card.heads.len()).sum::<usize>() != plan.heads.len()
        {
            return None;
        }
        Some(Self {
            cards,
            phase: LiveProductionNativeTopologyApplyPhase::Prepared,
            next_apply: 0,
            applied: 0,
            rollback_remaining: 0,
        })
    }

    pub const fn phase(&self) -> LiveProductionNativeTopologyApplyPhase {
        self.phase
    }

    pub fn current_card_index(&self) -> Option<usize> {
        match self.phase {
            LiveProductionNativeTopologyApplyPhase::Applying => {
                self.cards.get(self.next_apply).map(|card| card.card_index)
            }
            LiveProductionNativeTopologyApplyPhase::RollingBack => self
                .rollback_remaining
                .checked_sub(1)
                .and_then(|index| self.cards.get(index))
                .map(|card| card.card_index),
            _ => None,
        }
    }

    pub fn current_heads(&self) -> &[sophia_engine::RenderHeadId] {
        match self.phase {
            LiveProductionNativeTopologyApplyPhase::Applying => self
                .cards
                .get(self.next_apply)
                .map_or(&[], |card| card.heads.as_slice()),
            LiveProductionNativeTopologyApplyPhase::RollingBack => self
                .rollback_remaining
                .checked_sub(1)
                .and_then(|index| self.cards.get(index))
                .map_or(&[], |card| card.heads.as_slice()),
            _ => &[],
        }
    }

    pub fn begin_apply(&mut self) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Prepared {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::Applying;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    /// Starts a full reverse-card rollback after every candidate card applied.
    ///
    /// Runtime reconstruction and first presentation remain fallible after the
    /// blocking modesets succeed, so a terminal `Applied` coordinator must
    /// retain a route back to the published topology.
    pub fn begin_rollback_after_apply(&mut self) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applied
            || self.applied != self.cards.len()
        {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
        self.rollback_remaining = self.applied;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    pub fn begin_rollback_after_partial_apply(
        &mut self,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applying
            || self.applied == 0
            || self.applied >= self.cards.len()
        {
            return self.out_of_order();
        }
        self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
        self.rollback_remaining = self.applied;
        LiveProductionNativeTopologyApplyTransition::Accepted
    }

    pub fn observe_apply(
        &mut self,
        card_index: usize,
        outcome: crate::NativeTopologySubmitOutcome,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::Applying
            || self.current_card_index() != Some(card_index)
        {
            return self.out_of_order();
        }
        if outcome == crate::NativeTopologySubmitOutcome::Busy {
            return LiveProductionNativeTopologyApplyTransition::Retry;
        }
        if outcome != crate::NativeTopologySubmitOutcome::Accepted {
            if self.applied == 0 {
                self.phase = LiveProductionNativeTopologyApplyPhase::Failed;
                return LiveProductionNativeTopologyApplyTransition::FailedWithoutMutation {
                    card_index,
                };
            }
            self.phase = LiveProductionNativeTopologyApplyPhase::RollingBack;
            self.rollback_remaining = self.applied;
            return LiveProductionNativeTopologyApplyTransition::RollbackRequired {
                failed_card_index: card_index,
            };
        }

        let card = &self.cards[self.next_apply];
        let transition = if self.next_apply + 1 == self.cards.len() {
            self.phase = LiveProductionNativeTopologyApplyPhase::Applied;
            LiveProductionNativeTopologyApplyTransition::Applied {
                card_index,
                heads: card.heads.clone(),
            }
        } else {
            LiveProductionNativeTopologyApplyTransition::CardApplied {
                card_index,
                heads: card.heads.clone(),
            }
        };
        self.next_apply += 1;
        self.applied += 1;
        transition
    }

    pub fn observe_rollback(
        &mut self,
        card_index: usize,
        outcome: crate::NativeTopologySubmitOutcome,
    ) -> LiveProductionNativeTopologyApplyTransition {
        if self.phase != LiveProductionNativeTopologyApplyPhase::RollingBack
            || self.current_card_index() != Some(card_index)
        {
            return self.out_of_order();
        }
        if outcome == crate::NativeTopologySubmitOutcome::Busy {
            return LiveProductionNativeTopologyApplyTransition::Retry;
        }
        if outcome != crate::NativeTopologySubmitOutcome::Accepted {
            self.phase = LiveProductionNativeTopologyApplyPhase::Failed;
            return LiveProductionNativeTopologyApplyTransition::RollbackFailed { card_index };
        }
        let card = &self.cards[self.rollback_remaining - 1];
        self.rollback_remaining -= 1;
        if self.rollback_remaining == 0 {
            self.phase = LiveProductionNativeTopologyApplyPhase::RolledBack;
            LiveProductionNativeTopologyApplyTransition::RolledBack {
                card_index,
                heads: card.heads.clone(),
            }
        } else {
            LiveProductionNativeTopologyApplyTransition::CardRolledBack {
                card_index,
                heads: card.heads.clone(),
            }
        }
    }

    fn out_of_order(&self) -> LiveProductionNativeTopologyApplyTransition {
        if matches!(
            self.phase,
            LiveProductionNativeTopologyApplyPhase::Applied
                | LiveProductionNativeTopologyApplyPhase::RolledBack
                | LiveProductionNativeTopologyApplyPhase::Failed
        ) {
            LiveProductionNativeTopologyApplyTransition::Terminal
        } else {
            LiveProductionNativeTopologyApplyTransition::OutOfOrder
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyPlanError {
    Empty,
    DuplicateCurrentHead(sophia_engine::RenderHeadId),
    DuplicateCandidateHead(sophia_engine::RenderHeadId),
    MissingCurrentHead(sophia_engine::RenderHeadId),
    MissingCandidateHead(sophia_engine::RenderHeadId),
    InvalidOutput(OutputId),
    InvalidPrimaryHead(OutputId),
    InvalidGeneration(sophia_engine::RenderHeadId),
    ModeUnavailable(sophia_engine::RenderHeadId),
    PublishedSnapshotMismatch,
    Native(String),
}

impl core::fmt::Display for LiveProductionNativeTopologyPlanError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveProductionNativeTopologyPlanError {}

/// Binds a resolved authority candidate to the live session's existing native
/// objects without changing any output, exporter, runtime, or KMS state.
///
/// `resolve_mode` is the only hardware read. Keeping it injected makes the
/// projection deterministic in tests and makes the ownership boundary explicit:
/// the returned plan contains copied handles, never a second DRM owner.
pub fn plan_live_production_native_topology(
    current: &[LiveProductionNativeTopologyCurrentHead],
    resolved: &crate::LiveResolvedOutputTopology,
    mut resolve_mode: impl FnMut(
        LiveProductionNativeTopologyCurrentHead,
        crate::LibdrmNativeOutputTiming,
    ) -> Result<
        Option<::drm::control::Mode>,
        LiveProductionNativeTopologyPlanError,
    >,
) -> Result<LiveProductionNativeTopologyPlan, LiveProductionNativeTopologyPlanError> {
    if current.is_empty() || resolved.outputs.is_empty() {
        return Err(LiveProductionNativeTopologyPlanError::Empty);
    }
    let mut current_by_head = BTreeMap::new();
    for current in current.iter().copied() {
        if current_by_head.insert(current.head, current).is_some() {
            return Err(LiveProductionNativeTopologyPlanError::DuplicateCurrentHead(
                current.head,
            ));
        }
    }
    let output_ids = resolved
        .outputs
        .iter()
        .map(|output| output.id)
        .collect::<BTreeSet<_>>();
    if output_ids.len() != resolved.outputs.len()
        || resolved.logical_viewports.len() != resolved.outputs.len()
        || !output_ids.contains(&resolved.primary_output)
        || resolved.outputs.iter().any(|output| {
            !output.id.is_valid()
                || output.size.width <= 0
                || output.size.height <= 0
                || output.scale == 0
        })
    {
        return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
            resolved.primary_output,
        ));
    }
    for (output, viewport) in resolved.outputs.iter().zip(&resolved.logical_viewports) {
        if viewport.output != output.id
            || viewport.logical.width != output.size.width
            || viewport.logical.height != output.size.height
        {
            return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
                output.id,
            ));
        }
    }

    let enabled = resolved
        .targets
        .iter()
        .map(|target| (target.head, target))
        .collect::<BTreeMap<_, _>>();
    if enabled.len() != resolved.targets.len() {
        let duplicate = resolved
            .targets
            .iter()
            .map(|target| target.head)
            .find(|head| {
                resolved
                    .targets
                    .iter()
                    .filter(|target| target.head == *head)
                    .count()
                    > 1
            })
            .expect("a shorter map proves a duplicate");
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(duplicate));
    }
    if resolved.primary_heads.len() != output_ids.len()
        || resolved
            .primary_heads
            .keys()
            .any(|output| !output_ids.contains(output))
    {
        return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
            resolved.primary_output,
        ));
    }
    for output in &output_ids {
        let Some(primary) = resolved.primary_heads.get(output) else {
            return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
                *output,
            ));
        };
        if enabled.get(primary).map(|target| target.output) != Some(*output) {
            return Err(LiveProductionNativeTopologyPlanError::InvalidPrimaryHead(
                *output,
            ));
        }
    }
    let disabled = resolved
        .disabled_heads
        .iter()
        .map(|disabled| (disabled.head, disabled))
        .collect::<BTreeMap<_, _>>();
    if disabled.len() != resolved.disabled_heads.len() {
        let duplicate = resolved
            .disabled_heads
            .iter()
            .map(|disabled| disabled.head)
            .find(|head| {
                resolved
                    .disabled_heads
                    .iter()
                    .filter(|disabled| disabled.head == *head)
                    .count()
                    > 1
            })
            .expect("a shorter map proves a duplicate");
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(duplicate));
    }
    if let Some(head) = enabled.keys().find(|head| disabled.contains_key(head)) {
        return Err(LiveProductionNativeTopologyPlanError::DuplicateCandidateHead(*head));
    }
    for head in enabled.keys().chain(disabled.keys()) {
        if !current_by_head.contains_key(head) {
            return Err(LiveProductionNativeTopologyPlanError::MissingCurrentHead(
                *head,
            ));
        }
    }
    if let Some(head) = current_by_head
        .keys()
        .find(|head| !enabled.contains_key(head) && !disabled.contains_key(head))
    {
        return Err(LiveProductionNativeTopologyPlanError::MissingCandidateHead(
            *head,
        ));
    }

    let mut heads = Vec::with_capacity(current.len());
    for current in current.iter().copied() {
        let (candidate_target_generation, disposition) =
            if let Some(target) = enabled.get(&current.head) {
                if !output_ids.contains(&target.output)
                    || target.native_size.width <= 0
                    || target.native_size.height <= 0
                {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidOutput(
                        target.output,
                    ));
                }
                let expected_generation = current.target_generation.checked_add(1).ok_or(
                    LiveProductionNativeTopologyPlanError::InvalidGeneration(current.head),
                )?;
                if target.target_generation != expected_generation {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidGeneration(
                        current.head,
                    ));
                }
                let mode = resolve_mode(current, target.timing)?.ok_or(
                    LiveProductionNativeTopologyPlanError::ModeUnavailable(current.head),
                )?;
                let mut selection = crate::LibdrmNativePrimaryPlaneSelection::new(
                    current.selection.connector_handle(),
                    current.selection.crtc_handle(),
                    current.selection.plane_handle(),
                    target.native_size,
                    Some(mode),
                );
                // An output-policy transaction changes mode and logical
                // binding; it does not rediscover the fixed KMS route. Keep
                // the cursor plane selected beside that route. Dropping it
                // here would leave an already-admitted atomic cursor path
                // without a plane immediately after the topology commits.
                if let Some(cursor) = current.selection.cursor_plane() {
                    selection = selection.with_cursor_plane(cursor);
                }
                (
                    target.target_generation,
                    LiveProductionNativeTopologyDisposition::Enabled {
                        output: target.output,
                        selection,
                        scale: resolved
                            .outputs
                            .iter()
                            .find(|output| output.id == target.output)
                            .expect("candidate output identity was validated")
                            .scale,
                        refresh_millihz: target.timing.refresh_millihz,
                        transform: target.transform,
                        mapping: target.mapping,
                        vrr: target.vrr,
                    },
                )
            } else {
                let disabled = disabled[&current.head];
                let expected_generation = current.target_generation.checked_add(1).ok_or(
                    LiveProductionNativeTopologyPlanError::InvalidGeneration(current.head),
                )?;
                if disabled.target_generation != expected_generation {
                    return Err(LiveProductionNativeTopologyPlanError::InvalidGeneration(
                        current.head,
                    ));
                }
                (
                    disabled.target_generation,
                    LiveProductionNativeTopologyDisposition::Disabled,
                )
            };
        heads.push(LiveProductionNativeTopologyHeadPlan {
            head: current.head,
            card_index: current.card_index,
            previous_output: current.output,
            previous_enabled: current.enabled,
            previous_selection: current.selection,
            previous_target_generation: current.target_generation,
            previous_scale: current.scale,
            previous_refresh_millihz: current.refresh_millihz,
            previous_transform: current.transform,
            previous_mapping: current.mapping,
            previous_vrr: current.vrr,
            candidate_target_generation,
            disposition,
        });
    }
    Ok(LiveProductionNativeTopologyPlan {
        primary_output: resolved.primary_output,
        primary_heads: resolved.primary_heads.clone(),
        outputs: resolved.outputs.clone(),
        logical_viewports: resolved.logical_viewports.clone(),
        heads,
    })
}

/// Projects the published authority snapshot back into native render targets.
///
/// This is the rollback-side twin of candidate resolution. It deliberately
/// joins logical state from the published snapshot with physical size and
/// generation from the live head owner, preventing a provisional candidate
/// from contaminating the rollback image set.
pub fn project_live_production_published_topology(
    current: &[LiveProductionNativeTopologyCurrentHead],
    snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    mut selected_timing: impl FnMut(
        LiveProductionNativeTopologyCurrentHead,
    ) -> Result<
        crate::LibdrmNativeOutputTiming,
        LiveProductionNativeTopologyPlanError,
    >,
) -> Result<crate::LiveResolvedOutputTopology, LiveProductionNativeTopologyPlanError> {
    snapshot
        .validate()
        .map_err(|_| LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
    let mut output_by_head = BTreeMap::new();
    let mut mapping_by_head = BTreeMap::new();
    for group in &snapshot.groups {
        for member in &group.members {
            let head = sophia_engine::RenderHeadId::from_raw(member.head.raw());
            if output_by_head.insert(head, group.output).is_some()
                || mapping_by_head.insert(head, member.mapping).is_some()
            {
                return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
            }
        }
    }
    let descriptor_by_head = snapshot
        .heads
        .iter()
        .map(|head| (sophia_engine::RenderHeadId::from_raw(head.head.raw()), head))
        .collect::<BTreeMap<_, _>>();
    if descriptor_by_head.len() != snapshot.heads.len() || descriptor_by_head.len() != current.len()
    {
        return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
    }

    let mut targets = Vec::with_capacity(current.len());
    let mut disabled_heads = Vec::new();
    for native in current.iter().copied() {
        let descriptor = descriptor_by_head
            .get(&native.head)
            .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
        if !descriptor.connected
            || descriptor.enabled != native.enabled
            || descriptor.generation != native.target_generation
        {
            return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
        }
        if !native.enabled {
            if output_by_head.contains_key(&native.head) || descriptor.current_mode.is_some() {
                return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
            }
            disabled_heads.push(crate::LiveOutputAuthorityDisabledHead {
                head: native.head,
                target_generation: native.target_generation,
            });
            continue;
        }
        let output = output_by_head
            .get(&native.head)
            .copied()
            .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)?;
        let timing = selected_timing(native)?;
        if output != native.output
            || mapping_by_head.get(&native.head).copied() != Some(native.mapping)
            || u32::try_from(native.selection.size().width).ok() != Some(timing.width)
            || u32::try_from(native.selection.size().height).ok() != Some(timing.height)
            || timing.refresh_millihz != native.refresh_millihz
        {
            return Err(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch);
        }
        targets.push(crate::LiveOutputAuthorityHeadTarget {
            head: native.head,
            target_generation: native.target_generation,
            output,
            timing,
            native_size: native.selection.size(),
            transform: native.transform,
            mapping: native.mapping,
            vrr: native.vrr,
        });
    }
    let logical_viewports = snapshot
        .groups
        .iter()
        .map(|group| crate::LiveOutputAuthorityLogicalViewport {
            output: group.output,
            logical: group.logical,
        })
        .collect::<Vec<_>>();
    let outputs = snapshot
        .groups
        .iter()
        .map(|group| sophia_engine::HeadlessOutput {
            id: group.output,
            size: sophia_protocol::Size {
                width: group.logical.width,
                height: group.logical.height,
            },
            scale: 1,
        })
        .collect::<Vec<_>>();
    Ok(crate::LiveResolvedOutputTopology {
        primary_output: snapshot.primary_output,
        primary_heads: snapshot
            .groups
            .iter()
            .filter_map(|group| {
                group.members.first().map(|member| {
                    (
                        group.output,
                        sophia_engine::RenderHeadId::from_raw(member.head.raw()),
                    )
                })
            })
            .collect(),
        outputs,
        logical_viewports,
        disabled_heads,
        targets,
        // Connector grouping is a discovery/configuration input. Rendering
        // rollback targets needs only the already-resolved opaque members.
        mirror_grouping: crate::NativeMirrorGrouping::none(),
    })
}

impl LiveProductionNativeScanout {
    /// Establishes the first displayed generation of a logical output from
    /// independently lowered semantic frames.
    ///
    /// Renderer workers are enabled before any export. Every framebuffer and
    /// modeset property owner is then prepared without KMS mutation. Because a
    /// mirror group is card-local, one blocking card-scoped atomic commit makes
    /// the complete set visible; no head can expose a prepared prefix.
    pub(super) fn initialize_semantic_head_transaction(
        &mut self,
        output: OutputId,
        runtime: &mut crate::LiveBackendRuntimeAssembly,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        let indices = self.head_indices(output);
        if indices.is_empty() {
            return Err("semantic startup requires at least one head".into());
        }
        if indices.iter().any(|index| {
            let custody = &self.heads[*index].scanout_custody;
            !custody.can_adopt_displayed() || custody.cleanup_pending()
        }) {
            return Err("semantic startup found pre-existing native head custody".into());
        }
        let singleton = indices.len() == 1;
        if singleton
            && (runtime.rendered_primary_plane_scanout_displayed()
                || runtime.rendered_primary_plane_scanout_in_flight()
                || runtime.rendered_primary_plane_scanout_cleanup_pending())
        {
            return Err("semantic singleton startup found pre-existing runtime ownership".into());
        }
        let group = self.heads[indices[0]].group;
        if indices
            .iter()
            .any(|index| self.heads[*index].group != group)
        {
            return Err("one mirrored logical output cannot span DRM cards".into());
        }
        let identities = frames
            .iter()
            .map(|frame| {
                (
                    frame.head,
                    (
                        frame.scene_generation,
                        frame.target_generation,
                        frame.mapping,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let frame = self.queue_initial_head_composition_frames(output, frames)?;
        let required = indices
            .iter()
            .map(|index| self.heads[*index].head)
            .collect::<Vec<_>>();
        let mut workers = BTreeSet::new();
        for index in indices.iter().copied() {
            self.enable_head_renderer_worker(index)?;
            if !self.exporters[index].worker_enabled() {
                return Err("semantic startup renderer worker was not established".into());
            }
            // `workers` counts renderer threads this head can reach, which is
            // one whether it owns the thread or shares its group's. How many
            // threads the session actually runs is a session-wide fact and is
            // reported as one, in the resource record.
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=worker_ready output={} head={} workers=1",
                output.raw(),
                self.heads[index].head.raw(),
            );
            workers.insert(self.heads[index].head);
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut prepared =
            BTreeMap::<sophia_engine::RenderHeadId, LiveProductionPreparedTopologyHead>::new();
        loop {
            for index in indices.iter().copied() {
                let head = self.heads[index].head;
                if prepared.contains_key(&head) {
                    continue;
                }
                let selection = self.heads[index].selection;
                let vrr_enabled = topology_vrr_enabled(self.heads[index].vrr);
                match self.prepare_output_topology_head(head, selection, vrr_enabled) {
                    Ok(Some(owner)) => {
                        prepared.insert(head, owner);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.cancel_semantic_startup_resources(group, prepared);
                        return Err(error);
                    }
                }
            }
            if prepared.len() == indices.len() {
                break;
            }
            if Instant::now() >= deadline {
                self.cancel_semantic_startup_resources(group, prepared);
                return Err("semantic multi-head startup renderer deadline expired".into());
            }
            std::thread::yield_now();
        }

        let prepared_heads = prepared.keys().copied().collect::<BTreeSet<_>>();
        if reduce_live_production_semantic_startup_barrier(&required, &workers, &prepared_heads)
            != LiveProductionSemanticStartupBarrier::Ready
        {
            self.cancel_semantic_startup_resources(group, prepared);
            return Err("semantic multi-head startup prepare barrier was incomplete".into());
        }

        let changes = indices
            .iter()
            .map(|index| {
                let owner = prepared
                    .get(&self.heads[*index].head)
                    .expect("semantic startup prepared complete head coverage");
                crate::LibdrmNativeAtomicTopologyChange::Enabled(owner.atomic_head())
            })
            .collect::<Vec<_>>();
        loop {
            match crate::submit_native_topology_change_on_device(
                self.groups[group].session.card(),
                &changes,
            ) {
                crate::NativeTopologySubmitOutcome::Accepted => break,
                crate::NativeTopologySubmitOutcome::Busy if Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                outcome => {
                    self.cancel_semantic_startup_resources(group, prepared);
                    return Err(format!(
                        "semantic multi-head startup atomic commit failed: {outcome:?}"
                    )
                    .into());
                }
            }
        }

        let mut adoption_errors = Vec::new();
        for index in indices {
            let head_id = self.heads[index].head;
            let owner = prepared
                .remove(&head_id)
                .expect("accepted semantic startup retained every head owner");
            let displayed = crate::adopt_prepared_rendered_topology_head_after_commit(owner);
            if singleton {
                if let Err(displayed) =
                    runtime.try_adopt_presented_rendered_primary_plane_scanout(displayed)
                {
                    self.heads[index]
                        .scanout_custody
                        .adopt_displayed(
                            displayed.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }),
                        )
                        .expect("startup fallback custody prevalidated before commit");
                    adoption_errors.push(
                        "semantic singleton startup runtime rejected its displayed owner"
                            .to_owned(),
                    );
                }
            } else {
                self.heads[index]
                    .scanout_custody
                    .adopt_displayed(
                        displayed
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    )
                    .expect("startup head custody prevalidated before commit");
            }
            let exported_nonzero = self.exporters[index].composition_nonzero_rgb_pixels() > 0;
            if exported_nonzero {
                self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                self.heads[index].nonzero_exports =
                    self.heads[index].nonzero_exports.saturating_add(1);
            }
            self.submissions = self.submissions.saturating_add(1);
            trace_live_native_lifecycle("initial_modeset_complete");
            let head = &mut self.heads[index];
            head.submissions = head.submissions.saturating_add(1);
            head.presented_logical_checksum = head.last_checksum;
            head.presented_submissions = head.submissions;
            head.presented_content = head.pending_content.take();
            if head.output_frames.pending().is_some() {
                match head.output_frames.mark_initial_presented() {
                    Ok(presented) => {
                        trace_presented_output_damage(
                            "initial_presented",
                            head.output.id,
                            &presented,
                        );
                    }
                    Err(error) => adoption_errors.push(format!(
                        "initial compositor display-list transition failed for head {}: {error}",
                        head_id.raw(),
                    )),
                }
            }
            head.initial_modeset_submission = Some(head.submissions);
            let transition = self
                .output_lifecycles
                .get_mut(&output)
                .expect("a registered output has a head lifecycle")
                .mark_initialized(head_id);
            if !matches!(
                transition,
                LiveProductionMirrorHeadTransition::Accepted
                    | LiveProductionMirrorHeadTransition::GroupReady
            ) {
                adoption_errors.push(format!(
                    "semantic startup lifecycle rejected initialized head {}: {transition:?}",
                    head_id.raw(),
                ));
            }
            let (scene_generation, target_generation, mapping) = identities
                .get(&head_id)
                .copied()
                .expect("semantic startup retained head plan identity");
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=worker_composed output={} head={} frame={} scene_generation={} target_generation={} mapping={} exports=1",
                output.raw(),
                head_id.raw(),
                frame.raw(),
                scene_generation,
                target_generation,
                mapping.reduced_name(),
            );
        }
        if !prepared.is_empty() {
            return Err("semantic startup retained an unadopted head owner".into());
        }
        if !adoption_errors.is_empty() {
            return Err(format!(
                "semantic startup adoption failed after retaining every KMS owner: {}",
                adoption_errors.join("; "),
            )
            .into());
        }
        Ok(frame)
    }

    fn cancel_semantic_startup_resources(
        &mut self,
        group: usize,
        prepared: BTreeMap<sophia_engine::RenderHeadId, LiveProductionPreparedTopologyHead>,
    ) {
        for (head, owner) in prepared {
            let cancelled = crate::cancel_prepared_rendered_topology_head(
                self.groups[group].session.card(),
                owner,
            );
            if let Some(cleanup) = cancelled.cleanup {
                let cleanup =
                    cleanup.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>);
                if let Some(index) = self.head_index_for_head(head) {
                    if let Err(cleanup) = self.heads[index].scanout_custody.accept_cleanup(cleanup)
                    {
                        self.output_topology_cleanup.push((head, cleanup));
                    }
                } else {
                    self.output_topology_cleanup.push((head, cleanup));
                }
            }
            if cancelled.destroy != crate::LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed
            {
                self.retire_failures = self.retire_failures.saturating_add(1);
            }
        }
    }

    /// Releases semantic startup work that never reached KMS.
    ///
    /// A worker command is affine even before framebuffer preparation. It must
    /// be polled to a terminal owner and dropped; merely clearing the head's
    /// passive bookkeeping would detach that renderer lease from cleanup.
    pub(super) fn abort_semantic_startup_head_work(
        &mut self,
        output: OutputId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let indices = self.head_indices(output);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut pending = false;
            for index in indices.iter().copied() {
                if !self.exporters[index].pending_frame() {
                    continue;
                }
                if !self.exporters[index].worker_in_flight() {
                    self.exporters[index].discard_pending_frame();
                    continue;
                }
                let selection = self.heads[index].selection;
                let export =
                    crate::LiveRenderedScanoutBufferExporter::export_rendered_scanout_buffer(
                        &mut self.exporters[index],
                        crate::LiveGbmEglFrameTargetRecord::new(selection.size()),
                    );
                pending |= export.status == crate::LiveRendererScanoutBufferExportStatus::Pending;
                // A terminal worker export has not acquired DRM ownership yet;
                // dropping it here returns the renderer lease to its worker.
                drop(export);
            }
            if !pending
                && indices
                    .iter()
                    .all(|index| !self.exporters[*index].pending_frame())
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("semantic startup renderer abort deadline expired".into());
            }
            std::thread::yield_now();
        }
        for index in indices {
            let head = &mut self.heads[index];
            head.pending_content = None;
            head.rendering_content = None;
            head.output_frames.discard_pending();
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=aborted output={} head={} renderer_pending=0",
                output.raw(),
                head.head.raw(),
            );
        }
        Ok(())
    }

    /// Resolves a provisional IPC topology against the live DRM master without
    /// mutating the currently published head table or any scanout ownership.
    pub fn plan_output_topology(
        &self,
        resolved: &crate::LiveResolvedOutputTopology,
    ) -> Result<LiveProductionNativeTopologyPlan, LiveProductionNativeTopologyPlanError> {
        let current = self
            .heads
            .iter()
            .map(|head| {
                LiveProductionNativeTopologyCurrentHead::new_with_target(
                    head.head,
                    head.enabled,
                    head.group,
                    head.output.id,
                    head.selection,
                    head.target_generation,
                    head.scale,
                    head.refresh_millihz,
                    head.transform,
                    head.mapping,
                    head.vrr,
                )
            })
            .collect::<Vec<_>>();
        plan_live_production_native_topology(&current, resolved, |current, timing| {
            crate::resolve_native_connector_mode(
                self.groups[current.card_index].session.card(),
                current.selection.connector_handle(),
                timing,
            )
            .map_err(|error| LiveProductionNativeTopologyPlanError::Native(error.to_string()))
        })
    }

    /// Reconstructs the still-published topology as render targets for a
    /// rollback pool. This never consults the provisional candidate: logical
    /// positions come from the published authority snapshot, while native sizes
    /// and generations come from the live head owner.
    pub fn published_output_topology(
        &self,
        snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    ) -> Result<crate::LiveResolvedOutputTopology, LiveProductionNativeTopologyPlanError> {
        let capabilities = self
            .output_capabilities()
            .map_err(|error| LiveProductionNativeTopologyPlanError::Native(error.to_string()))?;
        let capability_by_head = capabilities
            .iter()
            .filter_map(|capability| capability.head().map(|head| (head, capability)))
            .collect::<BTreeMap<_, _>>();
        let current = self
            .heads
            .iter()
            .map(|head| {
                LiveProductionNativeTopologyCurrentHead::new_with_target(
                    head.head,
                    head.enabled,
                    head.group,
                    head.output.id,
                    head.selection,
                    head.target_generation,
                    head.scale,
                    head.refresh_millihz,
                    head.transform,
                    head.mapping,
                    head.vrr,
                )
            })
            .collect::<Vec<_>>();
        project_live_production_published_topology(&current, snapshot, |native| {
            capability_by_head
                .get(&native.head)
                .map(|capability| capability.selected_mode())
                .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)
        })
    }

    pub fn output_topology_preparation_active(&self) -> bool {
        self.output_topology_preparation.is_some()
    }

    /// Reports whether every ordinary frame/resource owner has retired. The
    /// session owner uses this before transferring scheduling authority to an
    /// output-topology transaction.
    /// Whether the scanout may *begin* a topology preparation.
    ///
    /// Deliberately not the same question as the runtime's
    /// `topology_rebind_quiescent`, despite the similar name, and the two must
    /// not be collapsed into one predicate. This one is false for the whole of
    /// an installed candidate, because `install_applied_output_topology`
    /// restores the preparation with phase `CandidateInstalled` -- and the
    /// runtime's rebind runs at exactly that moment. An AND of the two enforced
    /// at the rebind would reject every candidate installation. The owner's
    /// pre-apply wait ANDs them only because it precedes both.
    ///
    /// Queued work that has not crossed into a renderer worker is deliberately
    /// not counted. Installation discards exactly that state itself -- it
    /// refuses only `worker_in_flight` and then calls `discard_pending_frame`
    /// -- so requiring it to be absent first was stricter than the transition
    /// needs. It was also unreachable: nothing drains a queued frame while the
    /// owner is quarantined waiting on this predicate, so the wait could only
    /// end by expiring. What must still retire is everything holding a renderer
    /// lease or an in-flight KMS resource. A cohort retaining only the currently
    /// displayed owner is intentionally allowed: topology apply replaces that
    /// owner, just as it did before cohorts tracked last-head release.
    pub fn output_topology_preparation_quiescent(&self) -> bool {
        !self.layout_probe_cleanup_pending()
            && self.output_topology_preparation.is_none()
            && self.heads.iter().all(|head| {
                head.rendering_content.is_none()
                    && head.submitted_content.is_none()
                    && head.scanout_custody.submitted().is_none()
                    && head.prepared_scanout.is_none()
                    && !head.scanout_custody.cleanup_pending()
            })
            && self
                .exporters
                .iter()
                .all(|exporter| !exporter.worker_in_flight())
    }

    /// Names the first unmet clause of `output_topology_preparation_quiescent`,
    /// or `None` when quiescent.
    ///
    /// A wait that reports only that it timed out sends its reader back to the
    /// source to guess which owner was still holding a frame.
    pub fn output_topology_preparation_quiescence_blocker(&self) -> Option<&'static str> {
        if self.layout_probe_cleanup_pending() {
            return Some("layout_probe_cleanup");
        }
        if self.output_topology_preparation.is_some() {
            return Some("topology_preparation");
        }
        for head in &self.heads {
            if head.rendering_content.is_some() {
                return Some("head_rendering_content");
            }
            if head.submitted_content.is_some() {
                return Some("head_submitted_content");
            }
            if head.scanout_custody.submitted().is_some() {
                return Some("head_scanout_submission");
            }
            if head.prepared_scanout.is_some() {
                return Some("head_prepared_scanout");
            }
            if head.scanout_custody.cleanup_pending() {
                return Some("head_scanout_cleanup");
            }
        }
        if self
            .exporters
            .iter()
            .any(|exporter| exporter.worker_in_flight())
        {
            return Some("exporter_worker_in_flight");
        }
        None
    }

    /// Per-head pipeline state for the first head blocking quiescence.
    ///
    /// The clause name alone said content was stuck without saying where, and
    /// content only leaves `pending` once a submit reaches the kernel. Naming
    /// the head and every stage beside it is what distinguishes "nothing is
    /// submitting" from "a flip never came back".
    pub fn output_topology_quiescence_head_report(&self) -> Option<String> {
        let index = self.heads.iter().position(|head| {
            head.rendering_content.is_some()
                || head.submitted_content.is_some()
                || head.scanout_custody.submitted().is_some()
                || head.prepared_scanout.is_some()
                || head.scanout_custody.cleanup_pending()
        })?;
        let head = &self.heads[index];
        // Exporters are index-parallel with heads: both are built from one
        // zipped, jointly sorted list at discovery.
        let exporter = self.exporters.get(index);
        Some(format!(
            "head={} output={} enabled={} pending={} rendering={} submitted={} scanout={} prepared={} cleanup={} exporter_pending={} worker_in_flight={}",
            head.head.raw(),
            head.output.id.raw(),
            head.enabled,
            u8::from(head.pending_content.is_some()),
            u8::from(head.rendering_content.is_some()),
            u8::from(head.submitted_content.is_some()),
            u8::from(head.scanout_custody.submitted().is_some()),
            u8::from(head.prepared_scanout.is_some()),
            u8::from(head.scanout_custody.cleanup_pending()),
            exporter.map_or(2, |exporter| u8::from(exporter.pending_frame())),
            exporter.map_or(2, |exporter| u8::from(exporter.worker_in_flight())),
        ))
    }

    pub fn output_topology_preparation_phase(
        &self,
    ) -> Option<LiveProductionNativeTopologyPreparationPhase> {
        self.output_topology_preparation
            .as_ref()
            .map(|state| state.phase)
    }

    pub fn output_topology_failed_without_mutation(&self) -> bool {
        self.output_topology_preparation
            .as_ref()
            .is_some_and(|state| {
                state.phase == LiveProductionNativeTopologyPreparationPhase::Failed
                    && state.apply.applied == 0
            })
    }

    pub fn output_topology_allows_frame_service(&self) -> bool {
        self.output_topology_preparation
            .as_ref()
            .is_none_or(|state| {
                state.phase == LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
            })
    }

    pub fn output_topology_cleanup_pending(&self) -> bool {
        !self.output_topology_cleanup.is_empty()
    }

    pub fn request_abort_output_topology_preparation(&mut self, reason: impl Into<String>) -> bool {
        let Some(mut state) = self.output_topology_preparation.take() else {
            return false;
        };
        if state.phase == LiveProductionNativeTopologyPreparationPhase::Failed {
            self.output_topology_preparation = Some(state);
            return true;
        }
        state.failure.get_or_insert_with(|| reason.into());
        match state.phase {
            LiveProductionNativeTopologyPreparationPhase::PreparingCandidate
            | LiveProductionNativeTopologyPreparationPhase::PreparingRollback
            | LiveProductionNativeTopologyPreparationPhase::Prepared => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::Aborting;
            }
            LiveProductionNativeTopologyPreparationPhase::Applying => {
                if state.apply.applied == 0 {
                    if let Err(error) = self.cancel_partial_output_topology_resources(&mut state) {
                        state.failure = Some(format!(
                            "topology abort resource cancellation failed: {error}"
                        ));
                    }
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                } else if state.apply.begin_rollback_after_partial_apply()
                    == LiveProductionNativeTopologyApplyTransition::Accepted
                {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
                } else {
                    state.failure =
                        Some("topology abort could not enter partial-apply rollback".to_owned());
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Applied
            | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
            | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued => {
                if state.apply.begin_rollback_after_apply()
                    == LiveProductionNativeTopologyApplyTransition::Accepted
                {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
                } else {
                    state.failure = Some("topology abort could not enter full rollback".to_owned());
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::RollingBack
            | LiveProductionNativeTopologyPreparationPhase::RolledBack => {}
            LiveProductionNativeTopologyPreparationPhase::Aborting
            | LiveProductionNativeTopologyPreparationPhase::Failed => {}
        }
        self.output_topology_preparation = Some(state);
        true
    }

    pub fn retry_output_topology_cleanup(&mut self) -> usize {
        self.service_layout_probe_cleanup();
        let pending = core::mem::take(&mut self.output_topology_cleanup);
        for (head, cleanup) in pending {
            let Some(index) = self.head_index_for_head(head) else {
                self.output_topology_cleanup.push((head, cleanup));
                continue;
            };
            let retried =
                crate::retry_rendered_primary_plane_scanout_cleanup(self.card(index), cleanup);
            if let Some(cleanup) = retried.cleanup {
                self.output_topology_cleanup.push((head, cleanup));
            }
        }
        self.output_topology_cleanup.len()
    }

    /// Starts nonblocking renderer preparation for both sides of one topology
    /// transaction. No KMS request is submitted here.
    pub fn begin_output_topology_preparation(
        &mut self,
        plan: LiveProductionNativeTopologyPlan,
        rollback: crate::LiveResolvedOutputTopology,
        candidate_frames: Vec<crate::LiveProductionHeadCompositionFrame>,
        rollback_frames: Vec<crate::LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeTopologyPreparationReport, Box<dyn std::error::Error>> {
        self.invalidate_layout_probes();
        self.service_layout_probe_cleanup();
        if self.output_topology_preparation.is_some() {
            return Err("native output topology preparation is already active".into());
        }
        if !self.output_topology_preparation_quiescent() {
            return Err(
                "native output topology preparation requires quiescent frame ownership".into(),
            );
        }

        let mut candidate_frames =
            validate_live_production_topology_frames(&plan, candidate_frames, true)?;
        validate_live_production_rollback_topology(&plan, &rollback)?;
        let rollback_frames =
            validate_live_production_topology_frames(&plan, rollback_frames, false)?;
        self.prepare_output_topology_renderer_images(&candidate_frames)?;
        self.prepare_output_topology_renderer_images(&rollback_frames)?;
        let mut resources = LiveProductionNativeTopologyResources::new(&plan)
            .ok_or("native output topology resource cohort is invalid")?;

        // Disabled heads have no renderer work, but their property handles are
        // still part of prepare-all. Resolve them before mutating exporter queues.
        for head_plan in &plan.heads {
            if head_plan.disposition != LiveProductionNativeTopologyDisposition::Disabled {
                continue;
            }
            let prepared = crate::prepare_native_disabled_topology_head(
                self.groups[head_plan.card_index].session.card(),
                head_plan.previous_selection,
            );
            let owner = prepared.prepared.ok_or_else(|| {
                format!(
                    "native disabled topology head {} property preparation failed: {:?}",
                    head_plan.head.raw(),
                    prepared.status,
                )
            })?;
            resources
                .prepare_candidate_disabled(head_plan.head, owner)
                .map_err(|rejected| {
                    format!(
                        "native disabled topology head {} was rejected: {:?}",
                        head_plan.head.raw(),
                        rejected.transition,
                    )
                })?;
        }
        for head_plan in &plan.heads {
            if head_plan.previous_enabled {
                continue;
            }
            let prepared = crate::prepare_native_disabled_topology_head(
                self.groups[head_plan.card_index].session.card(),
                head_plan.previous_selection,
            );
            let owner = prepared.prepared.ok_or_else(|| {
                format!(
                    "native rollback-disabled head {} property preparation failed: {:?}",
                    head_plan.head.raw(),
                    prepared.status,
                )
            })?;
            resources
                .prepare_rollback_disabled(head_plan.head, owner)
                .map_err(|rejected| {
                    format!(
                        "native rollback-disabled head {} was rejected: {:?}",
                        head_plan.head.raw(),
                        rejected.transition,
                    )
                })?;
        }

        for head_plan in &plan.heads {
            let LiveProductionNativeTopologyDisposition::Enabled { .. } = head_plan.disposition
            else {
                continue;
            };
            let index = self
                .head_index_for_head(head_plan.head)
                .ok_or("topology preparation lost a live head")?;
            let mut frame = candidate_frames
                .remove(&head_plan.head)
                .expect("candidate frame coverage was validated");
            // A topology commit is not a frame: it proves a mode the head does
            // not yet run, and it must own the framebuffer it commits. A
            // client's buffer would be scanned out under a mode nobody has
            // validated it for, so a candidate frame carries no proof
            // regardless of the plan it came from.
            frame.frame.direct_scanout =
                sophia_engine::DirectScanoutVerdict::CompositionRequired("topology_commit");
            self.exporters[index].set_pending_mixed_frame(frame.frame);
        }

        let affected_heads = plan.heads.len();
        self.output_topology_preparation = Some(LiveProductionNativeTopologyPreparation {
            apply: LiveProductionNativeTopologyApplyCoordinator::new(&plan)
                .ok_or("native output topology apply coordinator is invalid")?,
            plan,
            rollback,
            resources,
            rollback_frames,
            phase: LiveProductionNativeTopologyPreparationPhase::PreparingCandidate,
            failure: None,
        });
        Ok(LiveProductionNativeTopologyPreparationReport {
            phase: LiveProductionNativeTopologyPreparationPhase::PreparingCandidate,
            candidate_prepared: self
                .output_topology_preparation
                .as_ref()
                .map_or(0, |state| state.resources.candidate_count()),
            rollback_prepared: 0,
            affected_heads,
        })
    }

    /// Advances renderer workers by one owner turn. The method returns
    /// `Prepared` only when the candidate and rollback resource sets are both
    /// complete; it never submits KMS.
    pub fn service_output_topology_preparation(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPreparationReport, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        let result = self.service_output_topology_preparation_inner(&mut state);
        if let Err(error) = &result {
            state.failure = Some(error.to_string());
            state.phase = LiveProductionNativeTopologyPreparationPhase::Aborting;
        }
        let report = LiveProductionNativeTopologyPreparationReport {
            phase: state.phase,
            candidate_prepared: state.resources.candidate_count(),
            rollback_prepared: state.resources.rollback_count(),
            affected_heads: state.plan.heads.len(),
        };
        self.output_topology_preparation = Some(state);
        if let Err(error) = result {
            tracing::warn!(
                "sophia_live_output_topology schema=1 status=preparation_aborting error={error} kms_submits=0"
            );
        }
        Ok(report)
    }

    /// Cancels a fully prepared transaction without submitting KMS and returns
    /// every affine renderer/native owner to the ordinary cleanup path.
    pub fn cancel_prepared_output_topology(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPlan, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Prepared
            || !state.resources.ready()
            || self
                .exporters
                .iter()
                .any(|exporter| exporter.pending_frame())
        {
            self.output_topology_preparation = Some(state);
            return Err("native output topology resources are not ready to cancel".into());
        }
        let mut cleanup_pending = 0usize;
        for head_plan in &state.plan.heads {
            self.head_index_for_head(head_plan.head)
                .ok_or("topology cancellation lost a live head")?;
            if let Some(candidate) = state.resources.take_candidate(head_plan.head) {
                match candidate {
                    LiveProductionNativeTopologyCandidateResource::Enabled(owner) => {
                        let cancelled = crate::cancel_prepared_rendered_topology_head(
                            self.groups[head_plan.card_index].session.card(),
                            owner,
                        );
                        if let Some(cleanup) = cancelled.cleanup {
                            self.output_topology_cleanup.push((
                                head_plan.head,
                                cleanup.map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                            ));
                            cleanup_pending = cleanup_pending.saturating_add(1);
                        }
                    }
                    LiveProductionNativeTopologyCandidateResource::Disabled(_) => {}
                }
            }
            if let Some(rollback) = state.resources.take_rollback(head_plan.head)
                && let LiveProductionNativeTopologyCandidateResource::Enabled(owner) = rollback
            {
                let cancelled = crate::cancel_prepared_rendered_topology_head(
                    self.groups[head_plan.card_index].session.card(),
                    owner,
                );
                if let Some(cleanup) = cancelled.cleanup {
                    self.output_topology_cleanup.push((
                        head_plan.head,
                        cleanup
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    ));
                    cleanup_pending = cleanup_pending.saturating_add(1);
                }
            }
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=prepared_cancelled heads={} cleanup_pending={} kms_submits=0",
            state.plan.heads.len(),
            cleanup_pending,
        );
        Ok(state.plan)
    }

    pub fn finish_failed_output_topology_preparation(
        &mut self,
    ) -> Result<(LiveProductionNativeTopologyPlan, String), Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Failed {
            self.output_topology_preparation = Some(state);
            return Err("native output topology preparation has not finished aborting".into());
        }
        Ok((
            state.plan,
            state
                .failure
                .unwrap_or_else(|| "native topology preparation failed".to_owned()),
        ))
    }

    pub fn begin_prepared_output_topology_apply(
        &mut self,
    ) -> Result<Vec<sophia_engine::RenderHeadId>, Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_mut()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Prepared
            || !state.resources.ready()
            || state.apply.begin_apply() != LiveProductionNativeTopologyApplyTransition::Accepted
        {
            return Err("native output topology resources are not ready to apply".into());
        }
        state.phase = LiveProductionNativeTopologyPreparationPhase::Applying;
        Ok(state.plan.heads.iter().map(|head| head.head).collect())
    }

    /// Submits at most one blocking card effect per owner turn.
    pub fn service_prepared_output_topology_apply(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyApplyTransition, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if !matches!(
            state.phase,
            LiveProductionNativeTopologyPreparationPhase::Applying
                | LiveProductionNativeTopologyPreparationPhase::RollingBack
        ) {
            self.output_topology_preparation = Some(state);
            return Err("native output topology apply is not active".into());
        }
        let card_index = state
            .apply
            .current_card_index()
            .ok_or("native topology apply coordinator has no current card")?;
        let rollback = state.phase == LiveProductionNativeTopologyPreparationPhase::RollingBack;
        let changes = topology_card_changes(&state, card_index, rollback)?;
        let group = self
            .groups
            .get(card_index)
            .ok_or("native topology apply references an unknown card")?;
        let outcome =
            crate::submit_native_topology_change_on_device(group.session.card(), &changes);
        let transition = if rollback {
            state.apply.observe_rollback(card_index, outcome)
        } else {
            state.apply.observe_apply(card_index, outcome)
        };
        match transition {
            LiveProductionNativeTopologyApplyTransition::RollbackRequired { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
            }
            LiveProductionNativeTopologyApplyTransition::Applied { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::Applied;
            }
            LiveProductionNativeTopologyApplyTransition::RolledBack { .. } => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::RolledBack;
            }
            LiveProductionNativeTopologyApplyTransition::FailedWithoutMutation { .. } => {
                state.failure = Some("the first card rejected the topology candidate".to_owned());
                self.cancel_partial_output_topology_resources(&mut state)?;
                state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
            }
            LiveProductionNativeTopologyApplyTransition::RollbackFailed { .. } => {
                state.failure =
                    Some("topology rollback failed after physical candidate mutation".to_owned());
                state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
            }
            LiveProductionNativeTopologyApplyTransition::Accepted
            | LiveProductionNativeTopologyApplyTransition::Retry
            | LiveProductionNativeTopologyApplyTransition::CardApplied { .. }
            | LiveProductionNativeTopologyApplyTransition::CardRolledBack { .. } => {}
            LiveProductionNativeTopologyApplyTransition::OutOfOrder
            | LiveProductionNativeTopologyApplyTransition::Terminal => {
                self.output_topology_preparation = Some(state);
                return Err("native topology apply coordinator rejected its own effect".into());
            }
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=card_effect card={} rollback={} outcome={outcome:?} transition={transition:?}",
            card_index,
            rollback,
        );
        self.output_topology_preparation = Some(state);
        Ok(transition)
    }

    pub fn request_output_topology_rollback(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_mut()
            .ok_or("native output topology preparation is not active")?;
        if !matches!(
            state.phase,
            LiveProductionNativeTopologyPreparationPhase::Applied
                | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
                | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
        ) || state.apply.begin_rollback_after_apply()
            != LiveProductionNativeTopologyApplyTransition::Accepted
        {
            return Err("native output topology cannot begin post-apply rollback".into());
        }
        state.failure = Some(reason.into());
        state.phase = LiveProductionNativeTopologyPreparationPhase::RollingBack;
        Ok(())
    }

    /// Adopts the candidate buffers accepted by every card while retaining the
    /// complete rollback side. Authority must not publish yet.
    pub fn install_applied_output_topology(
        &mut self,
    ) -> Result<Vec<sophia_engine::HeadlessOutput>, Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::Applied {
            self.output_topology_preparation = Some(state);
            return Err("native output topology candidate is not physically applied".into());
        }
        let result = self.install_output_topology_side(&mut state, true);
        match result {
            Ok(outputs) => {
                state.phase = LiveProductionNativeTopologyPreparationPhase::CandidateInstalled;
                self.output_topology_preparation = Some(state);
                Ok(outputs)
            }
            Err(error) => {
                self.output_topology_preparation = Some(state);
                Err(error)
            }
        }
    }

    /// Releases the rollback pool only after every replacement logical output
    /// has completed its first ordinary presentation cohort.
    pub fn commit_installed_output_topology(
        &mut self,
    ) -> Result<LiveProductionNativeTopologyPlan, Box<dyn std::error::Error>> {
        let state = self
            .output_topology_preparation
            .as_ref()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued {
            return Err("native output topology is not awaiting first presentation".into());
        }
        if state
            .plan
            .heads
            .iter()
            .any(|head| self.head_index_for_head(head.head).is_none())
        {
            return Err("topology commit lost a live head before finalization".into());
        }
        let mut state = self
            .output_topology_preparation
            .take()
            .expect("topology preparation was validated above");
        if let Err(error) = self.cancel_partial_output_topology_resources(&mut state) {
            self.output_topology_preparation = Some(state);
            return Err(error);
        }
        tracing::info!(
            "sophia_live_output_topology schema=1 status=committed heads={} outputs={} cleanup_pending={}",
            state.plan.heads.len(),
            self.logical_outputs.len(),
            self.output_topology_cleanup.len(),
        );
        Ok(state.plan)
    }

    /// Opens ordinary frame service only after one complete native-size cohort
    /// has been queued for every replacement logical output.
    pub fn arm_installed_output_topology_first_presentation(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self
            .output_topology_preparation
            .as_ref()
            .map(|state| state.phase)
            != Some(LiveProductionNativeTopologyPreparationPhase::CandidateInstalled)
        {
            return Err("native output topology candidate is not ready to arm".into());
        }
        for output in &self.logical_outputs {
            let indices = self.head_indices(output.id);
            if indices.is_empty()
                || indices.iter().any(|index| {
                    self.heads[*index].pending_content.is_none()
                        || !self.exporters[*index].pending_frame()
                })
            {
                return Err("native output topology first-frame coverage is incomplete".into());
            }
        }
        self.output_topology_preparation
            .as_mut()
            .expect("topology state checked above")
            .phase = LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued;
        Ok(())
    }

    /// Installs the published rollback buffers after every required reverse
    /// card effect succeeded, then releases any never-adopted candidate owner.
    pub fn install_rolled_back_output_topology(
        &mut self,
    ) -> Result<(LiveProductionNativeTopologyPlan, String), Box<dyn std::error::Error>> {
        let mut state = self
            .output_topology_preparation
            .take()
            .ok_or("native output topology preparation is not active")?;
        if state.phase != LiveProductionNativeTopologyPreparationPhase::RolledBack {
            self.output_topology_preparation = Some(state);
            return Err("native output topology rollback is not physically complete".into());
        }
        if let Err(error) = self.install_output_topology_side(&mut state, false) {
            self.output_topology_preparation = Some(state);
            return Err(error);
        }
        self.cancel_partial_output_topology_resources(&mut state)?;
        let reason = state
            .failure
            .take()
            .unwrap_or_else(|| "candidate apply rolled back".to_owned());
        tracing::info!(
            "sophia_live_output_topology schema=1 status=rolled_back heads={} outputs={} cleanup_pending={}",
            state.plan.heads.len(),
            self.logical_outputs.len(),
            self.output_topology_cleanup.len(),
        );
        Ok((state.plan, reason))
    }

    fn install_output_topology_side(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
        candidate: bool,
    ) -> Result<Vec<sophia_engine::HeadlessOutput>, Box<dyn std::error::Error>> {
        self.invalidate_layout_probes();
        let logical_outputs = if candidate {
            state.plan.outputs.clone()
        } else {
            state.rollback.outputs.clone()
        };
        if logical_outputs.is_empty() {
            return Err("installed native topology has no logical output".into());
        }
        let logical_by_id = logical_outputs
            .iter()
            .map(|output| (output.id, *output))
            .collect::<BTreeMap<_, _>>();
        if logical_by_id.len() != logical_outputs.len() {
            return Err("installed native topology repeats a logical output".into());
        }

        let mut installed = Vec::with_capacity(state.plan.heads.len());
        let mut registry = sophia_engine::EngineHeadRegistry::new();
        for head_plan in &state.plan.heads {
            let index = self
                .head_index_for_head(head_plan.head)
                .ok_or("topology installation lost a physical head")?;
            let (
                enabled,
                output,
                selection,
                target_generation,
                scale,
                refresh_millihz,
                transform,
                mapping,
                vrr,
            ) = if candidate {
                match head_plan.disposition {
                    LiveProductionNativeTopologyDisposition::Enabled {
                        output,
                        selection,
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                        vrr,
                    } => (
                        true,
                        output,
                        selection,
                        head_plan.candidate_target_generation,
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                        vrr,
                    ),
                    LiveProductionNativeTopologyDisposition::Disabled => (
                        false,
                        head_plan.previous_output,
                        head_plan.previous_selection,
                        head_plan.candidate_target_generation,
                        head_plan.previous_scale,
                        head_plan.previous_refresh_millihz,
                        head_plan.previous_transform,
                        head_plan.previous_mapping,
                        head_plan.previous_vrr,
                    ),
                }
            } else {
                (
                    head_plan.previous_enabled,
                    head_plan.previous_output,
                    head_plan.previous_selection,
                    head_plan.previous_target_generation,
                    head_plan.previous_scale,
                    head_plan.previous_refresh_millihz,
                    head_plan.previous_transform,
                    head_plan.previous_mapping,
                    head_plan.previous_vrr,
                )
            };
            let resource = if candidate {
                state.resources.candidate(head_plan.head)
            } else {
                state.resources.rollback(head_plan.head)
            }
            .ok_or("topology installation lost a prepared physical owner")?;
            if enabled
                != matches!(
                    resource,
                    LiveProductionNativeTopologyCandidateResource::Enabled(_)
                )
            {
                return Err("topology installation resource disposition mismatch".into());
            }
            let physical_output = sophia_engine::HeadlessOutput {
                id: output,
                size: selection.size(),
                scale,
            };
            let output_frames = OutputFramePresentationState::new(physical_output)?;
            if enabled
                && !registry
                    .admit(sophia_engine::HeadRenderTarget {
                        head: head_plan.head,
                        output,
                        target_generation,
                        native_size: selection.size(),
                        scale,
                        refresh_millihz,
                        transform,
                        mapping,
                    })
                    .is_admitted()
            {
                return Err("installed Engine head registry rejected a physical target".into());
            }
            installed.push(LiveProductionNativeInstalledHead {
                index,
                enabled,
                output,
                selection,
                target_generation,
                scale,
                refresh_millihz,
                transform,
                mapping,
                vrr,
                output_frames,
            });
        }
        if registry.output_count() != logical_outputs.len() {
            return Err("installed topology has a logical output without a physical head".into());
        }
        for (output, primary) in &state.plan.primary_heads {
            if registry.set_primary_head(*output, *primary)
                != sophia_engine::EngineLogicalOutputUpdate::Updated
            {
                return Err("installed topology names a primary outside its output".into());
            }
        }

        let mut lifecycles = BTreeMap::new();
        for output in logical_by_id.keys().copied() {
            let mut members = installed
                .iter()
                .filter(|head| head.enabled && head.output == output)
                .map(|head| self.heads[head.index].head)
                .collect::<Vec<_>>();
            if let Some(primary) = state.plan.primary_heads.get(&output)
                && let Some(index) = members.iter().position(|head| head == primary)
            {
                members.swap(0, index);
            }
            let mut lifecycle = LiveProductionMirrorGroupLifecycle::new(output, members.clone())
                .ok_or("installed logical output has no lifecycle members")?;
            for head in members {
                if !matches!(
                    lifecycle.mark_initialized(head),
                    LiveProductionMirrorHeadTransition::Accepted
                        | LiveProductionMirrorHeadTransition::GroupReady
                ) {
                    return Err("installed logical output lifecycle rejected initialization".into());
                }
            }
            lifecycles.insert(output, lifecycle);
        }

        for install in &installed {
            let custody = &self.heads[install.index].scanout_custody;
            if custody.submitted().is_some() || !custody.can_retire_displayed() {
                return Err(
                    "topology adoption waits for submitted ownership and cleanup capacity".into(),
                );
            }
            if self.exporters[install.index].worker_in_flight() {
                return Err("topology installation cannot replace active renderer work".into());
            }
        }
        for install in &installed {
            self.exporters[install.index].discard_pending_frame();
        }

        for install in installed {
            self.retire_topology_displayed_owner(install.index)?;
            let selected = if candidate {
                state
                    .resources
                    .take_candidate(self.heads[install.index].head)
            } else {
                state
                    .resources
                    .take_rollback(self.heads[install.index].head)
            }
            .expect("selected topology resources were prevalidated");
            if let LiveProductionNativeTopologyCandidateResource::Enabled(owner) = selected {
                self.heads[install.index]
                    .scanout_custody
                    .adopt_displayed(
                        crate::adopt_prepared_rendered_topology_head_after_commit(owner)
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    )
                    .expect("topology adoption prevalidated before resource take");
            }
            let head = &mut self.heads[install.index];
            head.enabled = install.enabled;
            head.selection = install.selection;
            head.target_generation = install.target_generation;
            head.scale = install.scale;
            head.refresh_millihz = install.refresh_millihz;
            head.transform = install.transform;
            head.mapping = install.mapping;
            head.vrr = install.vrr;
            head.output = sophia_engine::HeadlessOutput {
                id: install.output,
                size: install.selection.size(),
                scale: install.scale,
            };
            head.pending_callback = None;
            head.completion_mode = LiveProductionKmsCompletionMode::PageFlipPreferred;
            head.completion_fence_status = crate::LibdrmNativeCompletionFenceStatus::Unsupported;
            head.last_callback_serial = None;
            head.pending_content = None;
            head.rendering_content = None;
            head.submitted_content = None;
            head.presented_content = None;
            head.submitted_group_frame = None;
            head.prepared_group_frame = None;
            head.submitted_at = None;
            head.submitted_ust_usec = None;
            head.output_frames = install.output_frames;
        }
        self.logical_outputs = logical_outputs.clone();
        self.presentation_outputs = logical_outputs.len();
        self.output_lifecycles = lifecycles;
        self.output_cohorts.clear();
        self.deferred_mirror_generations.clear();
        self.production_page_flips = crate::LiveProductionPageFlipTracker::from_outputs(&registry);
        self.kernel_page_flip_ust.clear();
        Ok(logical_outputs)
    }

    fn retire_topology_displayed_owner(&mut self, index: usize) -> Result<(), &'static str> {
        let group = self.heads[index].group;
        self.heads[index].scanout_custody.retire_displayed(self.groups[group].session.card())
            .map(|_| ())
            .map_err(|()| "topology replacement retains displayed owner until cleanup capacity is available")
    }

    fn service_output_topology_preparation_inner(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match state.phase {
            LiveProductionNativeTopologyPreparationPhase::PreparingCandidate => {
                for head_plan in &state.plan.heads {
                    let LiveProductionNativeTopologyDisposition::Enabled { selection, vrr, .. } =
                        head_plan.disposition
                    else {
                        continue;
                    };
                    if state.resources.candidate(head_plan.head).is_some() {
                        continue;
                    }
                    let Some(owner) = self.prepare_output_topology_head(
                        head_plan.head,
                        selection,
                        topology_vrr_enabled(vrr),
                    )?
                    else {
                        continue;
                    };
                    state
                        .resources
                        .prepare_candidate_enabled(head_plan.head, owner)
                        .map_err(|rejected| {
                            format!(
                                "candidate topology owner for head {} was rejected: {:?}",
                                head_plan.head.raw(),
                                rejected.transition,
                            )
                        })?;
                }
                if state.resources.candidate_count() == state.plan.heads.len() {
                    for head_plan in &state.plan.heads {
                        if !head_plan.previous_enabled {
                            continue;
                        }
                        let index = self
                            .head_index_for_head(head_plan.head)
                            .ok_or("rollback topology preparation lost a live head")?;
                        if self.exporters[index].pending_frame() {
                            return Err(
                                "candidate exporter remained occupied after preparation".into()
                            );
                        }
                        let mut frame = state
                            .rollback_frames
                            .remove(&head_plan.head)
                            .expect("rollback frame coverage was validated");
                        // A topology commit is not a frame: it proves a mode
                        // the head does not yet run, and it must own the
                        // framebuffer it commits. A client's buffer would be
                        // scanned out under a mode nobody has validated it
                        // for, so a candidate or rollback frame carries no
                        // proof regardless of the plan it came from.
                        frame.frame.direct_scanout =
                            sophia_engine::DirectScanoutVerdict::CompositionRequired(
                                "topology_commit",
                            );
                        self.exporters[index].set_pending_mixed_frame(frame.frame);
                    }
                    state.phase = LiveProductionNativeTopologyPreparationPhase::PreparingRollback;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::PreparingRollback => {
                for head_plan in &state.plan.heads {
                    if state.resources.rollback(head_plan.head).is_some() {
                        continue;
                    }
                    if !head_plan.previous_enabled {
                        return Err("disabled rollback head lost its prepared detach owner".into());
                    }
                    let Some(owner) = self.prepare_output_topology_head(
                        head_plan.head,
                        head_plan.previous_selection,
                        Some(false),
                    )?
                    else {
                        continue;
                    };
                    state
                        .resources
                        .prepare_rollback(head_plan.head, owner)
                        .map_err(|rejected| {
                            format!(
                                "rollback topology owner for head {} was rejected: {:?}",
                                head_plan.head.raw(),
                                rejected.transition,
                            )
                        })?;
                }
                if state.resources.ready() {
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Prepared;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Prepared => {}
            LiveProductionNativeTopologyPreparationPhase::Applying
            | LiveProductionNativeTopologyPreparationPhase::RollingBack
            | LiveProductionNativeTopologyPreparationPhase::Applied
            | LiveProductionNativeTopologyPreparationPhase::CandidateInstalled
            | LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued
            | LiveProductionNativeTopologyPreparationPhase::RolledBack => {
                return Err("topology renderer preparation was serviced after apply began".into());
            }
            LiveProductionNativeTopologyPreparationPhase::Aborting => {
                let mut renderer_drained = true;
                for head_plan in &state.plan.heads {
                    let index = self
                        .head_index_for_head(head_plan.head)
                        .ok_or("topology abort lost a live head")?;
                    if !self.exporters[index].pending_frame() {
                        continue;
                    }
                    if !self.exporters[index].worker_in_flight() {
                        self.exporters[index].discard_pending_frame();
                        continue;
                    }
                    let selection = if state.resources.candidate_count() < state.plan.heads.len() {
                        match head_plan.disposition {
                            LiveProductionNativeTopologyDisposition::Enabled {
                                selection, ..
                            } => selection,
                            LiveProductionNativeTopologyDisposition::Disabled => {
                                head_plan.previous_selection
                            }
                        }
                    } else {
                        head_plan.previous_selection
                    };
                    let export =
                        crate::LiveRenderedScanoutBufferExporter::export_rendered_scanout_buffer(
                            &mut self.exporters[index],
                            crate::LiveGbmEglFrameTargetRecord::new(selection.size()),
                        );
                    if export.status == crate::LiveRendererScanoutBufferExportStatus::Pending {
                        renderer_drained = false;
                    }
                    // Dropping an exported worker owner returns its lease. No
                    // native framebuffer was built for this abort-only poll.
                    drop(export);
                }
                if renderer_drained
                    && self
                        .exporters
                        .iter()
                        .all(|exporter| !exporter.pending_frame())
                {
                    self.cancel_partial_output_topology_resources(state)?;
                    state.phase = LiveProductionNativeTopologyPreparationPhase::Failed;
                }
            }
            LiveProductionNativeTopologyPreparationPhase::Failed => {
                return Err(state
                    .failure
                    .clone()
                    .unwrap_or_else(|| "native topology preparation failed".to_owned())
                    .into());
            }
        }
        Ok(())
    }

    fn cancel_partial_output_topology_resources(
        &mut self,
        state: &mut LiveProductionNativeTopologyPreparation,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for head_plan in &state.plan.heads {
            self.head_index_for_head(head_plan.head)
                .ok_or("topology resource cancellation lost a live head")?;
            if let Some(candidate) = state.resources.take_candidate(head_plan.head) {
                match candidate {
                    LiveProductionNativeTopologyCandidateResource::Enabled(owner) => {
                        let cancelled = crate::cancel_prepared_rendered_topology_head(
                            self.groups[head_plan.card_index].session.card(),
                            owner,
                        );
                        if let Some(cleanup) = cancelled.cleanup {
                            self.output_topology_cleanup.push((
                                head_plan.head,
                                cleanup.map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                            ));
                        }
                    }
                    LiveProductionNativeTopologyCandidateResource::Disabled(_) => {}
                }
            }
            if let Some(rollback) = state.resources.take_rollback(head_plan.head)
                && let LiveProductionNativeTopologyCandidateResource::Enabled(owner) = rollback
            {
                let cancelled = crate::cancel_prepared_rendered_topology_head(
                    self.groups[head_plan.card_index].session.card(),
                    owner,
                );
                if let Some(cleanup) = cancelled.cleanup {
                    self.output_topology_cleanup.push((
                        head_plan.head,
                        cleanup
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    ));
                }
            }
        }
        Ok(())
    }

    fn prepare_output_topology_head(
        &mut self,
        head: sophia_engine::RenderHeadId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        vrr_enabled: Option<bool>,
    ) -> Result<Option<LiveProductionPreparedTopologyHead>, Box<dyn std::error::Error>> {
        let index = self
            .head_index_for_head(head)
            .ok_or("topology renderer preparation targets an unknown head")?;
        let group = self.heads[index].group;
        let mut prepared =
            crate::prepare_rendered_primary_plane_topology_head_from_target_and_selection_with(
                crate::LiveKmsScanoutTargetStatus::Ready,
                Some(crate::LiveGbmEglFrameTargetRecord::new(selection.size())),
                crate::LibdrmNativePrimaryPlaneSelectionResult {
                    status: crate::LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                    selection: Some(selection),
                },
                vrr_enabled,
                self.groups[group].session.card(),
                &mut self.exporters[index],
            );
        match prepared.status {
            crate::LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportPending => Ok(None),
            crate::LiveRenderedPrimaryPlaneScanoutPrepareStatus::Prepared => {
                let owner = prepared
                    .prepared
                    .take()
                    .ok_or("prepared topology renderer omitted its affine owner")?;
                match crate::prepare_rendered_topology_head_from_prepared_scanout(
                    owner,
                    vrr_enabled,
                ) {
                    Ok(owner) => Ok(Some(owner)),
                    Err(owner) => {
                        let cancelled = crate::cancel_prepared_rendered_primary_plane_scanout(
                            self.groups[group].session.card(),
                            owner,
                        );
                        if let Some(cleanup) = cancelled.cleanup {
                            self.output_topology_cleanup.push((
                                head,
                                cleanup.map_scanout_buffer(|owner| {
                                    Box::new(owner) as Box<dyn std::any::Any>
                                }),
                            ));
                        }
                        Err("modeset preparation produced a non-topology resource owner".into())
                    }
                }
            }
            status => {
                if let Some(cleanup) = prepared.cleanup.take() {
                    self.output_topology_cleanup.push((
                        head,
                        cleanup
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    ));
                }
                Err(format!(
                    "topology renderer preparation failed for head {}: {status:?}",
                    head.raw(),
                )
                .into())
            }
        }
    }

    fn prepare_output_topology_renderer_images(
        &mut self,
        frames: &BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let requirements = crate::live_topology_frame_renderer_image_requirements(frames);
        for (head, image_ids) in requirements {
            let target_index = self
                .head_index_for_head(head)
                .ok_or("topology renderer-image preparation targets an unknown head")?;
            for image_id in image_ids {
                if self.exporters[target_index]
                    .export_promoted_renderer_image(image_id)?
                    .is_some()
                {
                    continue;
                }
                if self.exporters[target_index].promote_renderer_image(image_id)?
                    && self.exporters[target_index]
                        .export_promoted_renderer_image(image_id)?
                        .is_some()
                {
                    continue;
                }

                let mut snapshot = None;
                for donor_index in 0..self.exporters.len() {
                    if donor_index == target_index {
                        continue;
                    }
                    if let Some(available) =
                        self.exporters[donor_index].export_promoted_renderer_image(image_id)?
                    {
                        snapshot = Some(available);
                        break;
                    }
                }
                let snapshot = snapshot.ok_or_else(|| {
                    format!(
                        "topology renderer image {} for head {} has no live donor",
                        image_id.raw(),
                        head.raw(),
                    )
                })?;
                if !self.exporters[target_index].restore_promoted_renderer_image(snapshot)?
                    && self.exporters[target_index]
                        .export_promoted_renderer_image(image_id)?
                        .is_none()
                {
                    return Err(format!(
                        "topology renderer image {} was not installed for head {}",
                        image_id.raw(),
                        head.raw(),
                    )
                    .into());
                }
                tracing::info!(
                    "sophia_live_output_topology schema=1 status=renderer_image_replicated head={} image={} kms_submits=0",
                    head.raw(),
                    image_id.raw(),
                );
            }
        }
        Ok(())
    }
}

fn topology_vrr_enabled(policy: sophia_protocol::OutputVrrPolicy) -> Option<bool> {
    match policy {
        sophia_protocol::OutputVrrPolicy::Disabled => Some(false),
        sophia_protocol::OutputVrrPolicy::Automatic => None,
        sophia_protocol::OutputVrrPolicy::Always => Some(true),
    }
}

fn topology_card_changes(
    state: &LiveProductionNativeTopologyPreparation,
    card_index: usize,
    rollback: bool,
) -> Result<Vec<crate::LibdrmNativeAtomicTopologyChange>, Box<dyn std::error::Error>> {
    let heads = state.resources.card_heads(card_index);
    if heads.is_empty() {
        return Err("topology card effect has no heads".into());
    }
    heads
        .into_iter()
        .map(|head| {
            let resource = if rollback {
                state.resources.rollback(head)
            } else {
                state.resources.candidate(head)
            }
            .ok_or("topology card effect is missing a prepared head")?;
            Ok(match resource {
                LiveProductionNativeTopologyCandidateResource::Enabled(owner) => {
                    crate::LibdrmNativeAtomicTopologyChange::Enabled(owner.atomic_head())
                }
                LiveProductionNativeTopologyCandidateResource::Disabled(owner) => {
                    crate::LibdrmNativeAtomicTopologyChange::Disabled(owner.atomic_head())
                }
            })
        })
        .collect()
}

pub fn validate_live_production_rollback_topology(
    plan: &LiveProductionNativeTopologyPlan,
    rollback: &crate::LiveResolvedOutputTopology,
) -> Result<(), Box<dyn std::error::Error>> {
    let outputs = rollback
        .outputs
        .iter()
        .map(|output| (output.id, output))
        .collect::<BTreeMap<_, _>>();
    if outputs.len() != rollback.outputs.len()
        || outputs.is_empty()
        || !outputs.contains_key(&rollback.primary_output)
    {
        return Err("rollback topology has invalid logical-output coverage".into());
    }
    let targets = rollback
        .targets
        .iter()
        .map(|target| (target.head, target))
        .collect::<BTreeMap<_, _>>();
    let disabled = rollback
        .disabled_heads
        .iter()
        .map(|head| (head.head, head))
        .collect::<BTreeMap<_, _>>();
    if targets.len() != rollback.targets.len()
        || disabled.len() != rollback.disabled_heads.len()
        || targets.keys().any(|head| disabled.contains_key(head))
        || targets.len().saturating_add(disabled.len()) != plan.heads.len()
    {
        return Err("rollback topology has invalid physical-head coverage".into());
    }
    for head in &plan.heads {
        if head.previous_enabled {
            let target = targets
                .get(&head.head)
                .ok_or("rollback topology omitted a previously enabled head")?;
            let output = outputs
                .get(&head.previous_output)
                .ok_or("rollback topology omitted a previous logical output")?;
            if target.output != head.previous_output
                || target.target_generation != head.previous_target_generation
                || target.native_size != head.previous_selection.size()
                || target.timing.width
                    != u32::try_from(target.native_size.width).unwrap_or_default()
                || target.timing.height
                    != u32::try_from(target.native_size.height).unwrap_or_default()
                || target.timing.refresh_millihz != head.previous_refresh_millihz
                || output.scale != head.previous_scale
                || target.transform != head.previous_transform
                || target.mapping != head.previous_mapping
                || target.vrr != head.previous_vrr
            {
                return Err("rollback topology changed previous enabled-head state".into());
            }
        } else {
            let previous = disabled
                .get(&head.head)
                .ok_or("rollback topology omitted a previously disabled head")?;
            if previous.target_generation != head.previous_target_generation {
                return Err("rollback topology changed previous disabled-head generation".into());
            }
        }
    }
    Ok(())
}

pub fn validate_live_production_topology_frames(
    plan: &LiveProductionNativeTopologyPlan,
    frames: Vec<crate::LiveProductionHeadCompositionFrame>,
    candidate: bool,
) -> Result<
    BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    Box<dyn std::error::Error>,
> {
    let mut by_head = BTreeMap::new();
    for frame in frames {
        let head = frame.head;
        if by_head.insert(head, frame).is_some() {
            return Err("topology composition repeats a physical head".into());
        }
    }
    let expected = plan
        .heads
        .iter()
        .filter(|head| {
            if candidate {
                matches!(
                    head.disposition,
                    LiveProductionNativeTopologyDisposition::Enabled { .. }
                )
            } else {
                head.previous_enabled
            }
        })
        .collect::<Vec<_>>();
    if by_head.len() != expected.len() {
        return Err("topology composition has incomplete physical-head coverage".into());
    }
    let scene_generation = by_head
        .values()
        .next()
        .map(|frame| frame.scene_generation)
        .filter(|generation| *generation != 0)
        .ok_or("topology composition has an invalid scene generation")?;
    if by_head
        .values()
        .any(|frame| frame.scene_generation != scene_generation)
    {
        return Err("topology composition frames disagree on scene generation".into());
    }
    for head in expected {
        let frame = by_head
            .get(&head.head)
            .ok_or("topology composition omitted a physical head")?;
        let (output, size, scale, target_generation, mapping) = if candidate {
            let LiveProductionNativeTopologyDisposition::Enabled {
                output,
                selection,
                scale,
                mapping,
                ..
            } = head.disposition
            else {
                unreachable!("candidate expected set excludes disabled heads");
            };
            (
                output,
                selection.size(),
                scale,
                head.candidate_target_generation,
                mapping,
            )
        } else {
            (
                head.previous_output,
                head.previous_selection.size(),
                head.previous_scale,
                head.previous_target_generation,
                head.previous_mapping,
            )
        };
        let damage = frame
            .frame
            .output_damage_snapshot
            .as_ref()
            .ok_or("topology composition frame has no damage snapshot")?;
        if damage.output
            != (sophia_engine::HeadlessOutput {
                id: output,
                size,
                scale,
            })
        {
            return Err("topology composition frame targets the wrong output extent".into());
        }
        if frame.target_generation != target_generation {
            return Err("topology composition frame targets a stale generation".into());
        }
        if frame.mapping != mapping {
            return Err("topology composition frame targets the wrong mapping".into());
        }
    }
    Ok(by_head)
}
