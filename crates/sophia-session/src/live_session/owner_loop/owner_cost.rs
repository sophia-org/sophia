//! Diagnostic-only owner CPU accounting; records are emitted after the loop exits.
#[derive(Clone, Copy)]
pub(super) enum Phase {
    Other = 0,
    Components = 1,
    Native = 2,
    Authority = 3,
}

#[cfg(not(feature = "diagnostic-owner-cost"))]
pub(super) struct Recorder;
#[cfg(not(feature = "diagnostic-owner-cost"))]
impl Recorder {
    pub(super) fn new(_: u64) -> Self {
        Self
    }
    pub(super) fn begin_pass(&mut self, _: [u64; 2], _: (u8, u8), _: [u64; 2]) {}
    pub(super) fn phase(&mut self, _: Phase) {}
    pub(super) fn measure<T>(&mut self, _: Phase, f: impl FnOnce() -> T) -> T {
        f()
    }
    pub(super) fn finish(self, _: bool, _: u64, _: (u8, u8)) {}
}

#[cfg(feature = "diagnostic-owner-cost")]
pub(super) use enabled::Recorder;
#[cfg(feature = "diagnostic-owner-cost")]
#[path = "owner_cost/enabled.rs"]
mod enabled;
