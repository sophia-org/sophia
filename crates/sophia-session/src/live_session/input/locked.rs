use super::*;
use crate::session_lock_input::{SessionLockInput, SessionLockKeyOutcome};

/// Physical input while the session lock holds the seat.
///
/// Keys go to the lock alone, after the VT and emergency recognizers, and
/// pointer input goes nowhere. Devices still arrive and leave as they do
/// unlocked, so a departing keyboard releases what it held in every
/// recognizer. Nothing here reaches the X frontend, the WM, a shell or a
/// launcher.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_locked_input(
    events: Vec<sophia_protocol::InputEventPacket>,
    lock: &mut SessionLockInput,
    client_keys: &mut SessionClientKeyState,
    input_sender: &impl RoutedInputIngress,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    emergency_chord: &mut EmergencyChordState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    next_input_delivery: &mut u64,
    now_msec: u64,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    let mut report = PhysicalInputRouteReport {
        events: events.len(),
        ..PhysicalInputRouteReport::default()
    };
    for event in events {
        match event.kind {
            sophia_protocol::InputEventKind::Key { keycode, pressed } => {
                report.keys_observed = report.keys_observed.saturating_add(1);
                keyboard_coverage.observe_key_at_device(event.device, keycode, pressed);
                match lock.observe_key(
                    event.device,
                    keycode,
                    pressed,
                    event.time_msec,
                    emergency_chord,
                    virtual_terminal_chord,
                ) {
                    SessionLockKeyOutcome::Consumed => {}
                    SessionLockKeyOutcome::EmergencyExit => report.emergency_exit = true,
                    SessionLockKeyOutcome::VirtualTerminal {
                        terminal,
                        trigger,
                        modifiers,
                    } => {
                        keyboard_coverage.observe_virtual_terminal(terminal);
                        report.virtual_terminal = Some(terminal);
                        report.virtual_terminal_trigger_keycode = Some(trigger);
                        report.virtual_terminal_modifier_keycodes = modifiers;
                    }
                }
            }
            sophia_protocol::InputEventKind::DeviceAdded {
                keyboard,
                pointer,
                touch,
                virtual_bus,
            } => report.devices_added.push(DeviceArrival {
                device: event.device,
                keyboard,
                pointer,
                touch,
                virtual_bus,
            }),
            sophia_protocol::InputEventKind::DeviceRemoved => {
                let removal = release_departed_device(
                    event.device,
                    client_keys,
                    input_sender,
                    &mut report.ingress_saturation,
                    modifiers,
                    key_repeat,
                    virtual_terminal_chord,
                    emergency_chord,
                    keyboard_coverage,
                    next_input_delivery,
                    now_msec,
                    &mut report.device_release_deliveries,
                )?;
                report.devices_removed.push(removal);
            }
            sophia_protocol::InputEventKind::PointerMotion
            | sophia_protocol::InputEventKind::PointerButton { .. }
            | sophia_protocol::InputEventKind::PointerAxis { .. } => {
                report.pointer_events = report.pointer_events.saturating_add(1);
            }
        }
    }
    Ok(report)
}
