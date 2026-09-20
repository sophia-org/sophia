use sophia_protocol::{DeviceId, SeatId, SurfaceId};
use std::collections::BTreeMap;

const EVDEV_KEY_LEFTCTRL: u32 = 29;
const EVDEV_KEY_LEFTALT: u32 = 56;
const EVDEV_KEY_RIGHTCTRL: u32 = 97;
const EVDEV_KEY_RIGHTALT: u32 = 100;
const EVDEV_KEY_LEFTSHIFT: u32 = 42;
const EVDEV_KEY_RIGHTSHIFT: u32 = 54;
const SHIFTED_PRINTABLE_KEYCODES: [u32; 21] = [
    41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 26, 27, 43, 39, 40, 51, 52, 53,
];
pub const SESSION_CLIENT_PRESSED_KEY_CAPACITY: usize = 256;
const VIRTUAL_TERMINAL_CHORD_TIMEOUT_MSEC: u64 = 5_000;

/// What one `record_routed` call did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPressedKeyAdmission {
    Recorded,
    AlreadyPressed,
    /// The ledger is full, so this press is not tracked and its release cannot
    /// be synthesized later. The endpoint epoch must close.
    Saturated,
}

impl SessionPressedKeyAdmission {
    pub const fn is_saturated(self) -> bool {
        matches!(self, Self::Saturated)
    }
}
pub const RUNTIME_DEADLINE_KEY_RELEASE_TIMEOUT_MSEC: u64 = 500;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RuntimeDeadlineKeyDrain {
    #[default]
    Idle,
    Draining {
        deadline_msec: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDeadlineKeyDrainDecision {
    BeginRelease,
    Waiting,
    Complete,
    /// The wait expired owing nothing but a policy answer.
    ///
    /// Separate from `TimedOut` because the two obligations end differently. A
    /// key the session believes a client holds is a fault if it is never
    /// released -- somebody is left with a stuck key. A projection request the
    /// session asked at the boundary has no consumer once the session stops,
    /// so waiting for it is worth a bounded try and abandoning it is not a
    /// failure. Ending a thirty-second run in error over one unanswered
    /// request is what made the distinction necessary.
    AbandonedPolicyRequests(usize),
    TimedOut,
}

impl RuntimeDeadlineKeyDrain {
    pub const fn is_draining(self) -> bool {
        matches!(self, Self::Draining { .. })
    }

    /// Observes what the session still owes at its runtime deadline.
    ///
    /// `pending_policy_requests` is waited for alongside the key state because a
    /// deadline lands at an arbitrary instant: a focus request raised by the
    /// last pointer motion before it cannot settle in the same tick, and ending
    /// on it discards the user's final intent and reports outstanding work that
    /// was never stuck. It parts company with key state when the wait expires:
    /// a key nobody released is a fault, an answer nobody will read is not.
    pub fn observe(
        &mut self,
        now_msec: u64,
        pressed_keys: usize,
        pending_deliveries: usize,
        release_barriers: usize,
        pending_policy_requests: usize,
    ) -> RuntimeDeadlineKeyDrainDecision {
        let pending = pressed_keys != 0
            || pending_deliveries != 0
            || release_barriers != 0
            || pending_policy_requests != 0;
        match *self {
            Self::Idle if !pending => RuntimeDeadlineKeyDrainDecision::Complete,
            Self::Idle => {
                *self = Self::Draining {
                    deadline_msec: now_msec
                        .saturating_add(RUNTIME_DEADLINE_KEY_RELEASE_TIMEOUT_MSEC),
                };
                if pressed_keys == 0 {
                    RuntimeDeadlineKeyDrainDecision::Waiting
                } else {
                    RuntimeDeadlineKeyDrainDecision::BeginRelease
                }
            }
            Self::Draining { .. } if !pending => RuntimeDeadlineKeyDrainDecision::Complete,
            Self::Draining { deadline_msec } if now_msec >= deadline_msec => {
                if pressed_keys == 0 && pending_deliveries == 0 && release_barriers == 0 {
                    RuntimeDeadlineKeyDrainDecision::AbandonedPolicyRequests(
                        pending_policy_requests,
                    )
                } else {
                    RuntimeDeadlineKeyDrainDecision::TimedOut
                }
            }
            Self::Draining { .. } => RuntimeDeadlineKeyDrainDecision::Waiting,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionClientPressedKey {
    pub surface: SurfaceId,
    pub seat: SeatId,
    pub device: DeviceId,
    pub keycode: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionClientKeyMetrics {
    pub peak_pressed: usize,
    pub synthetic_releases: usize,
    pub state_only_releases: usize,
    pub orphan_releases_suppressed: usize,
    pub removed_surface_keys: usize,
}

#[derive(Debug, Default)]
pub struct SessionClientKeyState {
    pressed: Vec<SessionClientPressedKey>,
    metrics: SessionClientKeyMetrics,
}

impl SessionClientKeyState {
    pub fn is_pressed(&self, key: SessionClientPressedKey) -> bool {
        self.pressed.contains(&key)
    }

    pub fn release_is_routable(&self, key: SessionClientPressedKey) -> bool {
        self.is_pressed(key)
    }

    /// Records what a client now believes about one key.
    ///
    /// A full ledger used to fail the session. It cannot: the ledger exists so
    /// held keys can be released later, and reaching its bound means releases
    /// were already lost, which is a reason to close the endpoint epoch and
    /// flush what is held rather than to take the desktop down. The caller
    /// decides that; this only reports which happened.
    pub fn record_routed(
        &mut self,
        key: SessionClientPressedKey,
        pressed: bool,
    ) -> SessionPressedKeyAdmission {
        if pressed {
            if self.pressed.contains(&key) {
                return SessionPressedKeyAdmission::AlreadyPressed;
            }
            if self.pressed.len() >= SESSION_CLIENT_PRESSED_KEY_CAPACITY {
                return SessionPressedKeyAdmission::Saturated;
            }
            self.pressed.push(key);
            self.metrics.peak_pressed = self.metrics.peak_pressed.max(self.pressed.len());
        } else if let Some(index) = self.pressed.iter().position(|pressed| *pressed == key) {
            self.pressed.swap_remove(index);
        } else {
            self.metrics.orphan_releases_suppressed =
                self.metrics.orphan_releases_suppressed.saturating_add(1);
        }
        SessionPressedKeyAdmission::Recorded
    }

    pub fn copy_surface_keys(
        &self,
        surface: SurfaceId,
        destination: &mut Vec<SessionClientPressedKey>,
    ) {
        destination.clear();
        destination.extend(
            self.pressed
                .iter()
                .copied()
                .filter(|pressed| pressed.surface == surface),
        );
    }

    pub fn copy_all_keys(&self, destination: &mut Vec<SessionClientPressedKey>) {
        destination.clear();
        destination.extend(self.pressed.iter().copied());
    }

    /// The keys one device still holds, across every surface.
    pub fn copy_device_keys(
        &self,
        device: DeviceId,
        destination: &mut Vec<SessionClientPressedKey>,
    ) {
        destination.clear();
        destination.extend(
            self.pressed
                .iter()
                .copied()
                .filter(|pressed| pressed.device == device),
        );
    }

    pub fn record_synthetic_release(&mut self, key: SessionClientPressedKey) {
        if let Some(index) = self.pressed.iter().position(|pressed| *pressed == key) {
            self.pressed.swap_remove(index);
            self.metrics.synthetic_releases = self.metrics.synthetic_releases.saturating_add(1);
        }
    }

    pub fn record_state_only_release(&mut self, key: SessionClientPressedKey) {
        if let Some(index) = self.pressed.iter().position(|pressed| *pressed == key) {
            self.pressed.swap_remove(index);
            self.metrics.state_only_releases = self.metrics.state_only_releases.saturating_add(1);
        }
    }

    pub fn clear_surface(&mut self, surface: SurfaceId) -> usize {
        let before = self.pressed.len();
        self.pressed.retain(|pressed| pressed.surface != surface);
        let removed = before.saturating_sub(self.pressed.len());
        self.metrics.removed_surface_keys =
            self.metrics.removed_surface_keys.saturating_add(removed);
        removed
    }

    pub fn pending_len(&self) -> usize {
        self.pressed.len()
    }

    pub fn metrics(&self) -> SessionClientKeyMetrics {
        self.metrics
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PhysicalShiftKeys {
    left: bool,
    right: bool,
}

impl PhysicalShiftKeys {
    const fn held(self) -> bool {
        self.left || self.right
    }
}

/// A shifted position counts only when the shift and the printable come
/// from the same device: two keyboards are not one keyboard with more keys.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PhysicalKeyboardCoverage {
    shifts: BTreeMap<DeviceId, PhysicalShiftKeys>,
    shifted_printable_mask: u32,
    virtual_terminal_mask: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalKeyboardCoverageSnapshot {
    pub shifted_positions: u32,
    pub shifted_positions_required: u32,
    pub virtual_terminals: u32,
    pub virtual_terminals_required: u32,
}

impl PhysicalKeyboardCoverage {
    /// Observes a key from a single, unnamed device. A seat names the device.
    pub fn observe_key(&mut self, keycode: u32, pressed: bool) {
        self.observe_key_at_device(DeviceId::INVALID, keycode, pressed);
    }

    pub fn observe_key_at_device(&mut self, device: DeviceId, keycode: u32, pressed: bool) {
        let shifts = self.shifts.entry(device).or_default();
        match keycode {
            EVDEV_KEY_LEFTSHIFT => shifts.left = pressed,
            EVDEV_KEY_RIGHTSHIFT => shifts.right = pressed,
            _ if pressed && shifts.held() => {
                if let Some(index) = SHIFTED_PRINTABLE_KEYCODES
                    .iter()
                    .position(|candidate| *candidate == keycode)
                {
                    self.shifted_printable_mask |= 1 << index;
                }
            }
            _ => {}
        }
        if !shifts.held() {
            self.shifts.remove(&device);
        }
    }

    /// The device left the seat, and any shift it held with it.
    pub fn forget_device(&mut self, device: DeviceId) {
        self.shifts.remove(&device);
    }

    /// Every device's shift, as when the seat is reopened.
    pub fn forget_all_devices(&mut self) {
        self.shifts.clear();
    }

    pub fn observe_virtual_terminal(&mut self, terminal: u8) {
        if (1..=12).contains(&terminal) {
            self.virtual_terminal_mask |= 1 << (terminal - 1);
        }
    }

    pub fn snapshot(&self) -> PhysicalKeyboardCoverageSnapshot {
        PhysicalKeyboardCoverageSnapshot {
            shifted_positions: self.shifted_printable_mask.count_ones(),
            shifted_positions_required: SHIFTED_PRINTABLE_KEYCODES.len() as u32,
            virtual_terminals: self.virtual_terminal_mask.count_ones(),
            virtual_terminals_required: 12,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct VirtualTerminalChordDeviceState {
    left_control: bool,
    right_control: bool,
    left_alt: bool,
    right_alt: bool,
    function_keys_down: u16,
    consumed_function_keys: u16,
    modifier_started_msec: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VirtualTerminalChordState {
    devices: BTreeMap<DeviceId, VirtualTerminalChordDeviceState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtualTerminalChordAction {
    Pass,
    Consume,
    Activate(u8),
}

impl VirtualTerminalChordState {
    pub fn observe(&mut self, keycode: u32, pressed: bool) -> VirtualTerminalChordAction {
        self.observe_at(keycode, pressed, 0)
    }

    /// Observes a physical key while bounding how long a modifier chord may
    /// remain armed.  Input loss must not turn a later, unrelated F-key press
    /// into a terminal switch; a real chord is expected to complete promptly.
    pub fn observe_at(
        &mut self,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
    ) -> VirtualTerminalChordAction {
        self.observe_at_device(DeviceId::INVALID, keycode, pressed, time_msec)
    }

    pub fn observe_at_device(
        &mut self,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
    ) -> VirtualTerminalChordAction {
        let state = self.devices.entry(device).or_default();
        let action = state.observe_at(keycode, pressed, time_msec);
        if !pressed && state.is_idle() {
            self.devices.remove(&device);
        }
        action
    }

    /// The device left the seat, and whatever chord it was building with it.
    pub fn forget_device(&mut self, device: DeviceId) {
        self.devices.remove(&device);
    }

    pub fn pressed_modifier_keycodes_for(&self, device: DeviceId) -> [Option<u32>; 4] {
        self.devices
            .get(&device)
            .map_or([None; 4], |state| state.pressed_modifier_keycodes())
    }

    pub fn pressed_modifier_keycodes(&self) -> [Option<u32>; 4] {
        self.pressed_modifier_keycodes_for(DeviceId::INVALID)
    }

    pub fn reset(&mut self) {
        self.devices.clear();
    }
}

impl VirtualTerminalChordDeviceState {
    fn observe_at(
        &mut self,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
    ) -> VirtualTerminalChordAction {
        if self.control()
            && self.modifier_started_msec.is_some_and(|started| {
                time_msec.saturating_sub(started) > VIRTUAL_TERMINAL_CHORD_TIMEOUT_MSEC
            })
        {
            self.left_control = false;
            self.right_control = false;
            self.left_alt = false;
            self.right_alt = false;
            self.consumed_function_keys = 0;
            self.modifier_started_msec = None;
        }
        match keycode {
            EVDEV_KEY_LEFTCTRL => {
                self.left_control = pressed;
                self.update_modifier_lifetime(pressed, time_msec);
            }
            EVDEV_KEY_RIGHTCTRL => {
                self.right_control = pressed;
                self.update_modifier_lifetime(pressed, time_msec);
            }
            EVDEV_KEY_LEFTALT => {
                self.left_alt = pressed;
                self.update_modifier_lifetime(pressed, time_msec);
            }
            EVDEV_KEY_RIGHTALT => {
                self.right_alt = pressed;
                self.update_modifier_lifetime(pressed, time_msec);
            }
            _ => {
                let Some(terminal) = virtual_terminal_for_evdev_key(keycode) else {
                    return VirtualTerminalChordAction::Pass;
                };
                let bit = 1u16 << (terminal - 1);
                let was_down = self.function_keys_down & bit != 0;
                if pressed {
                    self.function_keys_down |= bit;
                } else {
                    self.function_keys_down &= !bit;
                }
                if !pressed {
                    let consumed = self.consumed_function_keys & bit != 0;
                    self.consumed_function_keys &= !bit;
                    return if consumed {
                        VirtualTerminalChordAction::Consume
                    } else {
                        VirtualTerminalChordAction::Pass
                    };
                }
                if self.consumed_function_keys & bit != 0 {
                    return VirtualTerminalChordAction::Consume;
                }
                if !was_down && self.control() && self.alt() {
                    self.consumed_function_keys |= bit;
                    return VirtualTerminalChordAction::Activate(terminal);
                }
                return VirtualTerminalChordAction::Pass;
            }
        }
        VirtualTerminalChordAction::Pass
    }

    fn update_modifier_lifetime(&mut self, pressed: bool, time_msec: u64) {
        if pressed {
            self.modifier_started_msec.get_or_insert(time_msec);
        } else if !self.control() {
            self.modifier_started_msec = None;
        }
    }

    const fn pressed_modifier_keycodes(&self) -> [Option<u32>; 4] {
        [
            if self.left_control {
                Some(EVDEV_KEY_LEFTCTRL)
            } else {
                None
            },
            if self.right_control {
                Some(EVDEV_KEY_RIGHTCTRL)
            } else {
                None
            },
            if self.left_alt {
                Some(EVDEV_KEY_LEFTALT)
            } else {
                None
            },
            if self.right_alt {
                Some(EVDEV_KEY_RIGHTALT)
            } else {
                None
            },
        ]
    }

    const fn is_idle(self) -> bool {
        !self.control() && self.function_keys_down == 0 && self.consumed_function_keys == 0
    }

    const fn control(self) -> bool {
        self.left_control || self.right_control
    }

    const fn alt(self) -> bool {
        self.left_alt || self.right_alt
    }
}

const fn virtual_terminal_for_evdev_key(keycode: u32) -> Option<u8> {
    match keycode {
        59..=68 => Some((keycode - 58) as u8),
        87 => Some(11),
        88 => Some(12),
        _ => None,
    }
}
