use xkbcommon::xkb;

pub const XKB_RMLVO_FIELD_MAX_BYTES: usize = 128;
pub const XKB_DEFAULT_REPEAT_DELAY_MSEC: u16 = 660;
pub const XKB_DEFAULT_REPEAT_INTERVAL_MSEC: u16 = 40;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XkbRmlvoConfig {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
}

impl Default for XkbRmlvoConfig {
    fn default() -> Self {
        Self {
            rules: "evdev".to_owned(),
            model: "pc105".to_owned(),
            layout: "us".to_owned(),
            variant: String::new(),
            options: String::new(),
        }
    }
}

impl XkbRmlvoConfig {
    pub fn validate(&self) -> Result<(), XkbKeyboardError> {
        for value in [
            &self.rules,
            &self.model,
            &self.layout,
            &self.variant,
            &self.options,
        ] {
            if value.len() > XKB_RMLVO_FIELD_MAX_BYTES || value.as_bytes().contains(&0) {
                return Err(XkbKeyboardError::InvalidConfiguration);
            }
        }
        if self.rules.is_empty() || self.model.is_empty() || self.layout.is_empty() {
            return Err(XkbKeyboardError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XkbKeyboardError {
    InvalidConfiguration,
    KeymapCompilationFailed,
}

impl core::fmt::Display for XkbKeyboardError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "invalid XKB RMLVO configuration",
            Self::KeymapCompilationFailed => "XKB keymap compilation failed",
        })
    }
}

impl std::error::Error for XkbKeyboardError {}

/// Immutable, client-visible description compiled from the session RMLVO.
///
/// Core X11 and XKB replies must describe the same map used by the per-seat
/// state machines. Keeping the reduced wire representation here prevents the
/// two protocol paths from silently drifting apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XkbKeymapSnapshot {
    config: XkbRmlvoConfig,
    min_keycode: u8,
    max_keycode: u8,
    keysyms: Vec<[u32; 2]>,
    repeatable: Vec<bool>,
    modifier_map: Vec<(u8, u8)>,
}

impl XkbKeymapSnapshot {
    pub fn new(config: &XkbRmlvoConfig) -> Result<Self, XkbKeyboardError> {
        let keymap = compile_keymap(config)?;
        // The X11 setup contract exposes the full 8..=255 core keycode range.
        // xkbcommon may report a narrower range (normally starting at 9), so
        // preserve explicit NoSymbol entries at either edge.
        let min_keycode = 8;
        let max_keycode = u8::MAX;
        let mut keysyms = Vec::with_capacity(usize::from(max_keycode - min_keycode) + 1);
        let mut repeatable = Vec::with_capacity(usize::from(max_keycode - min_keycode) + 1);
        for raw in min_keycode..=max_keycode {
            let key = xkb::Keycode::new(u32::from(raw));
            let base = keymap
                .key_get_syms_by_level(key, 0, 0)
                .first()
                .map_or(0, |keysym| keysym.raw());
            let shifted = keymap
                .key_get_syms_by_level(key, 0, 1)
                .first()
                .map_or(base, |keysym| keysym.raw());
            keysyms.push([base, shifted]);
            repeatable.push(keymap.key_repeats(key));
        }
        Ok(Self {
            config: config.clone(),
            min_keycode,
            max_keycode,
            keysyms,
            repeatable,
            modifier_map: vec![
                (50, 1),
                (62, 1),
                (66, 2),
                (37, 4),
                (105, 4),
                (64, 8),
                (108, 8),
                (77, 16),
                (133, 64),
                (134, 64),
            ],
        })
    }

    pub fn config(&self) -> &XkbRmlvoConfig {
        &self.config
    }

    /// The core modifier mapping GetModifierMapping reports, derived from the
    /// same list the XKB map reports: per modifier bit, its keycodes in list
    /// order, padded to the widest.
    pub fn core_modifier_mapping(&self) -> (u8, Vec<u8>) {
        let mut per_modifier: [Vec<u8>; 8] = Default::default();
        for (keycode, mask) in &self.modifier_map {
            for (bit, slot) in per_modifier.iter_mut().enumerate() {
                if mask & (1 << bit) != 0 {
                    slot.push(*keycode);
                }
            }
        }
        let width = per_modifier.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let mut keycodes = Vec::with_capacity(width * 8);
        for slot in &per_modifier {
            keycodes.extend(slot.iter().copied());
            keycodes.extend(std::iter::repeat_n(0, width - slot.len()));
        }
        (u8::try_from(width).unwrap_or(u8::MAX), keycodes)
    }

    /// The modifier map as sets, for comparing a SetModifierMapping request
    /// against what is served: zero padding and order are not differences.
    pub fn modifier_sets(&self) -> [std::collections::BTreeSet<u8>; 8] {
        let mut sets: [std::collections::BTreeSet<u8>; 8] = Default::default();
        for (keycode, mask) in &self.modifier_map {
            for (bit, set) in sets.iter_mut().enumerate() {
                if mask & (1 << bit) != 0 {
                    set.insert(*keycode);
                }
            }
        }
        sets
    }

    pub fn core_mapping(&self, first_keycode: u8, count: u8) -> Vec<u32> {
        let mut result = Vec::with_capacity(usize::from(count) * 2);
        for offset in 0..count {
            let keycode = first_keycode.saturating_add(offset);
            let pair = keycode
                .checked_sub(self.min_keycode)
                .and_then(|index| self.keysyms.get(usize::from(index)))
                .copied()
                .unwrap_or([0, 0]);
            result.extend(pair);
        }
        result
    }

    pub fn xkb_keysyms(&self) -> Vec<[u32; 2]> {
        self.keysyms.clone()
    }

    pub fn modifier_map(&self) -> Vec<(u8, u8)> {
        self.modifier_map.clone()
    }

    pub fn evdev_key_repeats(&self, evdev_keycode: u32) -> bool {
        evdev_keycode
            .checked_add(8)
            .and_then(|keycode| u8::try_from(keycode).ok())
            .and_then(|keycode| keycode.checked_sub(self.min_keycode))
            .and_then(|index| self.repeatable.get(usize::from(index)))
            .copied()
            .unwrap_or(false)
    }

    pub const fn min_keycode(&self) -> u8 {
        self.min_keycode
    }
    pub const fn max_keycode(&self) -> u8 {
        self.max_keycode
    }
}

/// The core keyboard mapping a client may rewrite (ChangeKeyboardMapping):
/// one row per keycode, as wide as the widest row written, NoSymbol-padded.
/// Starts as the snapshot's two levels; GetKeyboardMapping reports it whole
/// and XKB GetMap reports its first two levels, so the two agree. Key events
/// carry keycodes only, and modifier state stays xkbcommon's: this is the
/// table a client translates with, which is what the request rewrites.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XCoreKeyboardMap {
    min_keycode: u8,
    max_keycode: u8,
    width: u8,
    rows: Vec<Vec<u32>>,
}

/// Why a ChangeKeyboardMapping was refused: a keycode outside the range,
/// the Value error the protocol names, carrying the first keycode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XKeyboardMapRefusal {
    KeycodeOutOfRange(u8),
}

impl XCoreKeyboardMap {
    pub fn from_snapshot(snapshot: &XkbKeymapSnapshot) -> Self {
        Self {
            min_keycode: snapshot.min_keycode,
            max_keycode: snapshot.max_keycode,
            width: 2,
            rows: snapshot.keysyms.iter().map(|pair| pair.to_vec()).collect(),
        }
    }

    pub const fn keysyms_per_keycode(&self) -> u8 {
        self.width
    }

    pub fn core_mapping(&self, first_keycode: u8, count: u8) -> Vec<u32> {
        let width = usize::from(self.width);
        let mut result = Vec::with_capacity(usize::from(count) * width);
        for offset in 0..count {
            let keycode = first_keycode.saturating_add(offset);
            match keycode
                .checked_sub(self.min_keycode)
                .and_then(|index| self.rows.get(usize::from(index)))
            {
                Some(row) => result.extend(row.iter().copied()),
                None => result.extend(std::iter::repeat_n(0, width)),
            }
        }
        result
    }

    /// The first two levels of every keycode, the shape the XKB map reports.
    pub fn xkb_keysyms(&self) -> Vec<[u32; 2]> {
        self.rows
            .iter()
            .map(|row| {
                let base = row.first().copied().unwrap_or(0);
                [base, row.get(1).copied().unwrap_or(base)]
            })
            .collect()
    }

    /// ChangeKeyboardMapping: `count` rows from `first_keycode`, each
    /// `per_keycode` keysyms; the map widens to the widest row written.
    pub fn change(
        &mut self,
        first_keycode: u8,
        per_keycode: u8,
        keysyms: &[u32],
    ) -> Result<u8, XKeyboardMapRefusal> {
        let count =
            u8::try_from(keysyms.len() / usize::from(per_keycode.max(1))).unwrap_or(u8::MAX);
        let last = u16::from(first_keycode) + u16::from(count) - u16::from(count > 0);
        if first_keycode < self.min_keycode || last > u16::from(self.max_keycode) {
            return Err(XKeyboardMapRefusal::KeycodeOutOfRange(first_keycode));
        }
        let width = usize::from(self.width.max(per_keycode));
        if width > usize::from(self.width) {
            for row in &mut self.rows {
                row.resize(width, 0);
            }
            self.width = u8::try_from(width).unwrap_or(u8::MAX);
        }
        for (offset, chunk) in keysyms.chunks(usize::from(per_keycode.max(1))).enumerate() {
            let index = usize::from(first_keycode - self.min_keycode) + offset;
            let row = &mut self.rows[index];
            row.iter_mut().for_each(|sym| *sym = 0);
            row[..chunk.len()].copy_from_slice(chunk);
        }
        Ok(count)
    }
}

fn compile_keymap(config: &XkbRmlvoConfig) -> Result<xkb::Keymap, XkbKeyboardError> {
    config.validate()?;
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS | xkb::CONTEXT_NO_ENVIRONMENT_NAMES);
    xkb::Keymap::new_from_names(
        &context,
        &config.rules,
        &config.model,
        &config.layout,
        &config.variant,
        Some(config.options.clone()),
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or(XkbKeyboardError::KeymapCompilationFailed)
}

pub struct XkbKeyboardState {
    state: xkb::State,
    // Keys submitted to this state, not effective modifiers or hardware
    // observations: an ordinary key has no modifier bit, and a released lock
    // key may leave one set. Native integration must send first/final edges only.
    down: [u64; 4],
    // Written before the C state changes. An interrupted update permanently
    // leaves private cleanup without evidence; a later key cannot repair it.
    physical_known: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XkbPhysicalKeyState {
    Released,
    Held,
    Unavailable,
    InvalidKey,
}

/// Components read from one actual XKB state, before or after an ordered edge.
/// Kept separate from the effective modifier mask: a lock is not a held key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XkbOrderedState {
    components: [u32; 13],
}

impl XkbOrderedState {
    pub(crate) fn components(self) -> [u32; 13] {
        self.components
    }

    pub(crate) fn changed_from(self, before: Self) -> u16 {
        self.components
            .iter()
            .zip(before.components)
            .enumerate()
            .fold(0, |mask, (index, (after, before))| {
                mask | (u16::from(*after != before) << index)
            })
    }
}

impl core::fmt::Debug for XkbKeyboardState {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("XkbKeyboardState")
            .finish_non_exhaustive()
    }
}

impl XkbKeyboardState {
    pub fn new(config: &XkbRmlvoConfig) -> Result<Self, XkbKeyboardError> {
        let keymap = compile_keymap(config)?;
        Ok(Self {
            state: xkb::State::new(&keymap),
            down: [0; 4],
            physical_known: true,
        })
    }

    pub fn map_evdev_key(&mut self, evdev_keycode: u32, pressed: bool) -> Option<(u8, u16)> {
        let x_keycode = evdev_keycode
            .checked_add(8)
            .and_then(|keycode| u8::try_from(keycode).ok().filter(|keycode| *keycode >= 8))?;
        let state = self.modifier_mask();
        let index = usize::from(x_keycode) / 64;
        let bit = 1u64 << (x_keycode % 64);
        let was_down = self.down[index] & bit != 0;
        // xkb_state_update_key requires balanced down/up calls. Preserve
        // ordinary behavior on duplicates, but never certify cleanup from
        // a bitmap after violating that requirement (modifiers may stick).
        let was_known = self.physical_known && was_down != pressed;
        self.physical_known = false;
        self.state.update_key(
            xkb::Keycode::new(u32::from(x_keycode)),
            if pressed {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            },
        );
        let slot = &mut self.down[index];
        if pressed {
            *slot |= bit;
        } else {
            *slot &= !bit;
        }
        self.physical_known = was_known;
        Some((x_keycode, state))
    }

    /// Source state for cleanup under the native owner's retained runner.
    /// This does not identify that owner or prove common/recipient settlement.
    #[allow(dead_code)] // The native keyboard integration will consume this.
    pub(crate) fn physical_key_state(&self, key: u8) -> XkbPhysicalKeyState {
        if key < 8 {
            XkbPhysicalKeyState::InvalidKey
        } else if !self.physical_known {
            XkbPhysicalKeyState::Unavailable
        } else if self.down[usize::from(key) / 64] & (1u64 << (key % 64)) != 0 {
            XkbPhysicalKeyState::Held
        } else {
            XkbPhysicalKeyState::Released
        }
    }

    pub fn modifier_mask(&self) -> u16 {
        u16::try_from(self.state.serialize_mods(xkb::STATE_MODS_EFFECTIVE) & 0xff).unwrap_or(0)
    }

    /// No mutation or allocation. An interrupted physical history does not
    /// acquire a fresh claim of currentness merely by serializing modifiers.
    pub(crate) fn ordered_state(&self) -> Option<XkbOrderedState> {
        self.physical_known.then(|| {
            // The X11 modifier domain is the eight real modifier bits.
            // Sophia's wire contract has an empty group compatibility map
            // and zero InternalMods/IgnoreLockMods (and no setters for them).
            // Under that contract all five derived modifier states equal
            // effective modifiers. They still have distinct change bits.
            let effective = self.state.serialize_mods(xkb::STATE_MODS_EFFECTIVE) & 0xff;
            XkbOrderedState {
                components: [
                    effective,
                    self.state.serialize_mods(xkb::STATE_MODS_DEPRESSED) & 0xff,
                    self.state.serialize_mods(xkb::STATE_MODS_LATCHED) & 0xff,
                    self.state.serialize_mods(xkb::STATE_MODS_LOCKED) & 0xff,
                    self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
                    self.state.serialize_layout(xkb::STATE_LAYOUT_DEPRESSED),
                    self.state.serialize_layout(xkb::STATE_LAYOUT_LATCHED),
                    self.state.serialize_layout(xkb::STATE_LAYOUT_LOCKED),
                    effective,
                    effective,
                    effective,
                    effective,
                    effective,
                ],
            }
        })
    }
}

impl Default for XkbKeyboardState {
    fn default() -> Self {
        Self::new(&XkbRmlvoConfig::default())
            .expect("the deterministic evdev/pc105/us XKB keymap must compile")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XCoreKeyboardMapper {
    shift: u8,
    control: u8,
    alt: u8,
    caps_lock: bool,
    num_lock: bool,
}

impl XCoreKeyboardMapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_locks(caps_lock: bool, num_lock: bool) -> Self {
        Self {
            caps_lock,
            num_lock,
            ..Self::default()
        }
    }

    pub fn map_evdev_key(&mut self, evdev_keycode: u32, pressed: bool) -> Option<(u8, u16)> {
        if evdev_keycode == 0 {
            return None;
        }
        let state = self.modifier_mask();
        match evdev_keycode {
            42 => update_modifier_bit(&mut self.shift, 1, pressed),
            54 => update_modifier_bit(&mut self.shift, 2, pressed),
            29 => update_modifier_bit(&mut self.control, 1, pressed),
            97 => update_modifier_bit(&mut self.control, 2, pressed),
            56 => update_modifier_bit(&mut self.alt, 1, pressed),
            100 => update_modifier_bit(&mut self.alt, 2, pressed),
            58 if pressed => self.caps_lock = !self.caps_lock,
            69 if pressed => self.num_lock = !self.num_lock,
            _ => {}
        }
        let x_keycode = evdev_keycode
            .checked_add(8)
            .and_then(|keycode| u8::try_from(keycode).ok().filter(|keycode| *keycode >= 8))?;
        Some((x_keycode, state))
    }

    pub fn modifier_mask(self) -> u16 {
        u16::from(self.shift > 0)
            | (u16::from(self.caps_lock) << 1)
            | (u16::from(self.control > 0) << 2)
            | (u16::from(self.alt > 0) << 3)
            | (u16::from(self.num_lock) << 4)
    }
}

fn update_modifier_bit(bits: &mut u8, bit: u8, pressed: bool) {
    if pressed {
        *bits |= bit;
    } else {
        *bits &= !bit;
    }
}

#[path = "keyboard/tests/physical_state.rs"]
mod physical_state_tests;
