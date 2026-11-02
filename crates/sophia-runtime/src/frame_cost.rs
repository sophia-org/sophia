//! Opt-in per-thread stage timers for device-free Session frame attribution.
//!
//! Off at compile time without the `diagnostic-frame-cost` feature and off at
//! run time until [`enable`]. Scopes are inclusive and may nest, so stage
//! totals are never summed. Each thread accumulates its own counters and an
//! outer CPU/elapsed window that is sampled whether or not timers are enabled.
//! A thread publishes a [`Snapshot`] to the drain queue when it observes a new
//! block generation (see [`begin_block`]) and when it exits. A generation
//! change while a timed scope is open invalidates both generations instead of
//! merging clocks across the boundary.

mod record;
#[cfg(all(feature = "diagnostic-frame-cost", target_os = "linux"))]
pub(crate) mod state;
#[cfg(not(all(feature = "diagnostic-frame-cost", target_os = "linux")))]
pub use disabled::*;
#[cfg(all(feature = "diagnostic-frame-cost", target_os = "linux"))]
pub use state::{begin_block, drain, enable, enabled, measure, register_thread, snapshot_thread};

/// Inclusive work scopes: runtime owned-ring path, then Session service, then
/// the harness-only supplied target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Stage {
    Transport,
    DecodeJournal,
    Authority,
    Admission,
    Reserve,
    Settle,
    SourceRelease,
    Retire,
    Collect,
    Validation,
    Copy,
    Pixels,
    OwnerService,
    Outputs,
    Allocations,
    Demands,
    Candidates,
    Projection,
    Submission,
    Completion,
    Logging,
    /// Work of a supplied deterministic target in a harness, never Session work.
    FixtureTarget,
}

impl Stage {
    pub const COUNT: usize = 22;
    pub const ALL: [Stage; Stage::COUNT] = [
        Stage::Transport,
        Stage::DecodeJournal,
        Stage::Authority,
        Stage::Admission,
        Stage::Reserve,
        Stage::Settle,
        Stage::SourceRelease,
        Stage::Retire,
        Stage::Collect,
        Stage::Validation,
        Stage::Copy,
        Stage::Pixels,
        Stage::OwnerService,
        Stage::Outputs,
        Stage::Allocations,
        Stage::Demands,
        Stage::Candidates,
        Stage::Projection,
        Stage::Submission,
        Stage::Completion,
        Stage::Logging,
        Stage::FixtureTarget,
    ];

    /// Snake-case key used in record lines.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Transport => "transport",
            Stage::DecodeJournal => "decode_journal",
            Stage::Authority => "authority",
            Stage::Admission => "admission",
            Stage::Reserve => "reserve",
            Stage::Settle => "settle",
            Stage::SourceRelease => "source_release",
            Stage::Retire => "retire",
            Stage::Collect => "collect",
            Stage::Validation => "validation",
            Stage::Copy => "copy",
            Stage::Pixels => "pixels",
            Stage::OwnerService => "owner_service",
            Stage::Outputs => "outputs",
            Stage::Allocations => "allocations",
            Stage::Demands => "demands",
            Stage::Candidates => "candidates",
            Stage::Projection => "projection",
            Stage::Submission => "submission",
            Stage::Completion => "completion",
            Stage::Logging => "logging",
            Stage::FixtureTarget => "fixture_target",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Worker,
    /// Synthetic peer thread of a fixture; its work is never owner CPU.
    Peer,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Worker => "worker",
            Role::Peer => "peer",
        }
    }
}

/// Why a snapshot was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The thread observed a new generation from [`begin_block`].
    Block,
    /// [`snapshot_thread`] peeked without resetting.
    Explicit,
    /// The thread exited.
    Drop,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::Block => "block",
            Reason::Explicit => "explicit",
            Reason::Drop => "drop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageCost {
    pub calls: u64,
    /// Scopes left by unwinding; their clocks are still accumulated.
    pub aborted: u64,
    pub cpu_nsec: u64,
    pub elapsed_nsec: u64,
    pub max_cpu_nsec: u64,
    pub max_elapsed_nsec: u64,
}

/// One thread's accounting for one generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub role: Role,
    pub reason: Reason,
    pub tid: u32,
    /// Process-wide order in which touched threads opened their state.
    pub thread_sequence: u64,
    pub generation: u64,
    /// Outer window, sampled at open/reset and at publish regardless of `enabled`.
    pub cpu_first_nsec: u64,
    pub cpu_now_nsec: u64,
    pub mono_first_nsec: u64,
    pub mono_now_nsec: u64,
    /// Timer entries taken while enabled and while disabled.
    pub enabled_calls: u64,
    pub disabled_calls: u64,
    /// Indexed by `Stage as usize`.
    pub stages: [StageCost; Stage::COUNT],
    pub clock_regressions: u32,
    pub saturations: u32,
    /// Set by any regression, saturation, failed clock read or generation
    /// change inside an open scope; the block is void.
    pub invalid: bool,
}

#[cfg(not(all(feature = "diagnostic-frame-cost", target_os = "linux")))]
mod disabled {
    use super::{Role, Snapshot, Stage};

    pub fn enable(_on: bool) {}
    pub fn enabled() -> bool {
        false
    }
    pub fn begin_block() -> u64 {
        0
    }
    pub fn drain() -> Vec<Snapshot> {
        Vec::new()
    }
    pub fn snapshot_thread() -> Option<Snapshot> {
        None
    }
    pub fn register_thread(_role: Role) {}
    #[inline(always)]
    pub fn measure<T>(_stage: Stage, f: impl FnOnce() -> T) -> T {
        f()
    }
}
