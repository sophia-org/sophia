use crate::geometry::{Point, Transform};
use crate::ids::{ApplicationRouteLeaseId, DeviceId, SeatId, SurfaceId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationRouteLeaseIdentity {
    pub id: ApplicationRouteLeaseId,
    pub seat: SeatId,
    pub frontend_sequence: u64,
    pub control_epoch: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputEventPacket {
    pub serial: u64,
    pub seat: SeatId,
    pub device: DeviceId,
    pub time_msec: u64,
    pub kind: InputEventKind,
    pub global_position: Option<Point>,
    pub target_surface: Option<SurfaceId>,
    pub local_position: Option<Point>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEventKind {
    PointerMotion,
    PointerButton {
        button: u32,
        pressed: bool,
    },
    PointerAxis {
        horizontal_v120: i32,
        vertical_v120: i32,
    },
    Key {
        keycode: u32,
        pressed: bool,
    },
    /// A physical device joined the seat. The packet's `device` is its
    /// identity for as long as it stays; a device that leaves and returns is
    /// announced again under a new one. Names and paths never travel here.
    DeviceAdded {
        keyboard: bool,
        pointer: bool,
        touch: bool,
        /// The kernel reports the device on the virtual bus: a uinput device,
        /// admitted like any other and marked so evidence cannot mistake it
        /// for hardware.
        virtual_bus: bool,
    },
    /// The device left the seat. Every key it still held is owed a release,
    /// which the session issues on seeing this.
    DeviceRemoved,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputRoute {
    pub input_serial: u64,
    pub target_surface: Option<SurfaceId>,
    pub global_position: Point,
    pub local_position: Option<Point>,
    pub transform: Transform,
    pub outcome: InputRouteOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputRouteOutcome {
    Routed,
    NoTarget,
    StaleTarget,
    Denied,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoutedInputRequest {
    pub serial: u64,
    pub seat: SeatId,
    pub device: DeviceId,
    pub time_msec: u64,
    pub target_surface: SurfaceId,
    pub global_position: Point,
    pub local_position: Point,
    pub kind: InputEventKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutedInputDecision {
    pub serial: u64,
    pub target_surface: SurfaceId,
    pub outcome: RoutedInputOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutedInputOutcome {
    Accepted,
    RejectedStaleTarget,
    RejectedDeniedNamespace,
    RejectedActiveGrab,
    RejectedFocusPolicy,
    RejectedUnsupportedEvent,
}
