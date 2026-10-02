//! Physical shortcut matching, owed to no protocol revision.
//!
//! Engine matches physical input against registered chords and emits opaque action
//! tokens. The public `sophia_wm_v1` path resolves these bindings from the
//! prepared shortcut and policy catalogs before they arrive here.

use crate::prelude::*;
use sophia_protocol::{
    PolicyActionLifecycleInterest, WM_MAX_BINDINGS, WmActionId, WmBindingRegistration,
    WmCapabilities, WmChromePolicy, WmModifierMask,
};

mod chord;
use chord::ChordBook;
pub use chord::{
    WM_CHORD_CREDITS, WmChordActivation, WmChordCreditsExhausted, WmChordEvent, WmChordToken,
};

/// Why a set of bindings could not become a registry.
///
/// A `&'static str` rather than an enum because callers reduce every cause to one
/// preparation message. A parallel enum would duplicate this vocabulary without
/// giving anyone another decision to make.
pub type WmShortcutRegistryError = &'static str;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmShortcutRegistry {
    bindings: BTreeMap<(u32, u32), WmActionId>,
    held: BTreeMap<u32, WmActionId>,
    pub(crate) capabilities: WmCapabilities,
    policy_generation: u64,
    chrome: WmChromePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmShortcutDecision {
    pub action: Option<WmActionId>,
    pub consumed: bool,
    /// The lifecycle chord `action` opened or joined, when the WM declared it.
    pub chord: Option<WmChordActivation>,
}

impl WmShortcutDecision {
    const fn pass() -> Self {
        Self {
            action: None,
            consumed: false,
            chord: None,
        }
    }
}

impl WmShortcutRegistry {
    /// Builds a registry from bindings a caller already resolved.
    ///
    /// Every rejection here is about the bindings themselves, not about who asked:
    /// an invalid action or keycode, an unsupported modifier, the reserved
    /// emergency chord, a duplicate chord, or more bindings than the wire admits.
    /// A caller that speaks a protocol revision checks its own version first and
    /// then calls this.
    pub fn new(
        bindings: &[WmBindingRegistration],
        capabilities: WmCapabilities,
        policy_generation: u64,
        chrome: WmChromePolicy,
    ) -> Result<Self, WmShortcutRegistryError> {
        if capabilities.bits & !WmCapabilities::SUPPORTED != 0 {
            return Err("unsupported WM capability");
        }
        if policy_generation == 0 {
            return Err("invalid WM policy generation");
        }
        if !valid_chrome_policy(chrome) {
            return Err("invalid WM chrome policy");
        }
        if bindings.len() > WM_MAX_BINDINGS {
            return Err("too many WM bindings");
        }

        let mut resolved = BTreeMap::new();
        for binding in bindings {
            if !binding.action.is_valid() || binding.keycode == 0 || binding.keycode > 0x2ff {
                return Err("invalid WM binding");
            }
            if binding.modifiers.bits & !WmModifierMask::SUPPORTED != 0 {
                return Err("unsupported WM modifier");
            }
            // Ctrl-Alt-Backspace belongs to emergency recovery and is never
            // available to a policy client, whatever it registers.
            if binding.keycode == 14
                && binding.modifiers.bits & (WmModifierMask::CONTROL | WmModifierMask::ALT)
                    == WmModifierMask::CONTROL | WmModifierMask::ALT
            {
                return Err("reserved emergency chord");
            }
            if resolved
                .insert((binding.keycode, binding.modifiers.bits), binding.action)
                .is_some()
            {
                return Err("duplicate WM chord");
            }
        }

        Ok(Self {
            bindings: resolved,
            held: BTreeMap::new(),
            capabilities,
            policy_generation,
            chrome,
        })
    }

    pub fn handle_key(
        &mut self,
        keycode: u32,
        modifiers: WmModifierMask,
        pressed: bool,
    ) -> WmShortcutDecision {
        if !pressed {
            return WmShortcutDecision {
                consumed: self.held.remove(&keycode).is_some(),
                ..WmShortcutDecision::pass()
            };
        }
        let Some(action) = self.lookup(keycode, modifiers) else {
            return WmShortcutDecision::pass();
        };
        let first_press = self.held.insert(keycode, action).is_none();
        WmShortcutDecision {
            action: first_press.then_some(action),
            consumed: true,
            chord: None,
        }
    }

    fn lookup(&self, keycode: u32, modifiers: WmModifierMask) -> Option<WmActionId> {
        self.bindings.get(&(keycode, modifiers.bits)).copied()
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    pub const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub const fn chrome(&self) -> WmChromePolicy {
        self.chrome
    }

    pub const fn supports_chrome_policy(&self) -> bool {
        self.capabilities.bits & WmCapabilities::POLICY_CHROME_V2 != 0
    }

    pub fn is_idle(&self) -> bool {
        self.held.is_empty()
    }
}

pub const WM_MAX_SHORTCUT_SEATS: usize = 16;
/// Devices tracked at once per seat while they have keys down. A device's
/// slot frees when its last key goes up.
pub const WM_MAX_SHORTCUT_DEVICES: usize = 16;
/// Every evdev keycode (KEY_MAX). Higher codes are never bound and pass.
const KEYCODE_LIMIT: u32 = 0x300;
const KEY_WORDS: usize = (KEYCODE_LIMIT / 64) as usize;

/// Matches physical keys for every seat and follows the WM's declared chords.
///
/// Each seat records exactly which keys are down on which device, so two
/// keyboards are told apart, a repeat or duplicate press fires nothing, and
/// the release of every consumed press is consumed too. The modifier mask is
/// derived from those records.
///
/// The record is bounded by device, never by an anonymous count. A device
/// that arrives while every slot is taken is refused by identity: its keys
/// pass unmatched and unconsumed until it is removed or the seat is reset. If
/// it pressed a modifier the mask is unknown, so the seat matches nothing
/// until then. Refusing more devices than there are slots latches the seat
/// unknown until `clear_seat`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmShortcutRouter {
    pub(crate) registry: WmShortcutRegistry,
    seats: BTreeMap<SeatId, WmSeatShortcutState>,
    chords: ChordBook,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WmSeatShortcutState {
    devices: Vec<DeviceKeys>,
    /// Devices refused a slot, and whether each pressed a modifier since.
    refused: BTreeMap<DeviceId, bool>,
    /// More devices were refused than could be named; only a seat reset
    /// makes the mask known again.
    saturated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceKeys {
    device: DeviceId,
    down: [u64; KEY_WORDS],
    /// Presses that were shortcuts; their releases are consumed with them.
    consumed: [u64; KEY_WORDS],
}

impl WmShortcutRouter {
    pub fn new(registry: WmShortcutRegistry) -> Self {
        Self {
            registry,
            seats: BTreeMap::new(),
            chords: ChordBook::default(),
        }
    }

    /// Install new bindings. Open chords end cancelled, but the keys down stay
    /// recorded, so the release of a press the old bindings consumed is still
    /// consumed rather than reaching a client unpaired.
    pub fn replace_registry(&mut self, registry: WmShortcutRegistry) {
        self.registry = registry;
        self.chords.cancel_all();
    }

    /// Route one physical key event at event time `time_msec`.
    pub fn route_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
    ) -> WmShortcutDecision {
        if !seat.is_valid() || keycode >= KEYCODE_LIMIT {
            return WmShortcutDecision::pass();
        }
        if !self.seats.contains_key(&seat) {
            if self.seats.len() >= WM_MAX_SHORTCUT_SEATS {
                return WmShortcutDecision::pass();
            }
            self.seats.insert(seat, WmSeatShortcutState::default());
        }
        let Some(state) = self.seats.get_mut(&seat) else {
            return WmShortcutDecision::pass();
        };
        let modifier = modifier_bit(keycode) != 0;
        if let Some(pressed_modifier) = state.refused.get_mut(&device) {
            // A refused device stays refused: one of its presses may already
            // have reached a client, so none of its keys can be matched.
            *pressed_modifier |= pressed && modifier;
            if !pressed {
                let modifiers_down = state.modifiers_down();
                self.chords
                    .key_released(seat, (device, keycode), modifiers_down);
            }
            return WmShortcutDecision::pass();
        }
        if !pressed {
            let consumed = state.release(device, keycode);
            let modifiers_down = state.modifiers_down();
            self.chords
                .key_released(seat, (device, keycode), modifiers_down);
            return WmShortcutDecision {
                consumed,
                ..WmShortcutDecision::pass()
            };
        }
        if let Some(keys) = state.devices.iter().find(|keys| keys.device == device)
            && keys.is_down(keycode)
        {
            // Autorepeat, or a duplicate press: it fires nothing and joins
            // nothing, and its consumption follows the original press.
            return WmShortcutDecision {
                consumed: keys.is_consumed(keycode),
                ..WmShortcutDecision::pass()
            };
        }
        if !state.devices.iter().any(|keys| keys.device == device) {
            if state.devices.len() >= WM_MAX_SHORTCUT_DEVICES {
                if state.refused.len() >= WM_MAX_SHORTCUT_DEVICES {
                    state.saturated = true;
                } else {
                    state.refused.insert(device, modifier);
                }
                return WmShortcutDecision::pass();
            }
            state.devices.push(DeviceKeys::new(device));
        }
        let matched = if state.mask_known() {
            let modifiers = state.modifier_mask();
            self.registry
                .lookup(keycode, modifiers)
                .map(|action| (action, modifiers))
        } else {
            None
        };
        state.press(device, keycode, matched.is_some());
        let Some((action, modifiers)) = matched else {
            return WmShortcutDecision::pass();
        };
        match self.chords.activate(
            seat,
            action,
            (device, keycode),
            modifiers.bits != 0,
            time_msec,
        ) {
            Ok(chord) => WmShortcutDecision {
                action: Some(action),
                consumed: true,
                chord,
            },
            // Every credit is owed to the WM: the press is still a shortcut,
            // but it queues nothing and opens nothing.
            Err(WmChordCreditsExhausted) => WmShortcutDecision {
                consumed: true,
                ..WmShortcutDecision::pass()
            },
        }
    }

    /// End every chord on the seat cancelled and forget its keys. This is the
    /// trusted reset that also clears a refused or saturated seat.
    pub fn clear_seat(&mut self, seat: SeatId) -> bool {
        self.chords.cancel_seat(seat);
        self.seats.remove(&seat).is_some()
    }

    /// End every chord on the seat cancelled, keeping its keys recorded. Used
    /// before synthetic releases (a VT switch) so those end nothing released.
    pub fn cancel_seat_chords(&mut self, seat: SeatId) {
        self.chords.cancel_seat(seat);
    }

    /// A device went away: its keys, or its refusal, are forgotten, and every
    /// chord on a seat it was part of ends cancelled.
    pub fn remove_device(&mut self, device: DeviceId) {
        for (seat, state) in &mut self.seats {
            let tracked = state.devices.len();
            state.devices.retain(|keys| keys.device != device);
            let refused = state.refused.remove(&device).is_some();
            if refused || state.devices.len() != tracked {
                self.chords.cancel_seat(*seat);
            }
        }
    }

    /// Whether the seat's modifier state is unknown, so it matches nothing:
    /// a refused device pressed a modifier, or the seat is saturated.
    pub fn seat_uncertain(&self, seat: SeatId) -> bool {
        self.seats
            .get(&seat)
            .is_some_and(|state| !state.mask_known())
    }

    /// The actions whose chords the WM follows, from its Configuration. Only
    /// chords opened afterwards use them.
    pub fn set_action_lifecycles(&mut self, interests: &[PolicyActionLifecycleInterest]) {
        self.chords.set_interests(interests);
    }

    /// A new WM epoch: chords and pending events are dropped and credits return.
    pub fn reset_chords(&mut self) {
        self.chords.reset();
    }

    /// The opener's Action was not admitted; the chord is dropped with its credit.
    pub fn chord_opener_refused(&mut self, token: WmChordToken) -> bool {
        self.chords.opener_refused(token)
    }

    /// A joining Action was not admitted; its trigger stops holding the chord.
    pub fn chord_join_refused(&mut self, token: WmChordToken, device: DeviceId, keycode: u32) {
        self.chords.join_refused(token, (device, keycode));
    }

    /// The chord's Ended was handed to the WM as the in-flight Cycle.
    pub fn chord_delivered(&mut self, token: WmChordToken) -> bool {
        self.chords.delivered(token)
    }

    /// Send Held for every chord due at `now_msec`.
    pub fn poll_chords(&mut self, now_msec: u64) {
        self.chords.poll(now_msec);
    }

    /// The event time of the next Held, to bound how long the owner may wait.
    pub fn next_deadline(&self) -> Option<u64> {
        self.chords.next_deadline()
    }

    /// Lifecycle events in the order Session must queue them.
    pub fn drain_chord_events(&mut self) -> Vec<WmChordEvent> {
        self.chords.drain()
    }

    pub fn chord_credits_free(&self) -> usize {
        self.chords.credits_free()
    }

    pub fn modifier_mask(&self, seat: SeatId) -> WmModifierMask {
        self.seats
            .get(&seat)
            .map(WmSeatShortcutState::modifier_mask)
            .unwrap_or(WmModifierMask { bits: 0 })
    }

    pub const fn policy_generation(&self) -> u64 {
        self.registry.policy_generation()
    }

    pub fn binding_count(&self) -> usize {
        self.registry.binding_count()
    }

    pub const fn chrome(&self) -> WmChromePolicy {
        self.registry.chrome()
    }

    pub const fn supports_chrome_policy(&self) -> bool {
        self.registry.supports_chrome_policy()
    }

    pub fn shortcut_idle(&self) -> bool {
        self.chords.is_idle()
            && self.seats.values().all(|state| {
                state.devices.is_empty() && state.refused.is_empty() && !state.saturated
            })
    }
}

pub(crate) fn valid_chrome_policy(chrome: WmChromePolicy) -> bool {
    let valid_style = |enabled: bool, width: u32| {
        width <= 64 && ((enabled && width > 0) || (!enabled && width == 0))
    };
    valid_style(chrome.focus_ring.enabled, chrome.focus_ring.width)
        && valid_style(chrome.frame.enabled, chrome.frame.width)
}

impl WmSeatShortcutState {
    fn mask_known(&self) -> bool {
        !self.saturated && !self.refused.values().any(|modifier| *modifier)
    }

    /// Whether a modifier may be down: a known one, or any while unknown.
    fn modifiers_down(&self) -> bool {
        !self.mask_known() || self.modifier_mask().bits != 0
    }

    fn modifier_mask(&self) -> WmModifierMask {
        let mut bits = 0;
        for keys in &self.devices {
            for keycode in [42, 54, 29, 97, 56, 100, 125, 126] {
                if keys.is_down(keycode) {
                    bits |= modifier_bit(keycode);
                }
            }
        }
        WmModifierMask { bits }
    }

    fn press(&mut self, device: DeviceId, keycode: u32, consumed: bool) {
        if let Some(keys) = self.devices.iter_mut().find(|keys| keys.device == device) {
            let (word, bit) = DeviceKeys::slot(keycode);
            keys.down[word] |= bit;
            if consumed {
                keys.consumed[word] |= bit;
            }
        }
    }

    /// Whether the released press was consumed. A device with nothing left
    /// down gives up its slot.
    fn release(&mut self, device: DeviceId, keycode: u32) -> bool {
        let Some(index) = self.devices.iter().position(|keys| keys.device == device) else {
            return false;
        };
        let keys = &mut self.devices[index];
        let consumed = keys.is_down(keycode) && keys.is_consumed(keycode);
        let (word, bit) = DeviceKeys::slot(keycode);
        keys.down[word] &= !bit;
        keys.consumed[word] &= !bit;
        if keys.down.iter().all(|word| *word == 0) {
            self.devices.remove(index);
        }
        consumed
    }
}

impl DeviceKeys {
    const fn new(device: DeviceId) -> Self {
        Self {
            device,
            down: [0; KEY_WORDS],
            consumed: [0; KEY_WORDS],
        }
    }

    const fn slot(keycode: u32) -> (usize, u64) {
        ((keycode / 64) as usize, 1 << (keycode % 64))
    }

    fn is_down(&self, keycode: u32) -> bool {
        let (word, bit) = Self::slot(keycode);
        self.down[word] & bit != 0
    }

    fn is_consumed(&self, keycode: u32) -> bool {
        let (word, bit) = Self::slot(keycode);
        self.consumed[word] & bit != 0
    }
}

const fn modifier_bit(keycode: u32) -> u32 {
    match keycode {
        42 | 54 => WmModifierMask::SHIFT,
        29 | 97 => WmModifierMask::CONTROL,
        56 | 100 => WmModifierMask::ALT,
        125 | 126 => WmModifierMask::SUPER,
        _ => 0,
    }
}
