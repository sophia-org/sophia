use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    BinaryCodecError, ContentAdmissionRefused, ContentGrant, ContentLimits,
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER, SOPHIA_SHELL_MAX_DESCRIPTORS,
    SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS, ShellV1ClientHello, ShellV1ServerWelcome, TransactionId,
};

use crate::{
    ContentAllocationError, ContentCandidateError, ContentStoreError, PolicyRole,
    ProtectionDomainEvidence, RoleEndpoint, RoleEndpointError,
};

mod accounting;
mod connection;
pub use connection::ShellTransportConnection;
mod launcher;
mod legacy;
pub use launcher::ShellLauncherCandidateEvent;
pub(crate) mod native_launcher;
pub use native_launcher::control::{
    NativeLauncherActivationDecision, NativeLauncherActivationEligibility,
};
mod negotiation;
mod negotiation_policy;
mod negotiation_service;
pub use legacy::ShellSessionTransport;
mod catalog_candidates;
mod catalog_responses;
mod content_actions;
mod content_admission;
mod content_allocations;
mod content_candidates;
mod content_resources;
mod control_budget;
mod descriptor;
mod descriptor_files;
mod descriptor_state;
mod files;
mod indicator_responses;
pub(crate) mod outbound;
mod outbox;
mod publication;
mod reference;
pub use reference::ShellReferenceCandidateEvent;
mod socket;
mod tabs;
mod wire;
pub use accounting::{ShellContentAccounting, ShellContentShutdown};
pub use content_admission::ShellContentAdmissionPolicy;
pub use socket::ShellClientTransport;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellTransportError {
    Endpoint(RoleEndpointError),
    Io(String),
    Codec(BinaryCodecError),
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

impl From<RoleEndpointError> for ShellTransportError {
    fn from(error: RoleEndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<BinaryCodecError> for ShellTransportError {
    fn from(error: BinaryCodecError) -> Self {
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
    endpoint: RoleEndpoint,
    /// The one wire of the current epoch, socket or files.
    wire: Option<wire::Wire>,
    /// The component's next logical qid, continued across file epochs.
    file_qids: u64,
    negotiation: Option<negotiation_service::PendingNegotiation>,
    capabilities: u64,
    peer_closed: bool,
    /// Typed Session-to-client records not yet in a wire's custody.
    output: outbox::ShellOutbox,
    action_cancellations: Vec<sophia_protocol::ContentAction>,
    indicator_response: Option<indicator_responses::PendingIndicatorResponse>,
    catalog_response: Option<catalog_responses::PendingCatalogResponse>,
    native_control: native_launcher::control::NativeControl,
    descriptor_state: descriptor_state::DescriptorState,
    tab_state: descriptor_state::DescriptorState<sophia_protocol::ShellTabSnapshot>,
    reference_state: reference::ReferenceState,
    launcher_state: launcher::LauncherState,
    connection_epoch: u64,
    reserved_limits: Option<ContentLimits>,
    content_grant: Option<ContentGrant>,
    content_limits: Option<ContentLimits>,
    store_grant: ContentGrant,
}

impl ShellComponentTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, ShellTransportError> {
        Ok(Self {
            endpoint: RoleEndpoint::bind_role_for_supervised_uid(
                directory,
                PolicyRole::Shell,
                expected_uid,
            )?,
            wire: None,
            file_qids: 1,
            negotiation: None,
            capabilities: 0,
            peer_closed: false,
            output: outbox::ShellOutbox::default(),
            action_cancellations: Vec::with_capacity(16),
            indicator_response: None,
            catalog_response: None,
            native_control: native_launcher::control::NativeControl::default(),
            descriptor_state: descriptor_state::DescriptorState::default(),
            tab_state: descriptor_state::DescriptorState::default(),
            reference_state: reference::ReferenceState::default(),
            launcher_state: launcher::LauncherState::default(),
            connection_epoch: 0,
            reserved_limits: None,
            content_grant: None,
            content_limits: None,
            store_grant: ContentGrant::default(),
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

    pub fn disconnect(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        if let Some(wire::Wire::Files(mut files)) = self.wire.take() {
            self.file_qids = files.export().next_qid();
            files.revoke();
        }
        if let Some(next) = self.negotiation.as_ref().and_then(|p| p.file_qids()) {
            self.file_qids = next;
        }
        self.negotiation = None;
        self.output.clear();
        self.action_cancellations.clear();
        self.indicator_response = None;
        self.catalog_response = None;
        self.native_control = native_launcher::control::NativeControl::default();
        self.descriptor_state = descriptor_state::DescriptorState::default();
        self.tab_state = descriptor_state::DescriptorState::default();
        self.reference_state = reference::ReferenceState::default();
        self.launcher_state = launcher::LauncherState::default();
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

    pub const fn supports_content_discrete_input(&self) -> bool {
        self.content_grant.is_some()
            && self.capabilities & sophia_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                != 0
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
        self.poll_io_bounded(epochs, 256 * 1024)
    }

    /// Bound each I/O direction, preserving partial framing and FIFO custody.
    /// Legacy service keeps its previous 256 KiB limits; native visits use 64 KiB.
    pub(super) fn poll_io_bounded(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        byte_budget: usize,
    ) -> Result<(), ShellTransportError> {
        if self.wire.is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        self.flush_indicator_response(epochs)?;
        self.flush_catalog_response(epochs)?;
        self.flush_native_activation(epochs)?;
        self.flush_native_close(epochs)?;
        self.flush_native_accept(epochs)?;
        self.flush_publication(epochs);
        let closed = match self.wire.as_mut() {
            None => return Err(ShellTransportError::NotConnected),
            Some(wire::Wire::Socket(socket)) => socket.turn(&mut self.output, byte_budget)?,
            Some(wire::Wire::Files(files)) => Self::turn_files(files, &mut self.output)?,
        };
        if closed {
            self.peer_closed = true;
        }
        Ok(())
    }

    /// Moves queued records into the journal, or publishes queued objects, in
    /// FIFO order. `Ok(false)` when the journal is full and records remain
    /// queued, still owned and charged.
    fn drain_file_output(
        files: &mut files::ShellFileWire,
        output: &mut outbox::ShellOutbox,
    ) -> Result<bool, ShellTransportError> {
        while let Some(queued) = output.front() {
            let (kind, body) = queued.record.native()?;
            let taken = if sophia_protocol::shell_files::shell_file_class(kind)
                == sophia_protocol::shell_files::ShellFileClass::Object
            {
                files.publish(kind, &body, queued.control)?
            } else {
                files.append(kind, &body, queued.control)?
            };
            if !taken {
                return Ok(false);
            }
            output.pop_front();
        }
        Ok(true)
    }

    /// File wire service: serve ready 9P requests, move queued records into
    /// the journal in FIFO order, then serve again so waiting reads see them.
    /// Returns whether the peer ended its connection.
    fn turn_files(
        files: &mut files::ShellFileWire,
        output: &mut outbox::ShellOutbox,
    ) -> Result<bool, ShellTransportError> {
        match files.turn() {
            Err(ShellTransportError::NotConnected) => return Ok(true),
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        let blocked = !Self::drain_file_output(files, output)?;
        files.check_ack_progress(blocked, std::time::Instant::now())?;
        match files.turn() {
            Err(ShellTransportError::NotConnected) => Ok(true),
            Err(error) => Err(error),
            Ok(()) => Ok(false),
        }
    }

    /// A taken client content record with its direction and grant checks.
    /// None ends the peer's stream once it closed.
    pub(super) fn admit_client_record(
        &self,
        taken: Option<(TransactionId, sophia_protocol::ShellContentRecord)>,
    ) -> Result<Option<(TransactionId, sophia_protocol::ShellContentRecord)>, ShellTransportError>
    {
        let Some((transaction, record)) = taken else {
            return self.nothing_inbound();
        };
        if !content_admission::client_record(&record) {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if content_admission::record_grant(&record) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(Some((transaction, record)))
    }
}
