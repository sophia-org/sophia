use super::*;

/// A device joined the seat, as the backend announced it: an opaque
/// identity and what it can do. Nothing here names hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DeviceArrival {
    pub(super) device: DeviceId,
    pub(super) keyboard: bool,
    pub(super) pointer: bool,
    pub(super) touch: bool,
    pub(super) virtual_bus: bool,
}

/// A device left the seat, and how many keys it was still holding: each of
/// those went to its client as a release, since the device that would have
/// released them is gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DeviceRemoval {
    pub(super) device: DeviceId,
    pub(super) released: usize,
}

/// Everything a departed device leaves behind on the session's side of the
/// seat: its held client keys, its repeat, and its share of every chord.
/// Runs in the ordered loop, so a key from another device later in the
/// same batch already sees the departed device's modifiers released.
#[allow(clippy::too_many_arguments)]
pub(super) fn release_departed_device(
    device: DeviceId,
    client_keys: &mut SessionClientKeyState,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    emergency_chord: &mut EmergencyChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    next_input_delivery: &mut u64,
    time_msec: u64,
    deliveries: &mut Vec<XAuthorityInputDeliveryId>,
) -> Result<DeviceRemoval, Box<dyn std::error::Error>> {
    key_repeat.cancel_device(device);
    virtual_terminal_chord.forget_device(device);
    // The session's chord is armed from the start, so a departure can only
    // ever complete an arming that already happened; it cannot trigger.
    let _ = emergency_chord.forget_device(device);
    keyboard_coverage.forget_device(device);
    let mut held = Vec::new();
    client_keys.copy_device_keys(device, &mut held);
    let mut owed = Vec::new();
    let released = flush_copied_client_pressed_keys(
        client_keys,
        &held,
        &mut owed,
        input_sender,
        ingress_saturation,
        modifiers,
        next_input_delivery,
        time_msec,
    )?;
    deliveries.extend(owed);
    Ok(DeviceRemoval { device, released })
}

/// Says what the pass did to the seat's devices, once per fact: every
/// arrival with what it can do, every departure with what it released, and
/// the first key from each device. Only opaque identities appear here.
pub(super) fn announce_device_lifecycle(
    report: &PhysicalInputRouteReport,
    udev_managed: bool,
    devices_keyed: &mut std::collections::BTreeSet<DeviceId>,
) -> std::io::Result<()> {
    if report.devices_added.is_empty()
        && report.devices_removed.is_empty()
        && report.devices_keyed.is_empty()
    {
        return Ok(());
    }
    let source = if udev_managed { "udev" } else { "paths" };
    for arrival in &report.devices_added {
        crate::session_println!(
            "sophia_live_session_input_device schema=1 status=added device={} keyboard={} pointer={} touch={} virtual={} source={source}",
            arrival.device.raw(),
            arrival.keyboard,
            arrival.pointer,
            arrival.touch,
            arrival.virtual_bus,
        );
    }
    for removal in &report.devices_removed {
        crate::session_println!(
            "sophia_live_session_input_device schema=1 status=removed device={} released={}",
            removal.device.raw(),
            removal.released,
        );
        if removal.released != 0 {
            crate::session_println!(
                "sophia_live_session_keys schema=1 status=released reason=device_removed device={} count={}",
                removal.device.raw(),
                removal.released,
            );
        }
    }
    for device in &report.devices_keyed {
        if devices_keyed.insert(*device) {
            crate::session_println!(
                "sophia_live_session_input_device schema=1 status=key_observed device={}",
                device.raw(),
            );
        }
    }
    use std::io::Write as _;
    std::io::stdout().flush()
}
