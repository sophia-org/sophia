//! Display-independent shell client for the Sophia desktop.
//!
//! The client speaks the `sophia_shell_fs_v1` file contract over 9P2000.L
//! (`files`). It grants no authority, renders no pixels and opens no X11 or
//! Wayland connection. Internals hold whole typed values (see `wire`); the
//! retiring Unix-socket wire (`socket`, feature `ipc-compat`) is the only
//! module that knows a socket frame exists.

mod candidate;
mod custody;
pub use custody::{Admission, Custody, Ticket};
mod catalog;
pub use catalog::{CatalogInbox, CatalogObservation};
mod files;
mod lifecycle;
mod outbox;
pub use lifecycle::*;
#[cfg(feature = "ipc-compat")]
mod socket;
mod wire;

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use sophia_shell_protocol::{
    ContentAdmissionRefused, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellV1ServerWelcome, TransactionId,
};

#[cfg(feature = "ipc-compat")]
use sophia_shell_ipc::IpcCodecError;
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
    /// A `sophia_shell_v1` socket frame failed to encode/decode.
    #[cfg(feature = "ipc-compat")]
    Codec(IpcCodecError),
    /// A `sophia_shell_fs_v1` envelope or record failed to encode/decode.
    FileCodec(sophia_shell_protocol::shell_files::ShellFilePayloadError),
    /// The underlying 9P pipeline failed (I/O, or a protocol violation it
    /// detected in a reply).
    Pipeline(sophia_9p_client::pipeline::PipelineError),
    AdmissionRefused(ContentAdmissionRefused),
    Lifecycle(ContentLifecycleError),
    UnsupportedRevision,
    MissingCapability,
    WrongDirection,
    QueueSaturated,
    PeerClosed,
    /// The current wire has no file-contract shape for this record: a lone
    /// Candidate Begin/Chunk/End outside a group (only a whole candidate is a
    /// file record).
    UnsupportedOnWire,
    /// A local invariant the file wire's own state machine relies on did not
    /// hold (an unexpected 9P reply shape, or an event out of the sequence
    /// the wire's own bookkeeping expected).
    Protocol(&'static str),
    /// `connect_from_env`'s environment selection was invalid.
    Environment(&'static str),
}

impl core::fmt::Display for ShellClientError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ShellClientError {}

#[cfg(feature = "ipc-compat")]
impl From<IpcCodecError> for ShellClientError {
    fn from(error: IpcCodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<sophia_shell_protocol::shell_files::ShellFilePayloadError> for ShellClientError {
    fn from(error: sophia_shell_protocol::shell_files::ShellFilePayloadError) -> Self {
        Self::FileCodec(error)
    }
}

impl From<sophia_shell_protocol::shell_files::ShellFileCodecError> for ShellClientError {
    fn from(error: sophia_shell_protocol::shell_files::ShellFileCodecError) -> Self {
        Self::FileCodec(error.into())
    }
}

impl From<sophia_9p_client::pipeline::PipelineError> for ShellClientError {
    fn from(error: sophia_9p_client::pipeline::PipelineError) -> Self {
        Self::Pipeline(error)
    }
}

/// One admitted shell connection. Calls other than `wait_for_io` are
/// nonblocking after negotiation.
pub struct ShellConnection {
    wire: Wire,
    welcome: ShellV1ServerWelcome,
    output: outbox::ClientOutbox,
    inbox: VecDeque<Inbound>,
    ledger: custody::Ledger,
}

impl ShellConnection {
    /// Connect over the retiring `sophia_shell_v1` socket wire, send exactly
    /// one Hello and validate the selected contract. Built only with the
    /// `ipc-compat` feature, as the rollback path until that wire is removed.
    #[cfg(feature = "ipc-compat")]
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
            ledger: custody::Ledger::default(),
        })
    }

    /// Connect over the native 9P file wire: Pipeline connect, attach, open
    /// the fixed nodes, then negotiate exactly as `connect` does. The
    /// attach's connection epoch (every record header must carry it) is read
    /// from `api` right after attach; see [`parse_shell_files_api_line`].
    pub fn connect_files(
        path: impl AsRef<Path>,
        options: ShellClientOptions,
    ) -> Result<Self, ShellClientError> {
        if options.minimum_revision == 0
            || options.minimum_revision > options.maximum_revision
            || options.handshake_timeout.is_zero()
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        let (wire, welcome, inbox) = files::FileWire::connect(path.as_ref(), &options)?;
        Ok(Self {
            wire: Wire::Files(Box::new(wire)),
            welcome,
            output: outbox::ClientOutbox::default(),
            inbox,
            ledger: custody::Ledger::default(),
        })
    }

    /// Selects a wire from the environment: exactly one of
    /// `SOPHIA_SHELL_9P_SOCKET` (file wire) or `SOPHIA_SHELL_SOCKET` (socket
    /// wire) must be set. Neither, or both, is refused outright: there is no
    /// fallback and no sniffing.
    pub fn connect_from_env(options: ShellClientOptions) -> Result<Self, ShellClientError> {
        let selection = select_env_wire(
            std::env::var_os("SOPHIA_SHELL_SOCKET"),
            std::env::var_os("SOPHIA_SHELL_9P_SOCKET"),
        )?;
        match selection {
            #[cfg(feature = "ipc-compat")]
            EnvWireSelection::Socket(path) => Self::connect(path, options),
            #[cfg(not(feature = "ipc-compat"))]
            EnvWireSelection::Socket(_) => Err(ShellClientError::Environment(
                "SOPHIA_SHELL_SOCKET needs the ipc-compat feature",
            )),
            EnvWireSelection::Files { socket } => Self::connect_files(socket, options),
        }
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
        self.enqueue_content_tracked(transaction, record)
            .map(|_| ())
    }

    /// [`Self::enqueue_content`], returning the admitted unit's ticket (see
    /// [`Self::custody`]).
    pub fn enqueue_content_tracked(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<Admission, ShellClientError> {
        let outbound = Outbound::Content(transaction, record.clone());
        let control = outbound.is_control();
        self.admit(outbound, control)
    }

    /// Encodes one outbound unit and admits all of its wire units, or none.
    fn admit(&mut self, outbound: Outbound, control: bool) -> Result<Admission, ShellClientError> {
        let units = self.wire.encode(outbound)?;
        let admission = self.output.enqueue(units, control, &mut self.ledger)?;
        self.wire.commit_encoded();
        Ok(admission)
    }

    /// What became of an admitted unit, or `None` once its ticket is older
    /// than the most recent 256 (never another unit's outcome). On the file
    /// wire a record reaches [`Custody::Submitted`] only when the session's
    /// `Submitted` event is observed; admission alone is local queueing.
    pub fn custody(&self, ticket: Ticket) -> Option<Custody> {
        self.ledger.get(ticket)
    }

    /// When to call [`Self::poll_io`] again even if the socket has nothing
    /// to read or write: a write the session refused with `EAGAIN` is retried
    /// then (or sooner, once an event is consumed). `None` when nothing waits.
    pub fn wake_deadline(&self) -> Option<std::time::Instant> {
        self.wire.wake_deadline()
    }

    /// Makes one bounded I/O pass, then waits at most `maximum` for readiness.
    /// Drain received units before waiting: a retained unit returns immediately.
    /// Retry deadlines and unfinished bounded local work also shorten the wait.
    /// This never waits for writability on an idle file connection. A signal
    /// may return early; call `poll_io` (or a typed receive) after waking.
    pub fn wait_for_io(&mut self, maximum: Duration) -> Result<(), ShellClientError> {
        self.poll_io()?;
        if !self.inbox.is_empty() {
            return Ok(());
        }
        let maximum = self.wake_deadline().map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(std::time::Instant::now()))
        });
        self.wire.wait_for_io(maximum, &self.output)
    }

    /// Atomically own a bounded group of bulk content records (for example a
    /// complete candidate). Refusal transfers none of the frames.
    pub fn enqueue_content_group(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<(), ShellClientError> {
        self.enqueue_content_group_tracked(transaction, records)
            .map(|_| ())
    }

    /// [`Self::enqueue_content_group`] with its tickets: one on the file wire
    /// (the whole candidate is one record), one per frame on the socket wire.
    pub fn enqueue_content_group_tracked(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<Admission, ShellClientError> {
        self.admit(Outbound::ContentGroup(transaction, records.to_vec()), false)
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
        self.enqueue_candidate_tracked(lifecycle, transaction, records)
            .map(|_| ())
    }

    /// [`Self::enqueue_candidate`] with its tickets.
    pub fn enqueue_candidate_tracked(
        &mut self,
        lifecycle: &mut ContentLifecycle,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<Admission, ShellClientError> {
        let units = self
            .wire
            .encode(Outbound::ContentGroup(transaction, records.to_vec()))?;
        let metadata = candidate::metadata(transaction, records)?;
        let admission = self
            .output
            .enqueue_after(units, false, &mut self.ledger, || {
                lifecycle
                    .register(metadata)
                    .map_err(ShellClientError::Lifecycle)
            })?;
        self.wire.commit_encoded();
        Ok(admission)
    }

    /// Atomically own both ACK and indicator request before committing a UI
    /// effect. No socket I/O follows admission; partial writes retain both.
    pub fn enqueue_indicator_action_response(
        &mut self,
        transaction: TransactionId,
        ack: &sophia_shell_protocol::ContentActionAck,
        activation: Option<(TransactionId, &ShellIndicatorActivation)>,
    ) -> Result<(), ShellClientError> {
        self.enqueue_indicator_action_response_tracked(transaction, ack, activation)
            .map(|_| ())
    }

    /// [`Self::enqueue_indicator_action_response`] with its tickets: the ACK
    /// first, then the activation when there is one.
    pub fn enqueue_indicator_action_response_tracked(
        &mut self,
        transaction: TransactionId,
        ack: &sophia_shell_protocol::ContentActionAck,
        activation: Option<(TransactionId, &ShellIndicatorActivation)>,
    ) -> Result<Admission, ShellClientError> {
        let outbound = Outbound::ActionResponse {
            transaction,
            ack: ack.clone(),
            activation: activation.map(|(transaction, activation)| (transaction, *activation)),
        };
        self.admit(outbound, true)
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
        self.enqueue_indicator_activation_tracked(transaction, activation)?;
        self.poll_io()
    }

    /// Queue one indicator activation without I/O, returning its ticket.
    pub fn enqueue_indicator_activation_tracked(
        &mut self,
        transaction: TransactionId,
        activation: &ShellIndicatorActivation,
    ) -> Result<Admission, ShellClientError> {
        self.admit(
            Outbound::IndicatorActivation(transaction, *activation),
            false,
        )
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
        self.wire
            .poll_io(&mut self.output, &mut self.inbox, &mut self.ledger)
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

#[cfg(feature = "ipc-compat")]
fn io_error(error: std::io::Error) -> ShellClientError {
    ShellClientError::Io(error.to_string())
}

/// What [`ShellConnection::connect_from_env`] selects and what it needs to
/// open it. Exposed so the selection rule below is testable directly, with
/// no process-global environment mutation: `std::env::set_var`/`remove_var`
/// are `unsafe` since edition 2024, and this workspace forbids unsafe code
/// outright.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvWireSelection {
    Socket(std::ffi::OsString),
    Files { socket: std::ffi::OsString },
}

/// The pure selection rule behind [`ShellConnection::connect_from_env`]:
/// exactly one of `socket` (the `SOPHIA_SHELL_SOCKET` value) or
/// `files_socket` (`SOPHIA_SHELL_9P_SOCKET`) must be given. Kept apart from
/// actually reading the environment so it is directly testable; see this
/// crate's `tests/connection.rs`.
pub fn select_env_wire(
    socket: Option<std::ffi::OsString>,
    files_socket: Option<std::ffi::OsString>,
) -> Result<EnvWireSelection, ShellClientError> {
    match (socket, files_socket) {
        (Some(_), Some(_)) => Err(ShellClientError::Environment(
            "both SOPHIA_SHELL_SOCKET and SOPHIA_SHELL_9P_SOCKET are set",
        )),
        (None, None) => Err(ShellClientError::Environment(
            "neither SOPHIA_SHELL_SOCKET nor SOPHIA_SHELL_9P_SOCKET is set",
        )),
        (Some(path), None) => Ok(EnvWireSelection::Socket(path)),
        (None, Some(path)) => Ok(EnvWireSelection::Files { socket: path }),
    }
}

/// The number of bytes `connect_files`/`connect_from_env` will read from
/// `api` before giving up. The real line
/// (`sophia-shell-files version=<u16> role=<role> epoch=<u64>
/// fd_transfer=none\n`) stays well under this, so a well-formed line always
/// reads whole in one 9P read; a read that fills the whole budget is treated
/// as oversize/malformed rather than silently truncated.
pub const SHELL_FILES_API_LINE_MAX_BYTES: u32 = 256;

/// Parses the `api` file's one line strictly: the exact key set and order
/// `sophia-shell-files version=<SHELL_FILE_API_VERSION> role=<role>
/// epoch=<connection epoch> fd_transfer=none`, terminated by exactly one
/// trailing newline and nothing else. Returns the connection epoch every
/// record header of this attach must carry -- the only field the file wire
/// needs from this line.
pub fn parse_shell_files_api_line(bytes: &[u8]) -> Result<u64, ShellClientError> {
    if bytes.len() >= SHELL_FILES_API_LINE_MAX_BYTES as usize {
        return Err(ShellClientError::Protocol("api line oversize"));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| ShellClientError::Protocol("api line not utf-8"))?;
    let line = text
        .strip_suffix('\n')
        .ok_or(ShellClientError::Protocol("api line missing newline"))?;
    let mut fields = line.split(' ');
    if fields.next() != Some("sophia-shell-files") {
        return Err(ShellClientError::Protocol("api line missing family"));
    }
    let version: u16 = fields
        .next()
        .and_then(|field| field.strip_prefix("version="))
        .and_then(|value| value.parse().ok())
        .ok_or(ShellClientError::Protocol("api line missing version"))?;
    if version != sophia_shell_protocol::shell_files::SHELL_FILE_API_VERSION {
        return Err(ShellClientError::Protocol("api line version mismatch"));
    }
    fields
        .next()
        .and_then(|field| field.strip_prefix("role="))
        .ok_or(ShellClientError::Protocol("api line missing role"))?;
    let epoch: u64 = fields
        .next()
        .and_then(|field| field.strip_prefix("epoch="))
        .and_then(|value| value.parse().ok())
        .filter(|epoch| *epoch != 0)
        .ok_or(ShellClientError::Protocol(
            "api line missing a nonzero epoch",
        ))?;
    if fields.next() != Some("fd_transfer=none") {
        return Err(ShellClientError::Protocol("api line missing fd_transfer"));
    }
    if fields.next().is_some() {
        return Err(ShellClientError::Protocol("api line has extra fields"));
    }
    Ok(epoch)
}
