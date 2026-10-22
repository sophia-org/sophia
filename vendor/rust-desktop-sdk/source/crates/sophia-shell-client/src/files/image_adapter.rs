//! Shell r10 image adapters: owned-ring CPU uploads by default, or explicit DMA capture.
//!
//! Both modes share bounded staged discovery over the session's 9P connection.
//! DMA mode submits ordinary ImageBegin tickets and sends a source only after
//! ImageAdmitted. Exact SourceReleased governs custody independently of status.
//!
//! Time has one explicit origin: image-enabled construction takes
//! `(Instant, origin_ms)`, the caller's monotonic instant and the
//! absolute CLOCK_MONOTONIC milliseconds that instant names. Every
//! session drive and image call converts through that pair;
//! `startup_deadline_ms` in [`ShellImageConfig`] is an absolute
//! deadline in that same caller millisecond domain. A drive instant
//! before the origin, or a millisecond value that overflows, is an
//! argument error and changes nothing.

#[path = "image_dma.rs"]
mod dma;

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sophia_image_producer::{
    ChannelState, CloseCause, ImagePath, ProducerError, ProducerRoleBounds, ReadyEvents,
    ResourceStatusKind, RingProducer, RingProducerConfig,
};

/// Negotiated capability bit 13: sealed image channel (lock plane;
/// shell revision 10 does not use it).
pub const IMAGE_CAPABILITY_SEALED: u64 = 1 << 13;
/// Negotiated capability bit 15: owned-ring image channel, shell
/// revision 10's sole CPU image source.
pub const IMAGE_CAPABILITY_OWNED_RING: u64 = 1 << 15;
/// Negotiated capability bit 14: optional producer-completed DMA-BUF capture.
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
    /// Caller storage bound for the formats object; at least 64 and
    /// never widened past the contract maximum.
    pub formats_capacity: usize,
    /// Owned-ring slots for the one registration: 0 means one slot,
    /// at most 4. The per-slot capacity is the discovered grant's
    /// `max_resource_bytes`, fixed at registration.
    pub ring_slots: u8,
    /// Fixed per-slot capacity in bytes: 0 means the grant's
    /// `max_resource_bytes` rounded down to a multiple of four. A
    /// capacity above the grant is refused locally before any write.
    pub ring_capacity: u64,
    /// The negotiated content grant epoch, from the caller's
    /// `ContentLimits`; part of every ring record's identity.
    pub content_grant_epoch: u64,
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

/// One routed `ResourceStatus` outcome for the producer. Ring
/// settlement matches by grant and resource; `transaction` is kept
/// for diagnosis and never routes by itself.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
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

impl crate::ShellConnection {
    /// Enable ISB discovery on this already negotiated content connection.
    /// Callers may first consume `ContentLimits` and use those bounds and the
    /// transfer timeout to construct `image`. Content and owned-ring capabilities
    /// must already be negotiated; this method grants no capability.
    ///
    /// `origin` and `origin_ms` name the same CLOCK_MONOTONIC instant. Startup
    /// still ends at the supplied absolute deadline. Readiness requires the
    /// authenticated side-channel Hello/ChannelReady exchange. An adapter is
    /// never replaced, including after channel closure.
    pub fn enable_image(
        &mut self,
        image: ShellImageConfig,
        origin: Instant,
        origin_ms: u64,
    ) -> Result<(), ShellClientError> {
        if self.image.is_some() {
            return Err(ShellClientError::InvalidArgument(
                "image adapter already enabled",
            ));
        }
        if self.wire.peer_closed() {
            return Err(ShellClientError::PeerClosed);
        }
        // Shell revision 10's sole CPU image source is the owned ring:
        // revision 10 with the owned-ring capability and content
        // surface. Sealed bit 13 is the lock plane and is not a shell
        // fallback. DMA capture is selected explicitly by enable_dma_image.
        let required = IMAGE_CAPABILITY_OWNED_RING
            | sophia_shell_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        if self.welcome.selected_revision != sophia_shell_protocol::SOPHIA_SHELL_OWNED_RING_REVISION
            || self.welcome.capabilities & required != required
            || self.welcome.capabilities & IMAGE_CAPABILITY_SEALED != 0
        {
            return Err(ShellClientError::MissingCapability);
        }
        if image.transfer_timeout_ms == 0 || image.formats_capacity < 64 {
            return Err(ShellClientError::InvalidArgument("image adapter options"));
        }
        let adapter = ImageAdapter::new(
            image,
            self.welcome.connection_epoch,
            self.welcome.capabilities,
            origin,
            origin_ms,
        )?;
        self.wire.image_enabled = true;
        self.image = Some(adapter);
        Ok(())
    }
    /// Enable producer-completed DMA-BUF capture on an r10 connection that granted bit 14.
    /// Owned-ring remains the default selected by `enable_image`.
    pub fn enable_dma_image(
        &mut self,
        image: ShellImageConfig,
        origin: Instant,
        origin_ms: u64,
    ) -> Result<(), ShellClientError> {
        let required = IMAGE_CAPABILITY_DMA_BUF
            | IMAGE_CAPABILITY_OWNED_RING
            | sophia_shell_protocol::SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        if self.welcome.selected_revision != 10 || self.welcome.capabilities & required != required
        {
            return Err(ShellClientError::MissingCapability);
        }
        let dma = dma::DmaAdapter::new(
            image.clone(),
            self.welcome.connection_epoch,
            self.welcome.capabilities,
            origin,
            origin_ms,
        )?;
        self.enable_image(image, origin, origin_ms)?;
        self.image.as_mut().expect("adapter enabled").dma = Some(dma);
        Ok(())
    }
}

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

    /// Issue the adapter's pending owned-ring control write: walk and
    /// open `owned-ring-v1` once, then write the exact bytes. Progress
    /// is reply-driven; the terminal outcome lands in
    /// `control_outcome` exactly once.
    pub(super) fn start_control_write(&mut self, bytes: Vec<u8>) {
        if self.control_write.is_some() || self.control_outcome.is_some() {
            return;
        }
        if let Some(fid) = self.control_fid {
            let Ok(tag) = self.pipeline.write(fid, 0, &bytes) else {
                self.control_outcome = Some(Err(sophia_9p_records::Errno::EIO.0));
                return;
            };
            self.control_write = Some(crate::files::ControlWrite {
                tag,
                phase: crate::files::ControlPhase::Write,
                fid,
                bytes,
            });
            return;
        }
        let Ok((tag, fid)) = self.pipeline.walk(
            self.root,
            &[sophia_shell_protocol::shell_files::OWNED_RING_CONTROL_FILE],
        ) else {
            self.control_outcome = Some(Err(sophia_9p_records::Errno::EIO.0));
            return;
        };
        self.control_write = Some(crate::files::ControlWrite {
            tag,
            phase: crate::files::ControlPhase::Walk,
            fid,
            bytes,
        });
    }

    pub(super) fn on_control_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        let Some(write) = &self.control_write else {
            return Ok(false);
        };
        if write.tag != tag {
            return Ok(false);
        }
        use crate::files::ControlPhase;
        let write = self.control_write.take().unwrap();
        let finish = |wire: &mut Self, outcome: Result<(), u32>| {
            wire.control_outcome = Some(outcome);
            Ok(true)
        };
        match (write.phase, reply) {
            (ControlPhase::Walk, Reply::Walk(qids)) if qids.len() == 1 => {
                match self.pipeline.lopen(write.fid, 1) {
                    Ok(tag) => {
                        self.control_write = Some(crate::files::ControlWrite {
                            tag,
                            phase: ControlPhase::Open,
                            ..write
                        });
                        Ok(true)
                    }
                    Err(_) => finish(self, Err(sophia_9p_records::Errno::EIO.0)),
                }
            }
            (ControlPhase::Open, Reply::Error(errno)) => {
                // A refused control open clunks the walked fid: the
                // connection keeps no custody of it. The clunk reply
                // is forgettable, like other custody-free clunks.
                let errno = errno.0;
                if let Ok(tag) = self.pipeline.clunk(write.fid) {
                    self.forgettable.insert(tag);
                }
                finish(self, Err(errno))
            }
            (ControlPhase::Open, Reply::Lopen { .. }) => {
                self.control_fid = Some(write.fid);
                match self.pipeline.write(write.fid, 0, &write.bytes) {
                    Ok(tag) => {
                        self.control_write = Some(crate::files::ControlWrite {
                            tag,
                            phase: ControlPhase::Write,
                            ..write
                        });
                        Ok(true)
                    }
                    Err(_) => finish(self, Err(sophia_9p_records::Errno::EIO.0)),
                }
            }
            (ControlPhase::Write, Reply::Write(count)) if *count as usize == write.bytes.len() => {
                finish(self, Ok(()))
            }
            (_, Reply::Error(errno)) => finish(self, Err(errno.0)),
            _ => finish(self, Err(sophia_9p_records::Errno::EIO.0)),
        }
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

/// One queued control-file write: the registration (`offer` is `None`)
/// or one offer record. A definitive refusal spends the offer; `EAGAIN`
/// retries these identical bytes on a later turn.
struct PendingControl {
    bytes: Vec<u8>,
    offer: Option<u64>,
}

/// The adapter state for one image-enabled connection: shell revision
/// 10's owned ring, the shell's sole CPU image source. Queries are on
/// this type; the connection drives it from its session drives.
pub struct ImageAdapter {
    pub(crate) dma: Option<dma::DmaAdapter>,
    config: ShellImageConfig,
    epoch: u64,
    ring: Option<RingProducer>,
    failure: Option<CloseCause>,
    socket: Option<std::os::fd::OwnedFd>,
    hello_sent: bool,
    channel_generation: u64,
    max_resource_bytes: u64,
    pending: Option<PendingControl>,
    origin: Instant,
    origin_ms: u64,
    /// The highest caller millisecond accepted so far: drives and
    /// image calls share one monotonic watermark, so a time that
    /// follows the origin but precedes an earlier accepted call is
    /// still rejected before anything mutates.
    last_ms: u64,
    stage: Stage,
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
        _capabilities: u64,
        origin: Instant,
        origin_ms: u64,
    ) -> Result<Self, ShellClientError> {
        if config.content_grant_epoch == 0 || config.ring_slots > 4 {
            return Err(ShellClientError::InvalidArgument("image ring config"));
        }
        Ok(Self {
            dma: None,
            config,
            epoch: connection_epoch,
            ring: None,
            failure: None,
            socket: None,
            hello_sent: false,
            channel_generation: 0,
            max_resource_bytes: 0,
            pending: None,
            origin,
            origin_ms,
            last_ms: origin_ms,
            stage: Stage::Discovering,
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
        if let Some(dma) = &mut self.dma {
            return dma.advance_clock(now);
        }
        let now_ms = self.now_ms(now)?;
        if now_ms < self.last_ms {
            return Err(ImageAdapterError::Argument);
        }
        self.last_ms = now_ms;
        Ok(now_ms)
    }

    /// Convert a session instant to the caller's millisecond domain.
    pub(crate) fn now_ms(&self, now: Instant) -> Result<u64, ImageAdapterError> {
        if let Some(dma) = &self.dma {
            return dma.now_ms(now);
        }
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

    /// Drive one adapter turn: routed records and statuses, discovery
    /// and connect stages, then the pending control write and source
    /// send. Control writes use bounded synchronous pipeline waits;
    /// replies for other tags stay buffered in arrival order.
    pub(crate) fn tick(
        &mut self,
        wire: &mut FileWire,
        ledger: &crate::custody::Ledger,
        now: Instant,
    ) -> Result<(), ShellClientError> {
        if let Some(dma) = &mut self.dma {
            return dma.tick(wire, ledger, now);
        }
        let now_ms = self
            .advance_clock(now)
            .map_err(|_| ShellClientError::InvalidArgument("image clock"))?;
        if wire.peer_closed() {
            self.on_session_final();
        }
        while let Some(routed) = wire.image_routed.pop_front() {
            let result = match routed {
                Routed::Event(event) => self.on_event(event, now_ms),
                Routed::Status(route) => self.on_status(route, now_ms),
            };
            if let Err(ProducerError::Argument) = result {
                return Err(ShellClientError::InvalidArgument("image clock"));
            }
        }
        if self.lost {
            if let Some(ring) = &mut self.ring {
                ring.journal_lost();
            }
            self.lost = false;
        }
        match self.stage {
            Stage::Discovering => self.discovery(wire, now_ms),
            Stage::Connecting => self.connect(now_ms),
            Stage::Running => {}
        }
        if self.stage != Stage::Running
            && self.failure.is_none()
            && now_ms >= self.config.startup_deadline_ms
        {
            self.failure = Some(CloseCause::Timeout);
            if let Some(ring) = &mut self.ring {
                ring.channel_closed();
            }
            self.stage = Stage::Running;
        }
        if let Some(ring) = &mut self.ring {
            ring.expire(now_ms);
        }
        self.drive_control(wire, now_ms);
        self.drive_source();
        Ok(())
    }

    fn on_event(
        &mut self,
        event: sophia_image_producer::ImageEvent,
        now_ms: u64,
    ) -> Result<(), ProducerError> {
        use sophia_image_producer::ImageEvent;
        match event {
            ImageEvent::Ring(event) => match &mut self.ring {
                Some(ring) => ring.ring_event(event, now_ms),
                None => Err(ProducerError::Refused),
            },
            ImageEvent::ChannelReady {
                channel_generation,
                granted_source_kinds,
            } => {
                if self.ring.is_some()
                    || channel_generation != self.channel_generation
                    || granted_source_kinds
                        != sophia_shell_protocol::shell_files::SHELL_IMAGE_SOURCE_OWNED_RING
                {
                    self.failure = Some(CloseCause::Fault);
                    return Ok(());
                }
                let identity = sophia_shell_protocol::shell_files::RingIdentity {
                    connection_epoch: self.epoch,
                    content_grant_epoch: self.config.content_grant_epoch,
                    channel_generation,
                    ring: 1,
                    generation: 1,
                };
                let mut ring = RingProducer::new(
                    RingProducerConfig {
                        identity,
                        slots: self.config.ring_slots,
                        capacity: self.config.ring_capacity,
                        max_resource_bytes: self.max_resource_bytes,
                        transfer_timeout_ms: self.config.transfer_timeout_ms,
                    },
                    now_ms,
                )?;
                if let Some(bytes) = ring.registration() {
                    self.pending = Some(PendingControl { bytes, offer: None });
                }
                self.ring = Some(ring);
                self.stage = Stage::Running;
                Ok(())
            }
            ImageEvent::ChannelClosed { .. } => {
                if let Some(ring) = &mut self.ring {
                    ring.channel_closed();
                } else {
                    self.failure = Some(CloseCause::Fault);
                }
                Ok(())
            }
            // Sealed-plane records are not used at revision 10.
            ImageEvent::ImageAdmitted { .. } | ImageEvent::SourceReleased { .. } => Ok(()),
        }
    }

    fn on_status(&mut self, route: StatusRoute, now_ms: u64) -> Result<(), ProducerError> {
        let Some(ring) = &mut self.ring else {
            return Ok(());
        };
        if ring
            .offer_for_resource(route.resource, route.generation)
            .is_some()
        {
            ring.resource_status(route.resource, route.generation, route.status, now_ms)
        } else {
            Ok(())
        }
    }

    fn discovery(&mut self, wire: &mut FileWire, now_ms: u64) {
        if self.ring.is_some() || self.failure.is_some() {
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
        match outcome {
            DiscoveryOutcome::Set {
                text,
                limits,
                formats,
            } => {
                self.discovery_text.clone_from(&text);
                let parsed = (|| {
                    use sophia_shell_protocol::shell_files::{
                        ShellFileClass, ShellFileKind, ShellImageDiscovery, ShellImageFormats,
                        ShellImageLimits, decode_shell_file_record_at_revision,
                    };
                    let limits_record = decode_shell_file_record_at_revision(
                        &limits,
                        ShellFileClass::Object,
                        sophia_shell_protocol::SOPHIA_SHELL_OWNED_RING_REVISION,
                    )
                    .ok()?;
                    let formats_record = decode_shell_file_record_at_revision(
                        &formats,
                        ShellFileClass::Object,
                        sophia_shell_protocol::SOPHIA_SHELL_OWNED_RING_REVISION,
                    )
                    .ok()?;
                    if limits_record.header.kind != ShellFileKind::ImageLimits
                        || formats_record.header.kind != ShellFileKind::ImageFormats
                    {
                        return None;
                    }
                    let discovery =
                        ShellImageDiscovery::decode(std::str::from_utf8(&text).ok()?).ok()?;
                    let limits = ShellImageLimits::decode(limits_record.body).ok()?;
                    let formats = ShellImageFormats::decode(formats_record.body).ok()?;
                    Some((discovery, limits, formats))
                })();
                let Some((discovery, limits, formats)) = parsed else {
                    self.failure = Some(CloseCause::Discovery);
                    self.stage = Stage::Running;
                    return;
                };
                if limits.channel_generation != discovery.channel_generation
                    || formats.channel_generation != discovery.channel_generation
                    || formats.limits_generation != discovery.limits_generation
                {
                    // An incoherent set may be a mid-reconnect mix; the
                    // staged reader retries on a later pass.
                    self.back_off(now_ms);
                    return;
                }
                if discovery.connection_epoch != self.epoch
                    || !matches!(discovery.source_kinds, 4 | 6)
                    || limits.limits_generation != discovery.limits_generation
                    || limits.max_resource_bytes == 0
                    || (self.config.bounds.max_resource_bytes != 0
                        && limits.max_resource_bytes > self.config.bounds.max_resource_bytes)
                {
                    self.failure = Some(CloseCause::Discovery);
                    self.stage = Stage::Running;
                    return;
                }
                self.channel_generation = discovery.channel_generation;
                self.max_resource_bytes = limits.max_resource_bytes;
                self.stage = Stage::Connecting;
                self.retry_delay_ms = 0;
                self.retry_at_ms = now_ms;
            }
            DiscoveryOutcome::Absent => {
                self.failure = Some(CloseCause::Absent);
                self.stage = Stage::Running;
            }
            DiscoveryOutcome::Failed => {
                self.failure = Some(CloseCause::Discovery);
                self.stage = Stage::Running;
            }
        }
    }

    fn connect(&mut self, now_ms: u64) {
        if self.hello_sent {
            return;
        }
        if now_ms < self.retry_at_ms {
            return;
        }
        let endpoint = sophia_shell_protocol::shell_files::ShellImageDiscovery::decode(
            std::str::from_utf8(&self.discovery_text).unwrap_or(""),
        )
        .map(|discovery| discovery.endpoint);
        let Ok(endpoint) = endpoint else {
            self.failure = Some(CloseCause::Discovery);
            self.stage = Stage::Running;
            return;
        };
        match sophia_image_producer::connect(&self.config.endpoint_dir, &endpoint) {
            Ok(fd) => {
                let hello = sophia_shell_protocol::shell_files::ShellImagePacket::Hello {
                    connection_epoch: self.epoch,
                    channel_generation: self.channel_generation,
                };
                let sent = hello
                    .encode()
                    .map_err(|_| CloseCause::Fault)
                    .and_then(|bytes| {
                        rustix::net::send(&fd, &bytes, rustix::net::SendFlags::NOSIGNAL)
                            .map_err(|_| CloseCause::Fault)
                    });
                match sent {
                    Ok(_) => {
                        self.socket = Some(fd);
                        self.hello_sent = true;
                    }
                    Err(cause) => {
                        self.failure = Some(cause);
                        self.stage = Stage::Running;
                    }
                }
            }
            Err(ProducerError::Again) => self.back_off(now_ms),
            Err(_) => {
                self.failure = Some(CloseCause::Discovery);
                self.stage = Stage::Running;
            }
        }
    }

    /// Drive the pending control write through the wire's
    /// reply-driven state machine. `EAGAIN` reissues the identical
    /// bytes; any other error definitively refuses the record.
    fn drive_control(&mut self, wire: &mut FileWire, now_ms: u64) {
        if let Some(outcome) = wire.control_outcome.take() {
            match outcome {
                Ok(()) => {
                    self.pending = None;
                }
                Err(11) => {}
                Err(errno) => self.refuse_pending(now_ms, errno),
            }
        }
        if let Some(pending) = &self.pending
            && wire.control_write.is_none()
            && wire.control_outcome.is_none()
        {
            wire.start_control_write(pending.bytes.clone());
        }
    }

    fn refuse_pending(&mut self, now_ms: u64, errno: u32) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if let Some(ring) = &mut self.ring {
            match pending.offer {
                // An offer whose acceptance an event already proved
                // cannot be refused by a late control error: the
                // stream is incoherent and the ring fails.
                Some(offer) if ring.offer(offer).is_some_and(|info| info.status.is_some()) => {
                    ring.control_refused(errno);
                }
                Some(offer) => {
                    let _ = ring.begin_refused(offer, now_ms);
                }
                None => ring.control_refused(errno),
            }
        }
    }

    /// Send the D4F1 source packet with its one descriptor, once the
    /// producer reports it after `RingAdmitted`.
    fn drive_source(&mut self) {
        let Some(ring) = &mut self.ring else {
            return;
        };
        let Some((bytes, fd)) = ring.source_packet() else {
            return;
        };
        let Some(socket) = &self.socket else {
            return;
        };
        use std::os::fd::AsFd;
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
        let rights = [fd];
        if !control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)) {
            return;
        }
        match rustix::net::sendmsg(
            socket.as_fd(),
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            rustix::net::SendFlags::NOSIGNAL,
        ) {
            Ok(_) => {
                let _ = ring.source_sent();
            }
            // A closed image socket fails the ring; Session's reset
            // journals no SlotReleased for live slots.
            Err(error) if error == rustix::io::Errno::PIPE => ring.channel_closed(),
            Err(_) => {}
        }
    }

    /// Queue one owned-ring upload: pixels are copied into a free
    /// slot and the offer record is written to the control file. The
    /// returned offer id is assigned by the producer; the image is
    /// adoptable only once [`ring_offer`](Self::ring_offer) reports
    /// `Settled`.
    pub(crate) fn upload_ring(
        &mut self,
        wire: &mut FileWire,
        upload: sophia_image_producer::RingUpload,
        pixels: &[u8],
        now: Instant,
    ) -> Result<u64, ImageAdapterError> {
        let now_ms = self.advance_clock(now)?;
        if wire.peer_closed() {
            return Err(ImageAdapterError::Closed);
        }
        if self.pending.is_some() {
            return Err(ImageAdapterError::Again);
        }
        let Some(ring) = &mut self.ring else {
            return Err(ImageAdapterError::Closed);
        };
        let (offer, bytes) = ring.upload(upload, pixels, now_ms)?;
        self.pending = Some(PendingControl {
            bytes,
            offer: Some(offer),
        });
        if self.pending.is_none()
            && self
                .ring
                .as_ref()
                .and_then(|ring| ring.offer(offer))
                .is_some_and(|info| info.state == sophia_image_producer::RingOfferState::Refused)
        {
            return Err(ImageAdapterError::Refused);
        }
        Ok(offer)
    }

    /// Direct-fill upload: `fill` writes the pixels into the mapped
    /// slot itself (no producer copy), then the offer is committed
    /// with exactly `byte_len` bytes and queued for the control file.
    pub(crate) fn upload_ring_fill(
        &mut self,
        wire: &mut FileWire,
        upload: sophia_image_producer::RingUpload,
        byte_len: usize,
        now: Instant,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<u64, ImageAdapterError> {
        let now_ms = self.advance_clock(now)?;
        if wire.peer_closed() {
            return Err(ImageAdapterError::Closed);
        }
        if self.pending.is_some() {
            return Err(ImageAdapterError::Again);
        }
        let Some(ring) = &mut self.ring else {
            return Err(ImageAdapterError::Closed);
        };
        let (offer, bytes) = {
            let mut slot = ring.acquire_slot(now_ms)?;
            fill(slot.pixels_mut());
            slot.commit(upload, byte_len, now_ms)?
        };
        self.pending = Some(PendingControl {
            bytes,
            offer: Some(offer),
        });
        Ok(offer)
    }

    /// The session ended: unsettled offers become Unknown, once.
    pub(crate) fn on_session_final(&mut self) {
        if let Some(dma) = &mut self.dma {
            return dma.on_session_final();
        }
        if self.final_notified {
            return;
        }
        self.final_notified = true;
        if let Some(ring) = &mut self.ring {
            ring.journal_lost();
        }
    }

    /// The next instant the caller should drive the connection for
    /// the adapter's sake, if any.
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        if let Some(dma) = &self.dma {
            return dma.next_deadline(now);
        }
        self.now_ms(now).ok()?;
        let mut due: Option<u64> = None;
        if matches!(self.stage, Stage::Discovering | Stage::Connecting) {
            due = Some(self.retry_at_ms);
            let startup = self.config.startup_deadline_ms;
            due = Some(due.map_or(startup, |current| current.min(startup)));
        }
        due.map(|due_ms| self.origin + Duration::from_millis(due_ms.saturating_sub(self.origin_ms)))
    }

    pub fn channel(&self) -> Option<ChannelState> {
        if let Some(dma) = &self.dma {
            return dma.channel();
        }
        if self.failure.is_some() {
            return Some(ChannelState::Closed);
        }
        self.ring.as_ref().map(|ring| {
            if ring.closed() {
                ChannelState::Closed
            } else if ring.ready() {
                ChannelState::Ready
            } else {
                ChannelState::HelloSent
            }
        })
    }

    pub fn cause(&self) -> Option<CloseCause> {
        if let Some(dma) = &self.dma {
            return dma.cause();
        }
        self.ring.as_ref().map(RingProducer::cause).or(self.failure)
    }

    pub fn path(&self) -> Option<ImagePath> {
        if let Some(dma) = &self.dma {
            return dma.path();
        }
        Some(if self.failure.is_some() {
            ImagePath::Error
        } else if self.ring.as_ref().is_some_and(RingProducer::ready) {
            ImagePath::Descriptor
        } else if self.ring.as_ref().is_some_and(RingProducer::closed) {
            ImagePath::Error
        } else {
            ImagePath::Wait
        })
    }

    /// One ring offer, including adoptability (`RingOfferState::Settled`).
    /// DMA custody is settled only after its exact SourceReleased record.
    pub fn dma_offer(&self, offer: u64) -> Option<sophia_image_producer::OfferInfo> {
        self.dma.as_ref().and_then(|dma| dma.offer(offer))
    }

    pub fn dma_offers(&self) -> Vec<sophia_image_producer::OfferInfo> {
        self.dma.as_ref().map_or_else(Vec::new, |dma| dma.offers())
    }

    pub fn ring_offer(&self, offer: u64) -> Option<sophia_image_producer::RingOfferInfo> {
        self.ring.as_ref().and_then(|ring| ring.offer(offer))
    }

    pub fn ring_offers(&self) -> Vec<sophia_image_producer::RingOfferInfo> {
        self.ring
            .as_ref()
            .map(|ring| ring.offers().collect())
            .unwrap_or_default()
    }

    pub fn ring_identity(&self) -> Option<sophia_shell_protocol::shell_files::RingIdentity> {
        self.ring
            .as_ref()
            .map(|ring| ring.registration_info().identity)
    }

    pub fn poll_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        if let Some(dma) = &self.dma {
            return dma.poll_fd();
        }
        use std::os::fd::AsFd;
        self.socket.as_ref().map(|socket| socket.as_fd())
    }

    pub fn poll_events(&self) -> ReadyEvents {
        if let Some(dma) = &self.dma {
            return dma.poll_events();
        }
        ReadyEvents::NONE
    }

    pub(crate) fn ring_forget(&mut self, offer: u64) -> Result<(), ImageAdapterError> {
        let Some(ring) = &mut self.ring else {
            return Err(ImageAdapterError::Closed);
        };
        ring.forget(offer)?;
        Ok(())
    }
}
