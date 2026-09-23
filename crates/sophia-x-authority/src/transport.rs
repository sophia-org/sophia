use std::collections::BTreeMap;
use std::os::fd::OwnedFd;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use sophia_protocol::{
    LayoutNodeKind, Rect, SurfaceConstraints, SurfaceId, SurfaceOutputReservations,
    SurfacePlacementPreference, SurfacePresentationIntent, SurfacePresentationIntentKind,
    SurfacePresentationRole, SurfaceTransaction, TransactionId,
};

use crate::{
    X11DispatchObservation, XAuthorityCpuBufferUpdate, XClientOutput, XDispatchResult,
    XServerFrontendClientId,
};

pub const X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY: usize = 256;
const X_AUTHORITY_BACKPRESSURE_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityProtocolErrorObservation {
    pub code: u8,
    pub sequence: u16,
    pub minor_code: u16,
    pub major_code: u8,
}

/// Privacy-preserving metadata change evidence. Protocol object IDs and value
/// bytes remain inside the X frontend; the session sees only the established
/// property name and bounded byte length.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityMetadataObservation {
    pub property_name: String,
    pub byte_len: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthoritySurfacePresentationObservation {
    pub surface: SurfaceId,
    pub role: SurfacePresentationRole,
    pub kind: LayoutNodeKind,
    pub placement_preference: SurfacePlacementPreference,
    pub owner: Option<SurfaceId>,
    pub stack_rank: u32,
    pub mapped: bool,
    pub geometry: Rect,
    pub constraints: SurfaceConstraints,
    pub generation: u64,
}

fn is_expected_client_probe_error(error: &crate::XClientError) -> bool {
    let window_geometry_probe = error.code == crate::XErrorCode::BadWindow
        && error.resource_id == 0
        && error.minor_code == 0
        && matches!(error.major_code, 3 | 14);
    let missing_randr_property_probe = error.code == crate::XErrorCode::BadAtom
        && error.resource_id == crate::X_ATOM_NONE
        && error.minor_code == u16::from(crate::X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE)
        && error.major_code == crate::X_RANDR_MAJOR_OPCODE;
    window_geometry_probe || missing_randr_property_probe
}

fn reduce_protocol_errors(
    outputs: &[XClientOutput],
    expected: bool,
) -> Vec<XAuthorityProtocolErrorObservation> {
    outputs
        .iter()
        .filter_map(|output| match output {
            XClientOutput::Error(error) if is_expected_client_probe_error(error) == expected => {
                Some(XAuthorityProtocolErrorObservation {
                    code: error.code.wire_code(),
                    sequence: error.sequence,
                    minor_code: error.minor_code,
                    major_code: error.major_code,
                })
            }
            _ => None,
        })
        // One dispatch carries one request, and a request cannot fail sixteen
        // ways, so this bound is structural rather than lossy: there is no
        // discarded count because nothing can reach it.
        .take(16)
        .collect()
}

/// Names the resource an unexpected protocol error refused, for an operator who
/// asked for it.
///
/// The observation deliberately carries no resource id: it is interpolated into
/// a default-level, archived evidence file, and a raw XID may not appear there.
/// Diagnosing which window a `BadWindow` names still needs the id, so it stays
/// here, behind a non-default level, where the frontend already owns it.
fn trace_protocol_error_resources(outputs: &[XClientOutput]) {
    for output in outputs {
        let XClientOutput::Error(error) = output else {
            continue;
        };
        if is_expected_client_probe_error(error) {
            continue;
        }
        tracing::debug!(
            code = error.code.wire_code(),
            major_code = error.major_code,
            minor_code = error.minor_code,
            resource_id = error.resource_id,
            sequence = error.sequence,
            "x11 protocol error refused a resource"
        );
    }
}

#[derive(Clone, Debug)]
pub struct XAuthorityDmaBufRegistration {
    pub pixmap: crate::XResourceId,
    pub descriptor: sophia_protocol::DmaBufDescriptor,
    pub plane_fds: Vec<Arc<OwnedFd>>,
}

impl PartialEq for XAuthorityDmaBufRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.pixmap == other.pixmap
            && self.descriptor == other.descriptor
            && self.plane_fds.len() == other.plane_fds.len()
    }
}

impl Eq for XAuthorityDmaBufRegistration {}

#[derive(Clone, Debug)]
pub struct XAuthorityFenceRegistration {
    pub fence: crate::XResourceId,
    pub handle: sophia_protocol::FenceHandle,
    pub initially_triggered: bool,
    pub fd: Arc<OwnedFd>,
}

impl PartialEq for XAuthorityFenceRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.fence == other.fence
            && self.handle == other.handle
            && self.initially_triggered == other.initially_triggered
    }
}

impl Eq for XAuthorityFenceRegistration {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XAuthorityObservedTransactionBatch {
    /// The frontend connection that caused this batch, when the source is an
    /// X11 socket dispatch. Direct authority dispatches have no connection and
    /// therefore retain `None`.
    pub client: Option<XServerFrontendClientId>,
    /// Session-issued admission facts for the causing frontend connection.
    /// Engine may compare these opaque identities but receives no X resource ID.
    pub admission: Option<sophia_protocol::ClientAdmissionContext>,
    /// Frontend-owned routes for live surfaces named by this observation.
    /// These remain stable when another classic-shared client causes the
    /// transaction.
    pub surface_routes: Vec<crate::XAuthoritySurfaceRouteObservation>,
    pub transaction: TransactionId,
    pub transactions: Vec<SurfaceTransaction>,
    /// Protocol-neutral presentation facts reduced from authority-private
    /// window attributes. Raw X11 object IDs remain inside the frontend.
    pub surface_presentations: Vec<XAuthoritySurfacePresentationObservation>,
    /// Lifecycle edges for policy admission. These are distinct from the
    /// current-state snapshots above so duplicate observations cannot be
    /// mistaken for new presentation requests.
    pub presentation_intents: Vec<SurfacePresentationIntent>,
    /// Frontend-confirmed surface lifetimes that ended in this batch.
    pub removed_surfaces: Vec<SurfaceId>,
    /// Complete output-reservation snapshots for surfaces changed by this
    /// batch. Empty reservations explicitly clear a previous snapshot.
    pub surface_output_reservations: Vec<SurfaceOutputReservations>,
    pub cpu_buffer_updates: Vec<XAuthorityCpuBufferUpdate>,
    /// Exact identities for authority-generated raster responses in this
    /// batch. Ordinary client request batches leave this empty.
    pub raster_responses: Vec<sophia_protocol::SurfaceRasterResponseIdentity>,
    pub dma_buf_registrations: Vec<XAuthorityDmaBufRegistration>,
    pub fence_registrations: Vec<XAuthorityFenceRegistration>,
    pub present_submissions: Vec<crate::XAuthorityPresentSubmission>,
    pub software_present_submissions: Vec<crate::XAuthoritySoftwarePresentSubmission>,
    pub released_dma_bufs: Vec<sophia_protocol::BufferHandle>,
    pub released_fences: Vec<sophia_protocol::FenceHandle>,
    /// Reduced protocol errors. Resource IDs and request payloads deliberately
    /// remain inside the X frontend boundary.
    pub protocol_errors: Vec<XAuthorityProtocolErrorObservation>,
    pub expected_protocol_errors: Vec<XAuthorityProtocolErrorObservation>,
    pub metadata: Vec<XAuthorityMetadataObservation>,
    /// Privacy-preserving core selection operation witnesses.
    pub selection_owner_change: bool,
    pub selection_conversion: bool,
}

impl XAuthorityObservedTransactionBatch {
    pub fn from_dispatch_result(result: &XDispatchResult) -> Option<Self> {
        let response = result.response.as_ref()?;
        if response.transactions.is_empty()
            && response.surfaces.is_empty()
            && response.removed_surfaces.is_empty()
        {
            return None;
        }

        Some(Self {
            client: None,
            admission: None,
            surface_routes: Vec::new(),
            transaction: response.transaction,
            transactions: response.transactions.clone(),
            surface_presentations: response
                .surfaces
                .iter()
                .map(|surface| XAuthoritySurfacePresentationObservation {
                    surface: surface.surface,
                    role: surface.presentation,
                    kind: surface.kind,
                    placement_preference: surface.placement_preference,
                    owner: surface.presentation_owner,
                    stack_rank: surface.stack_rank,
                    mapped: surface.mapped,
                    geometry: surface.geometry,
                    constraints: surface.constraints,
                    generation: surface.generation,
                })
                .collect(),
            presentation_intents: Vec::new(),
            removed_surfaces: response.removed_surfaces.clone(),
            surface_output_reservations: Vec::new(),
            cpu_buffer_updates: Vec::new(),
            raster_responses: Vec::new(),
            dma_buf_registrations: Vec::new(),
            fence_registrations: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
            released_dma_bufs: Vec::new(),
            released_fences: Vec::new(),
            protocol_errors: Vec::new(),
            expected_protocol_errors: Vec::new(),
            metadata: Vec::new(),
            selection_owner_change: false,
            selection_conversion: false,
        })
    }

    pub fn from_dispatch_observation(trace: &X11DispatchObservation) -> Option<Self> {
        // A departed client's request resolves its ticket with no facts, like an
        // aborted one: nothing was assembled and nobody is owed it.
        if matches!(
            trace.failure,
            Some(
                crate::X11ObservedDispatchFailure::DispatchAborted
                    | crate::X11ObservedDispatchFailure::UnpublishedEffects
                    | crate::X11ObservedDispatchFailure::ClientDeparted
            )
        ) {
            return None;
        }
        let dma_buf_registrations = trace
            .dri3_pixmap_import
            .and_then(|import| {
                let plane_fds = trace
                    .received_fds
                    .iter()
                    .map(|fd| fd.try_clone().map(Arc::new))
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                Some(XAuthorityDmaBufRegistration {
                    pixmap: import.pixmap,
                    descriptor: import.descriptor,
                    plane_fds,
                })
            })
            .into_iter()
            .collect::<Vec<_>>();
        let fence_registrations = trace
            .dri3_fence_import
            .and_then(|import| {
                Some(XAuthorityFenceRegistration {
                    fence: import.fence,
                    handle: import.handle,
                    initially_triggered: import.initially_triggered,
                    fd: Arc::new(trace.received_fds.first()?.try_clone().ok()?),
                })
            })
            .into_iter()
            .collect::<Vec<_>>();
        let response = trace.result.response.as_ref();
        trace_protocol_error_resources(&trace.result.outputs);
        let protocol_errors = reduce_protocol_errors(&trace.result.outputs, false);
        let expected_protocol_errors = reduce_protocol_errors(&trace.result.outputs, true);
        let metadata = trace
            .result
            .metadata_candidates
            .iter()
            .take(16)
            .map(|candidate| XAuthorityMetadataObservation {
                property_name: candidate.property_name.clone(),
                byte_len: candidate.byte_len,
            })
            .collect::<Vec<_>>();
        let selection_owner_change = trace.major_opcode == 22;
        let selection_conversion = trace.major_opcode == 24;
        if response.is_none()
            && dma_buf_registrations.is_empty()
            && fence_registrations.is_empty()
            && trace.present_submission.is_none()
            && trace.software_present_submission.is_none()
            && trace.released_dma_bufs.is_empty()
            && trace.released_fences.is_empty()
            && trace.surface_output_reservations.is_empty()
            && protocol_errors.is_empty()
            && expected_protocol_errors.is_empty()
            && metadata.is_empty()
            && !selection_owner_change
            && !selection_conversion
        {
            return None;
        }
        let transactions = response
            .map(|response| response.transactions.clone())
            .unwrap_or_default();
        let surface_presentations = response
            .map(|response| {
                response
                    .surfaces
                    .iter()
                    .map(|surface| XAuthoritySurfacePresentationObservation {
                        surface: surface.surface,
                        role: surface.presentation,
                        kind: surface.kind,
                        placement_preference: surface.placement_preference,
                        owner: surface.presentation_owner,
                        stack_rank: surface.stack_rank,
                        mapped: surface.mapped,
                        geometry: surface.geometry,
                        constraints: surface.constraints,
                        generation: surface.generation,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let removed_surfaces = response
            .map(|response| response.removed_surfaces.clone())
            .unwrap_or_default();
        let presentation_intents = surface_presentations
            .iter()
            .filter_map(|surface| {
                let kind = match trace.major_opcode {
                    7 if surface.role == SurfacePresentationRole::ClientPositioned => {
                        SurfacePresentationIntentKind::Withdraw
                    }
                    8 | 9
                        if surface.role == SurfacePresentationRole::PolicyManaged
                            && !surface.mapped =>
                    {
                        SurfacePresentationIntentKind::Request
                    }
                    10 if surface.role == SurfacePresentationRole::PolicyManaged => {
                        SurfacePresentationIntentKind::Withdraw
                    }
                    _ => return None,
                };
                Some(SurfacePresentationIntent {
                    surface: surface.surface,
                    kind,
                    role: surface.role,
                    surface_kind: surface.kind,
                    placement_preference: surface.placement_preference,
                    presentation_owner: surface.owner,
                    stack_rank: surface.stack_rank,
                    geometry: surface.geometry,
                    constraints: surface.constraints,
                    generation: surface.generation,
                })
            })
            .collect::<Vec<_>>();
        // Mapping establishes input ownership before the client draws. Publish
        // that surface's owner route with the mapped fact; waiting for pixels
        // deadlocks clients that wait for GrabSuccess before their first draw.
        // Unmapped passive helpers still acquire no route merely by existing.
        let routed_surfaces = transactions
            .iter()
            .map(|transaction| transaction.surface)
            .chain(presentation_intents.iter().map(|intent| intent.surface))
            .chain(
                surface_presentations
                    .iter()
                    .filter(|surface| surface.mapped)
                    .map(|surface| surface.surface),
            )
            .collect::<std::collections::BTreeSet<_>>();
        let surface_routes = trace
            .surface_routes
            .iter()
            .filter(|route| routed_surfaces.contains(&route.surface))
            .copied()
            .collect::<Vec<_>>();
        if transactions.is_empty()
            && surface_presentations.is_empty()
            && presentation_intents.is_empty()
            && removed_surfaces.is_empty()
            && dma_buf_registrations.is_empty()
            && fence_registrations.is_empty()
            && trace.present_submission.is_none()
            && trace.software_present_submission.is_none()
            && trace.released_dma_bufs.is_empty()
            && trace.released_fences.is_empty()
            && trace.surface_output_reservations.is_empty()
            && protocol_errors.is_empty()
            && expected_protocol_errors.is_empty()
            && metadata.is_empty()
            && !selection_owner_change
            && !selection_conversion
        {
            return None;
        }
        Some(Self {
            client: Some(trace.client),
            admission: trace.admission,
            surface_routes,
            transaction: trace.transaction,
            transactions,
            surface_presentations,
            presentation_intents,
            removed_surfaces,
            surface_output_reservations: trace.surface_output_reservations.clone(),
            cpu_buffer_updates: trace.cpu_buffer_updates.clone(),
            raster_responses: Vec::new(),
            dma_buf_registrations,
            fence_registrations,
            present_submissions: trace.present_submission.into_iter().collect(),
            software_present_submissions: trace.software_present_submission.into_iter().collect(),
            released_dma_bufs: trace.released_dma_bufs.to_vec(),
            released_fences: trace.released_fences.to_vec(),
            protocol_errors,
            expected_protocol_errors,
            metadata,
            selection_owner_change,
            selection_conversion,
        })
    }

    pub fn from_raster_response(response: crate::XAuthorityRasterRequirementResponse) -> Self {
        Self {
            client: None,
            admission: None,
            surface_routes: Vec::new(),
            transaction: response.identity.transaction,
            transactions: vec![response.transaction],
            surface_presentations: Vec::new(),
            presentation_intents: Vec::new(),
            removed_surfaces: Vec::new(),
            surface_output_reservations: Vec::new(),
            cpu_buffer_updates: response.cpu_buffer_updates,
            raster_responses: vec![response.identity],
            dma_buf_registrations: Vec::new(),
            fence_registrations: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
            released_dma_bufs: Vec::new(),
            released_fences: Vec::new(),
            protocol_errors: Vec::new(),
            expected_protocol_errors: Vec::new(),
            metadata: Vec::new(),
            selection_owner_change: false,
            selection_conversion: false,
        }
    }
}

/// Maps Engine-visible X11 surfaces back to the frontend client that created
/// or last updated them.
///
/// The Engine owns focus and hit testing; this table gives it the connection
/// identity required to turn that surface decision into an X11 input or
/// control route. A direct authority batch has no client identity and cannot
/// establish a route. Surface removals always clear a prior route.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XAuthorityClientSurfaceRoutes {
    clients: BTreeMap<
        SurfaceId,
        (
            XServerFrontendClientId,
            Option<sophia_protocol::ClientAdmissionContext>,
        ),
    >,
    retired: std::collections::BTreeSet<SurfaceId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityClientSurfaceRouteError {
    ConflictingObservation { surface: SurfaceId },
}

impl core::fmt::Display for XAuthorityClientSurfaceRouteError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ConflictingObservation { surface } => write!(
                formatter,
                "surface {:?} changed its frontend owner route without retirement",
                surface
            ),
        }
    }
}

impl std::error::Error for XAuthorityClientSurfaceRouteError {}

impl XAuthorityClientSurfaceRoutes {
    pub fn observe(
        &mut self,
        batch: &XAuthorityObservedTransactionBatch,
    ) -> Result<(), XAuthorityClientSurfaceRouteError> {
        let removed = batch
            .removed_surfaces
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mut updates = BTreeMap::new();
        for route in &batch.surface_routes {
            if removed.contains(&route.surface) || self.retired.contains(&route.surface) {
                continue;
            }
            let observed = (route.client, route.admission);
            if updates
                .insert(route.surface, observed)
                .is_some_and(|previous| previous != observed)
                || self
                    .clients
                    .get(&route.surface)
                    .filter(|_| !removed.contains(&route.surface))
                    .is_some_and(|current| *current != observed)
            {
                return Err(XAuthorityClientSurfaceRouteError::ConflictingObservation {
                    surface: route.surface,
                });
            }
        }
        for surface in &batch.removed_surfaces {
            self.clients.remove(surface);
            self.retired.insert(*surface);
        }
        for (surface, route) in updates {
            self.clients.insert(surface, route);
        }
        Ok(())
    }

    pub fn client_for_surface(&self, surface: SurfaceId) -> Option<XServerFrontendClientId> {
        self.clients.get(&surface).map(|(client, _)| *client)
    }

    pub fn surfaces_for_client(&self, client: XServerFrontendClientId) -> Vec<SurfaceId> {
        self.clients
            .iter()
            .filter_map(|(surface, (owner, _))| (*owner == client).then_some(*surface))
            .collect()
    }

    pub fn admission_for_surface(
        &self,
        surface: SurfaceId,
    ) -> Option<sophia_protocol::ClientAdmissionContext> {
        self.clients
            .get(&surface)
            .and_then(|(_, admission)| *admission)
    }

    pub fn surfaces_for_admission(
        &self,
        admission: sophia_protocol::ClientAdmissionContext,
    ) -> Vec<SurfaceId> {
        self.clients
            .iter()
            .filter_map(|(surface, (_, candidate))| {
                (*candidate == Some(admission)).then_some(*surface)
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XAuthorityTransportError {
    Backpressure { transaction: TransactionId },
    Cancelled { transaction: TransactionId },
    Disconnected { transaction: TransactionId },
}

impl core::fmt::Display for XAuthorityTransportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Backpressure { transaction } => write!(
                formatter,
                "X authority observed transaction channel is full for transaction {}",
                transaction.raw()
            ),
            Self::Cancelled { transaction } => write!(
                formatter,
                "X authority observation wait was cancelled for transaction {}",
                transaction.raw()
            ),
            Self::Disconnected { transaction } => write!(
                formatter,
                "X authority observed transaction channel is disconnected for transaction {}",
                transaction.raw()
            ),
        }
    }
}

impl std::error::Error for XAuthorityTransportError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityBackpressureTelemetryKind {
    Wait,
    Resume,
    Shutdown,
    TransportFailure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XAuthorityBackpressureFailure {
    Cancelled,
    Disconnected,
}

/// Value-free flow-control evidence for the Engine observation boundary.
///
/// A full bounded channel retains its batch while the worker waits. Telemetry
/// identifies only the client and logical transaction; protocol payloads stay
/// inside the authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XAuthorityBackpressureTelemetry {
    pub kind: XAuthorityBackpressureTelemetryKind,
    pub client: Option<XServerFrontendClientId>,
    pub transaction: TransactionId,
    pub waited: Duration,
    pub failure: Option<XAuthorityBackpressureFailure>,
}

/// Emits one observation without dropping it when the bounded Engine channel
/// is temporarily full.
///
/// The caller owns cancellation. A routed service sets it when supervision
/// explicitly disconnects clients; ordinary StopAccepting continues to drain
/// already accepted clients and their observations. The fail-fast
/// [`try_emit_x_authority_observation`] API remains available to probes and
/// callers that cannot wait.
pub fn emit_x_authority_observation_with_backpressure(
    sender: &SyncSender<XAuthorityObservedTransactionBatch>,
    trace: &X11DispatchObservation,
    cancellation: &AtomicBool,
    mut telemetry: impl FnMut(XAuthorityBackpressureTelemetry),
) -> Result<(), XAuthorityTransportError> {
    let Some(batch) = XAuthorityObservedTransactionBatch::from_dispatch_observation(trace) else {
        return Ok(());
    };
    emit_x_authority_batch_with_backpressure(sender, batch, cancellation, &mut telemetry)
}

fn emit_x_authority_batch_with_backpressure(
    sender: &SyncSender<XAuthorityObservedTransactionBatch>,
    mut batch: XAuthorityObservedTransactionBatch,
    cancellation: &AtomicBool,
    telemetry: &mut impl FnMut(XAuthorityBackpressureTelemetry),
) -> Result<(), XAuthorityTransportError> {
    let transaction = batch.transaction;
    let client = batch.client;
    match sender.try_send(batch) {
        Ok(()) => return Ok(()),
        Err(TrySendError::Disconnected(_)) => {
            telemetry(XAuthorityBackpressureTelemetry {
                kind: XAuthorityBackpressureTelemetryKind::TransportFailure,
                client,
                transaction,
                waited: Duration::ZERO,
                failure: Some(XAuthorityBackpressureFailure::Disconnected),
            });
            return Err(XAuthorityTransportError::Disconnected { transaction });
        }
        Err(TrySendError::Full(pending)) => batch = pending,
    }

    let started = Instant::now();
    telemetry(XAuthorityBackpressureTelemetry {
        kind: XAuthorityBackpressureTelemetryKind::Wait,
        client,
        transaction,
        waited: Duration::ZERO,
        failure: None,
    });
    loop {
        if cancellation.load(Ordering::Acquire) {
            telemetry(XAuthorityBackpressureTelemetry {
                kind: XAuthorityBackpressureTelemetryKind::Shutdown,
                client,
                transaction,
                waited: started.elapsed(),
                failure: Some(XAuthorityBackpressureFailure::Cancelled),
            });
            return Err(XAuthorityTransportError::Cancelled { transaction });
        }
        std::thread::sleep(X_AUTHORITY_BACKPRESSURE_RETRY_INTERVAL);
        match sender.try_send(batch) {
            Ok(()) => {
                telemetry(XAuthorityBackpressureTelemetry {
                    kind: XAuthorityBackpressureTelemetryKind::Resume,
                    client,
                    transaction,
                    waited: started.elapsed(),
                    failure: None,
                });
                return Ok(());
            }
            Err(TrySendError::Full(pending)) => batch = pending,
            Err(TrySendError::Disconnected(_)) => {
                telemetry(XAuthorityBackpressureTelemetry {
                    kind: XAuthorityBackpressureTelemetryKind::TransportFailure,
                    client,
                    transaction,
                    waited: started.elapsed(),
                    failure: Some(XAuthorityBackpressureFailure::Disconnected),
                });
                return Err(XAuthorityTransportError::Disconnected { transaction });
            }
        }
    }
}

pub fn try_emit_x_authority_transactions(
    sender: &SyncSender<XAuthorityObservedTransactionBatch>,
    result: &XDispatchResult,
) -> Result<Option<XAuthorityObservedTransactionBatch>, XAuthorityTransportError> {
    let Some(batch) = XAuthorityObservedTransactionBatch::from_dispatch_result(result) else {
        return Ok(None);
    };

    sender
        .try_send(batch.clone())
        .map_err(|error| match error {
            TrySendError::Full(batch) => XAuthorityTransportError::Backpressure {
                transaction: batch.transaction,
            },
            TrySendError::Disconnected(batch) => XAuthorityTransportError::Disconnected {
                transaction: batch.transaction,
            },
        })?;

    Ok(Some(batch))
}

pub fn try_emit_x_authority_observation(
    sender: &SyncSender<XAuthorityObservedTransactionBatch>,
    trace: &X11DispatchObservation,
) -> Result<Option<XAuthorityObservedTransactionBatch>, XAuthorityTransportError> {
    let Some(batch) = XAuthorityObservedTransactionBatch::from_dispatch_observation(trace) else {
        return Ok(None);
    };

    sender
        .try_send(batch.clone())
        .map_err(|error| match error {
            TrySendError::Full(batch) => XAuthorityTransportError::Backpressure {
                transaction: batch.transaction,
            },
            TrySendError::Disconnected(batch) => XAuthorityTransportError::Disconnected {
                transaction: batch.transaction,
            },
        })?;

    Ok(Some(batch))
}
