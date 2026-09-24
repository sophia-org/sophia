/// The highest core X button Sophia emits, and therefore the count it advertises.
///
/// One owner because there were three, and they had already drifted:
/// `map_evdev_button` emits 8 and 9 for a mouse's side buttons, `GetPointerMapping`
/// answered with seven entries, and XI2's master pointer declared seven under a
/// comment asserting the two agreed. They agreed by hand. Sophia advertises what
/// it implements, so the advertised count follows the mapper rather than the
/// other way around.
///
/// Buttons 4-7 are the scroll directions `map_axis_to_button` produces, so 1..=9
/// is the whole set a client can observe. Only 1-5 carry a bit in the core state
/// field; X has never had bits for the rest.
pub const X_POINTER_BUTTON_COUNT: u8 = 9;

/// The identity button mapping: entry `n` is button `n`, for as many buttons
/// as this authority can emit. What GetPointerMapping reports until a client
/// sets another (t166).
pub fn x_pointer_button_mapping() -> Vec<u8> {
    XPointerButtonMapping::identity().as_vec()
}

/// The core button mapping a client sets with SetPointerMapping: physical
/// button `n` is delivered as `logical[n - 1]`, and a zero entry disables it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPointerButtonMapping {
    logical: [u8; X_POINTER_BUTTON_COUNT as usize],
}

/// Why a SetPointerMapping was refused, each the Value error the protocol
/// names, carrying the value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XPointerMappingRefusal {
    /// The list is not as long as the buttons this authority advertises.
    LengthMismatch(u8),
    /// A nonzero logical button named twice.
    DuplicateButton(u8),
}

impl Default for XPointerButtonMapping {
    fn default() -> Self {
        Self::identity()
    }
}

impl XPointerButtonMapping {
    pub fn identity() -> Self {
        let mut logical = [0; X_POINTER_BUTTON_COUNT as usize];
        for (index, entry) in logical.iter_mut().enumerate() {
            *entry = index as u8 + 1;
        }
        Self { logical }
    }

    pub fn from_request(list: &[u8]) -> Result<Self, XPointerMappingRefusal> {
        if list.len() != usize::from(X_POINTER_BUTTON_COUNT) {
            return Err(XPointerMappingRefusal::LengthMismatch(
                u8::try_from(list.len()).unwrap_or(u8::MAX),
            ));
        }
        let mut seen = [false; 256];
        for entry in list {
            if *entry != 0 {
                if seen[usize::from(*entry)] {
                    return Err(XPointerMappingRefusal::DuplicateButton(*entry));
                }
                seen[usize::from(*entry)] = true;
            }
        }
        let mut logical = [0; X_POINTER_BUTTON_COUNT as usize];
        logical.copy_from_slice(list);
        Ok(Self { logical })
    }

    /// The logical button a physical one is delivered as, or `None` when the
    /// mapping disables it.
    pub fn logical(self, physical: u8) -> Option<u8> {
        let entry = *self.logical.get(usize::from(physical.checked_sub(1)?))?;
        (entry != 0).then_some(entry)
    }

    pub fn as_vec(self) -> Vec<u8> {
        self.logical.to_vec()
    }

    /// The physical buttons whose entry differs between two mappings.
    pub fn changed_buttons(self, other: Self) -> impl Iterator<Item = u8> {
        self.logical
            .into_iter()
            .zip(other.logical)
            .enumerate()
            .filter_map(|(index, (before, after))| (before != after).then_some(index as u8 + 1))
    }
}

/// XI2 reserves valuators 0 and 1 for relative pointer X and Y.
pub const X_POINTER_HORIZONTAL_SCROLL_VALUATOR: u16 = 2;
/// The vertical scroll valuator follows pointer X/Y and horizontal scrolling.
pub const X_POINTER_VERTICAL_SCROLL_VALUATOR: u16 = 3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XCorePointerMapper {
    button_state: u16,
    // Core event state cannot represent side buttons 8 and 9. Native grab
    // retirement needs the complete supported button set as well.
    held_buttons: u16,
    horizontal_scroll_v120: i32,
    vertical_scroll_v120: i32,
}

/// One evdev button mapped: the physical button, the logical one it is
/// delivered as (none when the mapping disables it), and the core state
/// before it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XMappedButton {
    pub physical: u8,
    pub logical: Option<u8>,
    pub state_before: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XScrollAxisUpdate {
    pub button: u8,
    pub horizontal_position_v120: Option<i32>,
    pub vertical_position_v120: Option<i32>,
}

impl XCorePointerMapper {
    const fn core_button_mask(button: u8) -> u16 {
        if button <= 5 { 1u16 << (button + 7) } else { 0 }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub const fn state(self) -> u16 {
        self.button_state
    }

    /// All supported physical buttons, including those absent from core state.
    #[cfg_attr(not(test), allow(dead_code))] // Ordered native release integration uses the full set.
    pub(crate) const fn all_buttons_released(self) -> bool {
        self.held_buttons == 0
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn button_is_pressed(self, button: u8) -> bool {
        button > 0
            && button <= X_POINTER_BUTTON_COUNT
            && self.held_buttons & (1u16 << (button - 1)) != 0
    }

    pub const fn horizontal_scroll_position_v120(self) -> i32 {
        self.horizontal_scroll_v120
    }

    pub const fn vertical_scroll_position_v120(self) -> i32 {
        self.vertical_scroll_v120
    }

    /// The core button an evdev code names, without moving anything.
    ///
    /// Separated from mapping because an ordered execution has to name the
    /// input it is validating before any effect, and mapping moves the button
    /// state. Additive: the ordinary path still maps and moves in one step.
    pub const fn peek_evdev_button(evdev_button: u32) -> Option<u8> {
        match evdev_button {
            272 => Some(1),
            274 => Some(2),
            273 => Some(3),
            275 => Some(8),
            276 => Some(9),
            _ => None,
        }
    }

    pub fn map_evdev_button(&mut self, evdev_button: u32, pressed: bool) -> Option<(u8, u16)> {
        self.map_evdev_button_mapped(XPointerButtonMapping::identity(), evdev_button, pressed)
            .and_then(|mapped| mapped.logical.map(|logical| (logical, mapped.state_before)))
    }

    /// An evdev button under a client's mapping (t166). The physical button
    /// is what is held; the logical one is what is delivered and what the
    /// core state bit follows. A disabled button is held and delivered to
    /// nobody, its state bit never set.
    pub fn map_evdev_button_mapped(
        &mut self,
        mapping: XPointerButtonMapping,
        evdev_button: u32,
        pressed: bool,
    ) -> Option<XMappedButton> {
        let physical = match evdev_button {
            272 => 1,
            274 => 2,
            273 => 3,
            275 => 8,
            276 => 9,
            _ => return None,
        };
        let logical = mapping.logical(physical);
        let mask = logical.map_or(0, Self::core_button_mask);
        let state_before = self.button_state;
        if pressed {
            self.button_state |= mask;
            self.held_buttons |= 1u16 << (physical - 1);
        } else {
            self.button_state &= !mask;
            self.held_buttons &= !(1u16 << (physical - 1));
        }
        Some(XMappedButton {
            physical,
            logical,
            state_before,
        })
    }

    pub const fn map_axis_to_button(horizontal_v120: i32, vertical_v120: i32) -> Option<u8> {
        if vertical_v120 < 0 {
            Some(4)
        } else if vertical_v120 > 0 {
            Some(5)
        } else if horizontal_v120 < 0 {
            Some(6)
        } else if horizontal_v120 > 0 {
            Some(7)
        } else {
            None
        }
    }

    pub fn map_axis(
        &mut self,
        horizontal_v120: i32,
        vertical_v120: i32,
    ) -> Option<XScrollAxisUpdate> {
        let button = Self::map_axis_to_button(horizontal_v120, vertical_v120)?;
        let horizontal_position_v120 = (horizontal_v120 != 0).then(|| {
            self.horizontal_scroll_v120 =
                self.horizontal_scroll_v120.saturating_add(horizontal_v120);
            self.horizontal_scroll_v120
        });
        let vertical_position_v120 = (vertical_v120 != 0).then(|| {
            self.vertical_scroll_v120 = self.vertical_scroll_v120.saturating_add(vertical_v120);
            self.vertical_scroll_v120
        });
        Some(XScrollAxisUpdate {
            button,
            horizontal_position_v120,
            vertical_position_v120,
        })
    }

    pub const fn axis_release_state(self, button: u8) -> u16 {
        self.button_state | Self::core_button_mask(button)
    }
}
