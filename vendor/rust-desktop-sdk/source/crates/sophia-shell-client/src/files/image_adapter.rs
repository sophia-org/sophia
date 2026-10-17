//! The opt-in BAR sealed-image adapter for an r9 shell session.
//!
//! The adapter owns one [`ImageProducer`]: it reads the
//! `image-channel`, `image-limits` and `image-formats` objects through
//! the session's own 9P wire with a dedicated bounded staged reader
//! (one complete set per pass, exact read errno, immutable EOF by a
//! zero-length read, clunk after every file), connects the discovered
//! endpoint, and routes image journal records and offer
//! `ResourceStatus` outcomes to the producer. Image records are
//! consumed by the session and never delivered as content;
//! `ResourceStatus` is routed and still delivered.
//!
//! Time has one explicit origin: image-enabled construction takes
//! `(Instant, origin_ms)`, the caller's monotonic instant and the
//! absolute CLOCK_MONOTONIC milliseconds that instant names. Every
//! session drive and image call converts through that pair;
//! `startup_deadline_ms` in [`ShellImageConfig`] is an absolute
//! deadline in that same caller millisecond domain. A drive instant
//! before the origin, or a millisecond value that overflows, is an
//! argument error and changes nothing.

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sophia_image_producer::{
    ChannelState, CloseCause, ImagePath, ImageProducer, ImageProducerConfig, ImageRole, OfferInfo,
    ProducerError, ProducerMetadata, ProducerRoleBounds, ReadyEvents, ResourceStatusKind,
};

/// Negotiated capability bit 13: sealed image channel.
pub const IMAGE_CAPABILITY_SEALED: u64 = 1 << 13;
/// Negotiated capability bit 14: DMA-BUF image channel. The adapter
/// never requests it and image opt-in refuses it as required.
pub const IMAGE_CAPABILITY_DMA_BUF: u64 = 1 << 14;

/// Caller configuration for the opt-in image adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellImageConfig {
    /// Directory holding the role's endpoint socket node.
    pub endpoint_dir: PathBuf,
    pub bounds: ProducerRoleBounds,
    pub transfer_timeout_ms: u32,
    /// Absolute deadline, in the caller's origin millisecond domain,
    /// for discovery through `ChannelReady`.
    pub startup_deadline_ms: u64,
    pub fallback_bytes: bool,
    /// Caller storage bound for the formats object; at least 64 and
    /// never widened past the contract maximum.
    pub formats_capacity: usize,
}

/// Adapter outcomes, mirroring the producer's result vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageAdapterError {
    Again,
    Busy,
    Closed,
    Exhausted,
    Refused,
    Argument,
    NotEnabled,
}

impl From<ProducerError> for ImageAdapterError {
    fn from(error: ProducerError) -> Self {
        match error {
            ProducerError::Again => Self::Again,
            ProducerError::Busy => Self::Busy,
            ProducerError::Closed => Self::Closed,
            ProducerError::Exhausted => Self::Exhausted,
            ProducerError::Refused => Self::Refused,
            ProducerError::Argument => Self::Argument,
        }
    }
}

/// A refused `image_begin`. `source` is the caller's descriptor on
/// every refusal before the producer or the queue took ownership;
/// it is `None` once either owns (or has dropped) it.
#[derive(Debug)]
pub struct ImageBeginError {
    pub error: ImageAdapterError,
    pub source: Option<OwnedFd>,
}

/// One admitted ImageBegin: its producer offer and session ticket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageBeginAdmission {
    pub offer: u64,
    pub ticket: crate::Ticket,
}

const TEXT_CAP: usize = 2048;
const LIMITS_CAP: usize = 136;
const NAMES: [&[u8]; 3] = [b"image-channel", b"image-limits", b"image-formats"];

/// One routed journal item for the producer, in wire order: image
/// records and offer statuses share one queue so an Accepted status
/// and a SourceReleased in the same journal read reach the producer
/// in the order the session journaled them.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Routed {
    Event(sophia_image_producer::ImageEvent),
    Status(StatusRoute),
}

/// One routed `ResourceStatus` outcome for the producer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StatusRoute {
    /// The event's transaction: status routes only to the offer it
    /// names, with matching resource identity.
    pub transaction: u64,
    pub resource: u64,
    pub generation: u64,
    pub status: ResourceStatusKind,
    pub unavailable: bool,
}

/// What one staged discovery pass produced.
#[derive(Debug)]
pub(crate) enum DiscoveryOutcome {
    Set {
        text: Vec<u8>,
        limits: Vec<u8>,
        formats: Vec<u8>,
    },
    Absent,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClunkAction {
    NextFile,
    Finish,
    Absent,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    NotStarted,
    Walk {
        tag: Tag,
        fid: Fid,
    },
    Open {
        tag: Tag,
        fid: Fid,
    },
    Read {
        tag: Tag,
        fid: Fid,
        iounit: u32,
    },
    Probe {
        tag: Tag,
        fid: Fid,
    },
    /// The file is read (or failed); the reader waits for the clunk
    /// reply before starting the next file or publishing an outcome.
    /// A clunk error fails the whole discovery pass.
    Clunk {
        tag: Tag,
        fid: Fid,
        action: ClunkAction,
    },
}

/// The staged reader for one discovery set. Buffers are per file and
/// capped; the formats cap is the caller's, clamped to the contract.
pub(crate) struct ImageFetch {
    index: usize,
    phase: Phase,
    bufs: [Vec<u8>; 3],
    formats_cap: usize,
}

use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, QidKind, Tag};

use super::{FileWire, O_RDONLY};
use crate::ShellClientError;

impl FileWire {
    pub(crate) fn start_image_fetch(&mut self, formats_capacity: usize) {
        if self.image_fetch.is_some() || self.image_discovery.is_some() {
            return;
        }
        self.image_fetch = Some(ImageFetch {
            index: 0,
            phase: Phase::NotStarted,
            bufs: [Vec::new(), Vec::new(), Vec::new()],
            formats_cap: formats_capacity
                .min(sophia_shell_protocol::shell_files::SHELL_FILE_MAX_IMAGE_FORMATS_BYTES),
        });
        self.service_pending = true;
    }

    fn image_cap(&self, fetch: &ImageFetch) -> usize {
        match fetch.index {
            0 => TEXT_CAP,
            1 => LIMITS_CAP,
            _ => fetch.formats_cap,
        }
    }

    /// One read's count: never past the remaining cap, the node's
    /// iounit, or one maximum-size message.
    fn image_read_count(&self, fetch: &ImageFetch, iounit: u32) -> u32 {
        let remaining = self.image_cap(fetch) - fetch.bufs[fetch.index].len();
        let by_message = self.pipeline.msize().saturating_sub(11).max(1);
        let by_iounit = if iounit == 0 { u32::MAX } else { iounit };
        by_message
            .min(by_iounit)
            .min(u32::try_from(remaining).unwrap_or(u32::MAX))
    }

    pub(super) fn drive_image_fetch(&mut self) -> Result<bool, ShellClientError> {
        let Some(fetch) = &mut self.image_fetch else {
            return Ok(false);
        };
        if fetch.phase != Phase::NotStarted {
            return Ok(false);
        }
        let name = NAMES[fetch.index];
        let (tag, fid) = self.pipeline.walk(self.root, &[name])?;
        if let Some(fetch) = &mut self.image_fetch {
            fetch.phase = Phase::Walk { tag, fid };
        }
        Ok(true)
    }

    /// Queue the clunk and wait for its reply; only then does the
    /// reader advance or publish. A failed clunk send is a protocol
    /// error, a clunk Rlerror becomes a Failed outcome.
    fn begin_image_clunk(&mut self, fid: Fid, action: ClunkAction) -> Result<(), ShellClientError> {
        let tag = self.pipeline.clunk(fid)?;
        if let Some(fetch) = &mut self.image_fetch {
            fetch.phase = Phase::Clunk { tag, fid, action };
        }
        Ok(())
    }

    fn finish_image_clunk(&mut self, action: ClunkAction) {
        match action {
            ClunkAction::NextFile => {
                if let Some(fetch) = &mut self.image_fetch {
                    fetch.index += 1;
                    fetch.phase = Phase::NotStarted;
                }
            }
            ClunkAction::Finish => {
                if let Some(fetch) = self.image_fetch.take() {
                    let [text, limits, formats] = fetch.bufs;
                    self.image_discovery = Some(DiscoveryOutcome::Set {
                        text,
                        limits,
                        formats,
                    });
                }
            }
            ClunkAction::Absent => {
                self.image_fetch = None;
                self.image_discovery = Some(DiscoveryOutcome::Absent);
            }
            ClunkAction::Failed => {
                self.image_fetch = None;
                self.image_discovery = Some(DiscoveryOutcome::Failed);
            }
        }
    }

    /// Exact read errno, mirroring the C adapter: ENOENT on
    /// `image-channel` alone is clean absence; every other read
    /// error - including ESTALE and EAGAIN from the node - is a
    /// fatal discovery error. The only retryable discovery outcome
    /// is a generation mismatch, which the producer reports after a
    /// complete set has been read.
    fn fail_image_fetch(&mut self, errno: Errno, fid: Option<Fid>) -> Result<(), ShellClientError> {
        let action = {
            let fetch = self.image_fetch.as_ref().expect("fetch in flight");
            if fetch.index == 0 && errno == Errno::ENOENT {
                ClunkAction::Absent
            } else {
                ClunkAction::Failed
            }
        };
        match fid {
            Some(fid) => self.begin_image_clunk(fid, action),
            None => {
                self.finish_image_clunk(action);
                Ok(())
            }
        }
    }

    pub(super) fn on_image_fetch_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        let Some(fetch) = &self.image_fetch else {
            return Ok(false);
        };
        let (phase_tag, fid) = match fetch.phase {
            Phase::Walk { tag, fid }
            | Phase::Open { tag, fid }
            | Phase::Read { tag, fid, .. }
            | Phase::Probe { tag, fid }
            | Phase::Clunk { tag, fid, .. } => (tag, fid),
            Phase::NotStarted => return Ok(false),
        };
        if phase_tag != tag {
            return Ok(false);
        }
        if let Phase::Clunk { action, .. } = fetch.phase {
            return match reply {
                Reply::Clunk => {
                    self.finish_image_clunk(action);
                    Ok(true)
                }
                // A clunk error fails the pass whatever it carried.
                Reply::Error(_) => {
                    self.finish_image_clunk(ClunkAction::Failed);
                    Ok(true)
                }
                _ => Err(ShellClientError::Protocol("image discovery clunk refused")),
            };
        }
        if let Reply::Error(errno) = reply {
            let walked = !matches!(fetch.phase, Phase::Walk { .. });
            self.fail_image_fetch(*errno, walked.then_some(fid))?;
            return Ok(true);
        }
        let cap = self.image_cap(fetch);
        match (fetch.phase, reply) {
            (Phase::Walk { .. }, Reply::Walk(qids)) if qids.len() == 1 => {
                if qids[0].kind != QidKind::File {
                    self.begin_image_clunk(fid, ClunkAction::Failed)?;
                    return Ok(true);
                }
                let open = self.pipeline.lopen(fid, O_RDONLY)?;
                self.image_fetch.as_mut().unwrap().phase = Phase::Open { tag: open, fid };
            }
            (Phase::Open { .. }, Reply::Lopen { qid, iounit }) => {
                if qid.kind != QidKind::File {
                    self.begin_image_clunk(fid, ClunkAction::Failed)?;
                    return Ok(true);
                }
                let iounit = *iounit;
                let fetch = self.image_fetch.as_ref().unwrap();
                let phase = if self.image_cap(fetch) == 0 {
                    Phase::Probe {
                        tag: self.pipeline.read(fid, 0, 1)?,
                        fid,
                    }
                } else {
                    let count = self.image_read_count(fetch, iounit);
                    Phase::Read {
                        tag: self.pipeline.read(fid, 0, count)?,
                        fid,
                        iounit,
                    }
                };
                self.image_fetch.as_mut().unwrap().phase = phase;
            }
            (Phase::Read { iounit, .. }, Reply::Read(data)) => {
                if data.is_empty() {
                    // Immutable EOF: the object ended exactly here,
                    // and the clunk reply gates the next file.
                    let action = if fetch.index < 2 {
                        ClunkAction::NextFile
                    } else {
                        ClunkAction::Finish
                    };
                    self.begin_image_clunk(fid, action)?;
                    return Ok(true);
                }
                if fetch.bufs[fetch.index].len() + data.len() > cap {
                    self.begin_image_clunk(fid, ClunkAction::Failed)?;
                    return Ok(true);
                }
                {
                    let fetch = self.image_fetch.as_mut().unwrap();
                    fetch.bufs[fetch.index].extend_from_slice(data);
                }
                let fetch = self.image_fetch.as_ref().unwrap();
                let offset = fetch.bufs[fetch.index].len() as u64;
                let phase = if fetch.bufs[fetch.index].len() == cap {
                    // Exactly the cap: one more byte must find the end.
                    Phase::Probe {
                        tag: self.pipeline.read(fid, offset, 1)?,
                        fid,
                    }
                } else {
                    let count = self.image_read_count(fetch, iounit);
                    Phase::Read {
                        tag: self.pipeline.read(fid, offset, count)?,
                        fid,
                        iounit,
                    }
                };
                self.image_fetch.as_mut().unwrap().phase = phase;
            }
            (Phase::Probe { .. }, Reply::Read(data)) if data.is_empty() => {
                let action = if fetch.index < 2 {
                    ClunkAction::NextFile
                } else {
                    ClunkAction::Finish
                };
                self.begin_image_clunk(fid, action)?;
            }
            (Phase::Probe { .. }, Reply::Read(_)) => {
                self.begin_image_clunk(fid, ClunkAction::Failed)?;
            }
            _ => return Err(ShellClientError::Protocol("image discovery fetch refused")),
        }
        Ok(true)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Discovering,
    Connecting,
    Running,
}

struct TicketSlot {
    ticket: u64,
    offer: u64,
}

/// The adapter state for one image-enabled connection. Queries are
/// on this type; the connection drives it from its session drives.
pub struct ImageAdapter {
    config: ShellImageConfig,
    producer: Option<ImageProducer>,
    origin: Instant,
    origin_ms: u64,
    /// The highest caller millisecond accepted so far: drives and
    /// image calls share one monotonic watermark, so a time that
    /// follows the origin but precedes an earlier accepted call is
    /// still rejected before anything mutates.
    last_ms: u64,
    stage: Stage,
    tickets: [Option<TicketSlot>; 4],
    offer_metadata: [Option<(u64, ProducerMetadata)>; 4],
    pass: u64,
    retry_at_ms: u64,
    retry_delay_ms: u64,
    discovery_text: Vec<u8>,
    lost: bool,
    final_notified: bool,
}

impl ImageAdapter {
    pub(crate) fn new(
        config: ShellImageConfig,
        connection_epoch: u64,
        capabilities: u64,
        origin: Instant,
        origin_ms: u64,
    ) -> Result<Self, ShellClientError> {
        let producer_config = ImageProducerConfig {
            role: ImageRole::Shell,
            revision: 9,
            connection_epoch,
            granted_kinds: u32::from(capabilities & IMAGE_CAPABILITY_SEALED != 0)
                | (u32::from(capabilities & IMAGE_CAPABILITY_DMA_BUF != 0) << 1),
            bounds: config.bounds,
            transfer_timeout_ms: config.transfer_timeout_ms,
            startup_deadline_ms: config.startup_deadline_ms,
            fallback_bytes: config.fallback_bytes,
        };
        // An invalid producer configuration is refused here, at
        // construction: no adapter exists to swallow it into.
        let producer = ImageProducer::new(producer_config, origin_ms)
            .map_err(|_| ShellClientError::InvalidArgument("image producer config"))?;
        let producer = Some(producer);
        Ok(Self {
            config,
            producer,
            origin,
            origin_ms,
            last_ms: origin_ms,
            stage: Stage::Discovering,
            tickets: Default::default(),
            offer_metadata: Default::default(),
            pass: 0,
            retry_at_ms: origin_ms,
            retry_delay_ms: 0,
            discovery_text: Vec::new(),
            lost: false,
            final_notified: false,
        })
    }

    /// Validate `now` against the origin and the shared watermark,
    /// and advance the watermark. Called before any drive or queue
    /// mutation that would act on this time.
    pub(crate) fn advance_clock(&mut self, now: Instant) -> Result<u64, ImageAdapterError> {
        let now_ms = self.now_ms(now)?;
        if now_ms < self.last_ms {
            return Err(ImageAdapterError::Argument);
        }
        self.last_ms = now_ms;
        Ok(now_ms)
    }

    /// Convert a session instant to the caller's millisecond domain.
    pub(crate) fn now_ms(&self, now: Instant) -> Result<u64, ImageAdapterError> {
        let elapsed = now
            .checked_duration_since(self.origin)
            .ok_or(ImageAdapterError::Argument)?;
        let elapsed_ms =
            u64::try_from(elapsed.as_millis()).map_err(|_| ImageAdapterError::Argument)?;
        self.origin_ms
            .checked_add(elapsed_ms)
            .ok_or(ImageAdapterError::Argument)
    }

    fn back_off(&mut self, now_ms: u64) {
        self.retry_delay_ms = match self.retry_delay_ms {
            0 => 4,
            delay if delay < 256 => delay * 2,
            _ => 256,
        };
        self.retry_at_ms = now_ms.saturating_add(self.retry_delay_ms);
    }

    /// Drive one adapter turn: routed records and statuses, ticket
    /// outcomes, discovery/connect stages, then producer dispatch
    /// with real readiness polled (zero wait) on its socket.
    pub(crate) fn tick(
        &mut self,
        wire: &mut FileWire,
        ledger: &crate::custody::Ledger,
        now: Instant,
    ) -> Result<(), ShellClientError> {
        // The clock conversion is validated before anything is
        // drained or mutated: a backwards or overflowing instant is
        // an argument error the session drive surfaces.
        let now_ms = self
            .advance_clock(now)
            .map_err(|_| ShellClientError::InvalidArgument("image clock"))?;
        if wire.peer_closed() {
            self.on_session_final();
        }
        {
            let Some(producer) = &mut self.producer else {
                return Ok(());
            };
            while let Some(routed) = wire.image_routed.pop_front() {
                let result = match routed {
                    Routed::Event(event) => producer.record(wire_epoch(wire), event, now_ms),
                    Routed::Status(route) => {
                        // Only the offer the status names, with the
                        // exact resource identity, is forwarded; any
                        // other status is the application's alone and
                        // never mutates image custody.
                        let named = producer.offer(route.transaction).is_some_and(|info| {
                            info.resource == route.resource
                                && info.resource_generation == route.generation
                        });
                        if named {
                            producer.resource_status(
                                route.resource,
                                route.generation,
                                route.status,
                                route.unavailable,
                                now_ms,
                            )
                        } else {
                            Ok(())
                        }
                    }
                };
                if let Err(ProducerError::Argument) = result {
                    return Err(ShellClientError::InvalidArgument("image clock"));
                }
            }
        }
        self.settle_tickets(ledger, now_ms);
        if self.lost {
            if let Some(producer) = &mut self.producer {
                producer.journal_lost();
            }
            self.lost = false;
        }
        match self.stage {
            Stage::Discovering => self.discovery(wire, now_ms),
            Stage::Connecting => self.connect(now_ms),
            Stage::Running => {}
        }
        let Some(producer) = &mut self.producer else {
            return Ok(());
        };
        if let Some(fd) = producer.poll_fd() {
            let wanted = producer.poll_events();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::from_bits_retain(wanted.0),
            )];
            let zero = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            if rustix::event::poll(&mut fds, Some(&zero)).is_ok() {
                let revents = ReadyEvents(fds[0].revents().bits());
                if let Err(ProducerError::Argument) = producer.dispatch(revents, now_ms) {
                    return Err(ShellClientError::InvalidArgument("image clock"));
                }
            }
        } else if let Err(ProducerError::Argument) = producer.dispatch(ReadyEvents::NONE, now_ms) {
            return Err(ShellClientError::InvalidArgument("image clock"));
        }
        Ok(())
    }

    fn settle_tickets(&mut self, ledger: &crate::custody::Ledger, now_ms: u64) {
        for slot in &mut self.tickets {
            let Some(entry) = slot.as_ref() else { continue };
            let (ticket, offer) = (entry.ticket, entry.offer);
            match ledger.get(crate::Ticket(ticket)) {
                Some(crate::Custody::Submitted) => *slot = None,
                Some(crate::Custody::Refused(_) | crate::Custody::DroppedUnsent) => {
                    if let Some(producer) = &mut self.producer {
                        let _ = producer.begin_refused(offer, now_ms);
                    }
                    *slot = None;
                }
                // Unknown fate, or local history evicted: the C
                // adapter treats both as journal loss.
                Some(crate::Custody::Unknown) | None => {
                    self.lost = true;
                    *slot = None;
                }
                _ => {}
            }
        }
    }

    fn discovery(&mut self, wire: &mut FileWire, now_ms: u64) {
        let Some(producer) = &self.producer else {
            return;
        };
        if producer.channel() != sophia_image_producer::ChannelState::Idle {
            self.stage = Stage::Running;
            return;
        }
        if wire.image_fetch.is_none() && wire.image_discovery.is_none() {
            if now_ms < self.retry_at_ms {
                return;
            }
            wire.start_image_fetch(self.config.formats_capacity);
            return;
        }
        let Some(outcome) = wire.image_discovery.take() else {
            return;
        };
        self.pass += 1;
        let result = match outcome {
            DiscoveryOutcome::Set {
                text,
                limits,
                formats,
            } => {
                self.discovery_text.clone_from(&text);
                // The staged reader fetched whole records; the
                // producer's discovery set carries bodies, with the
                // header epochs supplied separately.
                let parsed = (|| {
                    use sophia_shell_protocol::shell_files::{
                        ShellFileClass, ShellFileKind, decode_shell_file_record_at_revision,
                    };
                    let limits_record =
                        decode_shell_file_record_at_revision(&limits, ShellFileClass::Object, 9)
                            .ok()?;
                    let formats_record =
                        decode_shell_file_record_at_revision(&formats, ShellFileClass::Object, 9)
                            .ok()?;
                    if limits_record.header.kind != ShellFileKind::ImageLimits
                        || formats_record.header.kind != ShellFileKind::ImageFormats
                    {
                        return None;
                    }
                    Some((
                        limits_record.body.to_vec(),
                        formats_record.body.to_vec(),
                        limits_record.header.connection_epoch,
                        formats_record.header.connection_epoch,
                    ))
                })();
                let Some((limits_body, formats_body, limits_epoch, formats_epoch)) = parsed else {
                    match &mut self.producer {
                        Some(producer) => {
                            let _ = producer.discover(
                                sophia_image_producer::DiscoveryRead::Failed,
                                self.pass,
                                now_ms,
                            );
                            self.stage = Stage::Running;
                            return;
                        }
                        None => return,
                    }
                };
                let read = sophia_image_producer::DiscoveryRead::Present {
                    text: &text,
                    limits_body: &limits_body,
                    formats_body: &formats_body,
                    limits_connection_epoch: limits_epoch,
                    formats_connection_epoch: formats_epoch,
                };
                match &mut self.producer {
                    Some(producer) => producer.discover(read, self.pass, now_ms),
                    None => return,
                }
            }
            DiscoveryOutcome::Absent => match &mut self.producer {
                Some(producer) => producer.discover(
                    sophia_image_producer::DiscoveryRead::Absent,
                    self.pass,
                    now_ms,
                ),
                None => return,
            },
            DiscoveryOutcome::Failed => match &mut self.producer {
                Some(producer) => producer.discover(
                    sophia_image_producer::DiscoveryRead::Failed,
                    self.pass,
                    now_ms,
                ),
                None => return,
            },
        };
        match result {
            Ok(()) => {
                self.stage = Stage::Connecting;
                self.retry_delay_ms = 0;
                self.retry_at_ms = now_ms;
            }
            Err(ProducerError::Again) => self.back_off(now_ms),
            Err(_) => self.stage = Stage::Running,
        }
    }

    fn connect(&mut self, now_ms: u64) {
        if now_ms < self.retry_at_ms {
            return;
        }
        let endpoint = sophia_shell_protocol::shell_files::ShellImageDiscovery::decode(
            std::str::from_utf8(&self.discovery_text).unwrap_or(""),
        )
        .map(|discovery| discovery.endpoint);
        let Ok(endpoint) = endpoint else {
            if let Some(producer) = &mut self.producer {
                producer.close();
            }
            self.stage = Stage::Running;
            return;
        };
        match sophia_image_producer::connect(&self.config.endpoint_dir, &endpoint) {
            Ok(fd) => {
                let result = match &mut self.producer {
                    Some(producer) => producer.attach(fd, now_ms).map_err(|(error, fd)| {
                        drop(fd);
                        error
                    }),
                    None => Err(ProducerError::Closed),
                };
                if result.is_err()
                    && let Some(producer) = &mut self.producer
                {
                    producer.close();
                }
            }
            Err(ProducerError::Again) => {
                self.back_off(now_ms);
                return;
            }
            Err(_) => {
                if let Some(producer) = &mut self.producer {
                    producer.close();
                }
            }
        }
        self.stage = Stage::Running;
    }

    /// The session ended: unsettled offers become Unknown, once.
    pub(crate) fn on_session_final(&mut self) {
        if self.final_notified {
            return;
        }
        self.final_notified = true;
        if let Some(producer) = &mut self.producer {
            producer.journal_lost();
        }
    }

    /// The next instant the caller should drive the connection for
    /// the adapter's sake, if any.
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let now_ms = self.now_ms(now).ok()?;
        let mut due: Option<u64> = None;
        if matches!(self.stage, Stage::Discovering | Stage::Connecting) && self.producer.is_some() {
            due = Some(self.retry_at_ms);
        }
        if let Some(producer) = &self.producer
            && let Some(timeout) = producer.timeout(now_ms)
        {
            let at = now_ms.saturating_add(timeout);
            due = Some(due.map_or(at, |current| current.min(at)));
        }
        due.map(|due_ms| self.origin + Duration::from_millis(due_ms.saturating_sub(self.origin_ms)))
    }

    pub fn channel(&self) -> Option<ChannelState> {
        self.producer.as_ref().map(ImageProducer::channel)
    }

    pub fn cause(&self) -> Option<CloseCause> {
        self.producer.as_ref().map(ImageProducer::cause)
    }

    pub fn path(&self) -> Option<ImagePath> {
        self.producer.as_ref().map(ImageProducer::path)
    }

    pub fn offer(&self, offer: u64) -> Option<OfferInfo> {
        self.producer.as_ref().and_then(|p| p.offer(offer))
    }

    pub fn offers(&self) -> Vec<OfferInfo> {
        self.producer
            .as_ref()
            .map(|p| p.offers().collect())
            .unwrap_or_default()
    }

    pub fn poll_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.producer.as_ref().and_then(ImageProducer::poll_fd)
    }

    pub fn poll_events(&self) -> ReadyEvents {
        self.producer
            .as_ref()
            .map_or(ReadyEvents::NONE, ImageProducer::poll_events)
    }

    pub(crate) fn producer_mut(&mut self) -> Option<&mut ImageProducer> {
        self.producer.as_mut()
    }

    pub(crate) fn has_free_ticket(&self) -> bool {
        self.tickets.iter().any(Option::is_none)
    }

    pub(crate) fn register_ticket(
        &mut self,
        ticket: u64,
        offer: u64,
        metadata: ProducerMetadata,
    ) -> Result<(), ImageAdapterError> {
        let Some(slot) = self.tickets.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ImageAdapterError::Busy);
        };
        *slot = Some(TicketSlot { ticket, offer });
        if let Some(entry) = self.offer_metadata.iter_mut().find(|entry| entry.is_none()) {
            *entry = Some((offer, metadata));
        }
        Ok(())
    }

    pub(crate) fn ticket_metadata(&self, offer: u64) -> Option<ProducerMetadata> {
        self.offer_metadata
            .iter()
            .flatten()
            .find(|(id, _)| *id == offer)
            .map(|(_, metadata)| *metadata)
    }

    pub(crate) fn forget_offer(&mut self, offer: u64) {
        for entry in &mut self.offer_metadata {
            if entry.is_some_and(|(id, _)| id == offer) {
                *entry = None;
            }
        }
    }
}

fn wire_epoch(wire: &FileWire) -> u64 {
    wire.epoch
}
