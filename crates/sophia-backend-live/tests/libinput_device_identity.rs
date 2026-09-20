#![cfg(feature = "libinput-events")]

//! Per-device identity at the backend: what the roster mints, what an
//! announcement carries, and what a device the roster never met falls back
//! to. Driven through `reduce_observation`, the stage after libinput, so no
//! device is needed.

use std::fs;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use sophia_backend_live::{
    DeviceId, InputEventPacket, LibinputNativeEventReadReport, LiveLibinputEventReader,
    NATIVE_LIBINPUT_FIRST_MINTED_DEVICE_RAW, NativeDeviceCapabilities, NativeDeviceIdentity,
    NativeLibinputDeviceMap, NativeLibinputDeviceRecord, NativeLibinputDeviceRoster,
    NativeLibinputEventReader, NativeLibinputPolicyReport, NativeObservation, SeatId,
    native_device_virtual_bus_under,
};
use sophia_protocol::InputEventKind;

struct RejectingLibinputInterface;

impl input::LibinputInterface for RejectingLibinputInterface {
    fn open_restricted(&mut self, _path: &Path, _flags: i32) -> Result<OwnedFd, i32> {
        Err(1)
    }

    fn close_restricted(&mut self, _fd: OwnedFd) {}
}

const SEAT: SeatId = SeatId::from_raw(1);
const CLASS_KEYBOARD: DeviceId = DeviceId::from_raw(1);
const CLASS_POINTER: DeviceId = DeviceId::from_raw(2);

fn reader() -> NativeLibinputEventReader {
    NativeLibinputEventReader::new(
        input::Libinput::new_from_path(RejectingLibinputInterface),
        NativeLibinputDeviceMap::new(SEAT)
            .with_keyboard_device(CLASS_KEYBOARD)
            .with_pointer_device(CLASS_POINTER),
    )
}

fn keyboard(sysname: &str) -> NativeDeviceIdentity {
    NativeDeviceIdentity {
        sysname: sysname.to_owned(),
        vendor: 0x046d,
        product: 0xc31c,
        capabilities: NativeDeviceCapabilities {
            keyboard: true,
            pointer: false,
            touch: false,
        },
        virtual_bus: false,
    }
}

fn key(device: Option<DeviceId>, keycode: u32, pressed: bool, time_msec: u64) -> NativeObservation {
    NativeObservation::Key {
        device,
        keycode,
        pressed,
        time_msec,
    }
}

fn announced(reader: &mut NativeLibinputEventReader, identity: &NativeDeviceIdentity) -> DeviceId {
    let packet = reader
        .reduce_observation(NativeObservation::Added(identity.clone()))
        .expect("an admission is announced");
    assert!(matches!(packet.kind, InputEventKind::DeviceAdded { .. }));
    packet.device
}

#[test]
fn distinct_devices_are_minted_distinct_identities_above_the_class_range() {
    let mut reader = reader();

    let a = announced(&mut reader, &keyboard("event7"));
    let b = announced(&mut reader, &keyboard("event9"));

    assert_ne!(a, b);
    assert!(a.raw() >= NATIVE_LIBINPUT_FIRST_MINTED_DEVICE_RAW);
    assert!(b.raw() >= NATIVE_LIBINPUT_FIRST_MINTED_DEVICE_RAW);
    assert_eq!(reader.roster().len(), 2);
    assert_eq!(reader.device_inventory().len(), 2);
}

#[test]
fn an_announcement_carries_capabilities_and_the_bus_and_nothing_else() {
    let mut reader = reader();
    let mut identity = keyboard("event7");
    identity.virtual_bus = true;
    identity.capabilities.pointer = true;

    let packet = reader
        .reduce_observation(NativeObservation::Added(identity))
        .expect("announced");

    assert_eq!(packet.seat, SEAT);
    assert_eq!(packet.time_msec, 0);
    assert_eq!(packet.global_position, None);
    assert_eq!(
        packet.kind,
        InputEventKind::DeviceAdded {
            keyboard: true,
            pointer: true,
            touch: false,
            virtual_bus: true,
        }
    );
}

#[test]
fn a_device_that_leaves_and_returns_is_a_new_identity() {
    let mut reader = reader();
    let first = announced(&mut reader, &keyboard("event7"));

    let departed = reader
        .reduce_observation(NativeObservation::Removed {
            sysname: "event7".to_owned(),
        })
        .expect("a departure is announced");
    assert_eq!(departed.kind, InputEventKind::DeviceRemoved);
    assert_eq!(departed.device, first);
    assert!(reader.roster().is_empty());
    assert!(reader.device_inventory().is_empty());

    let returned = announced(&mut reader, &keyboard("event7"));

    assert_ne!(returned, first);
    assert!(returned.raw() > first.raw());
    assert_eq!(reader.device_inventory().len(), 1);
}

#[test]
fn announcements_keep_their_place_among_the_keys_of_the_devices_they_name() {
    let mut reader = reader();
    let a = announced(&mut reader, &keyboard("event7"));
    let b = announced(&mut reader, &keyboard("event9"));

    let held = reader
        .reduce_observation(key(Some(a), 42, true, 10))
        .expect("a key from a rostered device");
    let departed = reader
        .reduce_observation(NativeObservation::Removed {
            sysname: "event7".to_owned(),
        })
        .expect("announced");
    let other = reader
        .reduce_observation(key(Some(b), 30, true, 20))
        .expect("a key from the other device");

    assert_eq!(held.device, a);
    assert_eq!(departed.device, a);
    assert_eq!(other.device, b);
    assert!(held.serial < departed.serial && departed.serial < other.serial);
    assert_eq!(reader.policy_report().identity_fallbacks, 0);
}

#[test]
fn a_device_the_roster_never_met_falls_back_to_the_class_identity_and_is_counted() {
    let mut reader = reader();

    let stamped = reader
        .reduce_observation(key(None, 42, true, 10))
        .expect("a class identity stands in");
    let moved = reader
        .reduce_observation(NativeObservation::Motion {
            device: None,
            dx: 1.0,
            dy: 2.0,
            time_msec: 11,
        })
        .expect("a class identity stands in");

    assert_eq!(stamped.device, CLASS_KEYBOARD);
    assert_eq!(moved.device, CLASS_POINTER);
    assert_eq!(reader.policy_report().identity_fallbacks, 2);
}

#[test]
fn without_a_class_identity_an_unmet_device_produces_nothing() {
    let mut reader = NativeLibinputEventReader::new(
        input::Libinput::new_from_path(RejectingLibinputInterface),
        NativeLibinputDeviceMap::new(SEAT),
    );

    assert_eq!(reader.reduce_observation(key(None, 42, true, 10)), None);
    assert_eq!(reader.policy_report().identity_fallbacks, 0);
}

#[test]
fn a_departure_of_a_device_the_roster_never_met_is_nothing() {
    let mut reader = reader();
    let a = announced(&mut reader, &keyboard("event7"));

    let departed = reader.reduce_observation(NativeObservation::Removed {
        sysname: "event3".to_owned(),
    });

    assert_eq!(departed, None);
    assert_eq!(reader.roster().device_for("event7"), Some(a));
}

#[test]
fn a_wheel_is_scaled_and_a_silent_wheel_is_nothing() {
    let mut reader = reader();
    let mouse = NativeDeviceIdentity {
        capabilities: NativeDeviceCapabilities {
            keyboard: false,
            pointer: true,
            touch: false,
        },
        ..keyboard("event5")
    };
    let m = announced(&mut reader, &mouse);

    let scrolled = reader
        .reduce_observation(NativeObservation::Wheel {
            device: Some(m),
            horizontal: None,
            vertical: Some(120.0),
            time_msec: 5,
        })
        .expect("a wheel step");
    let silent = reader.reduce_observation(NativeObservation::Wheel {
        device: Some(m),
        horizontal: None,
        vertical: None,
        time_msec: 6,
    });

    assert_eq!(scrolled.device, m);
    assert_eq!(
        scrolled.kind,
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: 120,
        }
    );
    assert_eq!(silent, None);
}

#[test]
fn an_inventory_record_is_an_identity_and_capabilities_with_no_name() {
    fn carries_no_strings<T: Copy>() {}
    carries_no_strings::<NativeLibinputDeviceRecord>();

    let mut roster = NativeLibinputDeviceRoster::new();
    let record = roster.admit(&keyboard("event7"));

    assert_eq!(
        roster.inventory(),
        vec![NativeLibinputDeviceRecord {
            device: record.device,
            capabilities: NativeDeviceCapabilities {
                keyboard: true,
                pointer: false,
                touch: false,
            },
            virtual_bus: false,
        }]
    );
}

#[test]
fn retained_announcements_are_served_before_the_first_live_read() {
    let mut reader = reader();
    let first = reader
        .reduce_observation(NativeObservation::Added(keyboard("event7")))
        .expect("announced");
    let second = reader
        .reduce_observation(NativeObservation::Added(keyboard("event9")))
        .expect("announced");
    reader.retain_for_first_read([first.clone(), second.clone()]);
    assert_eq!(reader.retained_len(), 2);

    let partial = reader.read_ready_input_events(1);
    assert_eq!(partial.events, vec![first]);
    assert_eq!(
        partial.report,
        LibinputNativeEventReadReport::events_read(1, 1)
    );

    let rest = reader.read_ready_input_events(4);
    assert_eq!(rest.events, vec![second]);
    assert_eq!(reader.retained_len(), 0);

    let live = reader.read_ready_input_events(4);
    assert_eq!(live.events, Vec::<InputEventPacket>::new());
    assert_eq!(live.report, LibinputNativeEventReadReport::idle());
}

#[test]
fn the_seat_report_starts_without_fallbacks() {
    assert_eq!(NativeLibinputPolicyReport::default().identity_fallbacks, 0);
}

fn sysfs_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sophia-device-identity-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch sysfs");
    root
}

fn write_bustype(root: &Path, sysname: &str, text: &str) {
    let dir = root.join(sysname).join("device").join("id");
    fs::create_dir_all(&dir).expect("id dir");
    fs::write(dir.join("bustype"), text).expect("bustype");
}

#[test]
fn the_virtual_bus_is_read_from_the_kernel_and_nothing_else_counts_as_virtual() {
    let root = sysfs_root("bus");
    write_bustype(&root, "event7", "0006\n");
    write_bustype(&root, "event8", "0003\n");
    write_bustype(&root, "event9", "not a number\n");

    assert!(native_device_virtual_bus_under(&root, "event7"));
    assert!(!native_device_virtual_bus_under(&root, "event8"));
    assert!(!native_device_virtual_bus_under(&root, "event9"));
    assert!(!native_device_virtual_bus_under(&root, "event10"));
    assert!(!native_device_virtual_bus_under(&root, ""));
    assert!(!native_device_virtual_bus_under(&root, "../event7"));

    let _ = fs::remove_dir_all(&root);
}
