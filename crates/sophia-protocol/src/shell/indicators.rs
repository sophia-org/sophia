//! Wire-neutral typed model for the revision-6 shell indicator vocabulary.
use crate::InvalidRecord;
use crate::OutputId;

pub const SOPHIA_SHELL_INDICATOR_REVISION: u16 = 6;
pub const SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS: u64 = 1 << 9;
pub const SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION: u64 = 1 << 10;
pub const SOPHIA_SHELL_MAX_INDICATORS: usize = 256;
pub const SOPHIA_SHELL_MAX_OUTPUT_STATUS: usize = 16;
pub const SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES: usize = 32;

/// One selectable view pill. `indicator` and `action` are opaque identities the
/// policy client authored; the shell presents them and echoes them back. It
/// never learns which view it is choosing, and cannot name one that was not
/// published to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellIndicator {
    pub output: OutputId,
    pub indicator: u64,
    pub action: u64,
    pub slot: u32,
    pub state_bits: u16,
    pub label: String,
}

/// Per-output layout identity and focus bits, mirroring the projection record
/// the Engine already holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellOutputStatus {
    pub output: OutputId,
    pub focus_bits: u16,
    pub layout: String,
}

/// A complete replacement of the indicator set.
///
/// `active_output` is one global optional identity rather than a per-output
/// flag. Per-output booleans can disagree with each other, and the case this
/// exists for is an output that is focused while holding no window, so there is
/// no seat focus to infer it from and nothing else on the wire would say so.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellIndicatorSnapshot {
    pub connection_epoch: u64,
    pub generation: u64,
    pub active_output: Option<OutputId>,
    pub statuses: Vec<ShellOutputStatus>,
    pub indicators: Vec<ShellIndicator>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellIndicatorActivation {
    pub connection_epoch: u64,
    pub snapshot_generation: u64,
    pub output: OutputId,
    pub indicator: u64,
    pub action: u64,
    pub event_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ShellIndicatorActivationStatus {
    Accepted = 0,
    Stale = 1,
    Unknown = 2,
    Unauthorized = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellIndicatorActivationOutcome {
    pub connection_epoch: u64,
    pub snapshot_generation: u64,
    pub event_id: u64,
    pub status: ShellIndicatorActivationStatus,
    pub reason: u16,
}

fn require(ok: bool, field: &'static str) -> Result<(), InvalidRecord> {
    if ok {
        Ok(())
    } else {
        Err(InvalidRecord(field))
    }
}

/// The bound check `sophia_protocol::ipc::shell_indicators` used to run
/// inline before building its Begin/OutputStatus/Entry/End transfer: a
/// snapshot's indicator and output-status counts stay within the granted
/// maxima. Label bounds are enforced by the fixed-width text encoding
/// itself, not restated here.
pub(crate) fn validate(snapshot: &ShellIndicatorSnapshot) -> Result<(), InvalidRecord> {
    require(
        snapshot.indicators.len() <= SOPHIA_SHELL_MAX_INDICATORS,
        "shell indicator count",
    )?;
    require(
        snapshot.statuses.len() <= SOPHIA_SHELL_MAX_OUTPUT_STATUS,
        "shell indicator output status count",
    )
}
