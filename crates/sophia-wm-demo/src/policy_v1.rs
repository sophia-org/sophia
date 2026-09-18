use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    IpcCodecError, OutputId, POLICY_MAX_OUTPUTS, PolicyConfiguration, PolicyOutputProjection,
    PolicyProjectionOutcome, PolicyProjectionProposal, PolicyProjectionRequest,
    PolicySceneSnapshot, PolicySessionOperationOutcome, PolicySessionOperationRequest,
    PolicySurfacePlacement, PolicyTransform, Rect, SOPHIA_IPC_HEADER_LEN,
    SOPHIA_IPC_MAX_PAYLOAD_LEN, SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_BINDINGS,
    SOPHIA_WM_CAPABILITY_CONFIGURATION, SOPHIA_WM_CAPABILITY_MULTI_OUTPUT,
    SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION, SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
    SOPHIA_WM_MAX_BINDINGS, SOPHIA_WM_MAX_OUTPUTS, SOPHIA_WM_MAX_SURFACES,
    SOPHIA_WM_OUTCOME_COMMITTED, Size, SurfaceId, TransactionId, WmChromePolicy, WmV1ClientHello,
    WmV1DecodedSnapshot, WmV1ProfileCompletion, WmV1ProfileOutcome, WmV1SnapshotTransfer,
    decode_wm_v1_policy_configuration_outcome_frame, decode_wm_v1_policy_projection_outcome,
    decode_wm_v1_policy_projection_request, decode_wm_v1_policy_session_operation_outcome,
    decode_wm_v1_policy_snapshot, decode_wm_v1_profile_activate, decode_wm_v1_profile_prepare,
    decode_wm_v1_projection_outcome_frame, decode_wm_v1_projection_request_frame,
    decode_wm_v1_server_welcome_frame, decode_wm_v1_session_operation_outcome_frame,
    decode_wm_v1_snapshot_begin_frame, decode_wm_v1_snapshot_chunk_frame,
    decode_wm_v1_snapshot_end_frame, encode_wm_v1_client_hello_frame,
    encode_wm_v1_policy_configuration, encode_wm_v1_policy_configuration_frame,
    encode_wm_v1_policy_projection, encode_wm_v1_policy_session_operation_request,
    encode_wm_v1_profile_active, encode_wm_v1_profile_prepared,
    encode_wm_v1_projection_begin_frame, encode_wm_v1_projection_chunk_frame,
    encode_wm_v1_projection_end_frame, encode_wm_v1_session_operation_request_frame,
};

#[derive(Debug)]
pub enum PolicyV1ClientError {
    Io(std::io::Error),
    Codec(IpcCodecError),
    UnsupportedRevision(u16),
    InvalidWelcome,
    MissingCapability,
    ConnectionEpochMismatch,
    SceneGenerationMismatch,
    InvalidScene,
    ProfileActivationUnavailable,
    ProfileIdentityMismatch,
    ConfigurationRejected,
    TransactionExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatelessReferenceProjectionDecision {
    Settled,
    RetryFreshSnapshot,
    Fatal,
}

/// Whether this error is the socket read deadline expiring rather than a
/// failed or closed connection.
///
/// A client waiting on a second asynchronous result must distinguish the two:
/// the owner legitimately issues no cycle while a topology transaction is
/// preparing, and treating that quiet window as a fault kills the process in
/// the middle of the apply it was waiting for.
pub fn policy_client_read_timed_out(error: &PolicyV1ClientError) -> bool {
    matches!(
        error,
        PolicyV1ClientError::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            )
    )
}

/// The bundled reference policy has no state beyond one received cycle, so a
/// stale outcome can safely return to snapshot intake on the same connection.
/// Stateful policy peers must instead discard their speculative process state.
pub const fn stateless_reference_projection_decision(
    outcome: PolicyProjectionOutcome,
) -> StatelessReferenceProjectionDecision {
    match outcome {
        PolicyProjectionOutcome::Committed => StatelessReferenceProjectionDecision::Settled,
        // A stale generation and a timeout are both "the scene moved on",
        // differing only in what moved it. A timeout means some client was slow
        // to adopt its new geometry; the session preserves the committed layout
        // and rolls the proposal back, so re-proposing from a fresh snapshot is
        // the whole recovery. Treating it as fatal makes a slow client kill the
        // window manager, and a window manager that exits repeatedly for a
        // recoverable reason exhausts its restart budget and takes the session
        // with it.
        PolicyProjectionOutcome::RejectedStale | PolicyProjectionOutcome::TimedOut => {
            StatelessReferenceProjectionDecision::RetryFreshSnapshot
        }
        PolicyProjectionOutcome::RejectedInvalid | PolicyProjectionOutcome::Disconnected => {
            StatelessReferenceProjectionDecision::Fatal
        }
    }
}

impl core::fmt::Display for PolicyV1ClientError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyV1ClientError {}

impl From<std::io::Error> for PolicyV1ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<IpcCodecError> for PolicyV1ClientError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

/// Public revision-3 reference client shared by the conformance host and
/// native Sophia policy clients.
pub struct PolicyV1Client {
    stream: UnixStream,
    connection_epoch: u64,
    capabilities: u64,
    max_outputs: usize,
    max_surfaces: usize,
    max_bindings: usize,
    next_transaction: u64,
}

impl PolicyV1Client {
    pub fn connect(path: impl AsRef<Path>, timeout: Duration) -> Result<Self, PolicyV1ClientError> {
        let mut stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let hello = WmV1ClientHello {
            minimum_revision: 3,
            maximum_revision: 3,
            capabilities: SOPHIA_WM_CAPABILITY_BINDINGS
                | SOPHIA_WM_CAPABILITY_ACTIONS
                | SOPHIA_WM_CAPABILITY_MULTI_OUTPUT
                | SOPHIA_WM_CAPABILITY_CONFIGURATION
                | SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION
                | SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
        };
        stream.write_all(&encode_wm_v1_client_hello_frame(&hello)?)?;
        stream.flush()?;
        let welcome = decode_wm_v1_server_welcome_frame(&read_frame(&mut stream)?)?;
        if welcome.selected_revision != 3 {
            return Err(PolicyV1ClientError::UnsupportedRevision(
                welcome.selected_revision,
            ));
        }
        if welcome.connection_epoch == 0 {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        let max_outputs = usize::from(welcome.max_outputs);
        let max_surfaces = welcome.max_surfaces as usize;
        let max_bindings = usize::from(welcome.max_bindings);
        if max_outputs == 0
            || max_outputs > SOPHIA_WM_MAX_OUTPUTS
            || max_surfaces == 0
            || max_surfaces > SOPHIA_WM_MAX_SURFACES
            || max_bindings > SOPHIA_WM_MAX_BINDINGS
            || welcome.max_chunk_bytes == 0
            || welcome.max_chunk_bytes as usize > SOPHIA_IPC_MAX_PAYLOAD_LEN
        {
            return Err(PolicyV1ClientError::InvalidWelcome);
        }
        Ok(Self {
            stream,
            connection_epoch: welcome.connection_epoch,
            capabilities: welcome.capabilities,
            max_outputs,
            max_surfaces,
            max_bindings,
            // Profile activation uses server-owned transactions 1 and 2 in the
            // live host. Client transactions start after them so evidence never
            // relies on direction-local reuse being harmless.
            next_transaction: 3,
        })
    }

    /// Accepts the exact startup profile and installs an empty, valid policy
    /// configuration before the host publishes its first scene.
    pub fn activate_profile_and_configure(&mut self) -> Result<(), PolicyV1ClientError> {
        self.activate_profile_and_configure_with(Vec::new(), WmChromePolicy::default())
    }

    /// Accepts the exact startup profile and installs the caller's bounded
    /// action catalog before the host publishes its first scene.
    pub fn activate_profile_and_configure_with(
        &mut self,
        actions: Vec<sophia_protocol::PolicyActionRegistration>,
        chrome: WmChromePolicy,
    ) -> Result<(), PolicyV1ClientError> {
        if self.capabilities & SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION == 0
            || self.capabilities & SOPHIA_WM_CAPABILITY_CONFIGURATION == 0
            || actions
                .iter()
                .any(|action| action.session_operation_slot.is_some())
                && self.capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS == 0
        {
            return Err(PolicyV1ClientError::ProfileActivationUnavailable);
        }
        let prepare = decode_wm_v1_profile_prepare(&read_frame(&mut self.stream)?)?;
        if prepare.identity.connection_epoch != self.connection_epoch {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        self.stream
            .write_all(&encode_wm_v1_profile_prepared(WmV1ProfileCompletion {
                transaction: prepare.transaction,
                identity: prepare.identity,
                outcome: WmV1ProfileOutcome::Accepted,
            })?)?;
        self.stream.flush()?;

        let activate = decode_wm_v1_profile_activate(&read_frame(&mut self.stream)?)?;
        if activate.identity != prepare.identity {
            return Err(PolicyV1ClientError::ProfileIdentityMismatch);
        }
        self.stream
            .write_all(&encode_wm_v1_profile_active(WmV1ProfileCompletion {
                transaction: activate.transaction,
                identity: activate.identity,
                outcome: WmV1ProfileOutcome::Accepted,
            })?)?;
        self.stream.flush()?;

        let transaction = self.mint_transaction()?;
        let configuration = encode_wm_v1_policy_configuration(&PolicyConfiguration {
            connection_epoch: self.connection_epoch,
            generation: 1,
            actions,
            chrome,
        })?;
        self.stream
            .write_all(&encode_wm_v1_policy_configuration_frame(
                transaction,
                &configuration,
            )?)?;
        self.stream.flush()?;
        let (outcome_transaction, outcome) =
            decode_wm_v1_policy_configuration_outcome_frame(&read_frame(&mut self.stream)?)?;
        if outcome_transaction != transaction
            || outcome.connection_epoch != self.connection_epoch
            || outcome.configuration_generation != 1
        {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        if outcome.outcome != SOPHIA_WM_OUTCOME_COMMITTED {
            return Err(PolicyV1ClientError::ConfigurationRejected);
        }
        Ok(())
    }

    /// Requests one operation advertised in the scene snapshot and waits for
    /// the owner's terminal outcome.
    pub fn request_session_operation(
        &mut self,
        operation: u64,
        target: Option<SurfaceId>,
    ) -> Result<PolicySessionOperationOutcome, PolicyV1ClientError> {
        if self.capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS == 0 {
            return Err(PolicyV1ClientError::MissingCapability);
        }
        let transaction = self.mint_transaction()?;
        let request = PolicySessionOperationRequest {
            connection_epoch: self.connection_epoch,
            request_id: transaction.raw(),
            operation,
            target,
        };
        let wire = encode_wm_v1_policy_session_operation_request(request)?;
        self.stream
            .write_all(&encode_wm_v1_session_operation_request_frame(
                transaction,
                &wire,
            )?)?;
        self.stream.flush()?;
        let (outcome_transaction, wire) =
            decode_wm_v1_session_operation_outcome_frame(&read_frame(&mut self.stream)?)?;
        let outcome = decode_wm_v1_policy_session_operation_outcome(&wire)?;
        if outcome_transaction != transaction
            || outcome.connection_epoch != self.connection_epoch
            || outcome.request_id != request.request_id
        {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        Ok(outcome)
    }

    pub fn receive_snapshot(&mut self) -> Result<WmV1DecodedSnapshot, PolicyV1ClientError> {
        let (transaction, begin) =
            decode_wm_v1_snapshot_begin_frame(&read_frame(&mut self.stream)?)?;
        if begin.connection_epoch != self.connection_epoch {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        let mut chunks = Vec::with_capacity(usize::from(begin.chunk_count));
        for _ in 0..begin.chunk_count {
            let (chunk_transaction, chunk) =
                decode_wm_v1_snapshot_chunk_frame(&read_frame(&mut self.stream)?)?;
            if chunk_transaction != transaction {
                return Err(PolicyV1ClientError::ConnectionEpochMismatch);
            }
            chunks.push(chunk);
        }
        let (end_transaction, end) =
            decode_wm_v1_snapshot_end_frame(&read_frame(&mut self.stream)?)?;
        if end_transaction != transaction {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        let snapshot = decode_wm_v1_policy_snapshot(&WmV1SnapshotTransfer {
            transaction,
            begin,
            chunks,
            end,
        })?;
        if snapshot.scene.outputs.len() > self.max_outputs
            || snapshot.scene.surfaces.len() > self.max_surfaces
            || snapshot.actions.len() > self.max_bindings
        {
            return Err(PolicyV1ClientError::InvalidScene);
        }
        validate_policy_scene(&snapshot.scene)?;
        Ok(snapshot)
    }

    pub fn receive_projection_request(
        &mut self,
    ) -> Result<PolicyProjectionRequest, PolicyV1ClientError> {
        let (_, request) = decode_wm_v1_projection_request_frame(&read_frame(&mut self.stream)?)?;
        let request = decode_wm_v1_policy_projection_request(&request)?;
        if request.connection_epoch != self.connection_epoch {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        Ok(request)
    }

    pub fn send_projection(
        &mut self,
        proposal: &PolicyProjectionProposal,
    ) -> Result<(), PolicyV1ClientError> {
        if proposal.connection_epoch != self.connection_epoch {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        let transfer = encode_wm_v1_policy_projection(proposal)?;
        self.stream.write_all(&encode_wm_v1_projection_begin_frame(
            transfer.transaction,
            &transfer.begin,
        )?)?;
        for chunk in &transfer.chunks {
            self.stream.write_all(&encode_wm_v1_projection_chunk_frame(
                transfer.transaction,
                chunk,
            )?)?;
        }
        self.stream.write_all(&encode_wm_v1_projection_end_frame(
            transfer.transaction,
            &transfer.end,
        )?)?;
        self.stream.flush()?;
        Ok(())
    }

    pub fn receive_projection_outcome(
        &mut self,
        proposal: &PolicyProjectionProposal,
    ) -> Result<PolicyProjectionOutcome, PolicyV1ClientError> {
        let (transaction, outcome) =
            decode_wm_v1_projection_outcome_frame(&read_frame(&mut self.stream)?)?;
        if transaction != proposal.transaction
            || outcome.connection_epoch != self.connection_epoch
            || outcome.request_id != proposal.request_id
        {
            return Err(PolicyV1ClientError::ConnectionEpochMismatch);
        }
        Ok(decode_wm_v1_policy_projection_outcome(&outcome)?)
    }

    pub fn tile_once(
        &mut self,
        snapshot: &PolicySceneSnapshot,
        request: &PolicyProjectionRequest,
    ) -> Result<PolicyProjectionProposal, PolicyV1ClientError> {
        if request.affected_outputs.len() > 1
            && self.capabilities & SOPHIA_WM_CAPABILITY_MULTI_OUTPUT == 0
        {
            return Err(PolicyV1ClientError::MissingCapability);
        }
        tile_policy_scene(self.mint_transaction()?, snapshot, request)
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub fn new_transaction(&mut self) -> Result<TransactionId, PolicyV1ClientError> {
        self.mint_transaction()
    }

    fn mint_transaction(&mut self) -> Result<TransactionId, PolicyV1ClientError> {
        let transaction = TransactionId::from_raw(self.next_transaction);
        self.next_transaction = self
            .next_transaction
            .checked_add(1)
            .ok_or(PolicyV1ClientError::TransactionExhausted)?;
        Ok(transaction)
    }
}

/// Places each affected output's surfaces in equal columns. Newly mapped
/// surfaces are adopted by the active affected output exactly once, so a
/// policy client cannot leave them permanently unassigned while repeatedly
/// committing an empty layout.
pub fn tile_policy_scene(
    transaction: TransactionId,
    scene: &PolicySceneSnapshot,
    request: &PolicyProjectionRequest,
) -> Result<PolicyProjectionProposal, PolicyV1ClientError> {
    if scene.generation != request.scene_generation {
        return Err(PolicyV1ClientError::SceneGenerationMismatch);
    }
    validate_policy_scene(scene)?;
    if !transaction.is_valid()
        || request.affected_outputs.is_empty()
        || request.affected_outputs.len() > POLICY_MAX_OUTPUTS
    {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    let adoption_output = request
        .affected_outputs
        .iter()
        .copied()
        .find(|output| *output == scene.active_output)
        .unwrap_or(request.affected_outputs[0]);
    let mut outputs = Vec::with_capacity(request.affected_outputs.len());
    for output_id in &request.affected_outputs {
        let output = scene
            .outputs
            .iter()
            .find(|output| output.output == *output_id)
            .ok_or(PolicyV1ClientError::InvalidScene)?;
        let surfaces = scene
            .surfaces
            .iter()
            .filter(|surface| {
                surface.current_output == Some(*output_id)
                    || (surface.current_output.is_none()
                        && !surface.current_state.minimized
                        && *output_id == adoption_output)
            })
            .collect::<Vec<_>>();
        let mut placements = Vec::with_capacity(surfaces.len());
        let mut focus = output.focus;
        for (index, surface) in surfaces.iter().enumerate() {
            let geometry = column_geometry(output.bounds, index, surfaces.len())?;
            let requested_size = clamp_size(
                Size {
                    width: geometry.width,
                    height: geometry.height,
                },
                surface.constraints.min_size,
                surface.constraints.max_size,
            );
            if focus.is_none() && surface.capabilities.focusable {
                focus = Some(surface.surface);
            }
            placements.push(PolicySurfacePlacement {
                surface: surface.surface,
                surface_generation: surface.generation,
                geometry,
                requested_size: Some(requested_size),
                crop: None,
                transform: PolicyTransform::Identity,
                presentation: surface.current_state,
            });
        }
        outputs.push(PolicyOutputProjection {
            output: *output_id,
            placements,
            focus,
        });
    }
    Ok(PolicyProjectionProposal {
        launch_contexts: Vec::new(),
        output_launch_contexts: Vec::new(),
        translation_groups: Vec::new(),
        tab_groups: Vec::new(),
        transaction,
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        base_generation: request.scene_generation,
        active_output: scene.active_output,
        outputs,
        indicators: Vec::new(),
        output_statuses: Vec::new(),
    })
}

/// Counts the distinct surfaces carried by one immutable policy proposal.
///
/// A committed proposal is the authoritative layout decision. Callers that
/// gate a follow-up role on placement must inspect it directly instead of
/// waiting for a later scene snapshot to echo the same geometry.
pub fn policy_projection_distinct_surface_count(proposal: &PolicyProjectionProposal) -> usize {
    proposal
        .outputs
        .iter()
        .flat_map(|output| &output.placements)
        .map(|placement| placement.surface)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

/// Partitions manageable surfaces across two logical outputs using only
/// policy-visible geometry. Physical head and connector identity never enters
/// this decision.
pub fn partition_policy_scene_across_outputs(
    scene: &mut PolicySceneSnapshot,
) -> Result<OutputId, PolicyV1ClientError> {
    if scene.outputs.len() != 2 {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    let mut outputs = scene.outputs.iter().collect::<Vec<_>>();
    outputs.sort_by_key(|output| (output.bounds.x, output.bounds.y, output.output.raw()));
    let left = outputs[0].output;
    let rightmost = outputs[1].output;
    scene.active_output = rightmost;
    scene
        .surfaces
        .sort_by_key(|surface| (surface.surface.index(), surface.surface.generation()));
    for (index, surface) in scene.surfaces.iter_mut().enumerate() {
        surface.current_output = Some(if index == 0 { left } else { rightmost });
    }
    Ok(rightmost)
}

fn column_geometry(bounds: Rect, index: usize, count: usize) -> Result<Rect, PolicyV1ClientError> {
    if bounds.is_empty() || count == 0 {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    let count = i32::try_from(count).map_err(|_| PolicyV1ClientError::InvalidScene)?;
    let index = i32::try_from(index).map_err(|_| PolicyV1ClientError::InvalidScene)?;
    let width = bounds.width / count;
    if width <= 0 {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    let x = bounds.x + width * index;
    Ok(Rect {
        x,
        y: bounds.y,
        width: if index + 1 == count {
            bounds.x + bounds.width - x
        } else {
            width
        },
        height: bounds.height,
    })
}

fn validate_policy_scene(scene: &PolicySceneSnapshot) -> Result<(), PolicyV1ClientError> {
    if scene.generation == 0
        || scene.outputs.is_empty()
        || scene.outputs.len() > SOPHIA_WM_MAX_OUTPUTS
        || scene.surfaces.len() > SOPHIA_WM_MAX_SURFACES
    {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    let mut outputs = BTreeSet::new();
    for output in &scene.outputs {
        if !output.output.is_valid()
            || output.generation == 0
            || output.bounds.is_empty()
            || !outputs.insert(output.output)
        {
            return Err(PolicyV1ClientError::InvalidScene);
        }
    }
    let mut surfaces = BTreeSet::new();
    for surface in &scene.surfaces {
        if !surface.surface.is_valid()
            || surface.generation == 0
            || surface.geometry.is_empty()
            || !surfaces.insert(surface.surface)
            || surface
                .current_output
                .is_some_and(|output| !outputs.contains(&output))
            || surface
                .constraints
                .min_size
                .is_some_and(|size| size.width <= 0 || size.height <= 0)
            || surface
                .constraints
                .max_size
                .is_some_and(|size| size.width <= 0 || size.height <= 0)
            || matches!(
                (surface.constraints.min_size, surface.constraints.max_size),
                (Some(minimum), Some(maximum))
                    if minimum.width > maximum.width || minimum.height > maximum.height
            )
        {
            return Err(PolicyV1ClientError::InvalidScene);
        }
    }
    if scene.surfaces.iter().any(|surface| {
        surface
            .transient_owner
            .is_some_and(|owner| owner == surface.surface || !surfaces.contains(&owner))
    }) {
        return Err(PolicyV1ClientError::InvalidScene);
    }
    for output in &scene.outputs {
        if output.focus.is_some_and(|focus| {
            !scene.surfaces.iter().any(|surface| {
                surface.surface == focus
                    && surface.current_output == Some(output.output)
                    && surface.capabilities.focusable
            })
        }) {
            return Err(PolicyV1ClientError::InvalidScene);
        }
    }
    Ok(())
}

fn clamp_size(size: Size, minimum: Option<Size>, maximum: Option<Size>) -> Size {
    let mut size = size;
    if let Some(minimum) = minimum {
        size.width = size.width.max(minimum.width);
        size.height = size.height.max(minimum.height);
    }
    if let Some(maximum) = maximum {
        size.width = size.width.min(maximum.width);
        size.height = size.height.min(maximum.height);
    }
    size
}

fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>, PolicyV1ClientError> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header)?;
    let payload_len = u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("fixed header contains payload length"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(PolicyV1ClientError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream.read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])?;
    Ok(frame)
}
