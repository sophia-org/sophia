//! Aggregate wait attribution. Equal caps keep the first reason in caller order.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(usize)]
pub(crate) enum WaitReason {
    #[default]
    Maintenance,
    Input,
    InputReceipts,
    Frames,
    Topology,
    Seat,
    ShellInteraction,
    Lifecycle,
    Cursor,
    Controls,
    Proof,
    FrameDeadline,
    Present,
    NativeDeadline,
    Shortcut,
    ShellOutput,
    Pacer,
    Service,
}

const NAMES: [&str; 18] = [
    "maintenance",
    "input",
    "input_receipts",
    "frames",
    "topology",
    "seat",
    "shell_interaction",
    "lifecycle",
    "cursor",
    "controls",
    "proof",
    "frame_deadline",
    "present",
    "native_deadline",
    "shortcut",
    "shell_output",
    "pacer",
    "service",
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct WaitPlan {
    pub timeout: Duration,
    pub reason: WaitReason,
    pending: u32,
}

impl WaitPlan {
    pub fn new(timeout: Duration, reason: WaitReason) -> Self {
        Self {
            timeout,
            reason,
            pending: 0,
        }
    }

    pub fn pending(&mut self, reason: WaitReason) {
        self.pending |= 1 << reason as usize;
    }

    pub fn cap(&mut self, timeout: Duration, reason: WaitReason) {
        if timeout < self.timeout {
            self.timeout = timeout;
            self.reason = reason;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WaitAttribution {
    selected: [u64; NAMES.len()],
    expired: [u64; NAMES.len()],
    pending: [u64; NAMES.len()],
}

impl WaitAttribution {
    pub fn observe(&mut self, plan: WaitPlan, expired: bool) {
        let index = plan.reason as usize;
        self.selected[index] = self.selected[index].saturating_add(1);
        self.expired[index] = self.expired[index].saturating_add(u64::from(expired));
        for (i, total) in self.pending.iter_mut().enumerate() {
            *total = total.saturating_add(u64::from(plan.pending & (1 << i) != 0));
        }
    }
}

impl std::fmt::Display for WaitAttribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, name) in NAMES.iter().enumerate() {
            write!(
                f,
                " selected_{name}={} expired_{name}={} pending_{name}={}",
                self.selected[i], self.expired[i], self.pending[i]
            )?;
        }
        Ok(())
    }
}
