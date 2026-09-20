use crate::prelude::*;

use super::{
    LibinputNativeEventReadReport, LibinputNativeEventReadResult, NativeLibinputEventPoller,
    NativeLibinputPointerPolicy, apply_native_pointer_policy,
};

use input::DeviceCapability;
use input::event::{
    Event as NativeLibinputEvent, EventTrait,
    device::DeviceEvent,
    keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait},
    pointer::{Axis, ButtonState, PointerEvent, PointerEventTrait, PointerScrollEvent},
};
use sophia_protocol::{InputEventKind, Point};
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct NativeLibinputEventReader {
    libinput: input::Libinput,
    devices: NativeLibinputDeviceMap,
    policy: Arc<Mutex<NativeLibinputPolicyReport>>,
    pointer_policy: NativeLibinputPointerPolicy,
    pointer_position: Point,
    next_serial: u64,
    roster: NativeLibinputDeviceRoster,
    inventory: Arc<Mutex<Vec<NativeLibinputDeviceRecord>>>,
    /// Device announcements observed while opening, served before the first
    /// live read so the seat's inventory reaches the consumer in band and in
    /// order, the same way a later change does.
    retained: VecDeque<InputEventPacket>,
}

/// What one libinput event amounts to once the device behind it has been
/// looked up. This is the boundary between the stage that touches libinput
/// and the stage that mints identities and stamps packets, so the second can
/// be driven without a device. `device` on the per-event kinds is the roster's
/// identity for the event's device, or `None` if the roster has never met it.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeObservation {
    Added(NativeDeviceIdentity),
    Removed {
        sysname: String,
    },
    Motion {
        device: Option<DeviceId>,
        dx: f64,
        dy: f64,
        time_msec: u64,
    },
    Button {
        device: Option<DeviceId>,
        button: u32,
        pressed: bool,
        time_msec: u64,
    },
    Wheel {
        device: Option<DeviceId>,
        horizontal: Option<f64>,
        vertical: Option<f64>,
        time_msec: u64,
    },
    Key {
        device: Option<DeviceId>,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
    },
}

impl NativeLibinputEventReader {
    pub fn new(libinput: input::Libinput, devices: NativeLibinputDeviceMap) -> Self {
        Self::new_with_policies(
            libinput,
            devices,
            NativeLibinputPolicyReport::default(),
            NativeLibinputPointerPolicy::default(),
        )
    }

    pub fn new_with_policy(
        libinput: input::Libinput,
        devices: NativeLibinputDeviceMap,
        policy: NativeLibinputPolicyReport,
    ) -> Self {
        Self::new_with_policies(
            libinput,
            devices,
            policy,
            NativeLibinputPointerPolicy::default(),
        )
    }

    pub(crate) fn new_with_policies(
        libinput: input::Libinput,
        devices: NativeLibinputDeviceMap,
        policy: NativeLibinputPolicyReport,
        pointer_policy: NativeLibinputPointerPolicy,
    ) -> Self {
        Self {
            libinput,
            devices,
            policy: Arc::new(Mutex::new(policy)),
            pointer_policy,
            pointer_position: Point { x: 0.0, y: 0.0 },
            next_serial: 1,
            roster: NativeLibinputDeviceRoster::new(),
            inventory: Arc::new(Mutex::new(Vec::new())),
            retained: VecDeque::new(),
        }
    }

    pub const fn devices(&self) -> NativeLibinputDeviceMap {
        self.devices
    }

    pub const fn pointer_position(&self) -> Point {
        self.pointer_position
    }

    pub fn policy_report(&self) -> NativeLibinputPolicyReport {
        self.policy
            .lock()
            .map_or_else(|_| NativeLibinputPolicyReport::default(), |policy| *policy)
    }

    pub(crate) fn policy_handle(&self) -> Arc<Mutex<NativeLibinputPolicyReport>> {
        Arc::clone(&self.policy)
    }

    pub const fn roster(&self) -> &NativeLibinputDeviceRoster {
        &self.roster
    }

    /// The devices on the seat right now, as opaque records.
    pub fn device_inventory(&self) -> Vec<NativeLibinputDeviceRecord> {
        self.roster.inventory()
    }

    pub(crate) fn inventory_handle(&self) -> Arc<Mutex<Vec<NativeLibinputDeviceRecord>>> {
        Arc::clone(&self.inventory)
    }

    /// How many announcements wait to be served before the next live read.
    pub fn retained_len(&self) -> usize {
        self.retained.len()
    }

    pub fn libinput_mut(&mut self) -> &mut input::Libinput {
        &mut self.libinput
    }

    pub(crate) fn libinput_mut_ref(&self) -> &input::Libinput {
        &self.libinput
    }

    fn next_serial(&mut self) -> u64 {
        let serial = self.next_serial;
        self.next_serial = self.next_serial.saturating_add(1);
        serial
    }

    fn event_packet(
        &mut self,
        device: DeviceId,
        time_msec: u64,
        kind: InputEventKind,
        global_position: Option<Point>,
    ) -> InputEventPacket {
        InputEventPacket {
            serial: self.next_serial(),
            seat: self.devices.seat,
            device,
            time_msec,
            kind,
            global_position,
            target_surface: None,
            local_position: None,
        }
    }

    fn publish_inventory(&self) {
        if let Ok(mut inventory) = self.inventory.lock() {
            *inventory = self.roster.inventory();
        }
    }

    /// The identity an event is stamped with: the roster's, or the seat's
    /// class identity for a device the roster never met, counted so a seat
    /// running on fallbacks is visible in its report.
    fn resolve(&self, device: Option<DeviceId>, class: Option<DeviceId>) -> Option<DeviceId> {
        if let Some(device) = device {
            return Some(device);
        }
        let class = class?;
        if let Ok(mut policy) = self.policy.lock() {
            policy.identity_fallbacks = policy.identity_fallbacks.saturating_add(1);
        }
        Some(class)
    }

    fn identify(device: &input::Device) -> NativeDeviceIdentity {
        NativeDeviceIdentity {
            sysname: device.sysname().to_owned(),
            vendor: device.id_vendor(),
            product: device.id_product(),
            capabilities: NativeDeviceCapabilities {
                keyboard: device.has_capability(DeviceCapability::Keyboard),
                pointer: device.has_capability(DeviceCapability::Pointer),
                touch: device.has_capability(DeviceCapability::Touch),
            },
            virtual_bus: native_device_virtual_bus(device.sysname()),
        }
    }

    fn known(&self, device: &input::Device) -> Option<DeviceId> {
        self.roster.device_for(device.sysname())
    }

    /// The only stage that touches libinput. Device policy is applied here
    /// because it needs the device; everything after is `reduce_observation`.
    fn classify(&mut self, event: NativeLibinputEvent) -> Option<NativeObservation> {
        match event {
            NativeLibinputEvent::Device(DeviceEvent::Added(event)) => {
                let mut device = event.device();
                if let Ok(mut policy) = self.policy.lock()
                    && policy.udev_managed
                {
                    policy.devices_added = policy.devices_added.saturating_add(1);
                    policy.active_devices = policy.active_devices.saturating_add(1);
                    if device.has_capability(DeviceCapability::Keyboard) {
                        policy.keyboards = policy.keyboards.saturating_add(1);
                    }
                    if device.has_capability(DeviceCapability::Pointer) {
                        policy.pointers = policy.pointers.saturating_add(1);
                        let outcome = apply_native_pointer_policy(&mut device, self.pointer_policy);
                        policy.pointer_settings_unsupported = policy
                            .pointer_settings_unsupported
                            .saturating_add(outcome.unsupported);
                        if outcome.accepted() {
                            if self.pointer_policy.requires_device_configuration() {
                                policy.pointer_configured =
                                    policy.pointer_configured.saturating_add(1);
                            }
                        } else {
                            if policy.refused_setting.is_none() {
                                policy.refused_setting = outcome.refused;
                            }
                            policy.configuration_failures =
                                policy.configuration_failures.saturating_add(1);
                        }
                    }
                    if device.has_capability(DeviceCapability::Touch) {
                        policy.touch_devices = policy.touch_devices.saturating_add(1);
                    }
                    if device.config_tap_finger_count() > 0 {
                        policy.tap_capable = policy.tap_capable.saturating_add(1);
                        if device.config_tap_set_enabled(true).is_ok()
                            && device.config_tap_enabled()
                        {
                            policy.tap_enabled = policy.tap_enabled.saturating_add(1);
                        }
                    }
                }
                Some(NativeObservation::Added(Self::identify(&device)))
            }
            NativeLibinputEvent::Device(DeviceEvent::Removed(event)) => {
                let device = event.device();
                if let Ok(mut policy) = self.policy.lock()
                    && policy.udev_managed
                {
                    policy.devices_removed = policy.devices_removed.saturating_add(1);
                    policy.active_devices = policy.active_devices.saturating_sub(1);
                    if device.has_capability(DeviceCapability::Keyboard) {
                        policy.keyboards = policy.keyboards.saturating_sub(1);
                    }
                    if device.has_capability(DeviceCapability::Pointer) {
                        policy.pointers = policy.pointers.saturating_sub(1);
                    }
                    if device.has_capability(DeviceCapability::Touch) {
                        policy.touch_devices = policy.touch_devices.saturating_sub(1);
                    }
                }
                Some(NativeObservation::Removed {
                    sysname: device.sysname().to_owned(),
                })
            }
            NativeLibinputEvent::Pointer(PointerEvent::Motion(event)) => {
                Some(NativeObservation::Motion {
                    device: self.known(&event.device()),
                    dx: event.dx(),
                    dy: event.dy(),
                    time_msec: u64::from(event.time()),
                })
            }
            NativeLibinputEvent::Pointer(PointerEvent::Button(event)) => {
                Some(NativeObservation::Button {
                    device: self.known(&event.device()),
                    button: event.button(),
                    pressed: event.button_state() == ButtonState::Pressed,
                    time_msec: u64::from(event.time()),
                })
            }
            NativeLibinputEvent::Pointer(PointerEvent::ScrollWheel(event)) => {
                let axis = |axis| event.has_axis(axis).then(|| event.scroll_value_v120(axis));
                Some(NativeObservation::Wheel {
                    device: self.known(&event.device()),
                    horizontal: axis(Axis::Horizontal),
                    vertical: axis(Axis::Vertical),
                    time_msec: u64::from(event.time()),
                })
            }
            NativeLibinputEvent::Keyboard(KeyboardEvent::Key(event)) => {
                Some(NativeObservation::Key {
                    device: self.known(&event.device()),
                    keycode: event.key(),
                    pressed: event.key_state() == KeyState::Pressed,
                    time_msec: u64::from(event.time()),
                })
            }
            _ => None,
        }
    }

    /// Mints, evicts, falls back and stamps. Needs no device, so a test can
    /// drive it with observations it wrote itself.
    pub fn reduce_observation(
        &mut self,
        observation: NativeObservation,
    ) -> Option<InputEventPacket> {
        match observation {
            NativeObservation::Added(identity) => {
                let record = self.roster.admit(&identity);
                self.publish_inventory();
                // An announcement has no event time; a consumer that dates
                // events must not date this one.
                Some(self.event_packet(
                    record.device,
                    0,
                    InputEventKind::DeviceAdded {
                        keyboard: record.capabilities.keyboard,
                        pointer: record.capabilities.pointer,
                        touch: record.capabilities.touch,
                        virtual_bus: record.virtual_bus,
                    },
                    None,
                ))
            }
            NativeObservation::Removed { sysname } => {
                let record = self.roster.evict(&sysname)?;
                self.publish_inventory();
                Some(self.event_packet(record.device, 0, InputEventKind::DeviceRemoved, None))
            }
            NativeObservation::Motion {
                device,
                dx,
                dy,
                time_msec,
            } => {
                let device = self.resolve(device, self.devices.pointer_device)?;
                self.pointer_position.x += dx;
                self.pointer_position.y += dy;
                Some(self.event_packet(
                    device,
                    time_msec,
                    InputEventKind::PointerMotion,
                    Some(self.pointer_position),
                ))
            }
            NativeObservation::Button {
                device,
                button,
                pressed,
                time_msec,
            } => {
                let device = self.resolve(device, self.devices.pointer_device)?;
                Some(self.event_packet(
                    device,
                    time_msec,
                    InputEventKind::PointerButton { button, pressed },
                    Some(self.pointer_position),
                ))
            }
            NativeObservation::Wheel {
                device,
                horizontal,
                vertical,
                time_msec,
            } => {
                let device = self.resolve(device, self.devices.pointer_device)?;
                let scale = |value: Option<f64>| {
                    value.map_or(0, |value| self.pointer_policy.scale_scroll_v120(value))
                };
                let horizontal_v120 = scale(horizontal);
                let vertical_v120 = scale(vertical);
                if horizontal_v120 == 0 && vertical_v120 == 0 {
                    return None;
                }
                Some(self.event_packet(
                    device,
                    time_msec,
                    InputEventKind::PointerAxis {
                        horizontal_v120,
                        vertical_v120,
                    },
                    Some(self.pointer_position),
                ))
            }
            NativeObservation::Key {
                device,
                keycode,
                pressed,
                time_msec,
            } => {
                let device = self.resolve(device, self.devices.keyboard_device)?;
                Some(self.event_packet(
                    device,
                    time_msec,
                    InputEventKind::Key { keycode, pressed },
                    None,
                ))
            }
        }
    }

    fn reduce_event(&mut self, event: NativeLibinputEvent) -> Option<InputEventPacket> {
        let observation = self.classify(event)?;
        self.reduce_observation(observation)
    }

    /// Reads what libinput queued while the seat was being opened and keeps
    /// only the device announcements, to be served ahead of the first live
    /// read. Input that arrived before anyone was listening is dropped, as it
    /// always was.
    fn retain_open_announcements(&mut self, max_read: usize) {
        if self.libinput.dispatch().is_err() {
            return;
        }
        let mut read = 0;
        while read < max_read {
            let Some(event) = self.libinput.next() else {
                break;
            };
            read += 1;
            if let Some(packet) = self.reduce_event(event)
                && matches!(
                    packet.kind,
                    InputEventKind::DeviceAdded { .. } | InputEventKind::DeviceRemoved
                )
            {
                self.retained.push_back(packet);
            }
        }
    }

    /// Lets a test stand in for the open path: what it retains is served
    /// before the first live read, exactly as an opened seat's announcements.
    pub fn retain_for_first_read(&mut self, packets: impl IntoIterator<Item = InputEventPacket>) {
        self.retained.extend(packets);
    }
}

impl LiveLibinputEventReader for NativeLibinputEventReader {
    fn read_ready_input_events(&mut self, max_read: usize) -> LibinputNativeEventReadResult {
        if max_read == 0 {
            return LibinputNativeEventReadResult {
                report: LibinputNativeEventReadReport::idle(),
                events: Vec::new(),
            };
        }

        let mut events = Vec::new();
        while events.len() < max_read {
            let Some(packet) = self.retained.pop_front() else {
                break;
            };
            events.push(packet);
        }
        if events.len() >= max_read {
            return LibinputNativeEventReadResult {
                report: LibinputNativeEventReadReport::events_read(
                    events.len(),
                    self.retained.len(),
                ),
                events,
            };
        }

        if self.libinput.dispatch().is_err() {
            return LibinputNativeEventReadResult {
                report: LibinputNativeEventReadReport::read_failed(),
                events: Vec::new(),
            };
        }

        while events.len() < max_read {
            let Some(event) = self.libinput.next() else {
                break;
            };
            if let Some(packet) = self.reduce_event(event) {
                events.push(packet);
            }
        }

        if self.policy_report().configuration_failures > 0 {
            return LibinputNativeEventReadResult {
                report: LibinputNativeEventReadReport::read_failed(),
                events: Vec::new(),
            };
        }

        LibinputNativeEventReadResult {
            report: LibinputNativeEventReadReport::events_read(events.len(), 0),
            events,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirectLibinputInterface;

impl input::LibinputInterface for DirectLibinputInterface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        const O_ACCMODE: i32 = 3;
        const O_WRONLY: i32 = 1;
        const O_RDWR: i32 = 2;
        let access_mode = flags & O_ACCMODE;
        std::fs::OpenOptions::new()
            .read(access_mode != O_WRONLY)
            .write(access_mode == O_WRONLY || access_mode == O_RDWR)
            .custom_flags(flags & !O_ACCMODE)
            .open(path)
            .map(Into::into)
            .map_err(|error| error.raw_os_error().unwrap_or(1))
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}

#[cfg(feature = "seat-control")]
pub struct SeatLibinputInterface {
    opener: crate::LiveSeatDeviceOpener,
    leases: std::collections::HashMap<std::os::fd::RawFd, crate::LiveSeatDevice>,
}

#[cfg(feature = "seat-control")]
impl SeatLibinputInterface {
    fn new(opener: crate::LiveSeatDeviceOpener) -> Self {
        Self {
            opener,
            leases: std::collections::HashMap::new(),
        }
    }
}

#[cfg(feature = "seat-control")]
impl input::LibinputInterface for SeatLibinputInterface {
    fn open_restricted(&mut self, path: &Path, _flags: i32) -> Result<OwnedFd, i32> {
        use std::os::fd::AsRawFd;

        let lease = self.opener.open(path).map_err(|_| 13)?;
        let fd = lease
            .duplicate_owned_fd()
            .map_err(|error| error.raw_os_error().unwrap_or(1))?;
        self.leases.insert(fd.as_raw_fd(), lease);
        Ok(fd)
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        use std::os::fd::AsRawFd;

        self.leases.remove(&fd.as_raw_fd());
        drop(fd);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLibinputOpenError {
    NoDevices,
    TooManyDevices,
    InvalidDevicePath,
    DeviceUnavailable,
    /// A device refused a setting for a reason other than not having it.
    /// Carries which setting, because a session that will not start has
    /// to say what it would not accept.
    DeviceConfigurationFailed(&'static str),
    SeatAssignmentFailed,
    MissingKeyboard,
    MissingPointer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeLibinputPolicyReport {
    pub devices_added: usize,
    pub devices_removed: usize,
    pub active_devices: usize,
    pub keyboards: usize,
    pub pointers: usize,
    pub touch_devices: usize,
    pub tap_capable: usize,
    pub tap_enabled: usize,
    pub pointer_configured: usize,
    /// Preferences a device did not have. Counted, never fatal.
    pub pointer_settings_unsupported: usize,
    pub configuration_failures: usize,
    /// The first setting a device refused for a reason other than not having
    /// it, so a fatal configuration says which knob it died on.
    pub refused_setting: Option<&'static str>,
    pub udev_managed: bool,
    /// Events stamped with the seat's class identity because their device
    /// was not on the roster. Zero on a healthy seat; counted, never fatal.
    pub identity_fallbacks: usize,
}

impl core::fmt::Display for NativeLibinputOpenError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "native libinput open failed: {self:?}")
    }
}

impl std::error::Error for NativeLibinputOpenError {}

pub fn open_native_libinput_path_poller(
    paths: &[PathBuf],
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    open_native_libinput_path_poller_with_pointer_policy(
        paths,
        devices,
        max_read_per_poll,
        NativeLibinputPointerPolicy::default(),
    )
}

pub fn open_native_libinput_path_poller_with_pointer_policy(
    paths: &[PathBuf],
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    let pointer_policy =
        pointer_policy
            .validate()
            .ok_or(NativeLibinputOpenError::DeviceConfigurationFailed(
                "pointer-policy-bounds",
            ))?;
    if paths.is_empty() {
        return Err(NativeLibinputOpenError::NoDevices);
    }
    if paths.len() > 16 {
        return Err(NativeLibinputOpenError::TooManyDevices);
    }
    let mut libinput = input::Libinput::new_from_path(DirectLibinputInterface);
    let mut policy = NativeLibinputPolicyReport::default();
    for path in paths {
        let resolved = resolve_native_libinput_device_path(path)?;
        let path = resolved
            .to_str()
            .ok_or(NativeLibinputOpenError::InvalidDevicePath)?;
        let mut device = libinput
            .path_add_device(path)
            .ok_or(NativeLibinputOpenError::DeviceUnavailable)?;
        policy.devices_added = policy.devices_added.saturating_add(1);
        policy.active_devices = policy.active_devices.saturating_add(1);
        if device.has_capability(DeviceCapability::Keyboard) {
            policy.keyboards = policy.keyboards.saturating_add(1);
        }
        if device.has_capability(DeviceCapability::Pointer) {
            policy.pointers = policy.pointers.saturating_add(1);
            let outcome = apply_native_pointer_policy(&mut device, pointer_policy);
            policy.pointer_settings_unsupported = policy
                .pointer_settings_unsupported
                .saturating_add(outcome.unsupported);
            if let Some(setting) = outcome.refused {
                return Err(NativeLibinputOpenError::DeviceConfigurationFailed(setting));
            }
            if pointer_policy.requires_device_configuration() {
                policy.pointer_configured = policy.pointer_configured.saturating_add(1);
            }
        }
        if device.has_capability(DeviceCapability::Touch) {
            policy.touch_devices = policy.touch_devices.saturating_add(1);
        }
        if device.config_tap_finger_count() > 0 {
            policy.tap_capable = policy.tap_capable.saturating_add(1);
            match device.config_tap_set_enabled(true) {
                Ok(()) if device.config_tap_enabled() => {
                    policy.tap_enabled = policy.tap_enabled.saturating_add(1);
                }
                // A device reporting fingers whose tap will not turn on has no
                // tap to give. It is not a reason to refuse the seat.
                Ok(()) | Err(input::DeviceConfigError::Unsupported) => {
                    policy.pointer_settings_unsupported =
                        policy.pointer_settings_unsupported.saturating_add(1);
                }
                Err(_) => {
                    return Err(NativeLibinputOpenError::DeviceConfigurationFailed("tap"));
                }
            }
        }
    }
    Ok(NativeLibinputEventPoller::new(
        NativeLibinputEventReader::new_with_policies(libinput, devices, policy, pointer_policy),
        max_read_per_poll.clamp(1, 256),
    ))
}

pub fn open_native_libinput_udev_poller(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    open_native_libinput_udev_poller_with_pointer_policy(
        seat_name,
        devices,
        max_read_per_poll,
        NativeLibinputPointerPolicy::default(),
    )
}

pub fn open_native_libinput_udev_poller_with_pointer_policy(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    if seat_name.is_empty() || seat_name.len() > 64 || !seat_name.is_ascii() {
        return Err(NativeLibinputOpenError::SeatAssignmentFailed);
    }
    let mut libinput = input::Libinput::new_with_udev(DirectLibinputInterface);
    libinput
        .udev_assign_seat(seat_name)
        .map_err(|_| NativeLibinputOpenError::SeatAssignmentFailed)?;
    finish_udev_open(libinput, devices, max_read_per_poll, pointer_policy)
}

#[cfg(feature = "seat-control")]
pub fn open_native_libinput_udev_poller_with_seat(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    opener: crate::LiveSeatDeviceOpener,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    open_native_libinput_udev_poller_with_seat_and_pointer_policy(
        seat_name,
        devices,
        max_read_per_poll,
        opener,
        NativeLibinputPointerPolicy::default(),
    )
}

#[cfg(feature = "seat-control")]
pub fn open_native_libinput_udev_poller_with_seat_and_pointer_policy(
    seat_name: &str,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    opener: crate::LiveSeatDeviceOpener,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    if seat_name.is_empty() || seat_name.len() > 64 || !seat_name.is_ascii() {
        return Err(NativeLibinputOpenError::SeatAssignmentFailed);
    }
    let mut libinput = input::Libinput::new_with_udev(SeatLibinputInterface::new(opener));
    libinput
        .udev_assign_seat(seat_name)
        .map_err(|_| NativeLibinputOpenError::SeatAssignmentFailed)?;
    finish_udev_open(libinput, devices, max_read_per_poll, pointer_policy)
}

fn finish_udev_open(
    libinput: input::Libinput,
    devices: NativeLibinputDeviceMap,
    max_read_per_poll: usize,
    pointer_policy: NativeLibinputPointerPolicy,
) -> Result<NativeLibinputEventPoller<NativeLibinputEventReader>, NativeLibinputOpenError> {
    let pointer_policy =
        pointer_policy
            .validate()
            .ok_or(NativeLibinputOpenError::DeviceConfigurationFailed(
                "pointer-policy-bounds",
            ))?;
    let mut reader = NativeLibinputEventReader::new_with_policies(
        libinput,
        devices,
        NativeLibinputPolicyReport {
            udev_managed: true,
            ..NativeLibinputPolicyReport::default()
        },
        pointer_policy,
    );
    reader.retain_open_announcements(256);
    let policy = reader.policy_report();
    if policy.keyboards == 0 {
        return Err(NativeLibinputOpenError::MissingKeyboard);
    }
    if policy.pointers == 0 && policy.touch_devices == 0 {
        return Err(NativeLibinputOpenError::MissingPointer);
    }
    if policy.configuration_failures > 0 {
        return Err(NativeLibinputOpenError::DeviceConfigurationFailed(
            policy.refused_setting.unwrap_or("unknown-setting"),
        ));
    }
    Ok(NativeLibinputEventPoller::new(
        reader,
        max_read_per_poll.clamp(1, 256),
    ))
}

pub fn resolve_native_libinput_device_path(
    path: &Path,
) -> Result<PathBuf, NativeLibinputOpenError> {
    if !path.is_absolute() {
        return Err(NativeLibinputOpenError::InvalidDevicePath);
    }
    std::fs::canonicalize(path).map_err(|_| NativeLibinputOpenError::DeviceUnavailable)
}
