/// Decoded Xi requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XInputRequest {
    XiQueryVersion {
        major_version: u16,
        minor_version: u16,
    },
    XiQueryPointer {
        window: XResourceId,
        device_id: u16,
    },
    XiGetClientPointer,
    XiDeviceBell,
    XiGrabDevice {
        window: XResourceId,
        time: u32,
        cursor: Option<XResourceId>,
        device_id: u16,
        pointer_mode: u8,
        keyboard_mode: u8,
        owner_events: bool,
        event_mask: Vec<u32>,
    },
    XiUngrabDevice {
        device_id: u16,
        time: u32,
    },
    XiChangeCursor {
        window: XResourceId,
        cursor: Option<XResourceId>,
    },
    XiGetExtensionVersion,
    XiListInputDevices,
    XiQueryDevice {
        device_id: u16,
    },
    XiSelectEvents {
        window: XResourceId,
        masks: Vec<(u16, Vec<u32>)>,
    },
    XiGetFocus {
        device_id: u16,
    },
    XiGetProperty,
}
