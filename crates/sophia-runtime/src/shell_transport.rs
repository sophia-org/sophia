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
mod tabs;
mod wire;
pub use accounting::{ShellContentAccounting, ShellContentShutdown};
pub use content_admission::ShellContentAdmissionPolicy;

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
    /// The current epoch's protected 9P connection.
    wire: Option<Box<files::ShellFileWire>>,
    /// The component's next logical qid, continued across file epochs.
    file_qids: u64,
    negotiation: Option<negotiation_service::PendingNegotiation>,
    capabilities: u64,
    peer_closed: bool,
    /// Session explicitly services input once at the start of each owner pass.
    /// Later role visits share that input and only flush new publications.
    owner_serviced: bool,
    /// Typed Session-to-client records not yet in a wire's custody.
    output: outbox::ShellOutbox,
    /// The last turn left records queued because the journal was full. Only
    /// the peer's progress, which its socket reports, frees that room.
    output_blocked: bool,
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
            owner_serviced: false,
            output: outbox::ShellOutbox::default(),
            output_blocked: false,
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
        if let Some(mut files) = self.wire.take() {
            self.file_qids = files.export().next_qid();
            files.revoke();
        }
        if let Some(next) = self.negotiation.as_ref().and_then(|p| p.file_qids()) {
            self.file_qids = next;
        }
        self.negotiation = None;
        self.owner_serviced = false;
        self.output.clear();
        self.output_blocked = false;
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

    /// Starts an owner pass. Call once per pass, before visiting any roles.
    /// Requests arriving afterwards remain level-ready for the next pass.
    pub fn service_owner_turn(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        self.owner_serviced = false;
        self.poll_io(epochs)?;
        self.owner_serviced = true;
        Ok(())
    }

    /// Number of actual 9P services (input turns and publication flushes) in
    /// the current connection, including negotiation.
    /// This measures transport work, independently of the number of role visits.
    pub fn wire_turn_count(&self) -> u64 {
        self.wire.as_ref().map_or(0, |files| files.turn_count())
    }

    /// Bounded, nonblocking FIFO-to-journal handoff. After `service_owner_turn`
    /// opts into owner passes, role visits consume buffered input and flush new
    /// publications. Standalone callers continue to serve input on every call.
    pub fn poll_io(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        if self.wire.is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        self.flush_indicator_response(epochs)?;
        self.flush_catalog_response(epochs)?;
        self.flush_native_activation(epochs)?;
        self.flush_native_close(epochs)?;
        self.flush_native_accept(epochs)?;
        let (closed, blocked) = match self.wire.as_mut() {
            None => return Err(ShellTransportError::NotConnected),
            Some(files) => Self::turn_files(
                files,
                &mut self.output,
                !self.owner_serviced,
                std::time::Instant::now(),
            )?,
        };
        self.output_blocked = blocked;
        if closed {
            self.peer_closed = true;
        }
        Ok(())
    }

    /// Descriptors whose readiness this owner's next visit consumes, borrowed
    /// for one wait: the active wire, or a pending negotiation's listener or
    /// accepted wire. Nothing here accepts, reads or decides.
    ///
    /// The owner must visit whatever it subscribes, through `poll_io` or
    /// `poll_negotiation`, on its next pass; readiness is level-triggered.
    pub fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        use negotiation_service::Stage;
        use rustix::event::{PollFd, PollFlags};
        if let Some(files) = self.wire.as_ref() {
            return files.poll_fds();
        }
        match self.negotiation.as_ref().map(|pending| &pending.stage) {
            None => Vec::new(),
            Some(Stage::Waiting) => self
                .endpoint
                .accept_readiness()
                .map(|listener| PollFd::from_borrowed_fd(listener, PollFlags::IN))
                .into_iter()
                .collect(),
            Some(Stage::Accepted(stream)) => vec![PollFd::new(stream, PollFlags::IN)],
            Some(Stage::Files(files)) => files.poll_fds(),
        }
    }

    /// Records queued after the wire's last turn that the next turn can move.
    /// They have no descriptor to wake the owner, so it keeps a short wait.
    /// Records held behind a full journal are not counted: the peer frees
    /// that room by acknowledging, and its socket reports that.
    pub fn output_pending(&self) -> bool {
        self.wire.is_some()
            && !self.peer_closed
            && !self.output_blocked
            && self.output.records() != 0
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

    /// File wire service: optionally serve ready 9P requests, move queued
    /// records into the journal, and flush only when a publication needs it.
    /// Returns whether the peer ended its connection, and whether records
    /// remain queued behind a full journal.
    fn turn_files(
        files: &mut files::ShellFileWire,
        output: &mut outbox::ShellOutbox,
        serve_input: bool,
        now: std::time::Instant,
    ) -> Result<(bool, bool), ShellTransportError> {
        if serve_input {
            match files.turn() {
                Err(ShellTransportError::NotConnected) => return Ok((true, false)),
                Err(error) => return Err(error),
                Ok(()) => {}
            }
        }
        let blocked = !Self::drain_file_output(files, output)?;
        // A role-only visit cannot decide that a peer stopped acknowledging:
        // its next acknowledgement may still be waiting in the socket. Judge
        // the deadline only after the owner has serviced input.
        if serve_input {
            files.check_ack_progress(blocked, now)?;
        }
        if !files.publication_pending() {
            return Ok((false, blocked));
        }
        match files.flush() {
            Err(ShellTransportError::NotConnected) => Ok((true, blocked)),
            Err(error) => Err(error),
            Ok(()) => Ok((false, blocked)),
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
