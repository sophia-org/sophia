use sophia_protocol::DeviceId;
use std::collections::BTreeMap;

pub const EVDEV_KEY_BACKSPACE: u32 = 14;
pub const EVDEV_KEY_LEFTCTRL: u32 = 29;
pub const EVDEV_KEY_LEFTALT: u32 = 56;
pub const EVDEV_KEY_RIGHTALT: u32 = 100;
pub const EVDEV_KEY_RIGHTCTRL: u32 = 97;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmergencyChordAction {
    None,
    Armed,
    Triggered,
}

/// The chord keys one device holds. A chord is complete only within one
/// device: a control on one keyboard and a backspace on another are two
/// keyboards doing two things.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct EmergencyChordKeys {
    left_control: bool,
    right_control: bool,
    left_alt: bool,
    right_alt: bool,
    backspace: bool,
}

impl EmergencyChordKeys {
    /// Records a chord key; `false` for a key the chord does not use.
    fn observe(&mut self, keycode: u32, pressed: bool) -> bool {
        match keycode {
            EVDEV_KEY_LEFTCTRL => self.left_control = pressed,
            EVDEV_KEY_RIGHTCTRL => self.right_control = pressed,
            EVDEV_KEY_LEFTALT => self.left_alt = pressed,
            EVDEV_KEY_RIGHTALT => self.right_alt = pressed,
            EVDEV_KEY_BACKSPACE => self.backspace = pressed,
            _ => return false,
        }
        true
    }

    const fn complete(self) -> bool {
        (self.left_control || self.right_control)
            && (self.left_alt || self.right_alt)
            && self.backspace
    }

    const fn idle(self) -> bool {
        !(self.left_control
            || self.right_control
            || self.left_alt
            || self.right_alt
            || self.backspace)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyChordState {
    devices: BTreeMap<DeviceId, EmergencyChordKeys>,
    armed: bool,
    waiting_for_full_release: bool,
    arm_on_full_release: bool,
}

impl EmergencyChordState {
    pub const fn awaiting_arm() -> Self {
        Self {
            devices: BTreeMap::new(),
            armed: false,
            waiting_for_full_release: false,
            arm_on_full_release: false,
        }
    }

    pub const fn armed() -> Self {
        Self {
            devices: BTreeMap::new(),
            armed: true,
            waiting_for_full_release: false,
            arm_on_full_release: false,
        }
    }

    pub const fn is_armed(&self) -> bool {
        self.armed
    }

    /// Observes a key from a single, unnamed device. For callers that have
    /// one keyboard by construction; a seat names the device.
    pub fn observe(&mut self, keycode: u32, pressed: bool) -> EmergencyChordAction {
        self.observe_at_device(DeviceId::INVALID, keycode, pressed)
    }

    pub fn observe_at_device(
        &mut self,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
    ) -> EmergencyChordAction {
        let keys = self.devices.entry(device).or_default();
        if !keys.observe(keycode, pressed) {
            return EmergencyChordAction::None;
        }
        let complete = keys.complete();
        if keys.idle() {
            self.devices.remove(&device);
        }

        if self.waiting_for_full_release {
            return self.settle_release();
        }

        if !complete {
            return EmergencyChordAction::None;
        }
        self.chord_completed()
    }

    /// The device left the seat, and every chord key it held with it. A
    /// departure can be the release that finishes arming; it can never be
    /// the press that triggers.
    pub fn forget_device(&mut self, device: DeviceId) -> EmergencyChordAction {
        if self.devices.remove(&device).is_none() {
            return EmergencyChordAction::None;
        }
        if self.waiting_for_full_release {
            return self.settle_release();
        }
        EmergencyChordAction::None
    }

    fn chord_completed(&mut self) -> EmergencyChordAction {
        self.waiting_for_full_release = true;
        if self.armed {
            EmergencyChordAction::Triggered
        } else {
            // The guard may hand input to Engine after this transition. Do
            // not publish readiness while any key in the arm chord is down,
            // on any device.
            self.arm_on_full_release = true;
            EmergencyChordAction::None
        }
    }

    fn settle_release(&mut self) -> EmergencyChordAction {
        if !self.devices.values().all(|keys| keys.idle()) {
            return EmergencyChordAction::None;
        }
        self.waiting_for_full_release = false;
        if self.arm_on_full_release {
            self.arm_on_full_release = false;
            self.armed = true;
            return EmergencyChordAction::Armed;
        }
        EmergencyChordAction::None
    }
}

impl Default for EmergencyChordState {
    fn default() -> Self {
        Self::awaiting_arm()
    }
}
