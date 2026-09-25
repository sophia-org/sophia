use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PresentedPointerFocus {
    pub(super) output: sophia_protocol::OutputId,
    pub(super) target: Option<SurfaceId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhysicalPolicyInput {
    Action(WmActionId),
    PresentedAction(sophia_engine::PresentedPolicyAction),
    ClickFocus(SurfaceId),
    Hover(PresentedPointerFocus),
}

/// Holds shortcut ordering across an asynchronous hover-focus commit. Adjacent
/// motion is replaceable; a shortcut is an ordering boundary, never coalesced.
#[derive(Default)]
pub(super) struct PhysicalPolicyInputQueue {
    epoch: Option<u64>,
    pending: VecDeque<PhysicalPolicyInput>,
}

impl PhysicalPolicyInputQueue {
    pub(super) fn synchronize(&mut self, epoch: Option<u64>) {
        if self.epoch != epoch {
            self.epoch = epoch;
            self.pending.clear();
        }
    }

    pub(super) fn push(&mut self, input: PhysicalPolicyInput, hover_enabled: bool) -> bool {
        match input {
            PhysicalPolicyInput::Hover(observation) => {
                if !hover_enabled {
                    return true;
                }
                if let Some(PhysicalPolicyInput::Hover(pending)) = self.pending.back_mut() {
                    *pending = observation;
                    return true;
                }
            }
            PhysicalPolicyInput::Action(_) | PhysicalPolicyInput::PresentedAction(_) | PhysicalPolicyInput::ClickFocus(_) => {}
        }
        if self.pending.len() >= 256 {
            return false;
        }
        self.pending.push_back(input);
        true
    }

    pub(super) fn next(&mut self, hover_pending: bool) -> Option<PhysicalPolicyInput> {
        if hover_pending {
            None
        } else {
            self.pending.pop_front()
        }
    }
}
