//! What an adapter is given, and the whole of what it can do.

use sophia_protocol::{DeviceId, InputEventKind, Point, SurfaceId};
use sophia_x_authority::{XAuthorityInputDeliveryId, XServerFrontendClientId};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// One connection, named exactly.
///
/// Carried back from a revocation and reported beside a submission so a caller
/// can tell which connection it acted on without holding anything that acts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputConnection {
    pub client: XServerFrontendClientId,
    pub admission: sophia_protocol::ClientAdmissionId,
    pub connection_generation: u64,
}

/// What the order accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputAccepted {
    /// The delivery this submission will be answered against. The receipt for
    /// it arrives through the handle's delivery drain.
    pub delivery: XAuthorityInputDeliveryId,
    pub sequence: sophia_x_authority::ReadySequence,
    /// The serial Session put in the request.
    ///
    /// PASSIVE FACTS, NOT AN ENCODER RESULT. A caller checking wire bytes
    /// against what it submitted needs the exact serial and time that went
    /// into the request; reporting them here means the timestamp never has to
    /// be masked out of a comparison to make it pass.
    pub serial: u64,
    /// The millisecond Session stamped the request with, measured from this
    /// service's start.
    pub time_msec: u64,
}

/// Why a submission was not accepted.
///
/// THE REFUSAL CARRIES THE WORK BACK. The order hands the request to the
/// caller rather than dropping it, so an adapter that was refused still holds
/// exactly what it tried to submit and can decide what to do with it. An
/// error that had only named the reason would have destroyed the event to
/// report on it.
#[derive(Debug)]
pub enum PrivateInputSubmitError {
    /// The order refused it, with its own reason and the request kept.
    Refused(sophia_x_authority::PrivateSendError),
    /// The service has ended.
    Ended,
    /// This service's delivery or serial identities are used up. Refused
    /// before the order sees anything, because a reused delivery would let one
    /// receipt answer two requests.
    Exhausted,
}

/// The only thing an adapter receives.
///
/// OPAQUE ON PURPOSE. It holds the original ingress that was issued once for
/// this connection, so the grant, the device and the completion cell continue
/// across every request rather than being reissued per submission. It also
/// holds, privately, the owner it takes a lease from for the duration of one
/// call. An adapter can submit input for the connection it was issued for and
/// can do nothing else: there is no accessor here for the lease, the owner,
/// the issuer, the authority, the broker or any sender, and none is added.
pub struct PrivateInputSubmission {
    runtime: Arc<super::service::PrivateInputRuntime>,
    ingress: sophia_x_authority::PrivateIngress,
    connection: PrivateInputConnection,
    device: DeviceId,
    serial: AtomicU64,
}

impl PrivateInputSubmission {
    pub(super) fn new(
        runtime: Arc<super::service::PrivateInputRuntime>,
        ingress: sophia_x_authority::PrivateIngress,
        connection: PrivateInputConnection,
        device: DeviceId,
    ) -> Self {
        Self {
            runtime,
            ingress,
            connection,
            device,
            serial: AtomicU64::new(1),
        }
    }

    /// Which connection this handle acts for.
    pub fn connection(&self) -> PrivateInputConnection {
        self.connection
    }

    pub fn device(&self) -> DeviceId {
        self.device
    }

    /// Submit one pointer motion to a surface.
    pub fn submit_pointer_motion(
        &self,
        target: SurfaceId,
        global: Point,
        local: Point,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        self.route(target, global, local, InputEventKind::PointerMotion)
    }

    /// Submit one pointer button press or release.
    pub fn submit_pointer_button(
        &self,
        target: SurfaceId,
        button: u32,
        pressed: bool,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        self.submit(target, InputEventKind::PointerButton { button, pressed })
    }

    /// Submit one key press or release.
    pub fn submit_key(
        &self,
        target: SurfaceId,
        keycode: u32,
        pressed: bool,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        self.submit(target, InputEventKind::Key { keycode, pressed })
    }

    /// Submit an already-built event kind, for a caller that needs one this
    /// facade does not name.
    pub fn submit(
        &self,
        target: SurfaceId,
        kind: InputEventKind,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        self.route(target, Point::default(), Point::default(), kind)
    }

    /// THE LEASE IS TAKEN HERE AND RELEASED HERE. An adapter never holds one,
    /// and this never holds one across a wait: the order either takes the
    /// request or hands it back.
    fn route(
        &self,
        target: SurfaceId,
        global: Point,
        local: Point,
        kind: InputEventKind,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        let (delivery, time_msec) = self
            .runtime
            .next_delivery()
            .ok_or(PrivateInputSubmitError::Exhausted)?;
        let serial = self
            .serial
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                held.checked_add(1)
            })
            .map_err(|_| PrivateInputSubmitError::Exhausted)?;
        let route = sophia_x_authority::XAuthorityRoutedInput {
            request: sophia_protocol::RoutedInputRequest {
                serial,
                seat: self.runtime.seat,
                device: self.device,
                time_msec,
                target_surface: target,
                global_position: global,
                local_position: local,
                kind,
            },
            route_lease: None,
            delivery: Some(delivery),
            mode: sophia_x_authority::XAuthorityRoutedInputMode::Deliver,
        };
        match self.ingress.submit(&self.runtime.owner.lease(), route) {
            Ok(sequence) => Ok(PrivateInputAccepted {
                delivery,
                sequence,
                serial,
                time_msec,
            }),
            Err(refusal) => Err(PrivateInputSubmitError::Refused(refusal)),
        }
    }
}
