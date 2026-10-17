//! Passive output-role negotiation, proposal and settlement vocabulary.
//! The historical V1 names remain source-compatible while the socket adapter
//! is retired. These values contain no framing or transport behavior.
use crate::{OutputAuthoritySnapshot, OutputTopologyCandidate};

pub const SOPHIA_OUTPUT_INTERFACE_MAJOR: u16 = 1;
pub const SOPHIA_OUTPUT_INTERFACE_REVISION: u16 = 1;
pub const SOPHIA_OUTPUT_CAPABILITY_OBSERVE: u64 = 1 << 0;
pub const SOPHIA_OUTPUT_CAPABILITY_CONFIGURE: u64 = 1 << 1;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_NONE: u16 = 0;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_STALE: u16 = 1;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_PREPARATION: u16 = 2;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_APPLY: u16 = 3;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_HEAD_LOST: u16 = 4;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION: u16 = 5;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_ROLLBACK: u16 = 6;
pub const SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT: u16 = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputV1ClientHello {
    pub minimum_revision: u16,
    pub maximum_revision: u16,
    pub capabilities: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputV1ServerWelcome {
    pub selected_revision: u16,
    pub capabilities: u64,
    pub connection_epoch: u64,
    pub max_heads: u16,
    pub max_groups: u16,
    pub max_modes_per_head: u16,
    pub max_heads_per_group: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputV1Snapshot {
    pub connection_epoch: u64,
    pub snapshot: OutputAuthoritySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputV1Proposal {
    pub connection_epoch: u64,
    pub candidate: OutputTopologyCandidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputV1OutcomeKind {
    Validated,
    Committed,
    Stale,
    Rejected,
    RolledBack,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputV1Outcome {
    pub connection_epoch: u64,
    pub topology_epoch: u64,
    pub kind: OutputV1OutcomeKind,
    /// Stable reduced reason code. Zero means the outcome needs no detail.
    pub reason: u16,
}
