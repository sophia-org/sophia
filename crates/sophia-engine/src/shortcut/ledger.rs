//! Which keys are down on which device of a seat, and which of those presses
//! the router consumed, so every consumed press is paired with its release
//! and the modifier mask is derived from what is actually down.

use crate::prelude::*;
use sophia_protocol::WmModifierMask;

/// Every evdev keycode (KEY_MAX). Higher codes are never bound and pass.
pub(crate) const KEYCODE_LIMIT: u32 = 0x300;
const KEY_WORDS: usize = (KEYCODE_LIMIT / 64) as usize;
const MODIFIER_KEYCODES: [u32; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

/// Whether an evdev keycode is a Shift, Control, Alt or Super key, left or
/// right: the keys a chord is held by, which no capture may take from it.
pub fn is_modifier_keycode(keycode: u32) -> bool {
    MODIFIER_KEYCODES.contains(&keycode)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct WmSeatShortcutState {
    pub(crate) devices: Vec<DeviceKeys>,
    /// Devices refused a slot, and whether each pressed a modifier since.
    pub(crate) refused: BTreeMap<DeviceId, bool>,
    /// More devices were refused than could be named; only a seat reset
    /// makes the mask known again.
    pub(crate) saturated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeviceKeys {
    pub(crate) device: DeviceId,
    down: [u64; KEY_WORDS],
    /// Presses that were shortcuts; their releases are consumed with them.
    consumed: [u64; KEY_WORDS],
}

impl WmSeatShortcutState {
    pub(crate) fn mask_known(&self) -> bool {
        !self.saturated && !self.refused.values().any(|modifier| *modifier)
    }

    /// Whether a modifier may be down: a known one, or any while unknown.
    pub(crate) fn modifiers_down(&self) -> bool {
        !self.mask_known() || self.modifier_mask().bits != 0
    }

    pub(crate) fn modifier_mask(&self) -> WmModifierMask {
        let mut bits = 0;
        for keys in &self.devices {
            for keycode in MODIFIER_KEYCODES {
                if keys.is_down(keycode) {
                    bits |= modifier_bit(keycode);
                }
            }
        }
        WmModifierMask { bits }
    }

    /// Whether any key is down on any tracked device of the seat.
    pub(crate) fn any_down(&self) -> bool {
        !self.devices.is_empty()
    }

    pub(crate) fn is_down(&self, device: DeviceId, keycode: u32) -> bool {
        self.devices
            .iter()
            .any(|keys| keys.device == device && keys.is_down(keycode))
    }

    pub(crate) fn press(&mut self, device: DeviceId, keycode: u32, consumed: bool) {
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
    pub(crate) fn release(&mut self, device: DeviceId, keycode: u32) -> bool {
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
    pub(crate) const fn new(device: DeviceId) -> Self {
        Self {
            device,
            down: [0; KEY_WORDS],
            consumed: [0; KEY_WORDS],
        }
    }

    const fn slot(keycode: u32) -> (usize, u64) {
        ((keycode / 64) as usize, 1 << (keycode % 64))
    }

    pub(crate) fn is_down(&self, keycode: u32) -> bool {
        let (word, bit) = Self::slot(keycode);
        self.down[word] & bit != 0
    }

    pub(crate) fn is_consumed(&self, keycode: u32) -> bool {
        let (word, bit) = Self::slot(keycode);
        self.consumed[word] & bit != 0
    }
}

pub(crate) const fn modifier_bit(keycode: u32) -> u32 {
    match keycode {
        42 | 54 => WmModifierMask::SHIFT,
        29 | 97 => WmModifierMask::CONTROL,
        56 | 100 => WmModifierMask::ALT,
        125 | 126 => WmModifierMask::SUPER,
        _ => 0,
    }
}
