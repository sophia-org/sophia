//! Passive descriptor-role records, extracted from Sophia 262eb82bc.
//! This typed model does not select a transport or grant admission.
use crate::{AttentionState, DisplayLabel, OutputId, TrustLevel};

mod validation;
pub use validation::*;

pub const SOPHIA_SHELL_MAX_DESCRIPTORS: usize = 16;
pub const SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS: usize = 16;
/// Structural wire bound only. Engine still clamps an admitted reservation to
/// the realized output extents; this cap exists so a malformed frame cannot
/// promise a thickness no output could ever satisfy.
pub const SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX: u16 = 512;

/// A broker-issued, shell-recipient-scoped toplevel activation capability.
///
/// The record name fixes the issuer, recipient, and operation class. Its wire
/// integer has no meaning outside this family and exact epoch tuple.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ToplevelActionCapabilityRef {
    pub token: u64,
    pub issuer_epoch: u64,
    pub issuer_revocation_epoch: u64,
    pub recipient_epoch: u64,
    pub target_slot: u16,
    pub target_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellV1Descriptor {
    pub slot: u16,
    pub generation: u64,
    pub label: Option<DisplayLabel>,
    pub trust_level: TrustLevel,
    pub attention: AttentionState,
    pub action: ToplevelActionCapabilityRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellV1DescriptorSnapshot {
    pub connection_epoch: u64,
    pub snapshot_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    pub broker_epoch: u64,
    pub broker_revocation_epoch: u64,
    pub descriptors: Vec<ShellV1Descriptor>,
}

/// The output edge a shell candidate reserves for itself.
///
/// There is no `None` variant: a candidate that reserves nothing carries no
/// reservation record at all, so absence is unrepresentable rather than a
/// zero someone forgets to check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellV1ReservationEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// One exclusive work-area zone a shell candidate claims on its output.
///
/// The reservation rides on the candidate rather than a separate request
/// stream because the candidate owns both visuals and reservation: Engine
/// derives the work-area snapshot from this exact candidate, the WM answers
/// that exact snapshot, and presentation commits the coherent bundle or
/// nothing (Sophia's `ShellWorkAreaCoordination` model). Withdrawal is a
/// later candidate carrying no reservation, through the same path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1WorkAreaReservation {
    pub edge: ShellV1ReservationEdge,
    pub thickness_px: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1CandidateEntry {
    pub slot: u16,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellV1Candidate {
    pub connection_epoch: u64,
    pub snapshot_generation: u64,
    pub candidate_generation: u64,
    pub output: OutputId,
    pub visible: bool,
    pub selected_slot: Option<u16>,
    pub reservation: Option<ShellV1WorkAreaReservation>,
    pub entries: Vec<ShellV1CandidateEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellV1CandidateOutcomeKind {
    Prepared,
    Presented,
    Rejected,
    Superseded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1CandidateOutcome {
    pub connection_epoch: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub kind: ShellV1CandidateOutcomeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1Activation {
    pub connection_epoch: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub activation: u64,
    pub action: ToplevelActionCapabilityRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellV1ActivationDisposition {
    Consumed,
    RejectedStale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1ActivationAck {
    pub connection_epoch: u64,
    pub activation: u64,
    pub disposition: ShellV1ActivationDisposition,
}
