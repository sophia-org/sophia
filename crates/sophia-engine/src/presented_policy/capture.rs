use super::PresentedPolicyState;
use sophia_protocol::{DeviceId, PolicyPresentationIdentity, SeatId, WmActionId, WmModifierMask};
use std::collections::BTreeMap;

const MAX_POLICY_INPUT_DEBTS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentedPolicyAction {
    pub connection_epoch: u64,
    pub action: WmActionId,
    pub identity: PolicyPresentationIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyPointerHit {
    Pass,
    Blocked,
    Action(PresentedPolicyAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyInputDisposition {
    Pass,
    Consumed,
    Action(PresentedPolicyAction),
    CapacityExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Press {
    Key(u32),
    Button(u32),
}

/// Captured releases outlive publication, connection and action-queue state.
/// Application-owned sequences are routed by their existing owner before new
/// policy admission. Capacity failure latches closed until the physical input
/// owner is torn down; it must not forget an unrecorded consumed press.
#[derive(Debug, Default)]
pub struct PolicyInputCapture {
    debts: BTreeMap<(SeatId, DeviceId, Press), Option<PresentedPolicyAction>>,
    exhausted: bool,
}

impl PolicyInputCapture {
    /// Visible pixels without current input authority still swallow new keys.
    /// Existing application sequences retain their release obligation.
    pub fn block_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        application_capture_active: bool,
    ) -> PolicyInputDisposition {
        let key = (seat, device, Press::Key(keycode));
        if !pressed && self.debts.remove(&key).is_some() {
            return PolicyInputDisposition::Consumed;
        }
        if self.debts.contains_key(&key) {
            return PolicyInputDisposition::Consumed;
        }
        if application_capture_active {
            return PolicyInputDisposition::Pass;
        }
        if self.exhausted {
            return PolicyInputDisposition::Consumed;
        }
        if !pressed {
            return PolicyInputDisposition::Pass;
        }
        if self.remember(key, None) {
            PolicyInputDisposition::Consumed
        } else {
            PolicyInputDisposition::CapacityExceeded
        }
    }
    pub fn revoke(&mut self) {
        for target in self.debts.values_mut() {
            *target = None;
        }
    }

    pub fn remove_device(&mut self, device: DeviceId) {
        self.debts.retain(|(_, held, _), _| *held != device);
    }

    /// A protected owner consumed this event before policy routing. Retire its
    /// terminal debt without allowing a later release to reuse the old press.
    pub fn discard_release(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        kind: sophia_protocol::InputEventKind,
    ) {
        let press = match kind {
            sophia_protocol::InputEventKind::Key {
                keycode,
                pressed: false,
            } => Press::Key(keycode),
            sophia_protocol::InputEventKind::PointerButton {
                button,
                pressed: false,
            } => Press::Button(button),
            _ => return,
        };
        self.debts.remove(&(seat, device, press));
    }

    pub fn exhausted(&self) -> bool {
        self.exhausted
    }

    fn remember(
        &mut self,
        key: (SeatId, DeviceId, Press),
        action: Option<PresentedPolicyAction>,
    ) -> bool {
        if self.debts.len() >= MAX_POLICY_INPUT_DEBTS {
            self.revoke();
            self.exhausted = true;
            return false;
        }
        self.debts.insert(key, action);
        true
    }

    pub fn pointer(
        &mut self,
        state: &PresentedPolicyState,
        seat: SeatId,
        device: DeviceId,
        button: u32,
        pressed: bool,
        hit: PolicyPointerHit,
        application_owned: bool,
    ) -> PolicyInputDisposition {
        let key = (seat, device, Press::Button(button));
        if !pressed && let Some(captured) = self.debts.remove(&key) {
            return match captured {
                Some(action)
                    if hit == PolicyPointerHit::Action(action)
                        && state.action_is_current(
                            action.connection_epoch,
                            action.action,
                            action.identity,
                        ) =>
                {
                    PolicyInputDisposition::Action(action)
                }
                _ => PolicyInputDisposition::Consumed,
            };
        }
        if self.debts.contains_key(&key) {
            return PolicyInputDisposition::Consumed;
        }
        if application_owned {
            return PolicyInputDisposition::Pass;
        }
        if self.exhausted {
            return PolicyInputDisposition::Consumed;
        }
        if !pressed || hit == PolicyPointerHit::Pass {
            return PolicyInputDisposition::Pass;
        }
        let action = match hit {
            PolicyPointerHit::Action(action) => Some(action),
            _ => None,
        };
        if self.remember(key, action) {
            PolicyInputDisposition::Consumed
        } else {
            PolicyInputDisposition::CapacityExceeded
        }
    }

    pub fn key(
        &mut self,
        state: &PresentedPolicyState,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        modifiers: WmModifierMask,
        application_capture_active: bool,
    ) -> PolicyInputDisposition {
        let key = (seat, device, Press::Key(keycode));
        if !pressed && self.debts.remove(&key).is_some() {
            return PolicyInputDisposition::Consumed;
        }
        if self.debts.contains_key(&key) {
            return PolicyInputDisposition::Consumed;
        }
        if application_capture_active {
            return PolicyInputDisposition::Pass;
        }
        if self.exhausted {
            return PolicyInputDisposition::Consumed;
        }
        if !pressed || !state.modal_ready(false) {
            return PolicyInputDisposition::Pass;
        }
        let action = state
            .keyboard_action(keycode, modifiers, false)
            .map(|(action, identity)| PresentedPolicyAction {
                connection_epoch: state.publication().expect("modal state has publication").0,
                action,
                identity,
            });
        if !self.remember(key, None) {
            return PolicyInputDisposition::CapacityExceeded;
        }
        action.map_or(
            PolicyInputDisposition::Consumed,
            PolicyInputDisposition::Action,
        )
    }
}
