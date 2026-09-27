//! The Unix-socket wire: the only module in this crate that knows a frame
//! exists. Owns the stream, the raw byte input buffer, frame decoding and
//! partial multi-frame assembly (indicator and catalog Begin/.../End), the
//! handshake, and the `Outbound -> Vec<Vec<u8>>` encoder. Everything else in
//! the crate sees only `Outbound`/`Inbound` whole values.

use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;

use sophia_shell_ipc::{
    IpcCodecError, IpcMessageKind, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAX_PAYLOAD_LEN, decode_frame,
    decode_shell_catalog_action_frame, decode_shell_content_frame,
    decode_shell_indicator_activation_outcome, decode_shell_indicator_snapshot,
    decode_shell_persistent_catalog, decode_shell_v1_server_welcome_frame,
    encode_shell_catalog_action_frame, encode_shell_content_frame,
    encode_shell_indicator_activation, encode_shell_v1_client_hello_frame,
};
use sophia_shell_protocol::{
    ContentActionAck, SOPHIA_SHELL_MAX_APPLICATIONS, SOPHIA_SHELL_MAX_INDICATORS,
    SOPHIA_SHELL_MAX_OUTPUT_STATUS, ShellCatalogActionRecord, ShellContentRecord,
    ShellV1ClientHello, ShellV1ServerWelcome, TransactionId,
};

use crate::wire::{Inbound, Outbound};
use crate::{
    MAX_QUEUED_BYTES, MAX_QUEUED_FRAMES, ShellClientError, ShellClientOptions, client_record,
    io_error, outbox::ClientOutbox,
};

const MAX_INDICATOR_ASSEMBLY_FRAMES: usize =
    SOPHIA_SHELL_MAX_INDICATORS + SOPHIA_SHELL_MAX_OUTPUT_STATUS + 2;
const MAX_CATALOG_ASSEMBLY_FRAMES: usize = 2 * SOPHIA_SHELL_MAX_APPLICATIONS + 2;
const MAX_CATALOG_ASSEMBLY_BYTES: usize = 4 * 1024 * 1024;

/// One connection-scoped partial indicator publication in progress.
struct IndicatorAssembly {
    transaction: TransactionId,
    frames: Vec<Vec<u8>>,
}

/// One connection-scoped partial catalog publication in progress.
struct CatalogAssembly {
    transaction: TransactionId,
    frames: Vec<Vec<u8>>,
    bytes: usize,
}

/// A frame kind that never spans more than one frame and always becomes an
/// `Inbound` the moment it is decoded.
fn content_kind(kind: IpcMessageKind) -> bool {
    (IpcMessageKind::ShellContentAdmissionRefused as u16
        ..=IpcMessageKind::ShellContentActionAck as u16)
        .contains(&(kind as u16))
}

pub(crate) struct SocketWire {
    stream: UnixStream,
    input: Vec<u8>,
    indicators: Option<IndicatorAssembly>,
    catalog: Option<CatalogAssembly>,
    peer_closed: bool,
}

impl SocketWire {
    /// Wrap an already set up stream: nonblocking, past any handshake this
    /// wire needs. `connect` is production's only caller; a fixture that
    /// substitutes a bare stream pair for the handshake is the crate's own.
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            input: Vec::new(),
            indicators: None,
            catalog: None,
            peer_closed: false,
        }
    }

    /// Connect, send exactly one Hello and return the raw negotiated
    /// welcome. Revision/capability admission stays with the caller, which
    /// owns `ShellClientOptions`.
    pub(crate) fn connect(
        path: impl AsRef<Path>,
        options: ShellClientOptions,
    ) -> Result<(Self, ShellV1ServerWelcome), ShellClientError> {
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
        let response = read_handshake_frame(&mut stream)?;
        let (header, _) = decode_frame(&response)?;
        if header.message_kind == IpcMessageKind::ShellContentAdmissionRefused {
            let (_, record) = decode_shell_content_frame(&response)?;
            let ShellContentRecord::AdmissionRefused(refusal) = record else {
                return Err(ShellClientError::WrongDirection);
            };
            return Err(ShellClientError::AdmissionRefused(refusal));
        }
        let welcome = decode_shell_v1_server_welcome_frame(&response)?;
        stream.set_read_timeout(None).map_err(io_error)?;
        stream.set_write_timeout(None).map_err(io_error)?;
        stream.set_nonblocking(true).map_err(io_error)?;
        Ok((Self::new(stream), welcome))
    }

    pub(crate) fn peer_closed(&self) -> bool {
        self.peer_closed
    }

    /// Bounded nonblocking progress. A queue limit is a protocol failure, not
    /// permission to discard an accepted outcome. Each call reads and writes
    /// at most 256 KiB in at most 64 syscalls per direction.
    pub(crate) fn poll_io(
        &mut self,
        output: &mut ClientOutbox,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        let mut remaining = 256 * 1024;
        for _ in 0..64 {
            if remaining == 0 {
                break;
            }
            let Some(bytes) = output.front() else {
                break;
            };
            match self.stream.write(&bytes[..bytes.len().min(remaining)]) {
                Ok(0) => return Err(ShellClientError::PeerClosed),
                Ok(written) => {
                    output.written(written);
                    remaining -= written;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(io_error(error)),
            }
        }
        for _ in 0..64 {
            self.decode_input(inbox)?;
            let available = MAX_QUEUED_BYTES.saturating_sub(self.input.len());
            if available == 0 || inbox.len() == MAX_QUEUED_FRAMES {
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
            self.decode_input(inbox)?;
        }
        Ok(())
    }

    /// Chunk complete frames out of the byte buffer and fold each one into
    /// its typed unit: a self-contained frame becomes an `Inbound` at once, a
    /// frame belonging to a multi-frame publication joins the matching
    /// in-progress assembly and only surfaces once its End completes it.
    fn decode_input(&mut self, inbox: &mut VecDeque<Inbound>) -> Result<(), ShellClientError> {
        while self.input.len() >= SOPHIA_IPC_HEADER_LEN && inbox.len() < MAX_QUEUED_FRAMES {
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
            self.reduce_frame(frame, inbox)?;
        }
        Ok(())
    }

    fn reduce_frame(
        &mut self,
        frame: Vec<u8>,
        inbox: &mut VecDeque<Inbound>,
    ) -> Result<(), ShellClientError> {
        let (header, _) = decode_frame(&frame)?;
        if content_kind(header.message_kind) {
            let (transaction, record) = decode_shell_content_frame(&frame)?;
            inbox.push_back(Inbound::Content(transaction, record));
            return Ok(());
        }
        match header.message_kind {
            IpcMessageKind::ShellIndicatorActivateOutcome => {
                let (transaction, outcome) = decode_shell_indicator_activation_outcome(&frame)?;
                inbox.push_back(Inbound::IndicatorOutcome(transaction, outcome));
            }
            IpcMessageKind::ShellIndicatorsBegin => {
                if self.indicators.is_some() {
                    return Err(ShellClientError::WrongDirection);
                }
                self.indicators = Some(IndicatorAssembly {
                    transaction: header.transaction,
                    frames: vec![frame],
                });
            }
            IpcMessageKind::ShellIndicatorsOutputStatus | IpcMessageKind::ShellIndicatorsEntry => {
                let assembly = self
                    .indicators
                    .as_mut()
                    .ok_or(ShellClientError::WrongDirection)?;
                if header.transaction != assembly.transaction {
                    return Err(ShellClientError::WrongDirection);
                }
                if assembly.frames.len() >= MAX_INDICATOR_ASSEMBLY_FRAMES {
                    return Err(ShellClientError::QueueSaturated);
                }
                assembly.frames.push(frame);
            }
            IpcMessageKind::ShellIndicatorsEnd => {
                let mut assembly = self
                    .indicators
                    .take()
                    .ok_or(ShellClientError::WrongDirection)?;
                if header.transaction != assembly.transaction {
                    return Err(ShellClientError::WrongDirection);
                }
                assembly.frames.push(frame);
                let (transaction, snapshot) = decode_shell_indicator_snapshot(&assembly.frames)?;
                inbox.push_back(Inbound::Indicators(transaction, snapshot));
            }
            IpcMessageKind::ShellCatalogActivationOutcome => {
                let (transaction, ShellCatalogActionRecord::ActivationOutcome(outcome)) =
                    decode_shell_catalog_action_frame(&frame)?
                else {
                    return Err(ShellClientError::WrongDirection);
                };
                inbox.push_back(Inbound::CatalogOutcome(transaction, outcome));
            }
            IpcMessageKind::ShellApplicationsBegin => {
                if self.catalog.is_some() {
                    return Err(ShellClientError::WrongDirection);
                }
                let bytes = frame.len();
                self.catalog = Some(CatalogAssembly {
                    transaction: header.transaction,
                    frames: vec![frame],
                    bytes,
                });
            }
            IpcMessageKind::ShellApplicationsEntry | IpcMessageKind::ShellCatalogIdentity => {
                let assembly = self
                    .catalog
                    .as_mut()
                    .ok_or(ShellClientError::WrongDirection)?;
                if header.transaction != assembly.transaction {
                    return Err(ShellClientError::WrongDirection);
                }
                if assembly.frames.len() >= MAX_CATALOG_ASSEMBLY_FRAMES
                    || assembly.bytes.saturating_add(frame.len()) > MAX_CATALOG_ASSEMBLY_BYTES
                {
                    return Err(ShellClientError::QueueSaturated);
                }
                assembly.bytes += frame.len();
                assembly.frames.push(frame);
            }
            IpcMessageKind::ShellApplicationsEnd => {
                let mut assembly = self
                    .catalog
                    .take()
                    .ok_or(ShellClientError::WrongDirection)?;
                if header.transaction != assembly.transaction {
                    return Err(ShellClientError::WrongDirection);
                }
                if assembly.frames.len() >= MAX_CATALOG_ASSEMBLY_FRAMES
                    || assembly.bytes.saturating_add(frame.len()) > MAX_CATALOG_ASSEMBLY_BYTES
                {
                    return Err(ShellClientError::QueueSaturated);
                }
                assembly.frames.push(frame);
                let (transaction, catalog) = decode_shell_persistent_catalog(&assembly.frames)?;
                inbox.push_back(Inbound::Catalog(transaction, catalog));
            }
            _ => return Err(ShellClientError::WrongDirection),
        }
        Ok(())
    }
}

fn read_handshake_frame(stream: &mut UnixStream) -> Result<Vec<u8>, ShellClientError> {
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

/// Turn one whole outbound unit into this wire's frames. Direction and
/// atomic-echo checks that need no live connection state live here, next to
/// the encoding they gate; checks against the connection's own negotiated
/// epoch stay with the `ShellConnection` methods that own that state.
pub(crate) fn encode(outbound: Outbound) -> Result<Vec<Vec<u8>>, ShellClientError> {
    match outbound {
        Outbound::Content(transaction, record) => {
            if !client_record(&record) {
                return Err(ShellClientError::WrongDirection);
            }
            Ok(vec![encode_shell_content_frame(transaction, &record)?])
        }
        Outbound::ContentGroup(transaction, records) => {
            if records.is_empty() || records.len() > MAX_QUEUED_FRAMES / 2 {
                return Err(ShellClientError::QueueSaturated);
            }
            let mut frames = Vec::with_capacity(records.len());
            let mut bytes = 0usize;
            for record in &records {
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
        Outbound::IndicatorActivation(transaction, activation) => {
            Ok(vec![encode_shell_indicator_activation(
                transaction,
                &activation,
            )?])
        }
        Outbound::ActionResponse {
            transaction,
            ack,
            activation,
        } => {
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
                frames.push(encode_shell_indicator_activation(transaction, &activation)?);
            }
            Ok(frames)
        }
        Outbound::CatalogCandidateGroup {
            transaction,
            begin,
            chunks,
            end,
        } => {
            let mut frames = vec![encode_shell_catalog_action_frame(
                transaction,
                &ShellCatalogActionRecord::CandidateBegin(begin),
            )?];
            for chunk in chunks {
                frames.push(encode_shell_catalog_action_frame(
                    transaction,
                    &ShellCatalogActionRecord::CandidateChunk(chunk),
                )?);
            }
            frames.push(encode_shell_content_frame(
                transaction,
                &ShellContentRecord::CandidateEnd(end),
            )?);
            Ok(frames)
        }
        Outbound::CatalogActionResponse {
            transaction,
            ack,
            activation,
        } => {
            let mut frames = vec![encode_shell_content_frame(
                transaction,
                &ShellContentRecord::ActionAck(ack.clone()),
            )?];
            if let Some((transaction, activation)) = activation {
                let action = &activation.action;
                let expected = ContentActionAck {
                    grant: action.grant,
                    output: action.output,
                    candidate_generation: action.candidate_generation,
                    presentation_epoch: action.presentation_epoch,
                    interaction_generation: action.interaction_generation,
                    allocation: action.allocation,
                    target_id: action.target_id,
                    target_generation: action.target_generation,
                    action_id: action.action_id,
                    event_id: action.event_id,
                    disposition: 1,
                };
                if ack != expected {
                    return Err(ShellClientError::WrongDirection);
                }
                frames.push(encode_shell_catalog_action_frame(
                    transaction,
                    &ShellCatalogActionRecord::Activate(activation),
                )?);
            }
            Ok(frames)
        }
    }
}
