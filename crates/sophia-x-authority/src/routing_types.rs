use std::sync::mpsc::SyncSender;

use sophia_protocol::{
    ApplicationRouteLeaseIdentity, ClientAdmissionContext, ClientAdmissionId, Rect,
    RoutedInputRequest, SurfaceId, TransactionId,
};

use crate::{XAuthorityOutputUpdateOutcome, XResourceId, XServerFrontendClientId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum XPresentCompletionMode {
    Copy = 0,
    Flip = 1,
    Skip = 2,
    SuboptimalCopy = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityKeyEvent {
    pub keycode: u8,
    pub pressed: bool,
    pub state: u16,
    pub modifiers_after: u8,
    pub time_msec: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityPointerEventKind {
    Motion,
    Button {
        button: u8,
        pressed: bool,
    },
    Axis {
        button: u8,
        pressed: bool,
        horizontal_position_v120: Option<i32>,
        vertical_position_v120: Option<i32>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityPointerEvent {
    pub kind: XAuthorityPointerEventKind,
    pub surface: SurfaceId,
    pub root_x: i16,
    pub root_y: i16,
    pub event_x: i16,
    pub event_y: i16,
    pub state: u16,
    pub time_msec: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityInputEvent {
    Key(XAuthorityKeyEvent),
    Pointer(XAuthorityPointerEvent),
}

/// An Engine-selected input event addressed to one live X11 connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityClientInputEvent {
    pub client: XServerFrontendClientId,
    pub event: XAuthorityInputEvent,
    pub target_window: Option<XResourceId>,
    pub xi_event_type: Option<u16>,
    pub xi_event_window: Option<XResourceId>,
    pub xi_emulated_button_type: Option<u16>,
    pub xi_emulated_button_window: Option<XResourceId>,
    /// Selected XI2 pointer Enter/Leave events for this route.
    ///
    /// Keyboard FocusIn/FocusOut belongs to the authority focus transition,
    /// not to a later physical key delivery.
    pub xi_pointer_crossing_mask: u16,
    pub delivery: Option<XAuthorityInputDeliveryId>,
}

/// Protocol-neutral physical input after Engine hit-testing and focus policy.
#[derive(Clone, Debug, PartialEq)]
pub struct XAuthorityRoutedInput {
    pub request: RoutedInputRequest,
    pub route_lease: Option<ApplicationRouteLeaseIdentity>,
    pub delivery: Option<XAuthorityInputDeliveryId>,
    pub mode: XAuthorityRoutedInputMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityRouteLeaseUpdateKind {
    Confirmed,
    Rejected,
    Released,
}

/// Sanitized frontend observation for one Engine-issued application lease.
/// X resource IDs and frontend connection IDs never cross this boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityRouteLeaseUpdate {
    pub identity: ApplicationRouteLeaseIdentity,
    pub target_surface: SurfaceId,
    pub admission: ClientAdmissionContext,
    pub kind: XAuthorityRouteLeaseUpdateKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityRouteLeaseRelease {
    pub identity: ApplicationRouteLeaseIdentity,
    pub admission: ClientAdmissionContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityRoutedInputMode {
    Deliver,
    Repeat,
    StateOnly,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct XAuthorityInputDeliveryId(u64);

impl XAuthorityInputDeliveryId {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// How one routed input event ended.
///
/// `EpochRevoked` is deliberately apart from `RouteRejected`. A security epoch
/// advance is the session closing its own input boundary -- for a topology
/// change, a policy change, or a seat handover -- and every event it revokes
/// was revoked on purpose. `RouteRejected` means this event could not be
/// routed: no resolvable window, an unmappable button, a client whose queue is
/// full. The first is the session working; the second is a fault. Collapsing
/// them cost a live session, which ended because the pointer happened to move
/// while an output policy change closed the epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityInputDeliveryOutcome {
    Flushed,
    TargetGone,
    EpochRevoked,
    RouteRejected,
    WriteFailed,
    ClientDisconnected,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityClientInputDelivery {
    pub client: XServerFrontendClientId,
    pub delivery: XAuthorityInputDeliveryId,
    pub outcome: XAuthorityInputDeliveryOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityControlCommand {
    PublishMetadataRule {
        transaction: TransactionId,
        surface: SurfaceId,
        rule: sophia_protocol::MetadataDisclosureRule,
    },
    AdmitSurface {
        transaction: TransactionId,
        surface: SurfaceId,
        geometry: Rect,
    },
    ConfigureSurface {
        transaction: TransactionId,
        surface: SurfaceId,
        geometry: Rect,
    },
    SetPresentationState {
        transaction: TransactionId,
        surface: SurfaceId,
        state: sophia_protocol::PolicyPresentationState,
    },
    RestorePresentationState {
        transaction: TransactionId,
        surface: SurfaceId,
        state: sophia_protocol::PolicyPresentationState,
    },
    FocusSurface {
        transaction: TransactionId,
        surface: SurfaceId,
    },
    ClearFocus {
        transaction: TransactionId,
        surface: SurfaceId,
    },
    CloseSurface {
        transaction: TransactionId,
        surface: SurfaceId,
    },
    WithdrawSurface {
        transaction: TransactionId,
        surface: SurfaceId,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum XAuthorityControlKind {
    PublishMetadataRule,
    AdmitSurface,
    ConfigureSurface,
    SetPresentationState,
    RestorePresentationState,
    FocusSurface,
    ClearFocus,
    CloseSurface,
    WithdrawSurface,
}

impl XAuthorityControlCommand {
    pub const fn kind(self) -> XAuthorityControlKind {
        match self {
            Self::PublishMetadataRule { .. } => XAuthorityControlKind::PublishMetadataRule,
            Self::AdmitSurface { .. } => XAuthorityControlKind::AdmitSurface,
            Self::ConfigureSurface { .. } => XAuthorityControlKind::ConfigureSurface,
            Self::SetPresentationState { .. } => XAuthorityControlKind::SetPresentationState,
            Self::RestorePresentationState { .. } => {
                XAuthorityControlKind::RestorePresentationState
            }
            Self::FocusSurface { .. } => XAuthorityControlKind::FocusSurface,
            Self::ClearFocus { .. } => XAuthorityControlKind::ClearFocus,
            Self::CloseSurface { .. } => XAuthorityControlKind::CloseSurface,
            Self::WithdrawSurface { .. } => XAuthorityControlKind::WithdrawSurface,
        }
    }

    pub const fn transaction(self) -> TransactionId {
        match self {
            Self::PublishMetadataRule { transaction, .. }
            | Self::AdmitSurface { transaction, .. }
            | Self::ConfigureSurface { transaction, .. }
            | Self::SetPresentationState { transaction, .. }
            | Self::RestorePresentationState { transaction, .. }
            | Self::FocusSurface { transaction, .. }
            | Self::ClearFocus { transaction, .. }
            | Self::CloseSurface { transaction, .. }
            | Self::WithdrawSurface { transaction, .. } => transaction,
        }
    }

    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::PublishMetadataRule { surface, .. }
            | Self::AdmitSurface { surface, .. }
            | Self::ConfigureSurface { surface, .. }
            | Self::SetPresentationState { surface, .. }
            | Self::RestorePresentationState { surface, .. }
            | Self::FocusSurface { surface, .. }
            | Self::ClearFocus { surface, .. }
            | Self::CloseSurface { surface, .. }
            | Self::WithdrawSurface { surface, .. } => surface,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityClientControlCommand {
    pub client: XServerFrontendClientId,
    pub command: XAuthorityControlCommand,
}

/// The frontend-owned route for one live Engine surface.
///
/// `XAuthorityObservedTransactionBatch::client` names the connection that
/// caused a request. That actor may legally operate on another client's
/// surface in a classic shared namespace, so consumers must use this record
/// for input, control, and metadata ownership instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthoritySurfaceRouteObservation {
    pub surface: SurfaceId,
    pub client: XServerFrontendClientId,
    pub admission: Option<ClientAdmissionContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityClientMetadataCandidate {
    /// The frontend client that owns the candidate's surface route.
    pub client: XServerFrontendClientId,
    pub candidate: sophia_protocol::ReducedMetadataCandidate,
}

/// Why an operation could not take responsibility for work queued elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlDependentRefusal {
    /// This registry did not issue the registration.
    Foreign,
    /// No record is held for it, so there is nothing for the work to be
    /// counted against.
    NoLongerHeld,
    /// The operation is not being applied, so it is not in a position to be
    /// starting anything.
    NotApplying,
    /// The count cannot be advanced.
    Exhausted,
    /// The registry cannot be reached, so nothing can be established.
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityControlOutcome {
    Delivered,
    ClientGone,
    UnknownSurface,
    InvalidSize,
    AuthorityRejected,
    UnsupportedProtocol,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityControlAck {
    pub kind: XAuthorityControlKind,
    pub transaction: TransactionId,
    pub surface: SurfaceId,
    pub outcome: XAuthorityControlOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityClientControlAck {
    pub client: XServerFrontendClientId,
    pub acknowledgement: XAuthorityControlAck,
}

#[derive(Clone, Debug)]
pub enum XServerFrontendServiceCommand {
    UpdateWindowAllocationPreferences {
        snapshot: crate::XWindowAllocationPreferences,
        acknowledgement: SyncSender<crate::XWindowAllocationUpdate>,
    },
    InstallDeviceBundle {
        bundle: std::sync::Arc<crate::XServerFrontendDeviceBundle>,
        acknowledgement: SyncSender<Result<(), crate::XServerFrontendDeviceBundleError>>,
    },
    MarkDeviceGenerationUnavailable {
        generation: u64,
        acknowledgement: SyncSender<Result<(), crate::XServerFrontendDeviceBundleError>>,
    },
    StopAccepting,
    /// Close admission and client streams, but preserve ordered authority
    /// egress until accepted requests and resource teardown have drained.
    DrainAndDisconnect,
    /// Emergency cancellation after the owner's bounded drain has expired.
    StopAndDisconnect,
    RevokeAdmission {
        admission: ClientAdmissionId,
    },
    UpdateOutputTopology {
        snapshot: sophia_protocol::OutputTopologySnapshot,
        acknowledgement: SyncSender<XAuthorityOutputUpdateOutcome>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XServerFrontendRouteError {
    /// Private focus provenance could not be reserved before routing effects.
    FocusClaimRefused {
        client: XServerFrontendClientId,
        refusal: XFocusClaimRefusal,
    },
    /// An item an earlier turn took is still owned and unresolved, so the
    /// order does not run.
    ///
    /// Its application is unknown, and dequeuing into the same slot would
    /// overwrite the only record of it.
    OrderedItemUnresolved,
    LifecycleUnavailable,
    /// This instance is being drained by the ordered consumer, so the older
    /// route may not also drain it.
    ///
    /// Two consumers on one order is not a slower version of one: the older
    /// route takes an operation, discards the reservation made for it, and
    /// applies it without the execution the reservation exists for -- so work
    /// the ordered path was accepted for would be applied behind its back,
    /// with its request left unanswerable.
    OrderedRunnerEngaged,
    RecoveryShutdownFailed {
        client: XServerFrontendClientId,
    },
    UnknownClient {
        client: XServerFrontendClientId,
    },
    UnknownSurface {
        surface: SurfaceId,
    },
    /// A present named a window no surface route covers.
    ///
    /// Named by the window rather than the client, because any client may
    /// present to a window it can name -- ownership is not presentership.
    UnknownPresentWindow {
        window: XResourceId,
    },
    ClientQueueFull {
        client: XServerFrontendClientId,
    },
    DuplicatePresentation {
        transaction: TransactionId,
    },
    ClientQueueDisconnected {
        client: XServerFrontendClientId,
    },
    MetadataQueueFull,
    MetadataQueueDisconnected,
    DuplicateClient {
        client: XServerFrontendClientId,
    },
    /// No place could be reserved for what this connection might hand over.
    ///
    /// Refused BEFORE anything is published, because a connection whose
    /// accepted work would have nowhere to go must not be exposed at all.
    ContinuationUnavailable {
        client: XServerFrontendClientId,
    },
    /// No evidence custody could be reserved for this connection.
    ///
    /// Refused BEFORE anything is published, and for the same reason as the
    /// place above: a connection exposed first would be one that discovered
    /// afterwards that nothing outside it can keep what its worker leaves.
    ///
    /// DISTINCT FROM A MISSING PLACE. The place is storage inside the store;
    /// this is the keeper outside it, and a caller told the wrong one would
    /// look in the wrong direction.
    EvidenceCustodyUnavailable {
        client: XServerFrontendClientId,
    },
    DuplicateSurface {
        surface: SurfaceId,
    },
    RegistryPoisoned,
    /// The service owner offered for this act is not the one keeping this
    /// service's connections' evidence.
    ///
    /// Refused before anything is taken or advanced. Not a fact about any
    /// connection: what is refused is the association.
    ForeignServiceOwner,
    /// This client number is still held by the connection that had it.
    ///
    /// ITS ENDING IS RUNNING THE EFFECTS THAT ACT BY THAT NUMBER, or ended
    /// without establishing that reusing it is safe. Refusing here is
    /// deliberately stricter than the old behaviour, which let a successor
    /// take a number whose predecessor could still reach it.
    ///
    /// NOT A COMPLETED CLEANUP AND NOT A SETTLEMENT. It says the number is
    /// somebody's.
    ClientNumberExcluded {
        client: XServerFrontendClientId,
    },
    /// The XKB worker's command queue is full. Distinct from a poisoned lock:
    /// the worker is alive and behind, not broken.
    XkbWorkerSaturated,
    /// The XKB worker did not answer within its deadline, or has stopped.
    /// A keyboard translation that never returns would stall the whole routing
    /// thread, so the wait is bounded and this is what a timeout becomes.
    XkbWorkerUnavailable,
    /// A control could not claim execution, so none of its effects happened.
    ///
    /// Its outcome and cleanup belong to whatever refused the claim -- the
    /// completion record that still holds it, or whoever the record was
    /// handed to. Nothing is owed from here, and nothing may be applied.
    ControlNotClaimable {
        client: XServerFrontendClientId,
    },
    /// An effect a control was about to queue on another client could not be
    /// counted against the operation that would have caused it.
    ///
    /// Refused rather than queued: work nothing is counting lets its origin be
    /// settled while that work can still happen.
    DependentNotTracked {
        client: XServerFrontendClientId,
        refusal: ControlDependentRefusal,
    },
}

/// Why an origin could not reserve a private focus intent. This is neither
/// an applied receipt nor permission to retry an already accepted command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XFocusClaimRefusal {
    Unreachable,
    Unprepared,
    ForeignOrigin,
    IdentityExhausted,
}

/// Tracks the two independently ordered lifecycle phases of one X Present.
///
/// Copy normally idles its source before display completion; Flip normally
/// completes before its retained source becomes idle. The route remains live
/// until both phases have arrived exactly once.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct XPresentFeedbackPhases {
    complete: bool,
    idle: bool,
}

impl XPresentFeedbackPhases {
    pub fn observe_complete(&mut self) -> bool {
        if self.complete {
            return false;
        }
        self.complete = true;
        true
    }

    pub fn observe_idle(&mut self) -> bool {
        if self.idle {
            return false;
        }
        self.idle = true;
        true
    }

    pub const fn finished(self) -> bool {
        self.complete && self.idle
    }
}

impl core::fmt::Display for XServerFrontendRouteError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LifecycleUnavailable => {
                write!(formatter, "private connection lifecycle unavailable")
            }
            Self::FocusClaimRefused { client, refusal } => write!(
                formatter,
                "private focus claim refused for client {}: {refusal:?}",
                client.raw()
            ),
            Self::OrderedItemUnresolved => write!(
                formatter,
                "X11 ordered input consumer holds an unresolved item"
            ),
            Self::OrderedRunnerEngaged => write!(
                formatter,
                "X11 ordered input consumer already drains this order"
            ),
            Self::RecoveryShutdownFailed { client } => write!(
                formatter,
                "X11 recovery could not shut down client {}",
                client.raw()
            ),
            Self::UnknownClient { client } => {
                write!(
                    formatter,
                    "X11 route targets unknown client {}",
                    client.raw()
                )
            }
            Self::UnknownSurface { surface } => write!(
                formatter,
                "X11 route targets unknown Sophia surface {}:{}",
                surface.index(),
                surface.generation()
            ),
            Self::UnknownPresentWindow { window } => write!(
                formatter,
                "X11 present names window {:#x} with no surface route",
                window.local.raw()
            ),
            Self::ClientQueueFull { client } => {
                write!(
                    formatter,
                    "X11 route queue is full for client {}",
                    client.raw()
                )
            }
            Self::DuplicatePresentation { transaction } => write!(
                formatter,
                "X11 Present transaction {} is already pending",
                transaction.raw()
            ),
            Self::ClientQueueDisconnected { client } => write!(
                formatter,
                "X11 route queue disconnected for client {}",
                client.raw()
            ),
            Self::ControlNotClaimable { client } => write!(
                formatter,
                "X11 control for client {} could not claim execution",
                client.raw()
            ),
            Self::DependentNotTracked { client, refusal } => write!(
                formatter,
                "X11 control could not track the effect it would queue on client {}: {refusal:?}",
                client.raw()
            ),
            Self::MetadataQueueFull => formatter.write_str("X11 reduced metadata queue is full"),
            Self::MetadataQueueDisconnected => {
                formatter.write_str("X11 reduced metadata queue disconnected")
            }
            Self::DuplicateClient { client } => {
                write!(
                    formatter,
                    "X11 route client {} is already registered",
                    client.raw()
                )
            }
            Self::ContinuationUnavailable { client } => {
                write!(
                    formatter,
                    "no retained place is available for X11 route client {}",
                    client.raw()
                )
            }
            Self::ForeignServiceOwner => {
                write!(
                    formatter,
                    "this X11 route service is kept by a different owner"
                )
            }
            Self::ClientNumberExcluded { client } => {
                write!(
                    formatter,
                    "X11 route client {} is still held by the connection that had it",
                    client.raw()
                )
            }
            Self::EvidenceCustodyUnavailable { client } => {
                write!(
                    formatter,
                    "no evidence custody is available for X11 route client {}",
                    client.raw()
                )
            }
            Self::DuplicateSurface { surface } => write!(
                formatter,
                "X11 surface route {}:{} is already registered",
                surface.index(),
                surface.generation()
            ),
            Self::XkbWorkerSaturated => write!(formatter, "X11 keyboard translation queue is full"),
            Self::XkbWorkerUnavailable => write!(
                formatter,
                "X11 keyboard translation did not answer within its deadline"
            ),
            Self::RegistryPoisoned => formatter.write_str("X11 route registry lock poisoned"),
        }
    }
}

impl std::error::Error for XServerFrontendRouteError {}

/// A source-resolved emission with its original admitted delivery. The
/// constructor accepts no replacement recipient, origin or incarnation.
/// Neither this capsule nor its emission can be copied into another owner.
#[cfg(unix)]
#[derive(Debug)]
#[allow(dead_code)] // The ordered writer/consumer integration supplies production calls.
pub(crate) struct XAuthorityOrderedDelivery {
    /// `None` for a capsule nobody admitted: a release the ledger made when
    /// its source departed has no request, and so no delivery identity. Its
    /// writer answers through the finalizer alone.
    delivery: Option<crate::XAuthorityInputDeliveryId>,
    emission: crate::x11_socket::PrivateOrderedEmission,
    /// How this delivery's writer answers it.
    ///
    /// CARRIED, NOT LOOKED UP, and origin-bound. The writer answers the
    /// admission these bytes came from, through the one authority that owns
    /// the answer -- a delivery id fetched again at publication time would
    /// find whatever admission holds that number by then, and writing into a
    /// cell directly would leave the ledger's own account saying something
    /// else.
    finalizer: Option<std::sync::Arc<crate::x11_socket::PrivateDeliveryFinalizer>>,
}

/// Exactly which connection a writer serves.
///
/// RETAINED AT WORKER ADMISSION, never resolved per delivery. A writer that
/// looked its own identity up while serving would be comparing each capsule
/// against whatever the registry says at that moment -- which is the same
/// answer a stale capsule would already have been admitted by, so the
/// comparison would establish nothing.
///
/// A newtype rather than the identity itself, so what "exactly this
/// connection" means is decided in one place. Widening it later changes
/// `retained` and `admits`, not the writer.
#[cfg(unix)]
#[derive(Clone, Debug)]
#[allow(dead_code)] // The per-connection loop is not attached yet.
pub(crate) struct XAuthorityServedConnection {
    endpoint: crate::x11_socket::PrivateEndpointIdentity,
}

#[cfg(unix)]
#[allow(dead_code)] // The per-connection loop is not attached yet.
impl XAuthorityServedConnection {
    /// The endpoint this writer was started for.
    ///
    /// Taken from the registration that created this writer's receiver, not
    /// from a capsule and not from a later lookup by client id. A writer whose
    /// expectation came from either would be checking a capsule against
    /// something the capsule itself, or a replacement registration, decided.
    pub(crate) fn retained(endpoint: crate::x11_socket::PrivateEndpointIdentity) -> Self {
        Self { endpoint }
    }

    pub(crate) fn endpoint(&self) -> &crate::x11_socket::PrivateEndpointIdentity {
        &self.endpoint
    }

    /// Whether this capsule was minted for exactly the endpoint served.
    ///
    /// Asked before any byte of it is encoded or written, because writing is
    /// the thing that cannot be taken back: a frame put on a wire for another
    /// endpoint has been read by the time anyone could notice.
    pub(crate) fn admits(&self, delivery: &XAuthorityOrderedDelivery) -> bool {
        self.endpoint.matches(delivery.endpoint())
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XAuthorityOrderedAssemblyRefusal {
    DeliveryMissing,
}

#[cfg(unix)]
#[allow(dead_code)] // The source-only assembly is consumed by the ordered consumer.
impl XAuthorityOrderedDelivery {
    #[allow(clippy::result_large_err)] // Return the exact owned emission without allocating on refusal.
    pub(crate) fn from_emission(
        emission: crate::x11_socket::PrivateOrderedEmission,
    ) -> Result<
        Self,
        (
            XAuthorityOrderedAssemblyRefusal,
            crate::x11_socket::PrivateOrderedEmission,
        ),
    > {
        let Some(delivery) = emission.delivery() else {
            return Err((XAuthorityOrderedAssemblyRefusal::DeliveryMissing, emission));
        };
        Ok(Self {
            delivery: Some(delivery),
            emission,
            finalizer: None,
        })
    }

    /// A capsule for an event nobody admitted.
    ///
    /// It names no delivery, because none was ever issued for it, and its
    /// writer answers through the finalizer it is given and nothing else.
    /// The emission is expected to carry no delivery either: one that did
    /// would be an admitted event assembled the wrong way.
    pub(crate) fn unadmitted(emission: crate::x11_socket::PrivateOrderedEmission) -> Self {
        debug_assert!(
            emission.delivery().is_none(),
            "an emission with a delivery is assembled through from_emission"
        );
        Self {
            delivery: None,
            emission,
            finalizer: None,
        }
    }

    /// Give this capsule the finalizer its writer will answer through.
    ///
    /// Bound to the debt's own admission, so the writer and the executor are
    /// answering one admission rather than two lookups of one number.
    pub(crate) fn carry_finalizer(
        &mut self,
        finalizer: std::sync::Arc<crate::x11_socket::PrivateDeliveryFinalizer>,
    ) {
        self.finalizer = Some(finalizer);
    }

    pub(crate) fn finalizer(
        &self,
    ) -> Option<&std::sync::Arc<crate::x11_socket::PrivateDeliveryFinalizer>> {
        self.finalizer.as_ref()
    }

    pub(crate) fn client(&self) -> XServerFrontendClientId {
        XServerFrontendClientId::from_raw(self.emission.connection().recipient)
    }
    /// The delivery this capsule was admitted as, if anyone admitted it.
    ///
    /// Production asks this, which does not presume. The controls that only
    /// ever assemble admitted capsules keep a `delivery()` that does, defined
    /// beside them rather than here.
    pub(crate) fn admitted_delivery(&self) -> Option<crate::XAuthorityInputDeliveryId> {
        self.delivery
    }
    pub(crate) fn incarnation(&self) -> Option<sophia_input_authority::HoldIncarnation> {
        self.emission.incarnation()
    }
    pub(crate) fn recipient(&self) -> sophia_input_authority::ConnectionIdentity {
        self.emission.connection()
    }
    /// Exactly which endpoint these bytes are owed to.
    ///
    /// Source-derived and carried: no constructor here accepts one, so a
    /// capsule cannot be given an identity by whoever is about to be checked
    /// against it.
    pub(crate) fn endpoint(&self) -> &crate::x11_socket::PrivateEndpointIdentity {
        self.emission.endpoint()
    }
    pub(crate) fn emission(&self) -> &crate::x11_socket::PrivateOrderedEmission {
        &self.emission
    }
}
