//! The lock role's sealed-image channel for one connection epoch: the
//! owner's configuration, revision 2 selection, disclosure, and readiness
//! published only by the owner. An `ImageBegin` is busy while the channel
//! awaits its Hello, becomes an offer (see `offers.rs`) while it is ready,
//! and is refused cleanly with reason 9 once it is closed. Readiness
//! supplied through [`LockFileCustody::image_channel_ready`] is the owner's
//! statement (in production, after its authenticated Hello); this file does
//! not prove that Hello happened.
use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::offers::{OFFER_CREDITS, Offers};
use super::{LockFileCustody, LockFileLimits};

/// Not in `sophia-9p`'s set: the offer waits for the channel's Hello.
const EBUSY: Errno = Errno(16);

/// The sealed-memory source kind, the only one this owner grants.
const SEALED_KIND: u32 = LOCK_IMAGE_SOURCE_SEALED_MEMORY;

/// The revision that carries the image vocabulary.
pub(super) const IMAGE_REVISION: u16 = 2;

/// Owner-supplied image configuration for one epoch. All three objects are
/// validated and cross-checked before negotiation; nothing here grants a
/// capability until the provider requests it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockImageSettings {
    pub discovery: LockImageDiscovery,
    pub limits: LockImageLimits,
    pub formats: LockImageFormats,
}

/// Where the channel stands. Configuration alone never advances this; only
/// disclosure and the owner's ready or closed publications do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockImageReadiness {
    Disabled,
    AwaitingHello,
    Ready { granted_source_kinds: u32 },
    Closed { reason: LockChannelClosedReason },
}

pub(super) struct ImageChannel {
    settings: LockImageSettings,
    readiness: LockImageReadiness,
    /// One journal credit held from ChannelReady until ChannelClosed.
    close_held: bool,
    /// A ChannelClosed after a failed Hello, journalled as room allows.
    pending_closed: Option<LockChannelClosed>,
    /// The highest offer ID taken into custody, refused ones included.
    submitted_high_water: u64,
    pub(super) offers: Offers,
}

impl ImageChannel {
    /// Checks the configuration against itself and the epoch's limits.
    pub(super) fn new(
        settings: LockImageSettings,
        epoch: u64,
        limits: &LockFileLimits,
    ) -> Result<Self, Errno> {
        let invalid = |_| Errno::EINVAL;
        let LockImageSettings {
            discovery,
            limits: image,
            formats,
        } = &settings;
        discovery.encode().map_err(invalid)?;
        image.encode().map_err(invalid)?;
        formats.encode().map_err(invalid)?;
        let generations = discovery.channel_generation == image.channel_generation
            && discovery.channel_generation == formats.channel_generation
            && discovery.limits_generation == image.limits_generation
            && discovery.limits_generation == formats.limits_generation
            && discovery.formats_generation == formats.formats_generation;
        let budgets = discovery.max_offers == image.max_offers
            && discovery.max_packets_per_turn == image.max_packets_per_turn
            && discovery.validation_timeout_ms == image.validation_timeout_ms
            && discovery.validation_chunk_bytes == image.validation_chunk_bytes
            && discovery.capture_interest_timeout_ms == image.capture_interest_timeout_ms;
        // Isb only: DMA-BUF needs the later capture owner.
        let sealed = discovery.source_kinds == SEALED_KIND
            && image.max_validation_jobs != 0
            && image.max_dma_source_bytes == 0;
        // Offers equal min(4, the role's transfer slots), as the SDK
        // coherence checks require; outputs stay within the role's outputs,
        // validation jobs within offers, and backing holds one resource.
        let bounded = image.max_resource_bytes <= limits.max_resource_bytes
            && image.max_width <= limits.max_width_px
            && image.max_height <= limits.max_height_px
            && image.max_offers == u32::from(limits.upload_slots).min(4)
            && image.max_admitted_outputs <= u32::from(limits.max_outputs)
            && image.max_validation_jobs <= image.max_offers
            && image.max_sealed_backing_bytes >= image.max_resource_bytes;
        if discovery.connection_epoch != epoch
            || discovery.endpoint != crate::image_channel::IMAGE_ENDPOINT_BASENAME
            || !generations
            || !budgets
            || !sealed
            || !bounded
        {
            return Err(Errno::EINVAL);
        }
        // ChannelClosed and two terminal events per offer stay reservable
        // beside at least one ordinary event. Bytes need no check: at most
        // ten credits fit well inside the 2048-byte journal floor.
        let held = 1 + 2 * image.max_offers as usize;
        if limits.journal_records as usize <= held {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            settings,
            readiness: LockImageReadiness::Disabled,
            close_held: false,
            pending_closed: None,
            submitted_high_water: 0,
            offers: Offers::default(),
        })
    }

    pub(super) fn settings(&self) -> &LockImageSettings {
        &self.settings
    }

    /// Journal credits this channel holds: its `ChannelClosed` and each
    /// live offer's two terminal events.
    pub(super) fn credits(&self) -> usize {
        usize::from(self.close_held) + OFFER_CREDITS * self.offers.len()
    }

    /// Negotiation disclosed the channel: it now awaits the provider's Hello.
    pub(super) fn disclose(&mut self) {
        self.readiness = LockImageReadiness::AwaitingHello;
    }
}

impl LockFileCustody {
    /// Configures the sealed-image channel for this epoch. Allowed only
    /// before the provider negotiates; once set it cannot change.
    /// Formats generation equals the current lock topology generation, and
    /// follows each later topology.
    pub fn configure_image(&mut self, settings: LockImageSettings) -> Result<(), Errno> {
        self.live()?;
        if self.image.is_some()
            || !matches!(self.negotiation, super::Negotiation::Awaiting)
            || settings.formats.formats_generation != self.lock.topology_generation
        {
            return Err(Errno::EINVAL);
        }
        self.image = Some(ImageChannel::new(settings, self.epoch, &self.limits)?);
        Ok(())
    }

    /// The revision events and submissions use: 1 until negotiation selects.
    pub fn revision(&self) -> u16 {
        self.revision
    }

    /// Configured, revision 2 selected and the sealed kind granted.
    pub fn image_disclosed(&self) -> bool {
        self.image.is_some()
            && self.revision == IMAGE_REVISION
            && self.negotiated().is_ok_and(|negotiated| {
                negotiated.granted_capabilities & LOCK_FILE_CAPABILITY_SEALED_MEMORY != 0
            })
    }

    pub fn image_readiness(&self) -> LockImageReadiness {
        match &self.image {
            Some(image) if self.image_disclosed() => image.readiness,
            _ => LockImageReadiness::Disabled,
        }
    }

    /// The disclosed configuration, for the discovery objects and the
    /// owner's socket service. `None` until disclosure.
    pub fn image_settings(&self) -> Option<&LockImageSettings> {
        self.image
            .as_ref()
            .filter(|_| self.image_disclosed())
            .map(ImageChannel::settings)
    }

    /// Publishes `ChannelReady` after the owner's authenticated Hello. The
    /// generation fences a stale callback; the granted kinds must be the
    /// sealed kind. One credit for the future `ChannelClosed` is held first.
    pub fn image_channel_ready(
        &mut self,
        channel_generation: u64,
        granted_source_kinds: u32,
    ) -> Result<u64, Errno> {
        self.live()?;
        if self.image_readiness() != LockImageReadiness::AwaitingHello {
            return Err(Errno::EACCES);
        }
        let image = self.image.as_ref().expect("disclosed image");
        if channel_generation != image.settings.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        if granted_source_kinds != SEALED_KIND {
            return Err(Errno::EINVAL);
        }
        let body = LockChannelReady {
            channel_generation,
            granted_source_kinds,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        let sequence =
            self.journal_events(&[(LockFileKind::ChannelReady, body)], self.credits() + 1)?;
        let image = self.image.as_mut().expect("disclosed image");
        image.close_held = true;
        image.readiness = LockImageReadiness::Ready {
            granted_source_kinds,
        };
        Ok(sequence)
    }

    /// Publishes `ChannelClosed`, spending the credit held since readiness.
    pub fn image_channel_closed(
        &mut self,
        channel_generation: u64,
        reason: LockChannelClosedReason,
    ) -> Result<u64, Errno> {
        self.live()?;
        if !matches!(self.image_readiness(), LockImageReadiness::Ready { .. }) {
            return Err(Errno::EACCES);
        }
        let image = self.image.as_ref().expect("disclosed image");
        if channel_generation != image.settings.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        let body = LockChannelClosed {
            channel_generation,
            reason,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        let sequence = self.close_image_offers((LockFileKind::ChannelClosed, body), 1)?;
        let image = self.image.as_mut().expect("disclosed image");
        image.close_held = false;
        image.readiness = LockImageReadiness::Closed { reason };
        Ok(sequence)
    }

    /// Closes the channel after the owner's authenticated Hello failed or
    /// timed out. No credit is held before readiness, so the event is
    /// journalled now if room allows and otherwise from later turns; the
    /// closed state takes effect at once and never reopens.
    pub fn image_channel_hello_failed(
        &mut self,
        channel_generation: u64,
        reason: LockChannelClosedReason,
    ) -> Result<(), Errno> {
        self.live()?;
        if self.image_readiness() != LockImageReadiness::AwaitingHello {
            return Err(Errno::EACCES);
        }
        let image = self.image.as_mut().expect("disclosed image");
        if channel_generation != image.settings.discovery.channel_generation {
            return Err(Errno::ESTALE);
        }
        image.pending_closed = Some(LockChannelClosed {
            channel_generation,
            reason,
        });
        image.readiness = LockImageReadiness::Closed { reason };
        let _ = self.retry_image_publications();
        Ok(())
    }

    /// Journals a pending `ChannelClosed`, if any. Returns whether one was
    /// journalled; acknowledgement progress and owner turns call this.
    pub fn retry_image_publications(&mut self) -> Result<bool, Errno> {
        let Some(closed) = self.image.as_ref().and_then(|image| image.pending_closed) else {
            return Ok(false);
        };
        let body = closed.encode().map_err(|_| Errno::EINVAL)?;
        match self.journal_events(&[(LockFileKind::ChannelClosed, body)], self.credits()) {
            Ok(_) => {
                self.image.as_mut().expect("pending image").pending_closed = None;
                Ok(true)
            }
            Err(Errno::EAGAIN | Errno::ENOSPC) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Isb-only formats have no rows, so a new topology changes only the
    /// generation that discovery and the formats snapshot name.
    pub(super) fn follow_image_topology(&mut self) {
        let generation = self.lock.topology_generation;
        if let Some(image) = &mut self.image {
            image.settings.discovery.formats_generation = generation;
            image.settings.formats.formats_generation = generation;
        }
    }

    /// `ImageBegin`: the offer ID must exceed every one already taken into
    /// custody. Busy until the Hello; an offer while ready; refused with
    /// reason 9 once closed.
    pub(super) fn image_begin(
        &mut self,
        receipt: (LockFileKind, Vec<u8>),
        body: &[u8],
        now: std::time::Instant,
    ) -> Result<(), Errno> {
        let image = self
            .image
            .as_ref()
            .filter(|_| self.image_disclosed())
            .ok_or(Errno::EINVAL)?;
        let metadata = LockImageMetadata::decode(body).map_err(|_| Errno::EINVAL)?;
        if metadata.offer_id <= image.submitted_high_water {
            return Err(Errno::ESTALE);
        }
        match image.readiness {
            LockImageReadiness::AwaitingHello => return Err(EBUSY),
            LockImageReadiness::Ready { .. } => self.offer_image(receipt, metadata, now)?,
            LockImageReadiness::Closed { .. } => {
                let body = LockResourceStatus {
                    transaction: metadata.offer_id,
                    resource: LockResourceId {
                        id: metadata.resource,
                        generation: metadata.resource_generation,
                    },
                    status: LockResourceState::Rejected,
                    reason: LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE,
                    admitted_bytes: 0,
                }
                .encode()
                .map_err(|_| Errno::EINVAL)?;
                self.journal_events(
                    &[receipt, (LockFileKind::ResourceStatus, body)],
                    self.credits(),
                )?;
            }
            LockImageReadiness::Disabled => return Err(Errno::EINVAL),
        }
        self.image
            .as_mut()
            .expect("disclosed image")
            .submitted_high_water = metadata.offer_id;
        Ok(())
    }
}
