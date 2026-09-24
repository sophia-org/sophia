use super::*;
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LivePageFlipCallbackQueueReport {
    pub drained: usize,
    pub accepted: usize,
    pub rejected_unexpected_output: usize,
    pub rejected_stale_frame_serial: usize,
    pub last_accepted: Option<LivePageFlipCallbackReport>,
    /// Accepted callbacks retain physical-head identity for mirror-group
    /// retirement. `last_accepted` is kept for the single-head API.
    pub accepted_callbacks: Vec<LivePageFlipCallback>,
    pub disconnected: bool,
    pub max_reached: bool,
}

impl LivePageFlipCallbackQueueReport {
    // Its callers live in the native scanout, which is compiled only with
    // the libdrm-events and gbm-probe features; without them the
    // constructor has no caller, and a workspace clippy without features
    // read that as dead code.
    #[cfg_attr(
        not(all(feature = "libdrm-events", feature = "gbm-probe")),
        allow(dead_code)
    )]
    pub(crate) fn with_accepted_capacity(capacity: usize) -> Self {
        Self {
            accepted_callbacks: Vec::with_capacity(capacity),
            ..Self::default()
        }
    }

    pub(crate) fn record_observation(
        &mut self,
        callback: LivePageFlipCallback,
        callback_report: LivePageFlipCallbackReport,
    ) {
        self.drained = self.drained.saturating_add(1);
        self.record_decision(callback_report.decision);
        if callback_report.decision == LivePageFlipCallbackDecision::Accepted {
            self.last_accepted = Some(callback_report);
            self.accepted_callbacks.push(callback);
        }
    }

    fn record_decision(&mut self, decision: LivePageFlipCallbackDecision) {
        match decision {
            LivePageFlipCallbackDecision::Accepted => {
                self.accepted = self.accepted.saturating_add(1);
            }
            LivePageFlipCallbackDecision::RejectedUnexpectedOutput => {
                self.rejected_unexpected_output = self.rejected_unexpected_output.saturating_add(1);
            }
            LivePageFlipCallbackDecision::RejectedStaleFrameSerial => {
                self.rejected_stale_frame_serial =
                    self.rejected_stale_frame_serial.saturating_add(1);
            }
        }
    }
}

pub struct LivePageFlipCallbackQueue {
    receiver: Receiver<LivePageFlipCallback>,
    max_drain_per_tick: usize,
}

impl LivePageFlipCallbackQueue {
    pub fn new(receiver: Receiver<LivePageFlipCallback>, max_drain_per_tick: usize) -> Self {
        Self {
            receiver,
            max_drain_per_tick,
        }
    }

    pub(crate) fn drain_ready_with(
        &self,
        mut observe: impl FnMut(LivePageFlipCallback) -> LivePageFlipCallbackReport,
    ) -> LivePageFlipCallbackQueueReport {
        let mut report = LivePageFlipCallbackQueueReport::default();

        for _ in 0..self.max_drain_per_tick {
            match self.receiver.try_recv() {
                Ok(callback) => {
                    let callback_report = observe(callback);
                    report.record_observation(callback, callback_report);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    report.disconnected = true;
                    break;
                }
            }
        }

        report.max_reached = report.drained == self.max_drain_per_tick;
        report
    }
}
