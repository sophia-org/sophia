use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PresentedPointerFocus {
    pub(super) output: sophia_protocol::OutputId,
    pub(super) target: Option<SurfaceId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PhysicalPolicyInput {
    Action(WmActionId),
    /// A shortcut whose action the WM follows as a chord. It opened or joined
    /// `chord`, from `trigger`; whoever does not admit it tells the router.
    ChordAction(PhysicalChordAction),
    /// Held or Ended for a chord, in the order the router reported it.
    Chord(sophia_engine::WmChordEvent),
    PresentedAction(sophia_engine::PresentedPolicyAction),
    ClickFocus(SurfaceId),
    Hover(PresentedPointerFocus),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PhysicalChordAction {
    pub(super) action: WmActionId,
    pub(super) chord: sophia_engine::WmChordActivation,
    pub(super) device: sophia_protocol::DeviceId,
    pub(super) keycode: u32,
}

/// Move the router's lifecycle events into the policy inputs at this event's
/// boundary, so Action, Held and Ended keep one order.
pub(super) fn drain_chord_events(
    router: Option<&mut WmShortcutRouter>,
    inputs: &mut Vec<PhysicalPolicyInput>,
) {
    if let Some(router) = router {
        inputs.extend(
            router
                .drain_chord_events()
                .into_iter()
                .map(PhysicalPolicyInput::Chord),
        );
    }
}

impl PhysicalChordAction {
    /// Tell the router this activation was never admitted, so an opener's
    /// chord is dropped with its credit and a join's trigger stops holding.
    pub(super) fn refuse(self, router: Option<&mut WmShortcutRouter>) {
        let Some(router) = router else { return };
        if self.chord.opens {
            router.chord_opener_refused(self.chord.token);
        } else {
            router.chord_join_refused(self.chord.token, self.device, self.keycode);
        }
    }
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
            // The credits bound Ended, so the input bound never drops one.
            PhysicalPolicyInput::Chord(sophia_engine::WmChordEvent::Ended { .. }) => {
                self.pending.push_back(input);
                return true;
            }
            PhysicalPolicyInput::Action(_)
            | PhysicalPolicyInput::ChordAction(_)
            | PhysicalPolicyInput::Chord(_)
            | PhysicalPolicyInput::PresentedAction(_)
            | PhysicalPolicyInput::ClickFocus(_) => {}
        }
        if self.pending.len() >= 256 {
            return false;
        }
        self.pending.push_back(input);
        true
    }

    /// Admit one routed input at the first bound. A chord action the queue
    /// cannot take is refused to the router at once, so an opener's credit
    /// returns and a join's trigger stops holding; an Ended the refusal causes
    /// joins the back of this same queue, behind everything already in it.
    pub(super) fn admit(
        &mut self,
        input: PhysicalPolicyInput,
        hover_enabled: bool,
        router: Option<&mut WmShortcutRouter>,
    ) -> bool {
        if self.push(input, hover_enabled) {
            return true;
        }
        if let PhysicalPolicyInput::ChordAction(chorded) = input
            && let Some(router) = router
        {
            chorded.refuse(Some(&mut *router));
            for event in router.drain_chord_events() {
                self.push(PhysicalPolicyInput::Chord(event), hover_enabled);
            }
        }
        false
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Put an input back at the head, for a hold that must keep every input
    /// after it waiting in order.
    pub(super) fn hold(&mut self, input: PhysicalPolicyInput) {
        self.pending.push_front(input);
    }

    pub(super) fn next(&mut self, hover_pending: bool) -> Option<PhysicalPolicyInput> {
        if hover_pending {
            None
        } else {
            self.pending.pop_front()
        }
    }
}
