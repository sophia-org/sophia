//! Display-independent client transport for `sophia_shell_v1`.
//!
//! This crate owns framing and bounded socket queues. It grants no authority,
//! renders no pixels and opens no X11 or Wayland connection.

mod candidate;
mod lifecycle;
mod outbox;
pub use lifecycle::*;

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use sophia_protocol::{
    ContentAdmissionRefused, IpcCodecError, IpcMessageKind, SOPHIA_IPC_HEADER_LEN,
    SOPHIA_IPC_MAX_PAYLOAD_LEN, ShellContentRecord, ShellIndicatorActivation,
    ShellIndicatorActivationOutcome, ShellIndicatorSnapshot, ShellV1ClientHello,
    ShellV1ServerWelcome, TransactionId, decode_frame, decode_shell_content_frame,
    decode_shell_indicator_activation_outcome, decode_shell_indicator_snapshot,
    decode_shell_v1_server_welcome_frame, encode_shell_content_frame,
    encode_shell_indicator_activation, encode_shell_v1_client_hello_frame,
};

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
    stream: UnixStream,
    welcome: ShellV1ServerWelcome,
    input: Vec<u8>,
    output: outbox::ClientOutbox,
    inbox: VecDeque<Vec<u8>>,
    peer_closed: bool,
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
        let mut stream = UnixStream::connect(path).map_err(io_error)?;
        stream
            .set_read_timeout(Some(options.handshake_timeout))
            .map_err(io_error)?;
        stream
            .set_write_timeout(Some(options.handshake_timeout))
            .map_err(io_error)?;
        let hello = encode_shell_v1_client_hello_frame(ShellV1ClientHello {
            minimum_revision: options.minimum_revision,
            maximum_revision: options.maximum_revision,
            required_capabilities: options.required_capabilities,
        })?;
        stream.write_all(&hello).map_err(io_error)?;
        let response = read_frame(&mut stream)?;
        let (header, _) = decode_frame(&response)?;
        if header.message_kind == IpcMessageKind::ShellContentAdmissionRefused {
            let (_, record) = decode_shell_content_frame(&response)?;
            let ShellContentRecord::AdmissionRefused(refusal) = record else {
                return Err(ShellClientError::WrongDirection);
            };
            return Err(ShellClientError::AdmissionRefused(refusal));
        }
        let welcome = decode_shell_v1_server_welcome_frame(&response)?;
        if welcome.selected_revision < options.minimum_revision
            || welcome.selected_revision > options.maximum_revision
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        if welcome.capabilities & options.required_capabilities != options.required_capabilities {
            return Err(ShellClientError::MissingCapability);
        }
        stream.set_read_timeout(None).map_err(io_error)?;
        stream.set_write_timeout(None).map_err(io_error)?;
        stream.set_nonblocking(true).map_err(io_error)?;
        Ok(Self {
            stream,
            welcome,
            input: Vec::new(),
            output: outbox::ClientOutbox::default(),
            inbox: VecDeque::new(),
            peer_closed: false,
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
        if !client_record(record) {
            return Err(ShellClientError::WrongDirection);
        }
        let frame = encode_shell_content_frame(transaction, record)?;
        self.output.enqueue(
            vec![frame],
            matches!(record, ShellContentRecord::ActionAck(_)),
        )
    }

    /// Atomically own a bounded group of bulk content records (for example a
    /// complete candidate). Refusal transfers none of the frames.
    pub fn enqueue_content_group(
        &mut self,
        transaction: TransactionId,
        records: &[ShellContentRecord],
    ) -> Result<(), ShellClientError> {
        self.output
            .enqueue(encode_content_group(transaction, records)?, false)
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
        let frames = encode_content_group(transaction, records)?;
        let metadata = candidate::metadata(transaction, records)?;
        self.output.enqueue_after(frames, false, || {
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
        let mut frames = vec![encode_shell_content_frame(
            transaction,
            &ShellContentRecord::ActionAck(ack.clone()),
        )?];
        if let Some((transaction, activation)) = activation {
            if ack.disposition != 1
                || ack.event_id != activation.event_id
                || ack.grant.connection_epoch != activation.connection_epoch
                || ack.output.id != activation.output.raw()
                || ack.target_id != activation.indicator
                || ack.action_id != activation.action
            {
                return Err(ShellClientError::WrongDirection);
            }
            frames.push(encode_shell_indicator_activation(transaction, activation)?);
        }
        self.output.enqueue(frames, true)
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
        let at = self.inbox.iter().position(|frame| {
            decode_frame(frame).is_ok_and(|(header, _)| content_kind(header.message_kind))
        });
        let Some(frame) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.peer_closed {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        let (transaction, record) = decode_shell_content_frame(&frame)?;
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
        let Some(begin) = self.inbox.iter().position(|frame| {
            decode_frame(frame).is_ok_and(|(header, _)| {
                header.message_kind == IpcMessageKind::ShellIndicatorsBegin
            })
        }) else {
            return if self.peer_closed {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        let (header, _) = decode_frame(&self.inbox[begin])?;
        let transaction = header.transaction;
        let end = self
            .inbox
            .iter()
            .enumerate()
            .skip(begin)
            .find_map(|(index, frame)| {
                decode_frame(frame).ok().and_then(|(header, _)| {
                    (header.transaction == transaction
                        && header.message_kind == IpcMessageKind::ShellIndicatorsEnd)
                        .then_some(index)
                })
            });
        let Some(end) = end else {
            return Ok(None);
        };
        let mut frames = Vec::new();
        let mut retained = VecDeque::with_capacity(self.inbox.len());
        for (index, frame) in self.inbox.drain(..).enumerate() {
            if (begin..=end).contains(&index)
                && decode_frame(&frame).is_ok_and(|(header, _)| header.transaction == transaction)
            {
                frames.push(frame);
            } else {
                retained.push_back(frame);
            }
        }
        self.inbox = retained;
        decode_shell_indicator_snapshot(&frames)
            .map(Some)
            .map_err(Into::into)
    }

    /// Queue one activation naming an exact published indicator generation.
    pub fn send_indicator_activation(
        &mut self,
        transaction: TransactionId,
        activation: &ShellIndicatorActivation,
    ) -> Result<(), ShellClientError> {
        let frame = encode_shell_indicator_activation(transaction, activation)?;
        self.output.enqueue(vec![frame], false)?;
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
        let at = self.inbox.iter().position(|frame| {
            decode_frame(frame).is_ok_and(|(header, _)| {
                header.message_kind == IpcMessageKind::ShellIndicatorActivateOutcome
            })
        });
        let Some(frame) = at.and_then(|index| self.inbox.remove(index)) else {
            return if self.peer_closed {
                Err(ShellClientError::PeerClosed)
            } else {
                Ok(None)
            };
        };
        decode_shell_indicator_activation_outcome(&frame)
            .map(Some)
            .map_err(Into::into)
    }

    /// Bounded nonblocking progress. A queue limit is a protocol failure, not
    /// permission to discard an accepted outcome. Each call reads and writes
    /// at most 256 KiB in at most 64 syscalls per direction.
    pub fn poll_io(&mut self) -> Result<(), ShellClientError> {
        let mut remaining = 256 * 1024;
        for _ in 0..64 {
            if remaining == 0 {
                break;
            }
            let Some(bytes) = self.output.front() else {
                break;
            };
            match self.stream.write(&bytes[..bytes.len().min(remaining)]) {
                Ok(0) => return Err(ShellClientError::PeerClosed),
                Ok(written) => {
                    self.output.written(written);
                    remaining -= written;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(io_error(error)),
            }
        }
        for _ in 0..64 {
            self.decode_input()?;
            let retained = self.input.len() + self.inbox.iter().map(Vec::len).sum::<usize>();
            let available = MAX_QUEUED_BYTES.saturating_sub(retained);
            if available == 0 || self.inbox.len() == MAX_QUEUED_FRAMES {
                break;
            }
            let mut bytes = [0u8; 4096];
            let available = available.min(bytes.len());
            match self.stream.read(&mut bytes[..available]) {
                Ok(0) => {
                    self.peer_closed = true;
                    break;
                }
                Ok(read) => self.input.extend_from_slice(&bytes[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(io_error(error)),
            }
            self.decode_input()?;
        }
        Ok(())
    }

    fn decode_input(&mut self) -> Result<(), ShellClientError> {
        while self.input.len() >= SOPHIA_IPC_HEADER_LEN && self.inbox.len() < MAX_QUEUED_FRAMES {
            let payload = u32::from_le_bytes(self.input[16..20].try_into().unwrap()) as usize;
            if payload > SOPHIA_IPC_MAX_PAYLOAD_LEN {
                return Err(ShellClientError::Codec(IpcCodecError::PayloadTooLarge(
                    payload,
                )));
            }
            let frame_len = SOPHIA_IPC_HEADER_LEN + payload;
            if self.input.len() < frame_len {
                break;
            }
            let frame = self.input.drain(..frame_len).collect::<Vec<_>>();
            decode_frame(&frame)?;
            self.inbox.push_back(frame);
        }
        Ok(())
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

fn content_kind(kind: IpcMessageKind) -> bool {
    (IpcMessageKind::ShellContentAdmissionRefused as u16
        ..=IpcMessageKind::ShellContentActionAck as u16)
        .contains(&(kind as u16))
}

fn io_error(error: std::io::Error) -> ShellClientError {
    ShellClientError::Io(error.to_string())
}

fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>, ShellClientError> {
    let mut header = [0u8; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).map_err(io_error)?;
    let payload = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    if payload > SOPHIA_IPC_MAX_PAYLOAD_LEN {
        return Err(ShellClientError::Codec(IpcCodecError::PayloadTooLarge(
            payload,
        )));
    }
    let mut frame = Vec::with_capacity(SOPHIA_IPC_HEADER_LEN + payload);
    frame.extend_from_slice(&header);
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .map_err(io_error)?;
    decode_frame(&frame)?;
    Ok(frame)
}

fn encode_content_group(
    transaction: TransactionId,
    records: &[ShellContentRecord],
) -> Result<Vec<Vec<u8>>, ShellClientError> {
    if records.is_empty() || records.len() > MAX_QUEUED_FRAMES / 2 {
        return Err(ShellClientError::QueueSaturated);
    }
    let mut frames = Vec::with_capacity(records.len());
    let mut bytes = 0usize;
    for record in records {
        if !client_record(record) {
            return Err(ShellClientError::WrongDirection);
        }
        let frame = encode_shell_content_frame(transaction, record)?;
        bytes = bytes.saturating_add(frame.len());
        if bytes > MAX_QUEUED_BYTES {
            return Err(ShellClientError::QueueSaturated);
        }
        frames.push(frame);
    }
    Ok(frames)
}
