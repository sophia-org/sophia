//! The `sophia_shell_v1` socket wire: the one place shell traffic is framed as
//! IPC. Owners hand it typed records through the shared FIFO and receive typed
//! records from it; the frame header, its kinds, byte budgets and partial
//! writes stay here. Deleting this module and `Wire::Socket` removes the socket
//! without touching an owner.
//!
//! Besides typed records, this wire carries the legacy framed descriptor
//! exchange and multi-frame catalog and indicator transfers. They wait in
//! its own lane, stamped from the FIFO's admission
//! sequence, and are charged to the same record and byte budget.
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;

use sophia_protocol::{
    ContentLimits, IpcCodecError, SOPHIA_IPC_HEADER_LEN, SOPHIA_IPC_MAX_PAYLOAD_LEN,
    ShellCatalogActionRecord, ShellCatalogIdentity, ShellIndicatorSnapshot, ShellPersistentCatalog,
    TransactionId, encode_shell_application_catalog, encode_shell_catalog_action_frame,
    encode_shell_content_frame, encode_shell_indicator_activation_outcome,
    encode_shell_indicator_snapshot, encode_shell_native_launcher_frame,
};

use super::ShellTransportError;
use super::outbound::OutboundRecord;
use super::outbox::ShellOutbox;

mod client;
pub(super) mod descriptor;
mod descriptor_records;
mod inbound;
pub(super) mod negotiation;

pub use client::ShellClientTransport;

/// Frames retained in the inbox at once, as the file export's inbound bound.
const INBOX_FRAMES: usize = 64;
/// Input retained before negotiated limits exist.
const UNLIMITED_INPUT_BYTES: usize = 2 * 1024 * 1024;
/// Payload bytes one owner visit may take from the inbox (native, catalog).
const VISIT_PAYLOAD_BYTES: usize = 64 * 1024;

pub(super) struct SocketWire {
    stream: UnixStream,
    /// Bytes read but not yet a complete frame.
    input: Vec<u8>,
    /// Complete, header-checked frames the owners have not taken.
    inbox: VecDeque<Vec<u8>>,
    /// The advertised per-frame payload bound and input queue, fixed for the
    /// epoch by its content limits.
    max_frame_payload: usize,
    max_input_bytes: usize,
    /// Frames only this wire carries, in admission order.
    lane: VecDeque<LaneFrame>,
    lane_bytes: usize,
    lane_bulk: usize,
    lane_controls: usize,
    /// One catalog or indicator publication waiting for bulk capacity.
    publication: VecDeque<Vec<u8>>,
    /// The frame being written. Its FIFO or lane entry stays queued and
    /// charged until the last byte; only this one encoding is held.
    writing: Option<Writing>,
    /// Payload bytes the current owner visit may still take.
    visit: usize,
}

struct LaneFrame {
    sequence: u64,
    bytes: Box<[u8]>,
    control: bool,
}

enum Writing {
    Outbox { frame: Box<[u8]>, written: usize },
    Lane { written: usize },
}

impl SocketWire {
    pub(super) fn new(stream: UnixStream, limits: Option<&ContentLimits>) -> Self {
        Self {
            stream,
            input: Vec::new(),
            inbox: VecDeque::new(),
            max_frame_payload: limits.map_or(SOPHIA_IPC_MAX_PAYLOAD_LEN, |limits| {
                limits.max_frame_payload as usize
            }),
            max_input_bytes: limits.map_or(UNLIMITED_INPUT_BYTES, |limits| {
                limits.max_input_queue_bytes as usize
            }),
            lane: VecDeque::new(),
            lane_bytes: 0,
            lane_bulk: 0,
            lane_controls: 0,
            publication: VecDeque::new(),
            writing: None,
            visit: VISIT_PAYLOAD_BYTES,
        }
    }

    /// The frame a typed record becomes on this wire.
    pub(super) fn encode(record: &OutboundRecord) -> Result<Vec<u8>, ShellTransportError> {
        Ok(match record {
            // Descriptor socket owners retain their original frame lane until
            // their replacement is accepted; native file records never fall back.
            OutboundRecord::Descriptor(_) => return Err(ShellTransportError::WrongContentRecord),
            OutboundRecord::Content(transaction, record) => {
                encode_shell_content_frame(*transaction, record)?
            }
            OutboundRecord::NativeLauncher(transaction, record) => {
                encode_shell_native_launcher_frame(*transaction, record)?
            }
            OutboundRecord::CatalogAction(transaction, record) => {
                encode_shell_catalog_action_frame(*transaction, record)?
            }
            OutboundRecord::IndicatorOutcome(transaction, outcome) => {
                encode_shell_indicator_activation_outcome(*transaction, outcome)?
            }
        })
    }

    /// This wire's own admission bounds for one record, checked before any
    /// owner changes state: the advertised payload bound (a malformed record
    /// for this peer) and, for a control record, the credit's frame size.
    pub(super) fn admits(
        &self,
        record: &OutboundRecord,
        control_limit: Option<(usize, ShellTransportError)>,
    ) -> Result<(), ShellTransportError> {
        let frame = Self::encode(record)?;
        if frame.len() - SOPHIA_IPC_HEADER_LEN > self.max_frame_payload {
            return Err(ShellTransportError::WrongContentRecord);
        }
        if let Some((limit, error)) = control_limit
            && frame.len() > limit
        {
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn lane_records(&self) -> usize {
        self.lane.len()
    }

    pub(super) fn lane_controls(&self) -> usize {
        self.lane_controls
    }

    pub(super) fn lane_bulk_bytes(&self) -> usize {
        self.lane_bulk
    }

    pub(super) fn lane_bytes(&self) -> usize {
        self.lane_bytes
    }

    /// Queues one frame this wire alone carries, after the caller checked
    /// capacity. It takes the next position in the component's output order.
    pub(super) fn push_lane(&mut self, outbox: &mut ShellOutbox, frame: Vec<u8>, control: bool) {
        if frame.is_empty() {
            return;
        }
        let bytes = frame.into_boxed_slice();
        self.lane_bytes += bytes.len();
        if control {
            self.lane_controls += 1;
        } else {
            self.lane_bulk += bytes.len();
        }
        self.lane.push_back(LaneFrame {
            sequence: outbox.next_sequence(),
            bytes,
            control,
        });
    }

    fn pop_lane(&mut self) {
        let frame = self.lane.pop_front().expect("a lane frame");
        self.lane_bytes -= frame.bytes.len();
        if frame.control {
            self.lane_controls -= 1;
        } else {
            self.lane_bulk -= frame.bytes.len();
        }
    }

    pub(super) fn publication_pending(&self) -> bool {
        !self.publication.is_empty()
    }

    pub(super) fn queue_publication(&mut self, frames: Vec<Vec<u8>>) {
        self.publication.extend(frames);
    }

    /// Publication frames still waiting for bulk capacity, and their bytes.
    /// They are owned here but not yet in the output order, so neither the
    /// lane nor any admission budget counts them.
    pub(super) fn pending_publication(&self) -> (usize, usize) {
        (
            self.publication.len(),
            self.publication.iter().map(Vec::len).sum(),
        )
    }

    /// The next publication frame's size, if one waits for bulk capacity.
    pub(super) fn publication_front(&self) -> Option<usize> {
        self.publication.front().map(Vec::len)
    }

    /// Moves the front publication frame into the output order.
    pub(super) fn release_publication_front(&mut self, outbox: &mut ShellOutbox) {
        let frame = self.publication.pop_front().expect("a publication frame");
        self.push_lane(outbox, frame, false);
    }

    /// Nothing read, held or left to take.
    pub(super) fn input_idle(&self) -> bool {
        self.input.is_empty() && self.inbox.is_empty()
    }

    /// Retained inbound records and bytes, partial input included.
    pub(super) fn input_accounting(&self) -> (usize, usize) {
        (
            self.inbox.len(),
            self.input.len() + self.inbox.iter().map(Vec::len).sum::<usize>(),
        )
    }

    /// Starts one owner visit's payload allowance.
    pub(super) fn begin_visit(&mut self) {
        self.visit = VISIT_PAYLOAD_BYTES;
    }

    /// One bounded, nonblocking turn in each direction. Output drains the
    /// typed FIFO and this wire's lane in admission order; a partial write
    /// keeps its entry queued. Returns whether the peer closed its stream.
    pub(super) fn turn(
        &mut self,
        outbox: &mut ShellOutbox,
        byte_budget: usize,
    ) -> Result<bool, ShellTransportError> {
        self.send(outbox, byte_budget.min(256 * 1024))?;
        self.receive(byte_budget.min(256 * 1024))
    }

    /// Writes at most `remaining` bytes of the output order; a record leaves
    /// its queue only with its last byte.
    pub(in crate::shell_transport) fn send(
        &mut self,
        outbox: &mut ShellOutbox,
        mut remaining: usize,
    ) -> Result<(), ShellTransportError> {
        for _ in 0..64 {
            if remaining == 0 {
                break;
            }
            if self.writing.is_none() {
                let typed = outbox.front().map(|queued| queued.sequence);
                let lane = self.lane.front().map(|frame| frame.sequence);
                self.writing = match (typed, lane) {
                    (None, None) => break,
                    (Some(typed), lane) if lane.is_none_or(|lane| typed < lane) => {
                        let queued = outbox.front().expect("typed front");
                        Some(Writing::Outbox {
                            frame: Self::encode(&queued.record)?.into_boxed_slice(),
                            written: 0,
                        })
                    }
                    _ => Some(Writing::Lane { written: 0 }),
                };
            }
            let (bytes, written) = match self.writing.as_ref().expect("selected above") {
                Writing::Outbox { frame, written } => (&frame[..], *written),
                Writing::Lane { written } => {
                    (&self.lane.front().expect("lane front").bytes[..], *written)
                }
            };
            let end = bytes.len().min(written + remaining);
            match self.stream.write(&bytes[written..end]) {
                Ok(0) => return Err(ShellTransportError::NotConnected),
                Ok(count) => {
                    remaining -= count;
                    let complete = written + count == bytes.len();
                    match self.writing.as_mut().expect("selected above") {
                        Writing::Outbox { written, .. } | Writing::Lane { written } => {
                            *written += count;
                        }
                    }
                    if complete {
                        match self.writing.take().expect("selected above") {
                            Writing::Outbox { .. } => {
                                outbox.pop_front();
                            }
                            Writing::Lane { .. } => self.pop_lane(),
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(ShellTransportError::Io(error.to_string())),
            }
        }
        Ok(())
    }

    /// Reads at most `incoming` bytes into complete frames within the input
    /// bound. Returns whether the peer closed its stream.
    pub(in crate::shell_transport) fn receive(
        &mut self,
        mut incoming: usize,
    ) -> Result<bool, ShellTransportError> {
        let mut closed = false;
        for _ in 0..64 {
            self.decode_buffered_input()?;
            let retained = self.input.len() + self.inbox.iter().map(Vec::len).sum::<usize>();
            let available = self.max_input_bytes.saturating_sub(retained);
            if available == 0 || incoming == 0 || self.inbox.len() == INBOX_FRAMES {
                break;
            }
            let mut bytes = [0u8; 4096];
            let available = available.min(bytes.len()).min(incoming);
            match self.stream.read(&mut bytes[..available]) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(count) => {
                    self.input.extend_from_slice(&bytes[..count]);
                    incoming -= count;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(ShellTransportError::Io(error.to_string())),
            }
        }
        self.decode_buffered_input()?;
        Ok(closed)
    }

    fn decode_buffered_input(&mut self) -> Result<(), ShellTransportError> {
        while self.input.len() >= SOPHIA_IPC_HEADER_LEN && self.inbox.len() < INBOX_FRAMES {
            let payload = u32::from_le_bytes(self.input[16..20].try_into().unwrap()) as usize;
            if payload > SOPHIA_IPC_MAX_PAYLOAD_LEN {
                return Err(ShellTransportError::Codec(IpcCodecError::PayloadTooLarge(
                    payload,
                )));
            }
            let length = SOPHIA_IPC_HEADER_LEN + payload;
            if self.input.len() < length {
                break;
            }
            let frame = self.input.drain(..length).collect::<Vec<_>>();
            sophia_protocol::decode_frame(&frame)?;
            self.inbox.push_back(frame);
        }
        Ok(())
    }
}

/// The `ShellIndicatorsBegin`/status/indicator/.../End transfer of one
/// indicator snapshot.
pub(super) fn indicator_frames(
    transaction: TransactionId,
    snapshot: &ShellIndicatorSnapshot,
) -> Result<Vec<Vec<u8>>, ShellTransportError> {
    Ok(encode_shell_indicator_snapshot(transaction, snapshot)?)
}

/// The `ShellApplicationsBegin`/Entry/.../End transfer of one catalog, with one
/// r8 `Identity` record per entry before End when identities are present
/// (exactly `PublishedApplicationCatalog::persistent_frames`).
pub(super) fn catalog_frames(
    transaction: TransactionId,
    catalog: &ShellPersistentCatalog,
) -> Result<Vec<Vec<u8>>, ShellTransportError> {
    let mut frames = encode_shell_application_catalog(transaction, &catalog.catalog)?;
    if catalog.identities.is_empty() {
        return Ok(frames);
    }
    let end = frames
        .pop()
        .ok_or(ShellTransportError::WrongContentRecord)?;
    for entry in &catalog.catalog.entries {
        let identity = catalog
            .identities
            .get(&entry.slot)
            .ok_or(ShellTransportError::WrongContentRecord)?;
        frames.push(encode_shell_catalog_action_frame(
            transaction,
            &ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
                connection_epoch: catalog.catalog.connection_epoch,
                catalog_generation: catalog.catalog.generation,
                slot: entry.slot,
                identity: identity.clone(),
            }),
        )?);
    }
    frames.push(end);
    Ok(frames)
}

/// The message kind of a complete, header-checked frame.
fn kind(frame: &[u8]) -> u16 {
    u16::from_le_bytes([frame[6], frame[7]])
}
