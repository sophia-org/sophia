use std::os::fd::{AsFd, OwnedFd};

use crate::io;
use crate::producer::ImageProducer;
use crate::role;
use crate::{
    ChannelState, CloseCause, OfferInfo, OfferState, ProducerError, ProducerMetadata,
    ResourceStatusKind, SOURCE_KIND_SEALED,
};

#[derive(Default)]
pub(crate) struct Slot {
    pub(crate) metadata: ProducerMetadata,
    pub(crate) fd: Option<OwnedFd>,
    pub(crate) plane_offset: u32,
    pub(crate) plane_stride: u32,
    pub(crate) state: Option<OfferState>,
    pub(crate) deadline_ms: u64,
    pub(crate) deadline_passed: bool,
    pub(crate) status: Option<ResourceStatusKind>,
    pub(crate) unavailable: bool,
    pub(crate) disposition: u32,
    pub(crate) cancelled: bool,
}

impl Slot {
    fn info(&self) -> OfferInfo {
        OfferInfo {
            offer_id: self.metadata.offer_id,
            resource: self.metadata.resource,
            resource_generation: self.metadata.resource_generation,
            state: self.state.unwrap_or(OfferState::Refused),
            deadline_passed: self.deadline_passed,
            status: self.status,
            unavailable: self.unavailable,
            disposition: self.disposition,
            cancel_requested: self.cancelled,
        }
    }
    fn live_custody(&self) -> bool {
        !matches!(
            self.state,
            None | Some(OfferState::Refused | OfferState::Settled | OfferState::Unknown)
        )
    }
}

impl ImageProducer {
    /// Forward a `ResourceStatus` for an offer's resource. Rejected
    /// or Cancelled before admission refuses the offer; after
    /// admission the status is recorded and the offer still waits
    /// for `SourceReleased`.
    pub fn resource_status(
        &mut self,
        resource: u64,
        resource_generation: u64,
        status: ResourceStatusKind,
        unavailable: bool,
        now_ms: u64,
    ) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        if unavailable && status != ResourceStatusKind::Rejected {
            return Err(ProducerError::Argument);
        }
        let Some(index) = self.slots.iter().position(|slot| {
            slot.live_custody()
                && slot.metadata.resource == resource
                && slot.metadata.resource_generation == resource_generation
        }) else {
            return Ok(());
        };
        let slot = &mut self.slots[index];
        if slot.status.is_some()
            || (status == ResourceStatusKind::Accepted
                && !matches!(slot.state, Some(OfferState::Sent | OfferState::Ambiguous)))
        {
            return Err(self.fault());
        }
        slot.status = Some(status);
        slot.unavailable = unavailable;
        if slot.state == Some(OfferState::Begun) {
            slot.fd = None;
            slot.state = Some(OfferState::Refused);
        }
        if unavailable {
            self.fail(CloseCause::Unavailable);
        }
        Ok(())
    }

    /// The role refused the ImageBegin transaction itself.
    pub fn begin_refused(&mut self, offer: u64, now_ms: u64) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        let Some(slot) = self.slot_mut(offer) else {
            return Err(ProducerError::Refused);
        };
        if slot.state != Some(OfferState::Begun) {
            return Err(ProducerError::Refused);
        }
        slot.fd = None;
        slot.state = Some(OfferState::Refused);
        Ok(())
    }

    /// The 9P journal ended: unsettled offers become Unknown.
    pub fn journal_lost(&mut self) {
        for slot in &mut self.slots {
            if slot.state.is_some()
                && !matches!(slot.state, Some(OfferState::Refused | OfferState::Settled))
            {
                slot.state = Some(OfferState::Unknown);
            }
        }
        self.fail(CloseCause::JournalLost);
    }

    /// Close the socket (Caller). Offers keep their custody states.
    /// Start one offer in Ready: a free slot, a sealed source of a
    /// granted kind, metadata admissible under the discovered
    /// limits. On success the producer owns `source` and returns the
    /// assigned offer, the 96-byte ImageBegin body, and the metadata
    /// with its offer ID assigned. On any error the caller keeps
    /// `source`; the descriptor is never silently dropped here.
    pub fn begin(
        &mut self,
        metadata: ProducerMetadata,
        source: OwnedFd,
        now_ms: u64,
    ) -> Result<(u64, [u8; 96], ProducerMetadata), (ProducerError, OwnedFd)> {
        if metadata.source_kind != SOURCE_KIND_SEALED {
            return Err((ProducerError::Refused, source));
        }
        self.begin_source(metadata, source, 0, metadata.stride, now_ms)
    }

    /// Offer one producer-completed LINEAR XRGB8888 DMA-BUF. The caller must
    /// retain its GPU allocation and must not write it until SourceReleased.
    /// Disconnection leaves completion unknown. Refusal returns the descriptor.
    pub fn begin_dma_buf(
        &mut self,
        metadata: ProducerMetadata,
        source: OwnedFd,
        offset: u32,
        stride: u32,
        now_ms: u64,
    ) -> Result<(u64, [u8; 96], ProducerMetadata), (ProducerError, OwnedFd)> {
        if self.config.role != crate::ImageRole::Shell
            || self.config.revision != 10
            || metadata.source_kind != 2
        {
            return Err((ProducerError::Refused, source));
        }
        self.begin_source(metadata, source, offset, stride, now_ms)
    }

    fn begin_source(
        &mut self,
        metadata: ProducerMetadata,
        source: OwnedFd,
        offset: u32,
        stride: u32,
        now_ms: u64,
    ) -> Result<(u64, [u8; 96], ProducerMetadata), (ProducerError, OwnedFd)> {
        if let Err(error) = self.clock(now_ms) {
            return Err((error, source));
        }
        if self.channel != ChannelState::Ready {
            return Err((
                if self.channel == ChannelState::Closed {
                    ProducerError::Closed
                } else {
                    ProducerError::Again
                },
                source,
            ));
        }
        if self.next_offer == 0 {
            self.fail(CloseCause::Exhausted);
            return Err((ProducerError::Exhausted, source));
        }
        let mut free: Option<usize> = None;
        let mut live = 0u32;
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.state.is_none() {
                free.get_or_insert(index);
                continue;
            }
            live += 1;
            if slot.live_custody()
                && slot.metadata.resource == metadata.resource
                && slot.metadata.resource_generation == metadata.resource_generation
            {
                return Err((ProducerError::Refused, source));
            }
        }
        let Some(index) = free else {
            return Err((ProducerError::Busy, source));
        };
        if live >= self.max_offers {
            return Err((ProducerError::Busy, source));
        }
        let mut assigned = metadata;
        assigned.offer_id = self.next_offer;
        let admissible = match &self.expect {
            Some(expect) => role::admissible(expect, assigned),
            None => false,
        };
        let source_valid = match assigned.source_kind {
            SOURCE_KIND_SEALED if self.config.revision != 10 => {
                io::sealed_source(source.as_fd(), assigned.declared_bytes)
            }
            2 if self.config.role == crate::ImageRole::Shell && self.config.revision == 10 => {
                assigned.source_format == u32::from_le_bytes(*b"XR24")
                    && assigned.modifier == 0
                    && assigned.plane_count == 1
                    && assigned.descriptor_count == 1
                    && assigned.stride == 0
                    && assigned
                        .width
                        .checked_mul(4)
                        .is_some_and(|row| stride >= row)
                    && rustix::fs::seek(source.as_fd(), rustix::fs::SeekFrom::End(0)).is_ok_and(
                        |bytes| {
                            bytes
                                <= self
                                    .expect
                                    .as_ref()
                                    .map_or(0, role::Expect::dma_source_limit)
                                && assigned
                                    .height
                                    .checked_sub(1)
                                    .and_then(|rows| u64::from(stride).checked_mul(u64::from(rows)))
                                    .and_then(|span| span.checked_add(u64::from(offset)))
                                    .and_then(|span| {
                                        span.checked_add(u64::from(assigned.width) * 4)
                                    })
                                    .is_some_and(|span| span <= bytes)
                        },
                    )
            }
            _ => false,
        };
        if !admissible || !source_valid {
            return Err((ProducerError::Refused, source));
        }
        let body = role::metadata_body(self.config.role, assigned);
        let Ok(body) = body else {
            return Err((ProducerError::Refused, source));
        };
        let slot = &mut self.slots[index];
        *slot = Slot {
            metadata: assigned,
            plane_offset: offset,
            plane_stride: stride,
            fd: Some(source),
            state: Some(OfferState::Begun),
            deadline_ms: now_ms.saturating_add(u64::from(self.config.transfer_timeout_ms)),
            ..Slot::default()
        };
        self.next_offer = self.next_offer.wrapping_add(1);
        Ok((assigned.offer_id, body, assigned))
    }

    pub fn offer(&self, offer: u64) -> Option<OfferInfo> {
        self.slots
            .iter()
            .find(|slot| slot.state.is_some() && slot.metadata.offer_id == offer)
            .map(Slot::info)
    }

    /// Live offers in slot order.
    pub fn offers(&self) -> impl Iterator<Item = OfferInfo> + '_ {
        self.slots
            .iter()
            .filter(|slot| slot.state.is_some())
            .map(Slot::info)
    }

    /// Mark an offer cancelled. The mark is permanent even if the
    /// role's ResourceCancel is refused: an admitted source is never
    /// sent afterward and the descriptor stays held. Settlement is
    /// still only `SourceReleased` (or Unknown on journal loss).
    pub fn cancel(&mut self, offer: u64, now_ms: u64) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        let Some(slot) = self.slot_mut(offer) else {
            return Err(ProducerError::Argument);
        };
        if slot.cancelled
            || slot.status.is_some()
            || !matches!(
                slot.state,
                Some(
                    OfferState::Begun
                        | OfferState::Admitted
                        | OfferState::Sent
                        | OfferState::Ambiguous
                        | OfferState::Expired
                )
            )
        {
            return Err(ProducerError::Refused);
        }
        slot.cancelled = true;
        Ok(())
    }

    /// Free a Refused, Settled or Unknown slot.
    pub fn forget(&mut self, offer: u64) -> Result<(), ProducerError> {
        let Some(slot) = self.slot_mut(offer) else {
            return Err(ProducerError::Refused);
        };
        if !matches!(
            slot.state,
            Some(OfferState::Refused | OfferState::Settled | OfferState::Unknown)
        ) {
            return Err(ProducerError::Refused);
        }
        *slot = Slot::default();
        Ok(())
    }
}
