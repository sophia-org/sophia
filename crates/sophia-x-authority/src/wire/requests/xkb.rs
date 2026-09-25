/// Decoded Xkb requests; payloads retain their protocol representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XkbRequest {
    XkbUseExtension {
        wanted_major: u16,
        wanted_minor: u16,
    },
    XkbGetMap {
        full: u16,
        partial: u16,
    },
    XkbGetCompatMap {
        device_spec: u16,
    },
    XkbGetIndicatorMap {
        device_spec: u16,
    },
    XkbGetState,
    /// Asked to latch or lock modifiers and the keyboard group.
    ///
    /// Carried whole rather than reduced, because what this instance can
    /// honour is decided where the keyboard state lives, not here.
    XkbLatchLockState {
        affect_mod_locks: u8,
        mod_locks: u8,
        lock_group: bool,
        group_lock: u8,
        affect_mod_latches: u8,
        mod_latches: u8,
        latch_group: bool,
        group_latch: u16,
    },
    XkbGetControls,
    XkbGetNames {
        which: u32,
    },
    XkbGetDeviceInfo {
        device_spec: u16,
        wanted: u16,
    },
    XkbSelectEvents {
        affect_which: u16,
        clear: u16,
        select_all: u16,
        state_details: Option<(u16, u16)>,
    },
    XkbPerClientFlags {
        change: u32,
        value: u32,
    },
}
