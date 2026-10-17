use crate::{OutputId, Size, SurfaceId, WmActionId};
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WmModifierMask {
    pub bits: u32,
}

impl WmModifierMask {
    pub const SHIFT: u32 = 1 << 0;
    pub const CONTROL: u32 = 1 << 1;
    pub const ALT: u32 = 1 << 2;
    pub const SUPER: u32 = 1 << 3;
    pub const SUPPORTED: u32 = Self::SHIFT | Self::CONTROL | Self::ALT | Self::SUPER;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmBindingRegistration {
    pub action: WmActionId,
    pub keycode: u32,
    pub modifiers: WmModifierMask,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmRgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFocusRingStyle {
    pub enabled: bool,
    pub width: u32,
    pub color: WmRgb8,
}

impl Default for WmFocusRingStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 2,
            color: WmRgb8 {
                red: 0x70,
                green: 0xb7,
                blue: 0xff,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmFrameStyle {
    pub enabled: bool,
    pub width: u32,
    pub focused_color: WmRgb8,
    pub unfocused_color: WmRgb8,
}

impl Default for WmFrameStyle {
    fn default() -> Self {
        Self {
            enabled: false,
            width: 0,
            focused_color: WmFocusRingStyle::default().color,
            unfocused_color: WmRgb8 {
                red: 0x30,
                green: 0x30,
                blue: 0x30,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WmChromePolicy {
    pub focus_ring: WmFocusRingStyle,
    pub frame: WmFrameStyle,
}

impl WmChromePolicy {
    pub const fn clearance(self) -> u32 {
        let ring = if self.focus_ring.enabled {
            self.focus_ring.width
        } else {
            0
        };
        let frame = if self.frame.enabled {
            self.frame.width
        } else {
            0
        };
        if ring > frame { ring } else { frame }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayoutNodeCapabilities {
    pub movable: bool,
    pub resizable: bool,
    pub focusable: bool,
    pub closable: bool,
    pub fullscreenable: bool,
}

impl LayoutNodeCapabilities {
    pub const STANDARD_TOPLEVEL: Self = Self {
        movable: true,
        resizable: true,
        focusable: true,
        closable: true,
        fullscreenable: true,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceConstraints {
    pub min_size: Option<Size>,
    pub max_size: Option<Size>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyTranslationGroup {
    pub output: OutputId,
    pub group: u64,
    pub x: i32,
    pub y: i32,
    pub members: Vec<SurfaceId>,
}
