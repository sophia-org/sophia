use crate::image_read::ImageRead;
use sophia_image_producer::{
    ChannelState, CloseCause, DiscoveryRead, ImagePath, ImageProducer, ImageProducerConfig,
    ImageRole, ProducerError, ProducerMetadata, ProducerRoleBounds, ReadyEvents,
};
use sophia_lock_protocol::lock_files::*;
use std::{
    os::fd::{BorrowedFd, OwnedFd},
    path::PathBuf,
};

/// Opt-in sealed-image configuration. All times share Session's caller-owned
/// monotonic millisecond domain. No DMA capability is requested by this adapter.
#[derive(Clone, Debug)]
pub struct LockImageConfig {
    pub endpoint_dir: PathBuf,
    pub startup_deadline_ms: u64,
    pub transfer_timeout_ms: u32,
    pub formats_capacity: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageError {
    Producer(ProducerError),
    NotEnabled,
    NotReady,
    Busy,
    Protocol,
}
impl From<ProducerError> for ImageError {
    fn from(e: ProducerError) -> Self {
        Self::Producer(e)
    }
}
/// Failure before producer custody always returns the caller's source.
#[derive(Debug)]
pub struct ImageBeginError {
    pub error: ImageError,
    pub source: Option<OwnedFd>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageAdmission {
    pub offer: u64,
    pub submission: u64,
}

pub struct LockImageAdapter {
    pub(crate) producer: ImageProducer,
    config: LockImageConfig,
    pub(crate) read: Option<ImageRead>,
    index: usize,
    objects: [Vec<u8>; 3],
    pass: u64,
    retry_at: u64,
    retry_ms: u64,
    endpoint: Option<String>,
    selected: bool,
    unavailable: bool,
    pub(crate) begin_submission: Option<(u64, u64)>,
    pub(crate) metadata: [Option<ProducerMetadata>; 4],
}
impl LockImageAdapter {
    pub(crate) fn new(
        config: LockImageConfig,
        epoch: u64,
        limits: LockFileLimits,
        now: u64,
    ) -> Result<Self, ImageError> {
        if config.formats_capacity < 64 || config.endpoint_dir.as_os_str().is_empty() {
            return Err(ProducerError::Argument.into());
        }
        let producer = ImageProducer::new(
            ImageProducerConfig {
                role: ImageRole::Lock,
                revision: 2,
                connection_epoch: epoch,
                granted_kinds: 1,
                bounds: ProducerRoleBounds {
                    max_resource_bytes: limits.max_resource_bytes,
                    max_width: limits.max_width_px,
                    max_height: limits.max_height_px,
                    max_admitted_outputs: u32::from(limits.max_outputs),
                    transfer_slots: u32::from(limits.upload_slots),
                },
                transfer_timeout_ms: config.transfer_timeout_ms,
                startup_deadline_ms: config.startup_deadline_ms,
            },
            now,
        )?;
        Ok(Self {
            producer,
            config,
            read: None,
            index: 0,
            objects: std::array::from_fn(|_| Vec::new()),
            pass: 0,
            retry_at: now,
            retry_ms: 4,
            endpoint: None,
            selected: false,
            unavailable: false,
            begin_submission: None,
            metadata: [None; 4],
        })
    }
    pub fn producer(&self) -> &ImageProducer {
        &self.producer
    }
    pub fn path(&self) -> ImagePath {
        if self.unavailable {
            ImagePath::Error
        } else {
            self.producer.path()
        }
    }
    pub fn cause(&self) -> CloseCause {
        if self.unavailable {
            CloseCause::Unavailable
        } else {
            self.producer.cause()
        }
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.producer.poll_fd()
    }
    pub fn poll_events(&self) -> ReadyEvents {
        self.producer.poll_events()
    }
    pub(crate) fn select(&mut self, revision: u16, caps: u64) {
        if self.selected {
            return;
        }
        self.selected = true;
        if revision != 2 || caps & LOCK_FILE_CAPABILITY_SEALED_MEMORY == 0 {
            self.unavailable = true;
            self.producer.channel_unavailable();
        }
    }
    pub(crate) fn deadline(&self, now: u64) -> Option<u64> {
        let producer = self.producer.timeout(now).map(|n| now.saturating_add(n));
        let retry = (self.selected
            && !self.unavailable
            && self.read.is_none()
            && matches!(
                self.producer.channel(),
                ChannelState::Idle | ChannelState::Discovered
            ))
        .then_some(self.retry_at);
        match (producer, retry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    pub(crate) fn name(&self) -> &'static [u8] {
        [
            b"image-channel".as_slice(),
            b"image-limits",
            b"image-formats",
        ][self.index]
    }
    pub(crate) fn drive(&mut self, now: u64) -> Result<(), ImageError> {
        // Readiness is observed, never inferred merely from requested interest.
        let revents = if let Some(fd) = self.producer.poll_fd() {
            let mut poll = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN | rustix::event::PollFlags::OUT,
            )];
            rustix::event::poll(&mut poll, Some(&rustix::event::Timespec::default()))
                .map_err(|_| ImageError::Protocol)?;
            ReadyEvents(poll[0].revents().bits())
        } else {
            ReadyEvents::NONE
        };
        self.producer.dispatch(revents, now)?;
        if !self.selected || self.unavailable || now < self.retry_at {
            return Ok(());
        }
        match self.producer.channel() {
            ChannelState::Idle if self.read.is_none() => {
                let cap = match self.index {
                    0 => 2048,
                    1 => 136,
                    _ => self
                        .config
                        .formats_capacity
                        .min(LOCK_FILE_MAX_IMAGE_FORMATS_BYTES),
                };
                self.read = Some(ImageRead::new(cap).map_err(|_| ImageError::Protocol)?);
            }
            ChannelState::Discovered => {
                match sophia_image_producer::connect(
                    &self.config.endpoint_dir,
                    self.endpoint.as_deref().ok_or(ImageError::Protocol)?,
                ) {
                    Ok(fd) => self
                        .producer
                        .attach(fd, now)
                        .map_err(|(e, _)| ImageError::Producer(e))?,
                    Err(ProducerError::Again) => self.retry(now),
                    Err(e) => {
                        self.producer.close();
                        return Err(e.into());
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn retry(&mut self, now: u64) {
        self.retry_at = now.saturating_add(self.retry_ms);
        self.retry_ms = (self.retry_ms * 2).min(256);
    }
    pub(crate) fn read_complete(&mut self, now: u64) -> Result<(), ImageError> {
        if !self.read.as_ref().is_some_and(ImageRead::done) {
            return Ok(());
        }
        let read = self.read.take().unwrap();
        let clunk_failed = read.clunk_failed();
        let result = read.into_result().map_err(|_| ImageError::Protocol)?;
        match result {
            Ok(bytes) => self.objects[self.index] = bytes,
            Err(errno) => {
                let read = if errno == 2 && self.index == 0 && !clunk_failed {
                    DiscoveryRead::Absent
                } else {
                    DiscoveryRead::Failed
                };
                let _ = self.producer.discover(read, self.pass, now);
                return Ok(());
            }
        }
        self.index += 1;
        if self.index < 3 {
            return Ok(());
        }
        self.index = 0;
        let decoded = (|| {
            let limits =
                decode_lock_file_record_at_revision(&self.objects[1], LockFileClass::Object, 2)
                    .ok()?;
            let formats =
                decode_lock_file_record_at_revision(&self.objects[2], LockFileClass::Object, 2)
                    .ok()?;
            if limits.header.kind != LockFileKind::ImageLimits
                || formats.header.kind != LockFileKind::ImageFormats
            {
                return None;
            }
            let discovery =
                LockImageDiscovery::decode(std::str::from_utf8(&self.objects[0]).ok()?).ok()?;
            Some((limits, formats, discovery.endpoint))
        })();
        let outcome = if let Some((limits, formats, endpoint)) = decoded {
            self.endpoint = Some(endpoint);
            self.producer.discover(
                DiscoveryRead::Present {
                    text: &self.objects[0],
                    limits_body: limits.body,
                    formats_body: formats.body,
                    limits_connection_epoch: limits.header.connection_epoch,
                    formats_connection_epoch: formats.header.connection_epoch,
                },
                self.pass,
                now,
            )
        } else {
            self.producer
                .discover(DiscoveryRead::Failed, self.pass, now)
        };
        self.pass = self.pass.checked_add(1).ok_or(ImageError::Protocol)?;
        for bytes in &mut self.objects {
            bytes.clear();
        }
        if outcome == Err(ProducerError::Again) {
            self.retry(now);
        }
        Ok(())
    }
}

impl LockImageAdapter {
    pub(crate) fn settle_submission(
        &mut self,
        latest: Option<(u64, crate::submission::Custody)>,
        now: u64,
    ) {
        use crate::submission::Custody;
        let Some((submission, offer)) = self.begin_submission else {
            return;
        };
        match latest {
            Some((id, _)) if id != submission => (),
            Some((id, Custody::Submitted)) if id == submission => self.begin_submission = None,
            Some((id, Custody::Refused(_) | Custody::DroppedUnsent)) if id == submission => {
                let _ = self.producer.begin_refused(offer, now);
                self.begin_submission = None;
            }
            Some((id, Custody::Staged | Custody::Issued)) if id == submission => (),
            _ => {
                self.producer.journal_lost();
                self.begin_submission = None;
            }
        }
    }
    pub(crate) fn status(&mut self, v: LockResourceStatus, now: u64) -> Result<(), ProducerError> {
        use sophia_image_producer::ResourceStatusKind as K;
        if !self.producer.offers().any(|o| {
            o.offer_id == v.transaction
                && o.resource == v.resource.id
                && o.resource_generation == v.resource.generation
        }) {
            return Ok(());
        }
        let status = match v.status {
            LockResourceState::Accepted => K::Accepted,
            LockResourceState::Rejected => K::Rejected,
            LockResourceState::Cancelled => K::Cancelled,
            _ => return Ok(()),
        };
        let result = self.producer.resource_status(
            v.resource.id,
            v.resource.generation,
            status,
            v.reason == 9 && status == K::Rejected,
            now,
        );
        channel_result(result)
    }
    pub(crate) fn record(
        &mut self,
        epoch: u64,
        c: crate::journal::ImageControl,
        now: u64,
    ) -> Result<(), ProducerError> {
        use crate::journal::ImageControl as C;
        use sophia_image_producer::{ImageClosedReason as R, ImageEvent as E};
        let event = match c {
            C::Admitted(m) => E::ImageAdmitted {
                metadata: ProducerMetadata {
                    offer_id: m.offer_id,
                    resource: m.resource,
                    resource_generation: m.resource_generation,
                    admission_epoch: m.admission_epoch,
                    output_id: m.output_id,
                    allocation_id: m.allocation_id,
                    topology_generation: m.topology_generation,
                    source_kind: m.source_kind,
                    source_format: m.source_format,
                    width: m.width,
                    height: m.height,
                    stride: m.stride,
                    plane_count: m.plane_count,
                    descriptor_count: m.descriptor_count,
                    declared_bytes: m.declared_bytes,
                    modifier: m.modifier,
                },
            },
            C::Released(v) => E::SourceReleased {
                offer_id: v.offer_id,
                resource: v.resource,
                resource_generation: v.resource_generation,
                disposition: v.disposition as u32,
            },
            C::Ready(v) => E::ChannelReady {
                channel_generation: v.channel_generation,
                granted_source_kinds: v.granted_source_kinds,
            },
            C::Closed(v) => E::ChannelClosed {
                channel_generation: v.channel_generation,
                reason: match v.reason {
                    LockChannelClosedReason::PeerDisconnect => R::PeerDisconnect,
                    LockChannelClosedReason::ProtocolFault => R::ProtocolFault,
                    LockChannelClosedReason::HelloTimeout => R::HelloTimeout,
                    LockChannelClosedReason::GrantRevoked => R::GrantRevoked,
                },
            },
        };
        channel_result(self.producer.record(epoch, event, now))
    }
}

// A valid journal record may contradict channel custody. The producer faults
// that channel and retains its offers; the independent 9P journal stays live.
fn channel_result(result: Result<(), ProducerError>) -> Result<(), ProducerError> {
    match result {
        Err(ProducerError::Closed) => Ok(()),
        other => other,
    }
}
