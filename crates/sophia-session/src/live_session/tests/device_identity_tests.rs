// Two physical devices through one session's ingress: what a departure
// releases, what it forgets, and what a return is.

use super::super::{DeviceRemoval, PhysicalInputRouteReport};
use super::*;
use sophia_engine::InputFocusDecision;
use sophia_x_authority::XAuthorityRoutedInput;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

const A: DeviceId = DeviceId::from_raw(257);
const B: DeviceId = DeviceId::from_raw(258);
const LEFT_SHIFT: u32 = 42;
const LEFT_CONTROL: u32 = 29;
const LEFT_ALT: u32 = 56;
const BACKSPACE: u32 = 14;
const F1: u32 = 59;

struct Seat {
    seat: SeatId,
    focus: InputFocusState,
    committed: [CommittedSurfaceState; 1],
    input_sender: SyncSender<XAuthorityRoutedInput>,
    input_receiver: Receiver<XAuthorityRoutedInput>,
    modifiers: XCoreKeyboardMapper,
    key_repeat: KeyRepeatState,
    key_repeat_map: XkbKeymapSnapshot,
    client_keys: SessionClientKeyState,
    emergency: super::super::EmergencyChordState,
    virtual_terminal: crate::session_keyboard::VirtualTerminalChordState,
    keyboard_coverage: PhysicalKeyboardCoverage,
    pointer: SessionPointerPlacement,
    next_delivery: u64,
    serial: u64,
}

impl Seat {
    fn new() -> Self {
        let seat = SeatId::from_raw(1);
        let surface = SurfaceId::new(41, 1);
        let geometry = Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        };
        let committed = [CommittedSurfaceState {
            surface,
            committed_generation: 1,
            geometry,
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 1 },
                sophia_protocol::Size {
                    width: geometry.width,
                    height: geometry.height,
                },
            ),
            damage: Region::single(geometry),
        }];
        let mut focus = InputFocusState::new();
        assert_eq!(
            focus.focus_surface(seat, surface, &committed),
            InputFocusDecision::Focused
        );
        let (input_sender, input_receiver) = sync_channel(32);
        let (key_repeat, key_repeat_map) = test_key_repeat_parts();
        Self {
            seat,
            focus,
            committed,
            input_sender,
            input_receiver,
            modifiers: XCoreKeyboardMapper::new(),
            key_repeat,
            key_repeat_map,
            client_keys: SessionClientKeyState::default(),
            emergency: super::super::EmergencyChordState::armed(),
            virtual_terminal: crate::session_keyboard::VirtualTerminalChordState::default(),
            keyboard_coverage: PhysicalKeyboardCoverage::default(),
            pointer: SessionPointerPlacement::default(),
            next_delivery: 1,
            serial: 0,
        }
    }

    fn packet(&mut self, device: DeviceId, kind: InputEventKind) -> InputEventPacket {
        self.serial += 1;
        InputEventPacket {
            serial: self.serial,
            seat: self.seat,
            device,
            time_msec: self.serial,
            kind,
            global_position: None,
            target_surface: None,
            local_position: None,
        }
    }

    fn key(&mut self, device: DeviceId, keycode: u32, pressed: bool) -> InputEventPacket {
        self.packet(device, InputEventKind::Key { keycode, pressed })
    }

    fn arrival(&mut self, device: DeviceId) -> InputEventPacket {
        self.packet(
            device,
            InputEventKind::DeviceAdded {
                keyboard: true,
                pointer: false,
                touch: false,
                virtual_bus: false,
            },
        )
    }

    fn departure(&mut self, device: DeviceId) -> InputEventPacket {
        self.packet(device, InputEventKind::DeviceRemoved)
    }

    fn route(&mut self, events: Vec<InputEventPacket>) -> PhysicalInputRouteReport {
        route_input_events(
            events,
            &self.focus,
            &self.committed,
            &[],
            &XAuthorityClientSurfaceRoutes::default(),
            &self.input_sender,
            &mut self.modifiers,
            &mut self.key_repeat,
            &self.key_repeat_map,
            &mut self.client_keys,
            &mut self.emergency,
            &mut self.virtual_terminal,
            &mut self.keyboard_coverage,
            None,
            &mut self.pointer,
            false,
            false,
            false,
            PhysicalInputRoutingMode::Full,
            &mut self.next_delivery,
            self.serial,
            None,
            None,
            None,
        )
        .unwrap()
    }

    /// Everything routed to the client so far, as (device, kind).
    fn routed(&self) -> Vec<(DeviceId, InputEventKind)> {
        self.input_receiver
            .try_iter()
            .map(|input| (input.request.device, input.request.kind))
            .collect()
    }
}

#[test]
fn removing_a_device_releases_only_its_own_held_keys() {
    let mut seat = Seat::new();
    let events = vec![
        seat.key(A, LEFT_SHIFT, true),
        seat.key(B, LEFT_SHIFT, true),
        seat.key(B, LEFT_SHIFT, false),
    ];
    let report = seat.route(events);
    assert_eq!(report.keys_routed, 3);
    assert_eq!(seat.client_keys.pending_len(), 1, "A still holds its shift");
    let _ = seat.routed();

    let departure = seat.departure(A);
    let report = seat.route(vec![departure]);

    assert_eq!(
        seat.routed(),
        [(
            A,
            InputEventKind::Key {
                keycode: LEFT_SHIFT,
                pressed: false,
            }
        )]
    );
    assert_eq!(seat.client_keys.pending_len(), 0);
    assert_eq!(seat.modifiers.modifier_mask(), 0);
    assert_eq!(
        report.devices_removed,
        [DeviceRemoval {
            device: A,
            released: 1,
        }]
    );
    assert_eq!(report.device_release_deliveries.len(), 1);
    assert!(report.deliveries.is_empty(), "a release is tracked as one");
}

#[test]
fn keys_after_a_removal_in_the_same_batch_route_without_the_removed_modifier() {
    let mut seat = Seat::new();
    let events = vec![
        seat.key(A, LEFT_SHIFT, true),
        seat.departure(A),
        seat.key(B, 30, true),
        seat.key(B, 30, false),
    ];

    let report = seat.route(events);

    assert_eq!(
        seat.routed(),
        [
            (
                A,
                InputEventKind::Key {
                    keycode: LEFT_SHIFT,
                    pressed: true,
                }
            ),
            (
                A,
                InputEventKind::Key {
                    keycode: LEFT_SHIFT,
                    pressed: false,
                }
            ),
            (
                B,
                InputEventKind::Key {
                    keycode: 30,
                    pressed: true,
                }
            ),
            (
                B,
                InputEventKind::Key {
                    keycode: 30,
                    pressed: false,
                }
            ),
        ]
    );
    assert_eq!(seat.modifiers.modifier_mask(), 0);
    assert_eq!(report.devices_removed.len(), 1);
    assert_eq!(report.devices_keyed, [A, B]);
}

#[test]
fn a_removal_cancels_only_the_removed_devices_repeat() {
    let mut seat = Seat::new();
    let events = vec![seat.key(A, 30, true), seat.key(B, 31, true)];
    let _ = seat.route(events);
    let held_by_b = seat.key_repeat.active_target(seat.seat);
    assert_eq!(held_by_b.map(|target| target.device), Some(B));

    let departure = seat.departure(A);
    let _ = seat.route(vec![departure]);
    assert_eq!(seat.key_repeat.active_target(seat.seat), held_by_b);

    let departure = seat.departure(B);
    let _ = seat.route(vec![departure]);
    assert_eq!(seat.key_repeat.active_target(seat.seat), None);
}

#[test]
fn a_removal_forgets_the_devices_virtual_terminal_and_emergency_state() {
    let mut seat = Seat::new();
    let events = vec![
        seat.key(A, LEFT_CONTROL, true),
        seat.key(A, LEFT_ALT, true),
        seat.departure(A),
        seat.key(B, BACKSPACE, true),
        seat.key(B, F1, true),
    ];

    let report = seat.route(events);

    assert!(!report.emergency_exit);
    assert_eq!(report.virtual_terminal, None);
    assert_eq!(report.keys_routed, 4, "B's keys are ordinary keys");
    assert_eq!(seat.client_keys.pending_len(), 2, "B holds its two");
}

#[test]
fn a_replugged_device_is_a_new_identity() {
    let mut seat = Seat::new();
    let returned = DeviceId::from_raw(259);
    let events = vec![
        seat.arrival(A),
        seat.key(A, 30, true),
        seat.departure(A),
        seat.arrival(returned),
        seat.key(returned, 30, true),
    ];

    let report = seat.route(events);

    assert_eq!(
        report
            .devices_added
            .iter()
            .map(|arrival| arrival.device)
            .collect::<Vec<_>>(),
        [A, returned]
    );
    assert!(report.devices_added.iter().all(|arrival| arrival.keyboard));
    assert!(!report.devices_added[0].virtual_bus);
    assert_eq!(report.devices_removed.len(), 1);
    assert_eq!(report.devices_keyed, [A, returned]);
    assert_eq!(
        seat.client_keys.pending_len(),
        1,
        "the old identity's hold was released, the new one's stands"
    );
}
