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
    DuplicateSurface {
        surface: SurfaceId,
    },
    RegistryPoisoned,
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

/// One ordered delivery: what it answers for, and who it was resolved for.
///
/// Crate-private, with private fields and no constructor outside the resolver
/// that builds it under the guards. A caller able to assemble one could assert
/// a resolved state it never resolved -- which is the whole of what makes this
/// immutable rather than merely copied.
///
/// Deliberately NOT Copy or Clone. It carries custody of a delivery and its
/// origin, and a type that duplicates itself lets two holders each believe
/// they are the one that owes an outcome for it.
///
/// What the writer must put on the wire is not described here. The guarded
/// resolver already decides every target, form, coordinate, depth and crossing,
/// and restating any of that in this file would be a second description of the
/// same thing -- one that could drift from the one that is actually resolved
/// and would be believed because it looked like a field. The resolved emission
/// arrives as an opaque payload from the resolver's own vocabulary and is held
/// here, not re-modelled.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct XAuthorityOrderedDelivery {
    client: XServerFrontendClientId,
    delivery: crate::XAuthorityInputDeliveryId,
    incarnation: sophia_input_authority::HoldIncarnation,
    recipient: sophia_input_authority::ConnectionIdentity,
}

#[allow(dead_code)]
impl XAuthorityOrderedDelivery {
    /// Assemble one from parts, WITHOUT establishing that the parts belong
    /// together.
    ///
    /// This is not the assembly path and must not become one. Being
    /// crate-private limits who may make the claim; it does not make the claim
    /// true. A caller handing over four identities is asserting that this
    /// delivery, this incarnation and this connection were resolved as one
    /// thing, and nothing here checks that -- which is exactly the assertion
    /// this type's privacy exists to prevent.
    ///
    /// It exists only until assembly consumes a resolved emission and derives
    /// these identities from it, including the original delivery the source
    /// press or release carried. Until then: nothing enqueues a capsule, so
    /// nothing built this way can reach a recipient, and that is the only
    /// reason this is survivable rather than a hole.
    pub(crate) fn from_parts_unchecked(
        client: XServerFrontendClientId,
        delivery: crate::XAuthorityInputDeliveryId,
        incarnation: sophia_input_authority::HoldIncarnation,
        recipient: sophia_input_authority::ConnectionIdentity,
    ) -> Self {
        Self {
            client,
            delivery,
            incarnation,
            recipient,
        }
    }

    pub(crate) fn client(&self) -> XServerFrontendClientId {
        self.client
    }
    /// The delivery this answers for, as it was admitted.
    pub(crate) fn delivery(&self) -> crate::XAuthorityInputDeliveryId {
        self.delivery
    }
    /// The whole minted identity a settlement is named against.
    pub(crate) fn incarnation(&self) -> sophia_input_authority::HoldIncarnation {
        self.incarnation
    }
    /// The connection this was resolved for, exactly.
    pub(crate) fn recipient(&self) -> sophia_input_authority::ConnectionIdentity {
        self.recipient
    }
}
