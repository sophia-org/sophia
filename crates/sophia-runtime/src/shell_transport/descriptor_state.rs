//! Descriptor request, presentation and activation state belongs to the
//! component epoch. The file decoder cannot commit this state.
use sophia_protocol::{
    SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS, ShellV1DescriptorSnapshot, TransactionId,
};
use std::collections::VecDeque;

pub(super) struct DescriptorState<S = ShellV1DescriptorSnapshot> {
    pub(super) last_candidate_generation: u64,
    pub(super) requested_candidate: Option<(TransactionId, S)>,
    pub(super) pending_candidate: Option<PendingShellCandidate>,
    pub(super) presented_candidate: Option<(u64, u64)>,
    pub(super) pending_activations: VecDeque<(TransactionId, u64)>,
    /// Exact file response obligations: Prepared and one terminal outcome.
    pub(super) response_credits: usize,
    pub(super) unmatched_acks: u64,
}

impl<S> Default for DescriptorState<S> {
    fn default() -> Self {
        Self {
            last_candidate_generation: 0,
            requested_candidate: None,
            pending_candidate: None,
            presented_candidate: None,
            pending_activations: VecDeque::with_capacity(SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS),
            response_credits: 0,
            unmatched_acks: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingShellCandidate {
    pub(super) transaction: TransactionId,
    pub(super) generation: u64,
    pub(super) visible: bool,
    pub(super) prepared: bool,
}
