//! What an adapter is given, and the whole of what it can do.

use sophia_protocol::{DeviceId, InputEventKind, Point, SurfaceId};
use sophia_x_authority::{XAuthorityInputDeliveryId, XServerFrontendClientId};

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
    _private: (),
}

impl PrivateInputSubmission {
    /// Which connection this handle acts for.
    pub fn connection(&self) -> PrivateInputConnection {
        unimplemented!("service thread lands with the keeper work")
    }

    pub fn device(&self) -> DeviceId {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Submit one pointer motion to a surface.
    pub fn submit_pointer_motion(
        &self,
        _target: SurfaceId,
        _global: Point,
        _local: Point,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Submit one pointer button press or release.
    pub fn submit_pointer_button(
        &self,
        _target: SurfaceId,
        _button: u32,
        _pressed: bool,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Submit one key press or release.
    pub fn submit_key(
        &self,
        _target: SurfaceId,
        _keycode: u32,
        _pressed: bool,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        unimplemented!("service thread lands with the keeper work")
    }

    /// Submit an already-built event kind, for a caller that needs one this
    /// facade does not name.
    pub fn submit(
        &self,
        _target: SurfaceId,
        _kind: InputEventKind,
    ) -> Result<PrivateInputAccepted, PrivateInputSubmitError> {
        unimplemented!("service thread lands with the keeper work")
    }
}
