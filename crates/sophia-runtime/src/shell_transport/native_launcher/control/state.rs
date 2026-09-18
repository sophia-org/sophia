use super::*;

#[derive(Clone, Copy)]
pub(crate) struct NativePresented {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub content: crate::NativeLauncherCandidateBinding,
}

#[derive(Clone, Copy)]
pub(super) struct InputReceipt {
    pub event: NativeLauncherEvent,
    pub kind: NativeLauncherInputKind,
    pub ack: Option<u16>,
    pub issued: u64,
}
#[derive(Clone, Copy)]
pub(super) struct AcceptIntent {
    pub transaction: TransactionId,
    pub revision: u64,
    pub issued: u64,
}

/// Session-owned connection state. No pixel lease or second candidate owner.
/// The only Presented source is the actual candidate store's successful terminal
/// transition. Input receipts retain their original binding across replacements.
pub(in crate::shell_transport) struct NativeControl {
    pub opening: Option<NativeLauncherOpening>,
    pub presented: Option<NativePresented>,
    pub focus: Option<NativeLauncherBinding>,
    pub revision: u64,
    pub last_opening: u64,
    pub next_lease: u64,
    pub next_event: u64,
    pub last_issued: u64,
    pub last_service: u64,
    pub closing: Option<(TransactionId, u16)>,
    pub(super) inputs: [Option<InputReceipt>; 16],
    pub(super) accept: Option<AcceptIntent>,
}
impl Default for NativeControl {
    fn default() -> Self {
        Self {
            opening: None,
            presented: None,
            focus: None,
            revision: 0,
            last_opening: 0,
            next_lease: 1,
            next_event: 1,
            last_issued: 0,
            last_service: 0,
            closing: None,
            inputs: [None; 16],
            accept: None,
        }
    }
}
impl NativeControl {
    pub(in crate::shell_transport) fn input_occupancy(&self) -> usize {
        self.inputs.iter().flatten().count() + usize::from(self.accept.is_some())
    }
    pub(in crate::shell_transport) fn credits(&self) -> usize {
        usize::from(self.opening.is_some())
            + usize::from(self.focus.is_some())
            + usize::from(self.accept.is_some())
    }
    pub(super) fn active(&self) -> bool {
        self.opening.is_some() && self.closing.is_none()
    }
    pub(super) fn clear_opening(&mut self) {
        self.opening = None;
        self.presented = None;
        self.focus = None;
        self.accept = None;
        self.closing = None;
        self.inputs = [None; 16];
        self.revision = 0;
    }
    pub(super) fn slot(&self, maximum: usize) -> Option<usize> {
        if self.inputs.iter().flatten().count() >= maximum {
            return None;
        }
        self.inputs.iter().position(Option::is_none)
    }
}
