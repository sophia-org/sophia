use crate::{X_SETUP_ARGB_VISUAL, X_SETUP_DEFAULT_VISUAL, XAuthorityAccessError};

pub const X_TRUE_COLOR_RED_MASK: u32 = 0x00ff_0000;
pub const X_TRUE_COLOR_GREEN_MASK: u32 = 0x0000_ff00;
pub const X_TRUE_COLOR_BLUE_MASK: u32 = 0x0000_00ff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XColorRgb16 {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XTrueColorVisual {
    pub id: u32,
    pub depth: u8,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub alpha_mask: u32,
}

/// Why a window's colormap attribute could not be set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XWindowColormapError {
    /// The colormap does not exist.
    Color,
    /// The colormap's visual is not the window's.
    Match,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XColormapError {
    DuplicateId,
    UnknownVisual,
    Access(XAuthorityAccessError),
}

impl XTrueColorVisual {
    pub const fn valid_pixel_mask(self) -> u32 {
        self.red_mask | self.green_mask | self.blue_mask | self.alpha_mask
    }

    pub const fn screen_color(self, exact: XColorRgb16) -> XColorRgb16 {
        // Eight-bit TrueColor resolves through the high byte, then reports the
        // selected map entry across the full RGB16 reply field.
        XColorRgb16 {
            red: expand_component(component_index(exact.red)),
            green: expand_component(component_index(exact.green)),
            blue: expand_component(component_index(exact.blue)),
        }
    }

    pub const fn pixel(self, screen: XColorRgb16) -> u32 {
        // Core allocation on a depth-32 TrueColor visual returns opaque ARGB;
        // alpha is not a mutable colormap component.
        ((component_index(screen.red) as u32) << 16)
            | ((component_index(screen.green) as u32) << 8)
            | component_index(screen.blue) as u32
            | self.alpha_mask
    }

    pub const fn query(self, pixel: u32) -> Option<XColorRgb16> {
        if pixel & !self.valid_pixel_mask() != 0 {
            return None;
        }
        Some(XColorRgb16 {
            red: expand_component(((pixel & self.red_mask) >> 16) as u8),
            green: expand_component(((pixel & self.green_mask) >> 8) as u8),
            blue: expand_component((pixel & self.blue_mask) as u8),
        })
    }
}

pub const fn x_true_color_visual(id: u32) -> Option<XTrueColorVisual> {
    match id {
        X_SETUP_DEFAULT_VISUAL => Some(XTrueColorVisual {
            id,
            depth: 24,
            red_mask: X_TRUE_COLOR_RED_MASK,
            green_mask: X_TRUE_COLOR_GREEN_MASK,
            blue_mask: X_TRUE_COLOR_BLUE_MASK,
            alpha_mask: 0,
        }),
        X_SETUP_ARGB_VISUAL => Some(XTrueColorVisual {
            id,
            depth: 32,
            red_mask: X_TRUE_COLOR_RED_MASK,
            green_mask: X_TRUE_COLOR_GREEN_MASK,
            blue_mask: X_TRUE_COLOR_BLUE_MASK,
            alpha_mask: 0xff00_0000,
        }),
        _ => None,
    }
}

pub fn x_lookup_color_name(name: &str) -> Option<XColorRgb16> {
    // Keep this table bounded to retained clients. Unknown names must remain
    // distinguishable from valid white so dispatch can return BadName.
    let normalized: String = name
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .map(|character| character.to_ascii_lowercase())
        .collect();

    if let Some(suffix) = normalized
        .strip_prefix("gray")
        .or_else(|| normalized.strip_prefix("grey"))
    {
        if suffix.is_empty() {
            return Some(rgb8(190, 190, 190));
        }
        if let Ok(percent) = suffix.parse::<u8>()
            && percent <= 100
        {
            let value = u8::try_from(u16::from(percent) * 255 / 100).ok()?;
            return Some(rgb8(value, value, value));
        }
    }

    let (red, green, blue) = match normalized.as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" | "red1" => (255, 0, 0),
        "red2" => (238, 0, 0),
        "red3" => (205, 0, 0),
        "red4" => (139, 0, 0),
        "green" | "green1" => (0, 255, 0),
        "green2" => (0, 238, 0),
        "green3" => (0, 205, 0),
        "green4" => (0, 139, 0),
        "blue" | "blue1" => (0, 0, 255),
        "blue2" => (0, 0, 238),
        "blue3" => (0, 0, 205),
        "blue4" => (0, 0, 139),
        "yellow" | "yellow1" => (255, 255, 0),
        "yellow2" => (238, 238, 0),
        "yellow3" => (205, 205, 0),
        "yellow4" => (139, 139, 0),
        "cyan" | "cyan1" => (0, 255, 255),
        "cyan2" => (0, 238, 238),
        "cyan3" => (0, 205, 205),
        "cyan4" => (0, 139, 139),
        "magenta" | "magenta1" => (255, 0, 255),
        "magenta2" => (238, 0, 238),
        "magenta3" => (205, 0, 205),
        "magenta4" => (139, 0, 139),
        "orange" => (255, 165, 0),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        "purple" | "x11purple" => (160, 32, 240),
        "navy" | "navyblue" => (0, 0, 128),
        "gold" => (255, 215, 0),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgray" | "darkgrey" => (169, 169, 169),
        _ => return None,
    };
    Some(rgb8(red, green, blue))
}

const fn component_index(component: u16) -> u8 {
    (component >> 8) as u8
}

const fn expand_component(component: u8) -> u16 {
    (component as u16) * 0x0101
}

const fn rgb8(red: u8, green: u8, blue: u8) -> XColorRgb16 {
    XColorRgb16 {
        red: expand_component(red),
        green: expand_component(green),
        blue: expand_component(blue),
    }
}
