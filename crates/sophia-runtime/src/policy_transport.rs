use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use sophia_protocol::{
    IpcCodecError, IpcMessageKind, PolicyConfiguration, PolicyDirtyRequest,
    PolicyProjectionOutcome, PolicySessionOperationOutcome, PolicySessionOperationRequest,
    SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAX_PAYLOAD_LEN, SOPHIA_WM_CAPABILITY_CONFIGURATION,
    SOPHIA_WM_CAPABILITY_POLICY_DIRTY, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, TransactionId, WmV1PolicyConfigurationOutcome,
    WmV1ProfileCompletion, WmV1ProfileIdentity, WmV1ProfileOutcome, WmV1SnapshotBegin,
    WmV1SnapshotChunk, WmV1SnapshotEnd, decode_frame, decode_wm_v1_client_hello_frame,
    decode_wm_v1_policy_configuration, decode_wm_v1_policy_configuration_frame,
    decode_wm_v1_policy_dirty, decode_wm_v1_policy_dirty_frame,
    decode_wm_v1_policy_session_operation_request, decode_wm_v1_profile_active,
    decode_wm_v1_profile_prepared, decode_wm_v1_profile_rolled_back,
    decode_wm_v1_projection_begin_frame, decode_wm_v1_projection_chunk_frame,
    decode_wm_v1_projection_end_frame, decode_wm_v1_session_operation_request_frame,
    encode_wm_v1_policy_configuration_outcome_frame, encode_wm_v1_policy_projection_outcome,
    encode_wm_v1_policy_projection_request, encode_wm_v1_policy_session_operation_outcome,
    encode_wm_v1_profile_activate, encode_wm_v1_profile_prepare, encode_wm_v1_profile_rollback,
    encode_wm_v1_projection_outcome_frame, encode_wm_v1_projection_request_frame,
    encode_wm_v1_server_welcome_frame, encode_wm_v1_session_operation_outcome_frame,
    encode_wm_v1_snapshot_begin_frame, encode_wm_v1_snapshot_chunk_frame,
    encode_wm_v1_snapshot_end_frame,
};

use crate::{
    PolicyConnectionState, PolicyPeerIdentity, PolicyProfileCompletionDisposition,
    PolicyProfileHandoffEffect, PolicyProfileHandoffError, PolicyProfileHandoffKind,
    PolicyProfileHandoffModel, PolicyProfileHandoffMsg, PolicyProfileHandoffUpdate,
    PolicyRoleEndpoint, PolicyRoleEndpointError, PolicySnapshotAssembler, PolicyTransferError,
    QueuedPolicyProjection, reduce_policy_profile_handoff,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyTransportError {
    Endpoint(PolicyRoleEndpointError),
    Transfer(PolicyTransferError),
    Codec(IpcCodecError),
    Io(String),
    /// The socket's own timeout expired with no complete frame.
    ///
    /// Distinct from `Io` because it says nothing about the connection: the
    /// peer is simply slower than one window. Reported as an `Io` string it
    /// read as a broken transport and restarted a working window manager.
    TimedOut,
    UnexpectedMessage(IpcMessageKind),
    ProfileHandoff(PolicyProfileHandoffError),
    ProfileCompletionOutOfPhase,
    ProfileCompletionStale,
    ProfileRejected {
        kind: PolicyProfileHandoffKind,
        outcome: WmV1ProfileOutcome,
    },
    NotConnected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyClientEvent {
    ProjectionPending,
    Projection(QueuedPolicyProjection),
    Configuration {
        transaction: TransactionId,
        configuration: PolicyConfiguration,
    },
    Dirty {
        transaction: TransactionId,
        request: PolicyDirtyRequest,
    },
    SessionOperation {
        transaction: TransactionId,
        request: PolicySessionOperationRequest,
    },
    ProfileCompletion {
        kind: PolicyProfileHandoffKind,
        completion: WmV1ProfileCompletion,
    },
}

impl core::fmt::Display for PolicyTransportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PolicyTransportError {}

impl From<PolicyRoleEndpointError> for PolicyTransportError {
    fn from(error: PolicyRoleEndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<PolicyTransferError> for PolicyTransportError {
    fn from(error: PolicyTransferError) -> Self {
        Self::Transfer(error)
    }
}

impl From<IpcCodecError> for PolicyTransportError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<PolicyProfileHandoffError> for PolicyTransportError {
    fn from(error: PolicyProfileHandoffError) -> Self {
        Self::ProfileHandoff(error)
    }
}

/// Draft session-owned WM transport. It is not connected to the installed v7
/// path until the public-protocol milestone reaches its migration gate.
pub struct PolicyWmSessionTransport {
    endpoint: PolicyRoleEndpoint,
    connection: PolicyConnectionState,
    stream: Option<UnixStream>,
    read_buffer: Vec<u8>,
    peer: Option<PolicyPeerIdentity>,
    profile_activation: bool,
}

impl PolicyWmSessionTransport {
    fn with_endpoint(endpoint: PolicyRoleEndpoint, profile_activation: bool) -> Self {
        Self {
            endpoint,
            connection: PolicyConnectionState::default(),
            stream: None,
            read_buffer: Vec::new(),
            peer: None,
            profile_activation,
        }
    }

    pub fn bind(
        directory: impl AsRef<Path>,
        expected_peer: PolicyPeerIdentity,
    ) -> Result<Self, PolicyTransportError> {
        Ok(Self::with_endpoint(
            PolicyRoleEndpoint::bind(directory, expected_peer)?,
            false,
        ))
    }

    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, PolicyTransportError> {
        Ok(Self::with_endpoint(
            PolicyRoleEndpoint::bind_for_supervised_uid(directory, expected_uid)?,
            false,
        ))
    }

    /// Binds a supervised policy endpoint that requires exact profile
    /// activation before normal policy traffic is admitted.
    pub fn bind_for_supervised_uid_profile_activation(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, PolicyTransportError> {
        Ok(Self::with_endpoint(
            PolicyRoleEndpoint::bind_for_supervised_uid(directory, expected_uid)?,
            true,
        ))
    }

    /// Binds a transport that may negotiate the startup-only profile barrier.
    /// Selecting the capability does not activate a candidate; callers must
    /// settle the typed handoff reducer before opening graphical resources.
    pub fn bind_for_startup_profile_activation(
        directory: impl AsRef<Path>,
        expected_peer: PolicyPeerIdentity,
    ) -> Result<Self, PolicyTransportError> {
        Ok(Self::with_endpoint(
            PolicyRoleEndpoint::bind(directory, expected_peer)?,
            true,
        ))
    }

    pub fn authorize_supervised_pid(&mut self, pid: u32) -> Result<(), PolicyTransportError> {
        self.endpoint.authorize_supervised_pid(pid)?;
        Ok(())
    }

    pub fn socket_path(&self) -> &Path {
        self.endpoint.socket_path()
    }

    /// Returns the capability set selected during negotiation, or zero before it.
    ///
    /// Producers must gate outbound content on this rather than on the set the
    /// server supports, so a client never receives a record kind it must reject.
    pub fn selected_capabilities(&self) -> u64 {
        self.connection.selected_capabilities()
    }

    pub fn accept_and_negotiate(
        &mut self,
        connection_epoch: u64,
        timeout: Duration,
    ) -> Result<(), PolicyTransportError> {
        if self.stream.is_some() {
            return Err(PolicyTransportError::Transfer(
                PolicyTransferError::AlreadyConnected,
            ));
        }
        let mut stream = self.endpoint.accept_expected_timeout(timeout)?;
        let peer = self
            .endpoint
            .active_peer()
            .expect("accepted endpoint records its peer");
        let result = (|| {
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
            stream
                .set_write_timeout(Some(timeout))
                .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
            let frame = read_policy_frame(&mut stream)?;
            let hello = decode_wm_v1_client_hello_frame(&frame)?;
            let mut connection = self.connection.clone();
            connection.connect(connection_epoch)?;
            let welcome =
                connection.negotiate_profile_activation(&hello, self.profile_activation)?;
            let frame = encode_wm_v1_server_welcome_frame(&welcome)?;
            stream
                .write_all(&frame)
                .and_then(|()| stream.flush())
                .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
            self.connection = connection;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.endpoint.release_peer(peer);
            return Err(error);
        }
        self.stream = Some(stream);
        self.peer = Some(peer);
        Ok(())
    }

    pub fn receive_projection_part(
        &mut self,
    ) -> Result<Option<QueuedPolicyProjection>, PolicyTransportError> {
        match self.receive_client_event()? {
            PolicyClientEvent::ProjectionPending => Ok(None),
            PolicyClientEvent::Projection(projection) => Ok(Some(projection)),
            PolicyClientEvent::Configuration { .. } => Err(
                PolicyTransportError::UnexpectedMessage(IpcMessageKind::WmV1PolicyConfiguration),
            ),
            PolicyClientEvent::Dirty { .. } => Err(PolicyTransportError::UnexpectedMessage(
                IpcMessageKind::WmV1PolicyDirty,
            )),
            PolicyClientEvent::SessionOperation { .. } => {
                Err(PolicyTransportError::UnexpectedMessage(
                    IpcMessageKind::WmV1SessionOperationRequest,
                ))
            }
            PolicyClientEvent::ProfileCompletion { kind, .. } => {
                let message = match kind {
                    PolicyProfileHandoffKind::Prepare => IpcMessageKind::WmV1ProfilePrepared,
                    PolicyProfileHandoffKind::Activate => IpcMessageKind::WmV1ProfileActive,
                    PolicyProfileHandoffKind::Rollback => IpcMessageKind::WmV1ProfileRolledBack,
                };
                Err(PolicyTransportError::UnexpectedMessage(message))
            }
        }
    }

    pub fn receive_client_event(&mut self) -> Result<PolicyClientEvent, PolicyTransportError> {
        let frame = self
            .receive_frame(true)?
            .expect("blocking receive returns one frame");
        self.decode_client_event(&frame)
    }

    /// Waits for one client event across several socket timeouts.
    ///
    /// One expired window says only that the peer is slower than that window,
    /// which a policy client legitimately is after a topology change hands it a
    /// whole new layout to compute. Only a client that stays silent across the
    /// whole deadline is treated as gone.
    pub fn receive_client_event_within(
        &mut self,
        deadline: Duration,
    ) -> Result<PolicyClientEvent, PolicyTransportError> {
        let started = Instant::now();
        loop {
            match self.receive_client_event() {
                Err(PolicyTransportError::TimedOut) if started.elapsed() < deadline => {}
                other => return other,
            }
        }
    }

    /// Polls one complete control frame without consuming a partial frame.
    pub fn try_receive_client_event(
        &mut self,
    ) -> Result<Option<PolicyClientEvent>, PolicyTransportError> {
        self.receive_frame(false)?
            .map(|frame| self.decode_client_event(&frame))
            .transpose()
    }

    fn decode_client_event(
        &mut self,
        frame: &[u8],
    ) -> Result<PolicyClientEvent, PolicyTransportError> {
        let (header, _) = decode_frame(frame)?;
        match header.message_kind {
            IpcMessageKind::WmV1ProjectionBegin => {
                let (transaction, begin) = decode_wm_v1_projection_begin_frame(frame)?;
                self.connection.begin_projection(transaction, begin)?;
                Ok(PolicyClientEvent::ProjectionPending)
            }
            IpcMessageKind::WmV1ProjectionChunk => {
                let (transaction, chunk) = decode_wm_v1_projection_chunk_frame(frame)?;
                self.connection
                    .append_projection_chunk(transaction, chunk)?;
                Ok(PolicyClientEvent::ProjectionPending)
            }
            IpcMessageKind::WmV1ProjectionEnd => {
                let (transaction, end) = decode_wm_v1_projection_end_frame(frame)?;
                self.connection.finish_projection(transaction, end)?;
                Ok(PolicyClientEvent::Projection(
                    self.connection
                        .settle_queued()
                        .expect("a finished projection queues one transfer"),
                ))
            }
            IpcMessageKind::WmV1PolicyConfiguration => {
                let (transaction, wire) = decode_wm_v1_policy_configuration_frame(frame)?;
                let configuration = decode_wm_v1_policy_configuration(&wire)?;
                self.connection.admit_control_message(
                    transaction,
                    configuration.connection_epoch,
                    SOPHIA_WM_CAPABILITY_CONFIGURATION,
                )?;
                Ok(PolicyClientEvent::Configuration {
                    transaction,
                    configuration,
                })
            }
            IpcMessageKind::WmV1PolicyDirty => {
                let (transaction, wire) = decode_wm_v1_policy_dirty_frame(frame)?;
                let request = decode_wm_v1_policy_dirty(&wire)?;
                self.connection.admit_control_message(
                    transaction,
                    request.connection_epoch,
                    SOPHIA_WM_CAPABILITY_POLICY_DIRTY,
                )?;
                Ok(PolicyClientEvent::Dirty {
                    transaction,
                    request,
                })
            }
            IpcMessageKind::WmV1SessionOperationRequest => {
                let (transaction, wire) = decode_wm_v1_session_operation_request_frame(frame)?;
                let request = decode_wm_v1_policy_session_operation_request(&wire)?;
                self.connection.admit_control_message(
                    transaction,
                    request.connection_epoch,
                    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS,
                )?;
                Ok(PolicyClientEvent::SessionOperation {
                    transaction,
                    request,
                })
            }
            IpcMessageKind::WmV1ProfilePrepared
            | IpcMessageKind::WmV1ProfileActive
            | IpcMessageKind::WmV1ProfileRolledBack => {
                let (kind, completion) = match header.message_kind {
                    IpcMessageKind::WmV1ProfilePrepared => (
                        PolicyProfileHandoffKind::Prepare,
                        decode_wm_v1_profile_prepared(frame)?,
                    ),
                    IpcMessageKind::WmV1ProfileActive => (
                        PolicyProfileHandoffKind::Activate,
                        decode_wm_v1_profile_active(frame)?,
                    ),
                    IpcMessageKind::WmV1ProfileRolledBack => (
                        PolicyProfileHandoffKind::Rollback,
                        decode_wm_v1_profile_rolled_back(frame)?,
                    ),
                    _ => unreachable!(),
                };
                self.connection.admit_server_completion(
                    completion.transaction,
                    completion.identity.connection_epoch,
                    SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
                )?;
                Ok(PolicyClientEvent::ProfileCompletion { kind, completion })
            }
            other => Err(PolicyTransportError::UnexpectedMessage(other)),
        }
    }

    /// Classifies a socket error, keeping an expired timeout apart from a
    /// genuine transport fault.
    ///
    /// A socket carrying `SO_RCVTIMEO`/`SO_SNDTIMEO` reports expiry as
    /// `WouldBlock` on Linux and `TimedOut` elsewhere, so both mean the same
    /// thing here.
    fn classify_io(error: &std::io::Error) -> PolicyTransportError {
        match error.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                PolicyTransportError::TimedOut
            }
            _ => PolicyTransportError::Io(error.to_string()),
        }
    }

    fn receive_frame(&mut self, blocking: bool) -> Result<Option<Vec<u8>>, PolicyTransportError> {
        if let Some(frame) = take_buffered_frame(&mut self.read_buffer)? {
            return Ok(Some(frame));
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        stream
            .set_nonblocking(!blocking)
            .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
        let mut bytes = [0_u8; 8192];
        let outcome = loop {
            match stream.read(&mut bytes) {
                Ok(0) => break Err(PolicyTransportError::Io("policy peer closed".into())),
                Ok(count) => {
                    self.read_buffer.extend_from_slice(&bytes[..count]);
                    if let Some(frame) = take_buffered_frame(&mut self.read_buffer)? {
                        break Ok(Some(frame));
                    }
                }
                Err(error) if !blocking && error.kind() == std::io::ErrorKind::WouldBlock => {
                    break Ok(None);
                }
                // A blocking read that expires is the socket's own read
                // timeout, not a closed peer. Saying so lets the caller wait
                // again instead of tearing down a live connection; whatever
                // partial frame arrived stays in the read buffer.
                Err(error) => break Err(Self::classify_io(&error)),
            }
        };
        // Restored on every exit, not only the two that used to. Leaving the
        // socket non-blocking made the next blocking read return `WouldBlock`
        // immediately, which read as a dead transport and restarted a working
        // window manager.
        if !blocking {
            stream
                .set_nonblocking(false)
                .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
        }
        outcome
    }

    pub fn send_snapshot(
        &mut self,
        transaction: TransactionId,
        begin: &WmV1SnapshotBegin,
        chunks: &[WmV1SnapshotChunk],
        end: &WmV1SnapshotEnd,
    ) -> Result<(), PolicyTransportError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        let mut assembler = PolicySnapshotAssembler::new_with_capabilities(
            begin.connection_epoch,
            self.connection.selected_capabilities(),
        )?;
        assembler.begin(transaction, begin.clone())?;
        for chunk in chunks {
            assembler.append(transaction, chunk.clone())?;
        }
        assembler.finish(transaction, end.clone())?;

        let mut frames = Vec::with_capacity(chunks.len() + 2);
        frames.push(encode_wm_v1_snapshot_begin_frame(transaction, begin)?);
        for chunk in chunks {
            frames.push(encode_wm_v1_snapshot_chunk_frame(transaction, chunk)?);
        }
        frames.push(encode_wm_v1_snapshot_end_frame(transaction, end)?);
        for frame in frames {
            stream
                .write_all(&frame)
                .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
        }
        stream
            .flush()
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    pub fn send_profile_handoff(
        &mut self,
        effect: PolicyProfileHandoffEffect,
    ) -> Result<(), PolicyTransportError> {
        self.connection.require_server_control(
            effect.command.identity.connection_epoch,
            SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
        )?;
        let frame = match effect.kind {
            PolicyProfileHandoffKind::Prepare => encode_wm_v1_profile_prepare(effect.command)?,
            PolicyProfileHandoffKind::Activate => encode_wm_v1_profile_activate(effect.command)?,
            PolicyProfileHandoffKind::Rollback => encode_wm_v1_profile_rollback(effect.command)?,
        };
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        stream
            .write_all(&frame)
            .and_then(|()| stream.flush())
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    /// Executes the transport side of exact prepare/activate admission while
    /// the pure reducer remains the sole phase and correlation authority.
    pub fn activate_profile_handoff(
        &mut self,
        identity: WmV1ProfileIdentity,
        prepare_transaction: TransactionId,
        activate_transaction: TransactionId,
    ) -> Result<PolicyProfileHandoffModel, PolicyTransportError> {
        let mut model = PolicyProfileHandoffModel::new(identity);
        for (kind, transaction) in [
            (PolicyProfileHandoffKind::Prepare, prepare_transaction),
            (PolicyProfileHandoffKind::Activate, activate_transaction),
        ] {
            let settled = self.execute_profile_handoff_step(&model, kind, transaction)?;
            match settled.completion {
                Some(PolicyProfileCompletionDisposition::Accepted) => {
                    model = settled.model;
                }
                Some(PolicyProfileCompletionDisposition::Rejected(outcome)) => {
                    return Err(PolicyTransportError::ProfileRejected { kind, outcome });
                }
                Some(PolicyProfileCompletionDisposition::Stale) | None => {
                    return Err(PolicyTransportError::ProfileCompletionStale);
                }
            }
        }
        Ok(model)
    }

    /// Executes one reducer-owned handoff step and returns the candidate model
    /// even when the peer rejects it, so a coordinator can issue exact
    /// rollback without reconstructing transport phase state.
    pub fn execute_profile_handoff_step(
        &mut self,
        model: &PolicyProfileHandoffModel,
        kind: PolicyProfileHandoffKind,
        transaction: TransactionId,
    ) -> Result<PolicyProfileHandoffUpdate, PolicyTransportError> {
        let update = reduce_policy_profile_handoff(
            model,
            PolicyProfileHandoffMsg::Begin { kind, transaction },
        )?;
        self.send_profile_handoff(
            update
                .effect
                .expect("a valid profile begin always emits one effect"),
        )?;
        let PolicyClientEvent::ProfileCompletion {
            kind: completion_kind,
            completion,
        } = self.receive_client_event()?
        else {
            return Err(PolicyTransportError::ProfileCompletionOutOfPhase);
        };
        if completion_kind != kind {
            return Err(PolicyTransportError::ProfileCompletionOutOfPhase);
        }
        reduce_policy_profile_handoff(
            &update.model,
            PolicyProfileHandoffMsg::Completion { kind, completion },
        )
        .map_err(Into::into)
    }

    pub fn send_projection_request(
        &mut self,
        transaction: TransactionId,
        request: &sophia_protocol::PolicyProjectionRequest,
    ) -> Result<(), PolicyTransportError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        if matches!(
            request.cause,
            sophia_protocol::PolicyRequestCause::PointerFocus { .. }
        ) && self.connection.selected_capabilities()
            & sophia_protocol::SOPHIA_WM_CAPABILITY_POINTER_FOCUS
            == 0
        {
            return Err(PolicyTransferError::UnsupportedCapability.into());
        }
        let frame = if matches!(
            request.cause,
            sophia_protocol::PolicyRequestCause::OutputAction { .. }
        ) {
            if self.connection.selected_capabilities()
                & sophia_protocol::SOPHIA_WM_CAPABILITY_OUTPUT_ACTIONS
                == 0
            {
                return Err(PolicyTransferError::UnsupportedCapability.into());
            }
            let wire = sophia_protocol::encode_wm_output_action_request(request)?;
            sophia_protocol::encode_wm_v1_output_action_request_frame(transaction, &wire)?
        } else {
            let wire = encode_wm_v1_policy_projection_request(request)?;
            encode_wm_v1_projection_request_frame(transaction, &wire)?
        };
        stream
            .write_all(&frame)
            .and_then(|()| stream.flush())
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    pub fn send_projection_outcome(
        &mut self,
        transaction: TransactionId,
        request_id: u64,
        scene_generation: u64,
        outcome: PolicyProjectionOutcome,
    ) -> Result<(), PolicyTransportError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        let outcome = encode_wm_v1_policy_projection_outcome(
            self.connection.connection_epoch(),
            request_id,
            scene_generation,
            outcome,
        )?;
        let frame = encode_wm_v1_projection_outcome_frame(transaction, &outcome)?;
        stream
            .write_all(&frame)
            .and_then(|()| stream.flush())
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    pub fn send_configuration_outcome(
        &mut self,
        transaction: TransactionId,
        generation: u64,
        outcome: PolicyProjectionOutcome,
    ) -> Result<(), PolicyTransportError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        let outcome = match outcome {
            PolicyProjectionOutcome::Committed => sophia_protocol::SOPHIA_WM_OUTCOME_COMMITTED,
            PolicyProjectionOutcome::RejectedStale => {
                sophia_protocol::SOPHIA_WM_OUTCOME_REJECTED_STALE
            }
            PolicyProjectionOutcome::RejectedInvalid => {
                sophia_protocol::SOPHIA_WM_OUTCOME_REJECTED_INVALID
            }
            PolicyProjectionOutcome::TimedOut => sophia_protocol::SOPHIA_WM_OUTCOME_TIMED_OUT,
            PolicyProjectionOutcome::Disconnected => {
                sophia_protocol::SOPHIA_WM_OUTCOME_DISCONNECTED
            }
        };
        let frame = encode_wm_v1_policy_configuration_outcome_frame(
            transaction,
            &WmV1PolicyConfigurationOutcome {
                connection_epoch: self.connection.connection_epoch(),
                configuration_generation: generation,
                outcome,
            },
        )?;
        stream
            .write_all(&frame)
            .and_then(|()| stream.flush())
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    pub fn send_session_operation_outcome(
        &mut self,
        transaction: TransactionId,
        request_id: u64,
        outcome: PolicyProjectionOutcome,
    ) -> Result<(), PolicyTransportError> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(PolicyTransportError::NotConnected)?;
        let outcome =
            encode_wm_v1_policy_session_operation_outcome(PolicySessionOperationOutcome {
                connection_epoch: self.connection.connection_epoch(),
                request_id,
                outcome,
            })?;
        let frame = encode_wm_v1_session_operation_outcome_frame(transaction, &outcome)?;
        stream
            .write_all(&frame)
            .and_then(|()| stream.flush())
            .map_err(|error| PolicyTransportError::Io(error.to_string()))
    }

    pub fn disconnect(&mut self) -> Result<(), PolicyTransportError> {
        if self.stream.take().is_none() {
            return Err(PolicyTransportError::NotConnected);
        }
        self.connection.disconnect()?;
        self.read_buffer.clear();
        let peer = self
            .peer
            .take()
            .expect("connected transport records its peer");
        self.endpoint.release_peer(peer)?;
        Ok(())
    }

    pub const fn connection(&self) -> &PolicyConnectionState {
        &self.connection
    }
}

fn take_buffered_frame(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>, PolicyTransportError> {
    if buffer.len() < SOPHIA_IPC_HEADER_LEN {
        return Ok(None);
    }
    let payload_len = u32::from_le_bytes(
        buffer[16..20]
            .try_into()
            .expect("fixed frame payload range is present"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(PolicyTransportError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let frame_len = SOPHIA_IPC_HEADER_LEN + payload_len;
    if buffer.len() < frame_len {
        return Ok(None);
    }
    Ok(Some(buffer.drain(..frame_len).collect()))
}

fn read_policy_frame(stream: &mut UnixStream) -> Result<Vec<u8>, PolicyTransportError> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream
        .read_exact(&mut header)
        .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
    let payload_len = u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("fixed frame payload range is present"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(PolicyTransportError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .map_err(|error| PolicyTransportError::Io(error.to_string()))?;
    Ok(frame)
}
