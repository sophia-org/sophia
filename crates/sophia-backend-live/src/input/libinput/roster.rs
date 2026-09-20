use crate::prelude::*;

use std::path::Path;

/// The first identity the roster mints. The seat's class identities are
/// small (the session pins 1 and 2), and a minted identity must never be
/// mistaken for one of them.
pub const NATIVE_LIBINPUT_FIRST_MINTED_DEVICE_RAW: u64 = 256;

/// The kernel bus type a uinput device reports, `BUS_VIRTUAL` in
/// `linux/input.h`.
const BUS_VIRTUAL: u32 = 0x06;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeDeviceCapabilities {
    pub keyboard: bool,
    pub pointer: bool,
    pub touch: bool,
}

/// What libinput knows a device by. Built in one place from the libinput
/// device and consumed by the roster; the name and path stay here and are
/// never printed or placed on a packet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeDeviceIdentity {
    pub sysname: String,
    pub vendor: u32,
    pub product: u32,
    pub capabilities: NativeDeviceCapabilities,
    pub virtual_bus: bool,
}

/// One admitted device as the rest of the backend may see it: an opaque
/// identity and what it can do. Deliberately `Copy` with no strings, so an
/// inventory cannot carry a name out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLibinputDeviceRecord {
    pub device: DeviceId,
    pub capabilities: NativeDeviceCapabilities,
    pub virtual_bus: bool,
}

/// The devices currently on the seat, keyed by the kernel name libinput
/// reports on every event. Every admission mints a fresh identity, so a
/// device that leaves and returns is a new one and nothing that remembered
/// the old identity can be fooled by the return.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeLibinputDeviceRoster {
    entries: BTreeMap<String, NativeLibinputDeviceRecord>,
    next_raw: u64,
}

impl Default for NativeLibinputDeviceRoster {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeLibinputDeviceRoster {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            next_raw: NATIVE_LIBINPUT_FIRST_MINTED_DEVICE_RAW,
        }
    }

    /// Admits a device under a fresh identity. A name already present is
    /// replaced, which only happens if libinput announced an addition twice
    /// without a removal between; the earlier identity is then gone the same
    /// way a removal would have taken it.
    pub fn admit(&mut self, identity: &NativeDeviceIdentity) -> NativeLibinputDeviceRecord {
        let record = NativeLibinputDeviceRecord {
            device: DeviceId::from_raw(self.next_raw),
            capabilities: identity.capabilities,
            virtual_bus: identity.virtual_bus,
        };
        self.next_raw = self.next_raw.saturating_add(1);
        self.entries.insert(identity.sysname.clone(), record);
        record
    }

    pub fn evict(&mut self, sysname: &str) -> Option<NativeLibinputDeviceRecord> {
        self.entries.remove(sysname)
    }

    pub fn device_for(&self, sysname: &str) -> Option<DeviceId> {
        self.entries.get(sysname).map(|record| record.device)
    }

    pub fn inventory(&self) -> Vec<NativeLibinputDeviceRecord> {
        self.entries.values().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Whether the kernel reports the device on the virtual bus, read from sysfs
/// by the event node's name. Unreadable means not virtual: a device the
/// kernel will not describe is treated as hardware, which is the claim that
/// costs more to get wrong in the other direction.
pub fn native_device_virtual_bus(sysname: &str) -> bool {
    native_device_virtual_bus_under(Path::new("/sys/class/input"), sysname)
}

/// The same reading against any root, so a test can lay out a sysfs of its
/// own. A name that is not a plain node name reads as not virtual.
pub fn native_device_virtual_bus_under(sys_root: &Path, sysname: &str) -> bool {
    if sysname.is_empty() || sysname.contains('/') || sysname.contains("..") {
        return false;
    }
    let path = sys_root
        .join(sysname)
        .join("device")
        .join("id")
        .join("bustype");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| u32::from_str_radix(text.trim(), 16).ok())
        .is_some_and(|bus| bus == BUS_VIRTUAL)
}
