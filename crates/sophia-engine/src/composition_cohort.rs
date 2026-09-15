use crate::prelude::*;
use crate::{HeadFrameCandidate, HeadFrameCandidateId, RenderHeadId};

/// Engine-owned state of one immutable logical-output presentation cohort.
///
/// Native targets and KMS owners deliberately do not live here. The reducer
/// proves the prepare-all barrier, primary-owned logical presentation, and
/// last-head native-owner release for one scene generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPresentationCohort {
    output: OutputId,
    scene_generation: u64,
    primary: RenderHeadId,
    required: BTreeSet<RenderHeadId>,
    prepared: BTreeMap<RenderHeadId, HeadFrameCandidate>,
    submitted: BTreeSet<RenderHeadId>,
    flipped: BTreeSet<RenderHeadId>,
    cleanup_complete: BTreeSet<RenderHeadId>,
    skipped: BTreeSet<RenderHeadId>,
    lost: BTreeSet<RenderHeadId>,
    logical_content_checksum: Option<u64>,
    terminal: Option<OutputPresentationTerminal>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPresentationPhase {
    Preparing,
    Prepared,
    Submitting,
    AwaitingFlips,
    SettlingCleanup,
    Presented,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPresentationTerminal {
    Presented {
        logical_sequence: u64,
        ust_usec: u64,
    },
    Failed(OutputPresentationFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPresentationFailure {
    Preparation,
    Submission,
    HeadLost(RenderHeadId),
    Invariant,
    StaleTopology,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPresentationTransition {
    Accepted,
    PhaseReady,
    Duplicate,
    UnknownHead,
    WrongOutput,
    WrongSceneGeneration,
    InvalidCandidate,
    CandidateIdentityCollision,
    LogicalContentMismatch,
    PrepareBarrierIncomplete,
    NotPrepared,
    NotSubmitted,
    NotFlipped,
    Terminal,
}

impl OutputPresentationCohort {
    pub fn new(
        output: OutputId,
        scene_generation: u64,
        primary: RenderHeadId,
        heads: impl IntoIterator<Item = RenderHeadId>,
    ) -> Option<Self> {
        let required = heads.into_iter().collect::<BTreeSet<_>>();
        (!required.is_empty()
            && required.len() <= crate::MAX_HEADS_PER_OUTPUT
            && output.is_valid()
            && scene_generation != 0
            && primary.is_valid()
            && required.contains(&primary)
            && required.iter().all(|head| head.is_valid()))
        .then_some(Self {
            output,
            scene_generation,
            primary,
            required,
            prepared: BTreeMap::new(),
            submitted: BTreeSet::new(),
            flipped: BTreeSet::new(),
            cleanup_complete: BTreeSet::new(),
            skipped: BTreeSet::new(),
            lost: BTreeSet::new(),
            logical_content_checksum: None,
            terminal: None,
        })
    }

    pub const fn output(&self) -> OutputId {
        self.output
    }

    pub const fn scene_generation(&self) -> u64 {
        self.scene_generation
    }

    pub const fn primary_head(&self) -> RenderHeadId {
        self.primary
    }

    pub fn required_heads(&self) -> impl Iterator<Item = RenderHeadId> + '_ {
        self.required.iter().copied()
    }

    pub fn prepared_candidate(&self, head: RenderHeadId) -> Option<HeadFrameCandidate> {
        self.prepared.get(&head).copied()
    }

    /// Whether an accepted physical owner still owes its first flip observation.
    /// This remains true after logical failure so its native storage can drain.
    pub fn head_awaits_flip(&self, head: RenderHeadId) -> bool {
        self.submitted.contains(&head) && !self.flipped.contains(&head)
    }

    pub fn all_prepared(&self) -> bool {
        self.prepared.len() == self.required.len()
    }

    pub fn all_submitted(&self) -> bool {
        self.submitted == self.required
    }

    pub fn all_flipped(&self) -> bool {
        self.flipped == self.required
    }

    pub fn all_cleanup_complete(&self) -> bool {
        self.required
            .iter()
            .all(|head| self.cleanup_complete.contains(head) || self.skipped.contains(head))
    }

    /// Whether no physical head can retain this generation's native owner.
    /// Logical presentation may precede this boundary by several head vblanks.
    pub fn generation_releasable(&self) -> bool {
        self.all_cleanup_complete()
    }

    /// Whether every physical owner the backend accepted has subsequently
    /// reached cleanup. This remains meaningful after logical failure: a
    /// poisoned cohort must still drain kernel-owned buffers without ever
    /// becoming logically presented.
    pub fn accepted_owners_settled(&self) -> bool {
        self.submitted.is_subset(&self.cleanup_complete)
    }

    pub const fn logical_content_checksum(&self) -> Option<u64> {
        self.logical_content_checksum
    }

    pub const fn terminal(&self) -> Option<OutputPresentationTerminal> {
        self.terminal
    }

    pub fn phase(&self) -> OutputPresentationPhase {
        match self.terminal {
            Some(OutputPresentationTerminal::Presented { .. }) => {
                OutputPresentationPhase::Presented
            }
            Some(OutputPresentationTerminal::Failed(_)) => OutputPresentationPhase::Failed,
            None if !self.all_prepared() => OutputPresentationPhase::Preparing,
            None if self.submitted.is_empty() => OutputPresentationPhase::Prepared,
            None if !self.all_submitted() => OutputPresentationPhase::Submitting,
            None if !self.all_flipped() => OutputPresentationPhase::AwaitingFlips,
            None => OutputPresentationPhase::SettlingCleanup,
        }
    }

    pub fn head_may_prepare(&self, head: RenderHeadId) -> bool {
        self.terminal.is_none()
            && self.required.contains(&head)
            && !self.prepared.contains_key(&head)
            && self.submitted.is_empty()
    }

    pub fn head_may_submit(&self, head: RenderHeadId) -> bool {
        !matches!(self.terminal, Some(OutputPresentationTerminal::Failed(_)))
            && self.all_prepared()
            && self.required.contains(&head)
            && !self.submitted.contains(&head)
            && !self.skipped.contains(&head)
    }

    pub fn mark_prepared(&mut self, candidate: HeadFrameCandidate) -> OutputPresentationTransition {
        if matches!(self.terminal, Some(OutputPresentationTerminal::Failed(_))) {
            return OutputPresentationTransition::Terminal;
        }
        if candidate.output != self.output {
            return OutputPresentationTransition::WrongOutput;
        }
        if candidate.scene_generation != self.scene_generation {
            return OutputPresentationTransition::WrongSceneGeneration;
        }
        if !self.required.contains(&candidate.head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if candidate.candidate == HeadFrameCandidateId::INVALID || candidate.target_generation == 0
        {
            return OutputPresentationTransition::InvalidCandidate;
        }
        if self.submitted.contains(&candidate.head) {
            return OutputPresentationTransition::Terminal;
        }
        if self.prepared.contains_key(&candidate.head) {
            return OutputPresentationTransition::Duplicate;
        }
        if self
            .prepared
            .values()
            .any(|prepared| prepared.candidate == candidate.candidate)
        {
            return OutputPresentationTransition::CandidateIdentityCollision;
        }
        if self
            .logical_content_checksum
            .is_some_and(|checksum| checksum != candidate.logical_content_checksum)
        {
            return OutputPresentationTransition::LogicalContentMismatch;
        }
        self.logical_content_checksum = Some(candidate.logical_content_checksum);
        self.prepared.insert(candidate.head, candidate);
        if self.all_prepared() {
            OutputPresentationTransition::PhaseReady
        } else {
            OutputPresentationTransition::Accepted
        }
    }

    pub fn mark_submitted(&mut self, head: RenderHeadId) -> OutputPresentationTransition {
        if matches!(self.terminal, Some(OutputPresentationTerminal::Failed(_))) {
            return OutputPresentationTransition::Terminal;
        }
        if !self.required.contains(&head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if !self.all_prepared() {
            return OutputPresentationTransition::PrepareBarrierIncomplete;
        }
        if !self.prepared.contains_key(&head) {
            return OutputPresentationTransition::NotPrepared;
        }
        if self.skipped.contains(&head) {
            return OutputPresentationTransition::Terminal;
        }
        if !self.submitted.insert(head) {
            return OutputPresentationTransition::Duplicate;
        }
        if self.all_submitted() {
            OutputPresentationTransition::PhaseReady
        } else {
            OutputPresentationTransition::Accepted
        }
    }

    pub fn mark_flipped(
        &mut self,
        head: RenderHeadId,
        ust_usec: u64,
    ) -> OutputPresentationTransition {
        if !self.required.contains(&head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if !self.submitted.contains(&head) {
            return OutputPresentationTransition::NotSubmitted;
        }
        if !self.flipped.insert(head) {
            return OutputPresentationTransition::Duplicate;
        }
        if self.terminal.is_none() && head == self.primary {
            self.terminal = Some(OutputPresentationTerminal::Presented {
                logical_sequence: self.scene_generation,
                ust_usec,
            });
            OutputPresentationTransition::PhaseReady
        } else if self.generation_releasable() {
            OutputPresentationTransition::PhaseReady
        } else {
            OutputPresentationTransition::Accepted
        }
    }

    pub fn mark_cleanup_complete(&mut self, head: RenderHeadId) -> OutputPresentationTransition {
        if !self.required.contains(&head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if !self.flipped.contains(&head) {
            return OutputPresentationTransition::NotFlipped;
        }
        if !self.cleanup_complete.insert(head) {
            return OutputPresentationTransition::Duplicate;
        }
        if self.generation_releasable()
            || (matches!(self.terminal, Some(OutputPresentationTerminal::Failed(_)))
                && self.accepted_owners_settled())
        {
            OutputPresentationTransition::PhaseReady
        } else {
            OutputPresentationTransition::Accepted
        }
    }

    /// Drops a prepared candidate that this head never submitted because a
    /// newer complete generation replaced it. Submitted or displayed work is
    /// never skippable; it must reach physical cleanup.
    pub fn mark_skipped(&mut self, head: RenderHeadId) -> OutputPresentationTransition {
        if matches!(self.terminal, Some(OutputPresentationTerminal::Failed(_))) {
            return OutputPresentationTransition::Terminal;
        }
        if !self.required.contains(&head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if self.submitted.contains(&head) || self.flipped.contains(&head) {
            return OutputPresentationTransition::Terminal;
        }
        if !self.skipped.insert(head) {
            return OutputPresentationTransition::Duplicate;
        }
        if self.generation_releasable() {
            OutputPresentationTransition::PhaseReady
        } else {
            OutputPresentationTransition::Accepted
        }
    }

    pub fn mark_head_lost(&mut self, head: RenderHeadId) -> OutputPresentationTransition {
        if !self.required.contains(&head) {
            return OutputPresentationTransition::UnknownHead;
        }
        if !self.lost.insert(head) {
            return OutputPresentationTransition::Duplicate;
        }
        self.fail(OutputPresentationFailure::HeadLost(head));
        OutputPresentationTransition::PhaseReady
    }

    pub fn fail(&mut self, failure: OutputPresentationFailure) -> bool {
        if self.terminal.is_some() {
            return false;
        }
        self.terminal = Some(OutputPresentationTerminal::Failed(failure));
        true
    }
}
