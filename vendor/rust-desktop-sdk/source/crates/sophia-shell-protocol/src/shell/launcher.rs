//! Passive descriptor-role records, extracted from Sophia 262eb82bc.
//! This typed model does not select a transport or grant admission.
use crate::{OutputId, ShellV1CandidateOutcomeKind};

mod validation;
pub use validation::*;

pub const SOPHIA_SHELL_LAUNCHER_REVISION: u16 = 4;
pub const SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG: u64 = 1 << 5;
pub const SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER: u64 = 1 << 6;
pub const SOPHIA_SHELL_MAX_QUERY_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellLauncherOperation {
    Open = 0,
    Query = 1,
    Next = 2,
    Previous = 3,
    Dismiss = 4,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellLauncherRequest {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    pub presentation_epoch: u64,
    pub operation: ShellLauncherOperation,
    /// Focus-scoped committed text, never physical key events.
    pub query: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellLauncherCandidate {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub output: OutputId,
    pub visible: bool,
    pub selected: u16,
    pub entries: Vec<u16>,
    pub font_size: u16,
    /// Background, foreground, selection background, selection foreground.
    pub colors: [u32; 4],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellLauncherOutcome {
    pub connection_epoch: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub kind: ShellV1CandidateOutcomeKind,
}
/// Only Engine can originate this record from a retired, input-eligible target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellLauncherActivation {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub request_generation: u64,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub activation: u64,
    pub slot: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellLauncherActivationAck {
    pub activation: ShellLauncherActivation,
    pub consumed: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellLaunchStatus {
    Started = 1,
    Rejected = 2,
    Failed = 3,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellLaunchOutcome {
    pub activation: ShellLauncherActivation,
    pub status: ShellLaunchStatus,
}
