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

/// The button mapping `GetPointerMapping` reports.
///
/// Sophia remaps nothing, so entry `n` is button `n`, for as many buttons as it
/// can emit.
pub fn x_pointer_button_mapping() -> Vec<u8> {
    (1..=X_POINTER_BUTTON_COUNT).collect()
}

/// XI2 reserves valuators 0 and 1 for relative pointer X and Y.
pub const X_POINTER_HORIZONTAL_SCROLL_VALUATOR: u16 = 2;
/// The vertical scroll valuator follows pointer X/Y and horizontal scrolling.
pub const X_POINTER_VERTICAL_SCROLL_VALUATOR: u16 = 3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XCorePointerMapper {
    button_state: u16,
    horizontal_scroll_v120: i32,
    vertical_scroll_v120: i32,
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
        let (button, mask) = match evdev_button {
            272 => (1, 1 << 8),
            274 => (2, 1 << 9),
            273 => (3, 1 << 10),
            275 => (8, 0),
            276 => (9, 0),
            _ => return None,
        };
        let state = self.button_state;
        if pressed {
            self.button_state |= mask;
        } else {
            self.button_state &= !mask;
        }
        Some((button, state))
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
