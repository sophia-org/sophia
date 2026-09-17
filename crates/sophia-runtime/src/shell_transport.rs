use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    ContentAdmissionRefused, ContentGrant, ContentLimits, IpcCodecError, SOPHIA_IPC_HEADER_LEN,
    SOPHIA_IPC_MAX_PAYLOAD_LEN, SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
    SOPHIA_SHELL_INTERFACE_REVISION, SOPHIA_SHELL_MAX_DESCRIPTORS,
    SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS, ShellV1Activation, ShellV1ActivationAck,
    ShellV1Candidate, ShellV1CandidateOutcome, ShellV1ClientHello, ShellV1DescriptorSnapshot,
    ShellV1ServerWelcome, TransactionId, decode_shell_v1_activation_ack_frame,
    decode_shell_v1_activation_frame, decode_shell_v1_candidate_frame,
    decode_shell_v1_candidate_outcome_frame, decode_shell_v1_client_hello_frame,
    decode_shell_v1_descriptor_snapshot_frame, decode_shell_v1_server_welcome_frame,
    encode_shell_v1_activation_ack_frame, encode_shell_v1_activation_frame,
    encode_shell_v1_candidate_frame, encode_shell_v1_candidate_outcome_frame,
    encode_shell_v1_client_hello_frame, encode_shell_v1_descriptor_snapshot_frame,
    encode_shell_v1_server_welcome_frame,
};

use crate::{
    ContentAllocationError, ContentCandidateError, ContentStoreError, PolicyRole,
    PolicyRoleEndpoint, PolicyRoleEndpointError, ProtectionDomainEvidence,
};

mod accounting;
mod legacy;
mod negotiation;
pub use legacy::ShellSessionTransport;
mod content_actions;
mod content_admission;
mod content_allocations;
mod content_candidates;
mod content_resources;
mod control_budget;
mod indicator_responses;
mod outbox;
pub use accounting::{ShellContentAccounting, ShellContentShutdown};
pub use content_admission::ShellContentAdmissionPolicy;

const SHELL_IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellTransportError {
    Endpoint(PolicyRoleEndpointError),
    Io(String),
    Codec(IpcCodecError),
    UnsupportedRevision,
    MissingCapability,
    ContentAdmissionRefused(ContentAdmissionRefused),
    ContentStore(ContentStoreError),
    ContentAllocation(ContentAllocationError),
    ContentCandidate(ContentCandidateError),
    InvalidConnectionEpoch,
    WrongTransaction,
    WrongCandidate,
    WrongActivation,
    WrongContentRecord,
    WrongContentGrant,
    ContentQueueSaturated,
    ActivationQueueSaturated,
    NotConnected,
}

impl core::fmt::Display for ShellTransportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShellTransportError {}

impl From<PolicyRoleEndpointError> for ShellTransportError {
    fn from(error: PolicyRoleEndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<IpcCodecError> for ShellTransportError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<ContentStoreError> for ShellTransportError {
    fn from(error: ContentStoreError) -> Self {
        Self::ContentStore(error)
    }
}

impl From<ContentCandidateError> for ShellTransportError {
    fn from(error: ContentCandidateError) -> Self {
        Self::ContentCandidate(error)
    }
}

impl From<ContentAllocationError> for ShellTransportError {
    fn from(error: ContentAllocationError) -> Self {
        Self::ContentAllocation(error)
    }
}

pub struct ShellComponentTransport {
    endpoint: PolicyRoleEndpoint,
    stream: Option<UnixStream>,
    capabilities: u64,
    peer_closed: bool,
    input: Vec<u8>,
    output: outbox::ShellOutbox,
    action_cancellations: Vec<sophia_protocol::ContentAction>,
    indicator_response: Option<indicator_responses::PendingIndicatorResponse>,
    inbox: VecDeque<Vec<u8>>,
    connection_epoch: u64,
    reserved_limits: Option<ContentLimits>,
    content_grant: Option<ContentGrant>,
    content_limits: Option<ContentLimits>,
    store_grant: ContentGrant,
    last_candidate_generation: u64,
    requested_candidate: Option<(TransactionId, ShellV1DescriptorSnapshot)>,
    pending_candidate: Option<PendingShellCandidate>,
    presented_candidate: Option<(u64, u64)>,
    pending_activations: VecDeque<(TransactionId, u64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingShellCandidate {
    transaction: TransactionId,
    generation: u64,
    visible: bool,
    prepared: bool,
}

impl ShellComponentTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, ShellTransportError> {
        Ok(Self {
            endpoint: PolicyRoleEndpoint::bind_role_for_supervised_uid(
                directory,
                PolicyRole::Shell,
                expected_uid,
            )?,
            stream: None,
            capabilities: 0,
            peer_closed: false,
            input: Vec::new(),
            output: outbox::ShellOutbox::default(),
            action_cancellations: Vec::with_capacity(16),
            indicator_response: None,
            inbox: VecDeque::new(),
            connection_epoch: 0,
            reserved_limits: None,
            content_grant: None,
            content_limits: None,
            store_grant: ContentGrant::default(),
            last_candidate_generation: 0,
            requested_candidate: None,
            pending_candidate: None,
            presented_candidate: None,
            pending_activations: VecDeque::with_capacity(SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS),
        })
    }

    pub fn authorize_protected_peer(
        &mut self,
        evidence: &ProtectionDomainEvidence,
    ) -> Result<(), ShellTransportError> {
        self.endpoint.authorize_protected_peer(evidence)?;
        Ok(())
    }

    pub fn socket_path(&self) -> &Path {
        self.endpoint.socket_path()
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub fn content_limits(&self) -> Option<&ContentLimits> {
        self.content_limits.as_ref()
    }

    pub fn request_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<ShellV1Candidate, ShellTransportError> {
        self.begin_candidate_request(epochs, transaction, snapshot)?;
        let deadline = std::time::Instant::now() + SHELL_IO_TIMEOUT;
        loop {
            if let Some(candidate) = self.poll_candidate(epochs)? {
                return Ok(candidate);
            }
            if std::time::Instant::now() >= deadline {
                return Err(ShellTransportError::Io("shell candidate timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn begin_candidate_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(snapshot.connection_epoch)?;
        if self.pending_candidate.is_some() || self.requested_candidate.is_some() {
            return Err(ShellTransportError::WrongCandidate);
        }
        let frame = encode_shell_v1_descriptor_snapshot_frame(transaction, snapshot)?;
        self.requested_candidate = Some((transaction, snapshot.clone()));
        self.send_async(epochs, frame)
    }

    pub fn poll_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1Candidate>, ShellTransportError> {
        let Some((transaction, snapshot)) = self.requested_candidate.clone() else {
            return Ok(None);
        };
        let Some(frame) =
            self.poll_kind(epochs, sophia_protocol::IpcMessageKind::ShellV1Candidate)?
        else {
            return Ok(None);
        };
        self.requested_candidate = None;
        let (response_transaction, candidate) = decode_shell_v1_candidate_frame(&frame)?;
        if response_transaction != transaction {
            return Err(ShellTransportError::WrongTransaction);
        }
        self.require_epoch(candidate.connection_epoch)?;
        if candidate.snapshot_generation != snapshot.snapshot_generation
            || candidate.output != snapshot.output
            || candidate.candidate_generation <= self.last_candidate_generation
            || candidate.entries.iter().any(|entry| {
                !snapshot.descriptors.iter().any(|descriptor| {
                    descriptor.slot == entry.slot && descriptor.generation == entry.generation
                })
            })
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        self.last_candidate_generation = candidate.candidate_generation;
        self.pending_candidate = Some(PendingShellCandidate {
            transaction,
            generation: candidate.candidate_generation,
            visible: candidate.visible,
            prepared: false,
        });
        self.requested_candidate = None;
        Ok(Some(candidate))
    }

    pub fn send_candidate_outcome(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellV1CandidateOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.connection_epoch)?;
        let mut pending = self
            .pending_candidate
            .ok_or(ShellTransportError::WrongCandidate)?;
        if pending.transaction != transaction || pending.generation != outcome.candidate_generation
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        match outcome.kind {
            sophia_protocol::ShellV1CandidateOutcomeKind::Prepared if !pending.prepared => {
                pending.prepared = true;
            }
            sophia_protocol::ShellV1CandidateOutcomeKind::Presented if pending.prepared => {}
            sophia_protocol::ShellV1CandidateOutcomeKind::Rejected
            | sophia_protocol::ShellV1CandidateOutcomeKind::Superseded => {}
            _ => return Err(ShellTransportError::WrongCandidate),
        }
        let frame = encode_shell_v1_candidate_outcome_frame(transaction, outcome)?;
        self.send_async(epochs, frame)?;
        match outcome.kind {
            sophia_protocol::ShellV1CandidateOutcomeKind::Prepared => {
                self.pending_candidate = Some(pending);
            }
            sophia_protocol::ShellV1CandidateOutcomeKind::Presented => {
                self.presented_candidate = Some((
                    pending.generation,
                    if pending.visible {
                        outcome.presentation_epoch
                    } else {
                        0
                    },
                ));
                self.pending_candidate = None;
            }
            sophia_protocol::ShellV1CandidateOutcomeKind::Rejected
            | sophia_protocol::ShellV1CandidateOutcomeKind::Superseded => {
                self.pending_candidate = None;
            }
        }
        Ok(())
    }

    pub fn queue_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: ShellV1Activation,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(activation.connection_epoch)?;
        if self.presented_candidate
            != Some((
                activation.candidate_generation,
                activation.presentation_epoch,
            ))
            || activation.action.recipient_epoch != self.connection_epoch
        {
            return Err(ShellTransportError::WrongActivation);
        }
        if self.pending_activations.len() >= SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS {
            self.disconnect(epochs)?;
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        let frame = encode_shell_v1_activation_frame(transaction, activation)?;
        self.send_async(epochs, frame)?;
        self.pending_activations
            .push_back((transaction, activation.activation));
        Ok(())
    }

    pub fn receive_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<ShellV1ActivationAck, ShellTransportError> {
        let deadline = std::time::Instant::now() + SHELL_IO_TIMEOUT;
        loop {
            if let Some(ack) = self.poll_activation_ack(epochs)? {
                return Ok(ack);
            }
            if std::time::Instant::now() >= deadline {
                return Err(ShellTransportError::Io(
                    "shell acknowledgement timed out".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn poll_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1ActivationAck>, ShellTransportError> {
        let Some((expected_transaction, expected_activation)) =
            self.pending_activations.front().copied()
        else {
            return Ok(None);
        };
        let Some(frame) = self.poll_transaction(
            epochs,
            sophia_protocol::IpcMessageKind::ShellV1ActivationAck,
            expected_transaction,
        )?
        else {
            return Ok(None);
        };
        let (_, ack) = decode_shell_v1_activation_ack_frame(&frame)?;
        self.require_epoch(ack.connection_epoch)?;
        if ack.activation != expected_activation {
            return Err(ShellTransportError::WrongActivation);
        }
        self.pending_activations.pop_front();
        Ok(Some(ack))
    }

    pub fn disconnect(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        self.stream = None;
        self.input.clear();
        self.output.clear();
        self.action_cancellations.clear();
        self.indicator_response = None;
        self.inbox.clear();
        self.requested_candidate = None;
        self.pending_candidate = None;
        self.presented_candidate = None;
        self.pending_activations.clear();
        self.content_grant = None;
        self.content_limits = None;
        self.reserved_limits = None;
        epochs.disconnect(self.store_grant);
        if let Some(peer) = self.endpoint.active_peer() {
            self.endpoint.release_peer(peer)?;
        }
        Ok(())
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), ShellTransportError> {
        if epoch == self.connection_epoch && epoch != 0 {
            Ok(())
        } else {
            Err(ShellTransportError::InvalidConnectionEpoch)
        }
    }

    pub const fn supports_shortcut_catalog(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG != 0
    }

    pub const fn supports_reference(&self) -> bool {
        let mask = sophia_protocol::SOPHIA_SHELL_CAPABILITY_SHORTCUT_CATALOG
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_REFERENCE_SHEET;
        self.capabilities & mask == mask
    }

    pub const fn supports_launcher(&self) -> bool {
        let mask = sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
            | sophia_protocol::SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER;
        self.capabilities & mask == mask
    }

    pub const fn supports_tabs(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_TAB_GROUPS != 0
    }

    pub const fn supports_indicators(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS != 0
    }

    pub const fn supports_indicator_activation(&self) -> bool {
        self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION != 0
    }

    pub const fn supports_content(&self) -> bool {
        self.content_grant.is_some()
    }

    pub const fn content_grant(&self) -> Option<ContentGrant> {
        self.content_grant
    }

    pub fn content_reserved_bytes(&self, epochs: &crate::ContentEpochRegistry) -> u64 {
        epochs.reserved_bytes()
    }

    pub fn content_backing_reserved_bytes(&self, epochs: &crate::ContentEpochRegistry) -> u64 {
        epochs.reserved_backing_bytes()
    }

    pub fn content_usage(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Option<crate::ContentMemoryUsage> {
        epochs
            .resources(self.store_grant)
            .map(|store| store.usage())
    }

    pub fn lease_content_resource(
        &self,
        epochs: &crate::ContentEpochRegistry,
        grant: ContentGrant,
        resource: sophia_protocol::ContentResourceId,
    ) -> Result<crate::ContentResourceLease, ShellTransportError> {
        epochs
            .resources(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?
            .lease(grant, resource)
            .map_err(Into::into)
    }

    /// Bounded, nonblocking I/O shared by persistent tabs and the r1 facade.
    pub fn poll_io(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        if self.stream.is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        self.flush_indicator_response(epochs)?;
        let stream = self
            .stream
            .as_mut()
            .ok_or(ShellTransportError::NotConnected)?;
        let mut remaining = 256 * 1024;
        for _ in 0..64 {
            if remaining == 0 || self.output.is_empty() {
                break;
            }
            let bytes = self.output.front();
            match stream.write(&bytes[..bytes.len().min(remaining)]) {
                Ok(0) => return Err(ShellTransportError::NotConnected),
                Ok(n) => {
                    self.output.written(n);
                    remaining -= n;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(ShellTransportError::Io(e.to_string())),
            }
        }
        for _ in 0..64 {
            Self::decode_buffered_input(&mut self.input, &mut self.inbox)?;
            let limit = self
                .content_limits
                .as_ref()
                .map_or(2 * 1024 * 1024, |limits| {
                    limits.max_input_queue_bytes as usize
                });
            let retained = self.input.len() + self.inbox.iter().map(Vec::len).sum::<usize>();
            let available = limit.saturating_sub(retained);
            if available == 0 || self.inbox.len() == 64 {
                break;
            }
            let mut bytes = [0u8; 4096];
            let available = available.min(bytes.len());
            match stream.read(&mut bytes[..available]) {
                Ok(0) => {
                    self.peer_closed = true;
                    break;
                }
                Ok(n) => self.input.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(ShellTransportError::Io(e.to_string())),
            }
        }
        Self::decode_buffered_input(&mut self.input, &mut self.inbox)?;
        Ok(())
    }

    fn decode_buffered_input(
        input: &mut Vec<u8>,
        inbox: &mut VecDeque<Vec<u8>>,
    ) -> Result<(), ShellTransportError> {
        while input.len() >= SOPHIA_IPC_HEADER_LEN && inbox.len() < 64 {
            let payload = u32::from_le_bytes(input[16..20].try_into().unwrap()) as usize;
            if payload > SOPHIA_IPC_MAX_PAYLOAD_LEN {
                return Err(ShellTransportError::Codec(IpcCodecError::PayloadTooLarge(
                    payload,
                )));
            }
            let length = SOPHIA_IPC_HEADER_LEN + payload;
            if input.len() < length {
                break;
            }
            let frame = input.drain(..length).collect::<Vec<_>>();
            sophia_protocol::decode_frame(&frame)?;
            inbox.push_back(frame);
        }
        Ok(())
    }

    pub fn send_async(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        frame: Vec<u8>,
    ) -> Result<(), ShellTransportError> {
        if !self.bulk_capacity_available(epochs, frame.len()) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.output.push(frame, false);
        self.poll_io(epochs)
    }

    pub fn poll_kind(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        kind: sophia_protocol::IpcMessageKind,
    ) -> Result<Option<Vec<u8>>, ShellTransportError> {
        self.poll_io(epochs)?;
        let at = self
            .inbox
            .iter()
            .position(|f| u16::from_le_bytes([f[6], f[7]]) == kind as u16);
        let result = at.and_then(|i| self.inbox.remove(i));
        if result.is_none() && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(result)
    }

    pub fn poll_transaction(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        kind: sophia_protocol::IpcMessageKind,
        tx: TransactionId,
    ) -> Result<Option<Vec<u8>>, ShellTransportError> {
        self.poll_io(epochs)?;
        let at = self.inbox.iter().position(|f| {
            u16::from_le_bytes([f[6], f[7]]) == kind as u16
                && u64::from_le_bytes(f[8..16].try_into().unwrap()) == tx.raw()
        });
        let frame = at.and_then(|i| self.inbox.remove(i));
        if frame.is_none() && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(frame)
    }
}

pub struct ShellClientTransport {
    stream: UnixStream,
    connection_epoch: u64,
}

impl ShellClientTransport {
    pub fn connect(path: impl AsRef<Path>) -> Result<Self, ShellTransportError> {
        let mut stream = UnixStream::connect(path)
            .map_err(|error| ShellTransportError::Io(error.to_string()))?;
        configure_stream(&stream)?;
        let hello = ShellV1ClientHello {
            minimum_revision: SOPHIA_SHELL_INTERFACE_REVISION,
            maximum_revision: SOPHIA_SHELL_INTERFACE_REVISION,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
        };
        write_frame(&mut stream, &encode_shell_v1_client_hello_frame(hello)?)?;
        let welcome = decode_shell_v1_server_welcome_frame(&read_frame(&mut stream)?)?;
        if welcome.selected_revision != SOPHIA_SHELL_INTERFACE_REVISION
            || welcome.capabilities & SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER == 0
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        stream
            .set_read_timeout(None)
            .map_err(|error| ShellTransportError::Io(error.to_string()))?;
        Ok(Self {
            stream,
            connection_epoch: welcome.connection_epoch,
        })
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.connection_epoch
    }

    pub fn receive_snapshot(
        &mut self,
    ) -> Result<(TransactionId, ShellV1DescriptorSnapshot), ShellTransportError> {
        let (transaction, snapshot) =
            decode_shell_v1_descriptor_snapshot_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(snapshot.connection_epoch)?;
        Ok((transaction, snapshot))
    }

    pub fn send_candidate(
        &mut self,
        transaction: TransactionId,
        candidate: &ShellV1Candidate,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(candidate.connection_epoch)?;
        write_frame(
            &mut self.stream,
            &encode_shell_v1_candidate_frame(transaction, candidate)?,
        )
    }

    pub fn receive_candidate_outcome(
        &mut self,
    ) -> Result<(TransactionId, ShellV1CandidateOutcome), ShellTransportError> {
        let (transaction, outcome) =
            decode_shell_v1_candidate_outcome_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(outcome.connection_epoch)?;
        Ok((transaction, outcome))
    }

    pub fn receive_activation(
        &mut self,
    ) -> Result<(TransactionId, ShellV1Activation), ShellTransportError> {
        let (transaction, activation) =
            decode_shell_v1_activation_frame(&read_frame(&mut self.stream)?)?;
        self.require_epoch(activation.connection_epoch)?;
        Ok((transaction, activation))
    }

    pub fn acknowledge_activation(
        &mut self,
        transaction: TransactionId,
        ack: ShellV1ActivationAck,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(ack.connection_epoch)?;
        write_frame(
            &mut self.stream,
            &encode_shell_v1_activation_ack_frame(transaction, ack)?,
        )
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), ShellTransportError> {
        if epoch == self.connection_epoch && epoch != 0 {
            Ok(())
        } else {
            Err(ShellTransportError::InvalidConnectionEpoch)
        }
    }
}

fn configure_stream(stream: &UnixStream) -> Result<(), ShellTransportError> {
    stream
        .set_read_timeout(Some(SHELL_IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(SHELL_IO_TIMEOUT)))
        .map_err(|error| ShellTransportError::Io(error.to_string()))
}

fn write_frame(stream: &mut UnixStream, frame: &[u8]) -> Result<(), ShellTransportError> {
    stream
        .write_all(frame)
        .map_err(|error| ShellTransportError::Io(error.to_string()))
}

fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>, ShellTransportError> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream
        .read_exact(&mut header)
        .map_err(|error| ShellTransportError::Io(error.to_string()))?;
    let payload_len = u32::from_le_bytes(
        header[16..20]
            .try_into()
            .expect("fixed frame payload range is present"),
    ) as usize;
    if payload_len > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(ShellTransportError::Codec(IpcCodecError::PayloadTooLarge(
            payload_len,
        )));
    }
    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .map_err(|error| ShellTransportError::Io(error.to_string()))?;
    Ok(frame)
}
