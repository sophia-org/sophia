// The records a reply carries inside it: device descriptions for XInput's
// two device queries, and mode and monitor descriptions for RandR.
//
// Split by subject from `client_output.rs`, which keeps the output, reply,
// error and event vocabularies themselves. These are the shapes those
// replies are built from rather than replies in their own right, and none of
// them is reachable except through one.

/// One device in an XI1 `ListInputDevices` reply.
///
/// Separate from `XXiDeviceInfo` because XI1 and XI2 describe a device
/// differently: XI1 names the type with an atom, reports a `DeviceUse`, and has
/// no vocabulary for scroll classes. Both are projected from one table in the
/// XI dispatcher, so the difference is a shape difference, not a second truth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XXiLegacyDeviceInfo {
    pub device_id: u8,
    pub device_type: u32,
    pub device_use: u8,
    pub name: String,
    pub classes: Vec<XXiLegacyDeviceClass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XXiLegacyDeviceClass {
    Key { min_keycode: u8, max_keycode: u8 },
    Button { button_count: u16 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XXiDeviceInfo {
    pub device_id: u16,
    pub device_type: u16,
    pub attachment: u16,
    pub name: String,
    pub classes: Vec<XXiDeviceClass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XXiDeviceClass {
    Key {
        source_id: u16,
        keys: Vec<u32>,
    },
    Button {
        source_id: u16,
        button_count: u16,
    },
    Valuator {
        source_id: u16,
        number: u16,
        label: u32,
        min: i64,
        max: i64,
        value: i64,
    },
    Scroll {
        source_id: u16,
        number: u16,
        scroll_type: u16,
        flags: u32,
        increment: i64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XRandrModeInfo {
    pub id: u32,
    pub width: u16,
    pub height: u16,
    pub refresh_millihz: u32,
    /// The scanout timing this mode runs, when the output reported one.
    ///
    /// `None` means the encoder has to describe a mode it was never told the
    /// shape of, which it does by declaring no blanking at all -- a modeline
    /// that cannot physically exist, and therefore cannot be mistaken for a
    /// measured one.
    pub timing: Option<sophia_protocol::OutputModeTiming>,
    pub name: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XRandrMonitorInfo {
    pub name: u32,
    pub primary: bool,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub mm_width: u32,
    pub mm_height: u32,
    pub outputs: Vec<u32>,
}

