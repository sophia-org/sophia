//! Display-independent shell client for the Sophia desktop.
//!
//! The client speaks the `sophia_shell_fs_v1` file contract over 9P2000.L
//! (`files`). It grants no authority, renders no pixels and opens no X11 or
//! Wayland connection. Internals hold whole typed values (see `wire`).

mod batch;
mod candidate;
pub use batch::{ContentBatchItem, ShellBatchItem};
mod custody;
pub use custody::{Admission, Custody, CustodyLookupError, Ticket};
mod catalog;
pub use catalog::{CatalogInbox, CatalogObservation};
mod descriptor;
mod files;
pub use descriptor::DescriptorObservation;
mod lifecycle;
mod native_launcher;
pub use native_launcher::NativeLauncherObservation;
mod native_lifecycle;
pub use native_lifecycle::{
    NativeActionReservation, NativeActionStatus, NativeAllocationParams, NativeInputReservation,
    NativeInputStatus, NativeLauncherLifecycle, NativeLauncherSession, NativeLifecycleState,
    NativeObligations, NativePresentation, NativeResourceState, NativeScene, NativeSessionResponse,
};
mod outbox;
pub use outbox::{QueueLimits, QueueReservation};
mod reservation;
pub use lifecycle::*;
mod wire;

use std::collections::VecDeque;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;
use std::time::Duration;

pub use sophia_9p_client::pipeline::IoInterest;

use sophia_shell_protocol::{
    ContentAdmissionRefused, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellV1ServerWelcome, TransactionId,
};

use wire::{Inbound, Outbound};

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
    /// Invalid caller input; the dispatch did not change the connection.
    InvalidArgument(&'static str),
    Io(String),
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
    wire: Box<files::FileWire>,
    welcome: ShellV1ServerWelcome,
    output: outbox::ClientOutbox,
    inbox: VecDeque<Inbound>,
    ledger: custody::Ledger,
}

/// Validated shell records whose local admission capacity is held exclusively.
/// No ticket is issued and nothing is sent until `commit`. Drop or `cancel`
/// discards it. The connection cannot be driven or used for another admission
/// while this handle exists. It reserves local capacity, not server acceptance.
///
/// ```compile_fail
/// use sophia_shell_client::ShellConnection;
/// use sophia_shell_protocol::{ShellContentRecord, TransactionId};
/// fn cannot_drive(connection: &mut ShellConnection, tx: TransactionId, record: &ShellContentRecord) {
///     let reservation = connection.reserve_content(tx, record).unwrap();
///     connection.poll_io().unwrap(); // the reservation exclusively borrows it
///     reservation.commit();
/// }
/// ```
#[must_use = "commit the prepared content, or drop it to cancel"]
pub struct ContentReservation<'a> {
    connection: &'a mut ShellConnection,
    prepared: Option<outbox::PreparedQueue>,
}

impl ContentReservation<'_> {
    /// Admit all prepared units with no I/O, allocation or capacity failure.
    /// Tickets begin in `Queued`; later disconnects can still leave them unsent
    /// or with unknown custody. This does not establish semantic acceptance.
    pub fn commit(mut self) -> Admission {
        let prepared = self.prepared.take().expect("reservation consumed once");
        let admission = self
            .connection
            .output
            .commit(prepared, &mut self.connection.ledger);
        self.connection.wire.commit_encoded();
        admission
    }

    /// Release the prepared content without admitting it or issuing tickets.
    pub fn cancel(self) {}
}

impl Drop for ContentReservation<'_> {
    fn drop(&mut self) {
        self.connection.wire.cancel_encoded();
    }
}

impl ShellConnection {
    /// Continue on the exact role pipeline/root returned by launch
    /// authentication, without Tversion or another attach. Takes exclusive
    /// ownership; the root must be the only live fid and all replies drained.
    /// Keep the separate generation-channel owner alive for this launch.
    /// Failure closes this role connection and never falls back to plain attach.
    pub fn from_attached(
        pipeline: sophia_9p_client::pipeline::Pipeline,
        root: sophia_9p_records::Fid,
        options: ShellClientOptions,
    ) -> Result<Self, ShellClientError> {
        if options.minimum_revision == 0
            || options.minimum_revision > options.maximum_revision
            || options.handshake_timeout.is_zero()
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        let (wire, welcome, inbox) = files::FileWire::from_attached(pipeline, root, &options)?;
        Ok(Self {
            wire: Box::new(wire),
            welcome,
            output: outbox::ClientOutbox::default(),
            inbox,
            ledger: custody::Ledger::default(),
        })
    }

    /// Connect over the native 9P file wire: Pipeline connect, attach, open
    /// the fixed nodes, then validate the selected contract. The
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
            wire: Box::new(wire),
            welcome,
            output: outbox::ClientOutbox::default(),
            inbox,
            ledger: custody::Ledger::default(),
        })
    }

    /// Requires `SOPHIA_SHELL_9P_SOCKET`; any retired `SOPHIA_SHELL_SOCKET`
    /// value is refused before connecting.
    pub fn connect_from_env(options: ShellClientOptions) -> Result<Self, ShellClientError> {
        let selection = select_env_wire(
            std::env::var_os("SOPHIA_SHELL_SOCKET"),
            std::env::var_os("SOPHIA_SHELL_9P_SOCKET"),
        )?;
        match selection {
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

    /// Validate and reserve one content operation before a local side effect.
    /// The record is copied; no borrow of application data survives. Commit
    /// consumes the handle; dropping it cancels. Failed or cancelled preparation
    /// issues no custody tickets, though internal submission IDs may be skipped.
    pub fn reserve_content(
        &mut self,
        transaction: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        let outbound = Outbound::Content(transaction, record.clone());
        let control = outbound.is_control();
        self.reserve_outbound(outbound, control)
    }

    /// Prepare a whole content candidate atomically, as for
    /// `enqueue_content_group_tracked`. This does not register lifecycle
    /// metadata; use `enqueue_candidate_tracked` when that is required.
    pub fn reserve_content_group(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        self.reserve_outbound(Outbound::ContentGroup(transaction, records.to_vec()), false)
    }

    fn reserve_outbound(
        &mut self,
        outbound: Outbound,
        control: bool,
    ) -> Result<ContentReservation<'_>, ShellClientError> {
        if self.wire.peer_closed() {
            return Err(ShellClientError::PeerClosed);
        }
        let units = self.wire.encode(outbound)?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(units, control, &mut self.ledger)?;
        Ok(ContentReservation {
            connection: self,
            prepared: Some(prepared),
        })
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

    /// Query custody while distinguishing evicted tickets from unissued ones.
    /// Use tickets only with the connection that issued them. This does no I/O
    /// and remains available after disconnect. `Unavailable` is lost local
    /// history, whereas `Custody::Unknown` is a retained uncertain outcome.
    pub fn custody_result(&self, ticket: Ticket) -> Result<Custody, CustodyLookupError> {
        self.ledger.lookup(ticket)
    }

    /// End locally without I/O. Retain custody and already-decoded observations
    /// for inspection: queued units become DroppedUnsent, a possibly submitted
    /// unit becomes Unknown, and observed Submitted remains Submitted. Repeated
    /// close is harmless. The owned descriptor stays open until this value drops;
    /// unregister it from readiness monitoring and do not use it directly.
    pub fn close(&mut self) {
        self.wire.close(&mut self.output, &mut self.ledger);
    }

    /// When to call [`Self::poll_io`] again even if the socket has nothing
    /// to read or write: a write the session refused with `EAGAIN` is retried
    /// then (or sooner, once an event is consumed). `None` when nothing waits.
    pub fn wake_deadline(&self) -> Option<std::time::Instant> {
        self.wire.wake_deadline()
    }

    /// Interests for a level-triggered external event loop. Call `poll_io`
    /// and drain typed events before sleeping, then recompute after every
    /// operation that changes connection state. If `immediate` is true,
    /// service/drain again without sleeping; otherwise monitor `as_fd()` for
    /// the requested readiness and hangup/errors. Limit the wait by
    /// `wake_deadline()`, even when the socket has no activity. On waking,
    /// call `poll_io`; readiness does not guarantee progress.
    ///
    /// Errors are terminal; stop registering this connection. Already received
    /// typed events can still be drained through the usual receive methods.
    /// This query performs no I/O and does not read the clock.
    pub fn io_interest(&self) -> Result<IoInterest, ShellClientError> {
        let mut interest = self.wire.io_interest()?;
        interest.immediate |= !self.inbox.is_empty();
        Ok(interest)
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
        self.wire.wait_for_io(maximum)
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
        if let Some((_, activation)) = activation
            && (ack.disposition != 1
                || ack.event_id != activation.event_id
                || ack.grant.connection_epoch != activation.connection_epoch
                || ack.output.id != activation.output.raw()
                || ack.target_id != activation.indicator
                || ack.action_id != activation.action)
        {
            return Err(ShellClientError::WrongDirection);
        }
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
    /// permission to discard an accepted outcome. Each call runs at most 32
    /// transport rounds, each limited to 256 KiB and 64 syscalls per direction.
    pub fn poll_io(&mut self) -> Result<(), ShellClientError> {
        self.wire
            .poll_io(&mut self.output, &mut self.inbox, &mut self.ledger, None)
    }

    /// Nonblocking external-loop dispatch, with caller time and a shared
    /// per-call byte budget. At most `byte_budget` bytes and 32 syscall attempts
    /// per direction are spent across all internal rounds (at most 32).
    /// `now` is used for every retry decision in this call, without a clock read.
    /// Successive supplied times must not decrease. Backward time or a value
    /// that cannot represent a 256 ms retry is rejected without state changes.
    /// Zero budget permits local work but no socket I/O. Query `io_interest` and
    /// drain typed events before sleeping, and service again by `wake_deadline`.
    ///
    /// Use `take_*` receive methods with this API: `poll_*` and `wait_for_io`
    /// invoke the default clock/budget driver. If mixing drivers, supplied time
    /// must follow the real `Instant` timeline; simulated time requires using
    /// only `dispatch` and non-driving receive methods after connection setup.
    pub fn dispatch(
        &mut self,
        now: std::time::Instant,
        byte_budget: usize,
    ) -> Result<(), ShellClientError> {
        self.wire.poll_io(
            &mut self.output,
            &mut self.inbox,
            &mut self.ledger,
            Some((now, byte_budget)),
        )
    }
}

/// Borrow the role socket for readiness monitoring only. Do not read, write,
/// close, duplicate or change its flags outside the SDK. The connection keeps
/// ownership; unregister it from an external event loop before dropping it.
impl AsFd for ShellConnection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.wire.as_fd()
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

/// What [`ShellConnection::connect_from_env`] selects and what it needs to
/// open it. Exposed so the selection rule below is testable directly, with
/// no process-global environment mutation: `std::env::set_var`/`remove_var`
/// are `unsafe` since edition 2024, and this workspace forbids unsafe code
/// outright.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvWireSelection {
    Files { socket: std::ffi::OsString },
}

/// The pure selection rule behind [`ShellConnection::connect_from_env`]:
/// `socket` (the retired `SOPHIA_SHELL_SOCKET` value) must be absent and
/// `files_socket` (`SOPHIA_SHELL_9P_SOCKET`) must be given. Kept apart from
/// actually reading the environment so it is directly testable; see this
/// crate's `tests/connection.rs`.
pub fn select_env_wire(
    socket: Option<std::ffi::OsString>,
    files_socket: Option<std::ffi::OsString>,
) -> Result<EnvWireSelection, ShellClientError> {
    match (socket, files_socket) {
        (Some(_), _) => Err(ShellClientError::Environment(
            "SOPHIA_SHELL_SOCKET is retired; use SOPHIA_SHELL_9P_SOCKET",
        )),
        (None, None) => Err(ShellClientError::Environment(
            "SOPHIA_SHELL_9P_SOCKET is missing",
        )),
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
/// record header of this attach must carry.
pub fn parse_shell_files_api_line(bytes: &[u8]) -> Result<u64, ShellClientError> {
    parse_files_api(bytes).map(|(epoch, _)| epoch)
}

fn parse_files_api(bytes: &[u8]) -> Result<(u64, &str), ShellClientError> {
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
    let role = fields
        .next()
        .and_then(|field| field.strip_prefix("role="))
        .filter(|role| {
            !role.is_empty() && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
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
    Ok((epoch, role))
}
