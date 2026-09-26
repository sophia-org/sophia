//! Display-independent client transport for `sophia_shell_v1`.
//!
//! This crate owns framing and bounded socket queues. It grants no authority,
//! renders no pixels and opens no X11 or Wayland connection. Internals hold
//! whole typed values (see `wire`); today's Unix-socket IPC wire is the only
//! implementation of that seam, kept entirely inside `socket`.

mod candidate;
mod catalog;
pub use catalog::{CatalogInbox, CatalogObservation};
mod lifecycle;
mod outbox;
pub use lifecycle::*;
mod socket;
mod wire;

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    ContentAdmissionRefused, IpcCodecError, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellV1ServerWelcome, TransactionId,
};

use wire::{Inbound, Outbound, Wire};

const MAX_QUEUED_BYTES: usize = 2 * 1024 * 1024;
const MAX_QUEUED_FRAMES: usize = 64;

/// Connection setup requested by a shell implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellClientOptions {
    pub minimum_revision: u16,
    pub maximum_revision: u16,
    pub required_capabilities: u64,
    pub handshake_timeout: Duration,
}

/// A negotiated connection failure. Admission refusal is distinct from an I/O
/// failure so a shell can report operator policy without claiming corruption.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellClientError {
    Io(String),
    Codec(IpcCodecError),
    AdmissionRefused(ContentAdmissionRefused),
    Lifecycle(ContentLifecycleError),
    UnsupportedRevision,
    MissingCapability,
    WrongDirection,
    QueueSaturated,
    PeerClosed,
}

impl core::fmt::Display for ShellClientError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShellClientError {}

impl From<IpcCodecError> for ShellClientError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

/// One admitted shell connection. Calls are nonblocking after negotiation.
pub struct ShellConnection {
    wire: Wire,
    welcome: ShellV1ServerWelcome,
    output: outbox::ClientOutbox,
    inbox: VecDeque<Inbound>,
}

impl ShellConnection {
    /// Connect, send exactly one Hello and validate the selected contract.
    pub fn connect(
        path: impl AsRef<Path>,
        options: ShellClientOptions,
    ) -> Result<Self, ShellClientError> {
        if options.minimum_revision == 0
            || options.minimum_revision > options.maximum_revision
            || options.handshake_timeout.is_zero()
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        let (socket, welcome) = socket::SocketWire::connect(path, options)?;
        if welcome.selected_revision < options.minimum_revision
            || welcome.selected_revision > options.maximum_revision
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        if welcome.capabilities & options.required_capabilities != options.required_capabilities {
            return Err(ShellClientError::MissingCapability);
        }
        Ok(Self {
            wire: Wire::Socket(socket),
            welcome,
            output: outbox::ClientOutbox::default(),
            inbox: VecDeque::new(),
        })
    }

    pub const fn welcome(&self) -> ShellV1ServerWelcome {
        self.welcome
    }

    pub const fn connection_epoch(&self) -> u64 {
        self.welcome.connection_epoch
    }

    /// Queue one client-to-session content record, then make bounded progress.
    pub fn send_content(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<(), ShellClientError> {
        self.enqueue_content(transaction, record)?;
        self.poll_io()
    }

    /// Transfer one record to the bounded outbox without doing socket I/O.
    /// Saturation leaves ownership with the caller for a later service turn.
    pub fn enqueue_content(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<(), ShellClientError> {
        let outbound = Outbound::Content(transaction, record.clone());
        let control = outbound.is_control();
        let units = self.wire.encode(outbound)?;
        self.output.enqueue(units, control)
    }

    /// Atomically own a bounded group of bulk content records (for example a
    /// complete candidate). Refusal transfers none of the frames.
    pub fn enqueue_content_group(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<(), ShellClientError> {
        let units = self
            .wire
            .encode(Outbound::ContentGroup(transaction, records.to_vec()))?;
        self.output.enqueue(units, false)
    }

    /// Own a complete candidate and its exact lifecycle metadata together.
    /// Refusal changes neither the candidate watermark nor the outbound FIFO.
    /// This method performs no socket I/O and activates no input targets.
    pub fn enqueue_candidate(
        &mut self,
        lifecycle: &mut ContentLifecycle,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<(), ShellClientError> {
        let units = self
            .wire
            .encode(Outbound::ContentGroup(transaction, records.to_vec()))?;
        let metadata = candidate::metadata(transaction, records)?;
        self.output.enqueue_after(units, false, || {
            lifecycle
                .register(metadata)
                .map_err(ShellClientError::Lifecycle)
        })
    }

    /// Atomically own both ACK and indicator request before committing a UI
    /// effect. No socket I/O follows admission; partial writes retain both.
    pub fn enqueue_indicator_action_response(
        &mut self,
        transaction: TransactionId,
        ack: &sophia_protocol::ContentActionAck,
        activation: Option<(TransactionId, &ShellIndicatorActivation)>,
    ) -> Result<(), ShellClientError> {
        let outbound = Outbound::ActionResponse {
            transaction,
            ack: ack.clone(),
            activation: activation.map(|(transaction, activation)| (transaction, *activation)),
        };
        let units = self.wire.encode(outbound)?;
        self.output.enqueue(units, true)
    }

    /// Take the oldest session-to-client content record while retaining other
    /// shell workflows in their original order for future typed adapters.
    pub fn poll_content(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellClientError> {
        self.poll_io()?;
        self.take_content()
    }

    /// Drain one already-buffered observation without additional socket I/O.
    pub fn take_content(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellContentRecord)>, ShellClientError> {
        let at = self
            .inbox
            .iter()
            .position(|item| matches!(item, Inbound::Content(_, _)));
        let Some(Inbound::Content(transaction, record)) =
            at.and_then(|index| self.inbox.remove(index))
        else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        if !server_record(&record) {
            return Err(ShellClientError::WrongDirection);
        }
        Ok(Some((transaction, record)))
    }

    /// Take one complete revision-6 indicator publication. Frames belonging
    /// to other shell workflows remain queued in their original order.
    pub fn poll_indicators(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorSnapshot)>, ShellClientError> {
        self.poll_io()?;
        self.take_indicators()
    }

    /// Drain one already-buffered observation without additional socket I/O.
    pub fn take_indicators(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorSnapshot)>, ShellClientError> {
        let at = self
            .inbox
            .iter()
            .position(|item| matches!(item, Inbound::Indicators(_, _)));
        let Some(Inbound::Indicators(transaction, snapshot)) =
            at.and_then(|index| self.inbox.remove(index))
        else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        Ok(Some((transaction, snapshot)))
    }

    /// Queue one activation naming an exact published indicator generation.
    pub fn send_indicator_activation(
        &mut self,
        transaction: TransactionId,
        activation: &ShellIndicatorActivation,
    ) -> Result<(), ShellClientError> {
        let units = self
            .wire
            .encode(Outbound::IndicatorActivation(transaction, *activation))?;
        self.output.enqueue(units, false)?;
        self.poll_io()
    }

    /// Take one exact indicator activation result.
    pub fn poll_indicator_activation_outcome(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivationOutcome)>, ShellClientError> {
        self.poll_io()?;
        self.take_indicator_activation_outcome()
    }

    /// Drain one already-buffered observation without additional socket I/O.
    pub fn take_indicator_activation_outcome(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivationOutcome)>, ShellClientError> {
        let at = self
            .inbox
            .iter()
            .position(|item| matches!(item, Inbound::IndicatorOutcome(_, _)));
        let Some(Inbound::IndicatorOutcome(transaction, outcome)) =
            at.and_then(|index| self.inbox.remove(index))
        else {
            return if self.wire.peer_closed() {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        Ok(Some((transaction, outcome)))
    }

    /// Bounded nonblocking progress. A queue limit is a protocol failure, not
    /// permission to discard an accepted outcome. Each call reads and writes
    /// at most 256 KiB in at most 64 syscalls per direction.
    pub fn poll_io(&mut self) -> Result<(), ShellClientError> {
        self.wire.poll_io(&mut self.output, &mut self.inbox)
    }
}

fn client_record(record: &ShellContentRecord) -> bool {
    matches!(
        record,
        ShellContentRecord::AllocationRequest(_)
            | ShellContentRecord::ResourceBegin(_)
            | ShellContentRecord::ResourceChunk(_)
            | ShellContentRecord::ResourceEnd(_)
            | ShellContentRecord::ResourceCancel(_)
            | ShellContentRecord::ResourceRetire(_)
            | ShellContentRecord::CandidateBegin(_)
            | ShellContentRecord::CandidateChunk(_)
            | ShellContentRecord::CandidateEnd(_)
            | ShellContentRecord::FrameDemand(_)
            | ShellContentRecord::FrameDemandCancel(_)
            | ShellContentRecord::ActionAck(_)
    )
}

fn server_record(record: &ShellContentRecord) -> bool {
    matches!(
        record,
        ShellContentRecord::AdmissionRefused(_)
            | ShellContentRecord::Limits(_)
            | ShellContentRecord::OutputFacts(_)
            | ShellContentRecord::AllocationResult(_)
            | ShellContentRecord::ResourceStatus(_)
            | ShellContentRecord::ResourceReleased(_)
            | ShellContentRecord::CandidateOutcome(_)
            | ShellContentRecord::FramePermit(_)
            | ShellContentRecord::Action(_)
    )
}

fn io_error(error: std::io::Error) -> ShellClientError {
    ShellClientError::Io(error.to_string())
}
