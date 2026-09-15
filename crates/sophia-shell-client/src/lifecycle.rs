//! Ordered content observations shared by toolkit-independent shell clients.
//!
//! Call `dispatch` in wire order. Native Presented changes active targets;
//! Prepared and resource release never do. This reducer performs no UI effect
//! and sends no ACK. The client must own outbound capacity before doing either.

use sophia_protocol::{
    ContentAction, ContentCandidateBegin, ContentLimits, ContentOutputFactsEntry, ContentOutputId,
    ContentSurface, ContentTarget, ShellContentRecord, TransactionId,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentLifecycleError {
    WrongGrant,
    WrongDirection,
    UnknownCandidate,
    WrongTransaction,
    InvalidCandidate,
    InvalidOutcome,
    Capacity,
}

#[derive(Clone, Debug)]
pub struct ClientContentCandidate {
    /// Transaction of CandidateBegin (outcomes correlate to Begin, not End).
    pub transaction: TransactionId,
    pub begin: ContentCandidateBegin,
    pub surfaces: Vec<ContentSurface>,
    pub targets: Vec<ContentTarget>,
}

#[derive(Clone, Debug)]
pub struct ClientPresentedContent {
    pub candidate: ClientContentCandidate,
    pub presentation_epoch: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentActionDispatch {
    /// Exact current targets match. Outbound reservation still precedes effect.
    Eligible,
    /// Send a rejected ACK without running an application effect.
    Rejected,
    /// A cancellation is never acknowledged, even if its action is unknown.
    Cancelled,
}

/// The transaction and original record survive dispatch unchanged.
#[derive(Clone, Debug)]
pub struct ContentDispatch {
    pub transaction: TransactionId,
    pub record: ShellContentRecord,
    pub action: Option<ContentActionDispatch>,
}

pub struct ContentLifecycle {
    limits: ContentLimits,
    pending: BTreeMap<u64, ClientContentCandidate>,
    presented: BTreeMap<(u64, u64), ClientPresentedContent>,
    actions: BTreeMap<u64, ContentAction>,
    action_high_water: u64,
    candidate_high_water: u64,
    facts_generation: u64,
    outputs: Vec<ContentOutputFactsEntry>,
}

impl ContentLifecycle {
    pub fn new(limits: ContentLimits) -> Result<Self, ContentLifecycleError> {
        limits
            .validate()
            .map_err(|_| ContentLifecycleError::InvalidCandidate)?;
        Ok(Self {
            limits,
            pending: BTreeMap::new(),
            presented: BTreeMap::new(),
            actions: BTreeMap::new(),
            action_high_water: 0,
            candidate_high_water: 0,
            facts_generation: 0,
            outputs: Vec::new(),
        })
    }

    /// Register the complete planned metadata when CandidateBegin is owned by
    /// the outbound queue, in Begin order. Partial assembly creates no targets;
    /// only a later native Presented can do that. Pixels stay in client slots.
    pub fn register(
        &mut self,
        candidate: ClientContentCandidate,
    ) -> Result<(), ContentLifecycleError> {
        let begin = &candidate.begin;
        if begin.grant != self.limits.grant {
            return Err(ContentLifecycleError::WrongGrant);
        }
        if begin.output.id == 0
            || begin.output.generation == 0
            || begin.facts_generation == 0
            || begin.pacing_permit == 0
            || begin.interaction_generation == 0
            || !candidate.transaction.is_valid()
            || begin.candidate_generation <= self.candidate_high_water
            || begin.surface_count as usize != candidate.surfaces.len()
            || begin.target_count as usize != candidate.targets.len()
            || begin.surface_count > self.limits.max_candidate_surfaces
            || begin.target_count > self.limits.max_candidate_targets
            || candidate
                .targets
                .iter()
                .any(|target| target.surface_index as usize >= candidate.surfaces.len())
            || self.pending.contains_key(&begin.candidate_generation)
            || (self.facts_generation != 0 && !self.current_output(&candidate))
        {
            return Err(ContentLifecycleError::InvalidCandidate);
        }
        let per_output = self
            .pending
            .values()
            .filter(|current| current.begin.output == begin.output)
            .count();
        if self.pending.len() >= self.limits.max_pending_candidates_total as usize
            || per_output
                >= (self.limits.max_pending_candidates_per_output
                    + self.limits.max_open_candidates_per_output) as usize
        {
            return Err(ContentLifecycleError::Capacity);
        }
        self.candidate_high_water = begin.candidate_generation;
        self.pending.insert(begin.candidate_generation, candidate);
        Ok(())
    }

    pub fn presented(&self, output: ContentOutputId) -> Option<&ClientPresentedContent> {
        self.presented.get(&(output.id, output.generation))
    }

    /// Settle action metadata after its receipt/effect workflow has completed.
    /// Old pending actions never keep their old targets active.
    pub fn finish_action(&mut self, event_id: u64) {
        self.actions.remove(&event_id);
    }

    pub fn dispatch(
        &mut self,
        transaction: TransactionId,
        record: ShellContentRecord,
    ) -> Result<ContentDispatch, ContentLifecycleError> {
        let grant = match &record {
            ShellContentRecord::Limits(value) => value.grant,
            ShellContentRecord::OutputFacts(value) => value.grant,
            ShellContentRecord::AllocationResult(value) => value.grant,
            ShellContentRecord::ResourceStatus(value) => value.grant,
            ShellContentRecord::ResourceReleased(value) => value.grant,
            ShellContentRecord::CandidateOutcome(value) => value.grant,
            ShellContentRecord::FramePermit(value) => value.grant,
            ShellContentRecord::Action(value) => value.grant,
            _ => return Err(ContentLifecycleError::WrongDirection),
        };
        if grant != self.limits.grant {
            return Err(ContentLifecycleError::WrongGrant);
        }
        let mut action_dispatch = None;
        match &record {
            ShellContentRecord::OutputFacts(facts) => {
                if facts.facts_generation <= self.facts_generation {
                    return Err(ContentLifecycleError::InvalidOutcome);
                }
                if facts.outputs.len() > self.limits.max_outputs as usize {
                    return Err(ContentLifecycleError::Capacity);
                }
                self.facts_generation = facts.facts_generation;
                self.outputs.clone_from(&facts.outputs);
                self.presented.retain(|_, current| {
                    candidate_matches_outputs(&current.candidate, &facts.outputs)
                });
            }
            ShellContentRecord::CandidateOutcome(outcome) => {
                if outcome.grant != self.limits.grant {
                    return Err(ContentLifecycleError::WrongGrant);
                }
                let candidate = self
                    .pending
                    .get(&outcome.candidate_generation)
                    .ok_or(ContentLifecycleError::UnknownCandidate)?;
                if candidate.transaction != transaction {
                    return Err(ContentLifecycleError::WrongTransaction);
                }
                if candidate.begin.output != outcome.output {
                    return Err(ContentLifecycleError::InvalidOutcome);
                }
                match outcome.kind {
                    1 => {
                        if outcome.reason != 0
                            || outcome.presentation_epoch != 0
                            || outcome.work_area_generation == 0
                            || outcome.wm_commit_generation == 0
                        {
                            return Err(ContentLifecycleError::InvalidOutcome);
                        }
                    } // Prepared retains the old active targets.
                    2 => {
                        if outcome.presentation_epoch == 0
                            || outcome.reason != 0
                            || (self.facts_generation != 0 && !self.current_output(candidate))
                        {
                            return Err(ContentLifecycleError::InvalidOutcome);
                        }
                        let key = (outcome.output.id, outcome.output.generation);
                        if self.presented.get(&key).is_some_and(|current| {
                            outcome.presentation_epoch <= current.presentation_epoch
                                || outcome.candidate_generation
                                    <= current.candidate.begin.candidate_generation
                        }) {
                            return Err(ContentLifecycleError::InvalidOutcome);
                        }
                        if !self.presented.contains_key(&key)
                            && self.presented.len() >= self.limits.max_outputs as usize
                        {
                            return Err(ContentLifecycleError::Capacity);
                        }
                        let candidate = self.pending.remove(&outcome.candidate_generation).unwrap();
                        self.presented.insert(
                            key,
                            ClientPresentedContent {
                                candidate,
                                presentation_epoch: outcome.presentation_epoch,
                            },
                        );
                    }
                    3 | 4 => {
                        self.pending.remove(&outcome.candidate_generation);
                    }
                    _ => return Err(ContentLifecycleError::InvalidOutcome),
                }
            }
            ShellContentRecord::Action(action) => {
                if action.grant != self.limits.grant {
                    return Err(ContentLifecycleError::WrongGrant);
                }
                if action.kind == 3 {
                    if self
                        .actions
                        .get(&action.event_id)
                        .is_some_and(|pending| same_action(pending, action))
                    {
                        self.actions.remove(&action.event_id);
                    }
                    action_dispatch = Some(ContentActionDispatch::Cancelled);
                } else {
                    let eligible = action.reason == 0
                        && action.event_id > self.action_high_water
                        && self.action_matches(action);
                    if eligible && self.actions.len() >= self.limits.max_pending_actions as usize {
                        return Err(ContentLifecycleError::Capacity);
                    }
                    self.action_high_water = self.action_high_water.max(action.event_id);
                    if eligible {
                        self.actions.insert(action.event_id, action.clone());
                    }
                    action_dispatch = Some(if eligible {
                        ContentActionDispatch::Eligible
                    } else {
                        ContentActionDispatch::Rejected
                    });
                }
            }
            // Resource outcomes are deliberately independent of presentation.
            _ => {}
        }
        Ok(ContentDispatch {
            transaction,
            record,
            action: action_dispatch,
        })
    }

    fn current_output(&self, candidate: &ClientContentCandidate) -> bool {
        candidate_matches_outputs(candidate, &self.outputs)
    }

    fn action_matches(&self, action: &ContentAction) -> bool {
        let Some(current) = self.presented(action.output) else {
            return false;
        };
        let candidate = &current.candidate;
        candidate.begin.candidate_generation == action.candidate_generation
            && current.presentation_epoch == action.presentation_epoch
            && candidate.begin.interaction_generation == action.interaction_generation
            && candidate.targets.iter().any(|target| {
                candidate.surfaces[target.surface_index as usize].allocation == action.allocation
                    && target.target_id == action.target_id
                    && target.target_generation == action.target_generation
                    && target.action_id == action.action_id
                    && target.action_kind == action.kind
            })
    }
}

fn candidate_matches_outputs(
    candidate: &ClientContentCandidate,
    outputs: &[ContentOutputFactsEntry],
) -> bool {
    outputs.iter().any(|entry| {
        entry.output == candidate.begin.output
            && candidate
                .surfaces
                .iter()
                .all(|surface| surface.scale_generation == entry.scale_generation)
    })
}

fn same_action(left: &ContentAction, right: &ContentAction) -> bool {
    let mut expected = left.clone();
    expected.kind = right.kind;
    expected.reason = right.reason;
    expected == *right
}
