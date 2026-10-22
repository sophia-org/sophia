//! Four live offers, each retaining its terminal journal credits and exact Source identity.
use super::super::journal::JournalReservation;
use super::*;
use crate::{ContentEpochRegistry, IsbImageJobs, IsbValidationWorker};
use std::os::fd::OwnedFd;

pub(in crate::shell_transport) const MAX_IMAGE_OFFERS: usize = 4;
const STATUS_BYTES: usize = SHELL_FILE_HEADER_BYTES + 56;
const RELEASE_BYTES: usize = SHELL_FILE_HEADER_BYTES + 32;

pub(super) struct Offer {
    pub(super) metadata: ShellImageMetadata,
    /// The CPU validation job; `None` for a DMA-BUF capture offer, whose
    /// reservation the capture owner holds instead.
    pub(super) job: Option<u64>,
    #[cfg(feature = "experimental-shell-capture")]
    pub(super) capture: Option<super::capture_offers::CaptureCharge>,
    pub(super) status: Option<JournalReservation>,
    pub(super) release: Option<JournalReservation>,
    pub(super) received: bool,
    pub(super) source: Option<OwnedFd>,
}

#[derive(Default)]
pub(super) struct ImageOffers {
    pub enabled: bool,
    pub(super) high_water: u64,
    // Submitted custody consumes an offer identity even if admission is later
    // refused. Only admission advances the separate Source-classification mark.
    submitted_high_water: u64,
    pub(super) live: [Option<Offer>; MAX_IMAGE_OFFERS],
    /// Fixed when ImageBegin is accepted, so owner retries cannot renew it.
    queued: [Option<(u64, Instant)>; MAX_IMAGE_OFFERS],
}

impl ImageOffers {
    pub(super) fn accepts_id(&self, id: u64) -> bool {
        id > self.submitted_high_water
    }
    /// Submission queues at most `max_offers` ImageBegins, so a queued slot
    /// is always free here.
    pub(super) fn note_submitted(&mut self, id: u64, deadline: Instant) {
        self.submitted_high_water = id;
        if let Some(slot) = self.queued.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((id, deadline));
        }
    }
    pub(super) fn note_refused(&mut self, id: u64) {
        self.submitted_high_water = id;
    }
    #[cfg(any(test, feature = "experimental-shell-capture"))]
    pub(super) fn queued_deadline(&self, id: u64) -> Option<Instant> {
        self.queued
            .iter()
            .flatten()
            .find_map(|(queued, deadline)| (*queued == id).then_some(*deadline))
    }
    pub(super) fn dequeue(&mut self, id: u64) {
        for slot in &mut self.queued {
            if slot.is_some_and(|(queued, _)| queued == id) {
                *slot = None;
            }
        }
    }
    pub(super) fn close_sources(&mut self) {
        for offer in self.live.iter_mut().flatten() {
            offer.source = None;
        }
    }
    fn collect(&mut self) {
        for slot in &mut self.live {
            if slot
                .as_ref()
                .is_some_and(|offer| offer.status.is_none() && offer.release.is_none())
            {
                *slot = None;
            }
        }
    }
}

impl ShellFiles {
    pub(in crate::shell_transport) fn enable_image_offers(&mut self) -> Result<(), Errno> {
        if !matches!(
            self.image_readiness(),
            ShellImageReadiness::Ready {
                granted_source_kinds: 1
            }
        ) {
            return Err(Errno::EACCES);
        }
        self.image.as_mut().ok_or(Errno::EACCES)?.offers.enabled = true;
        Ok(())
    }

    /// The oldest queued offer of any source kind. CPU owners must use
    /// [`Self::pending_cpu_image_offer`] so they never consume a DMA-BUF offer.
    pub(in crate::shell_transport) fn pending_image_offer(&self) -> Option<ShellImageMetadata> {
        match self.inbound.front()? {
            Inbound::ImageBegin(metadata) => Some(*metadata),
            _ => None,
        }
    }

    pub(in crate::shell_transport) fn pending_cpu_image_offer(&self) -> Option<ShellImageMetadata> {
        self.pending_image_offer()
            .filter(|metadata| metadata.source_kind != SHELL_IMAGE_SOURCE_DMA_BUF)
    }

    pub(in crate::shell_transport) fn image_validation_timeout(&self) -> Option<Duration> {
        let state = self.image.as_ref()?;
        let transfer = self.content_limits.as_ref()?.transfer_timeout_ms;
        Some(Duration::from_millis(u64::from(
            transfer.min(state.config.limits.validation_timeout_ms),
        )))
    }

    pub(in crate::shell_transport) fn refuse_image_offer(
        &mut self,
        reason: sophia_protocol::ContentReason,
    ) -> Result<(), Errno> {
        self.refuse_image_offer_reason(reason as u16)
    }

    pub(in crate::shell_transport) fn refuse_image_offer_reason(
        &mut self,
        reason: u16,
    ) -> Result<(), Errno> {
        if reason == 0 {
            return Err(Errno::EINVAL);
        }
        let metadata = self.pending_image_offer().ok_or(Errno::EAGAIN)?;
        if !self.publish_image_refusal_reason(
            PendingImageRefusal {
                offer_id: metadata.offer_id,
                admission_epoch: metadata.admission_epoch,
                resource: ContentResourceId {
                    id: metadata.resource,
                    generation: metadata.resource_generation,
                },
            },
            reason,
        )? {
            return Err(Errno::EAGAIN);
        }
        self.inbound.pop_front();
        self.image
            .as_mut()
            .expect("pending image offer")
            .offers
            .dequeue(metadata.offer_id);
        Ok(())
    }

    /// All journal failures precede owner admission. Terminal holds survive
    /// publication and can be spent only by the corresponding status/release.
    pub(in crate::shell_transport) fn admit_image_offer(
        &mut self,
        grant: ContentGrant,
        epochs: &mut ContentEpochRegistry,
        jobs: &mut IsbImageJobs,
        worker: &mut IsbValidationWorker,
        now: u64,
        deadline: Instant,
    ) -> Result<u64, Errno> {
        if !matches!(
            self.image_readiness(),
            ShellImageReadiness::Ready {
                granted_source_kinds: 1
            }
        ) {
            return Err(Errno::EACCES);
        }
        let metadata = self.pending_cpu_image_offer().ok_or(Errno::EAGAIN)?;
        let state = self.image.as_mut().ok_or(Errno::EACCES)?;
        if grant.connection_epoch != self.epoch
            || metadata.admission_epoch != grant.content_grant_epoch
            || metadata.offer_id <= state.offers.high_water
        {
            return Err(Errno::ESTALE);
        }
        // Until the role has separate byte/descriptor backing sub-pools,
        // count all retained backing against this ceiling conservatively.
        // Accepted images remain charged after their offer slot is released.
        let usage = epochs.resources(grant).ok_or(Errno::ESTALE)?.usage();
        if worker.pending() >= state.config.limits.max_validation_jobs as usize
            || usage
                .backing
                .checked_add(metadata.declared_bytes)
                .is_none_or(|bytes| bytes > state.config.limits.max_sealed_backing_bytes)
        {
            return Err(Errno::EAGAIN);
        }
        let slot = state.offers.live[..state.config.limits.max_offers as usize]
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(Errno::EAGAIN)?;
        metadata
            .admissible(&ShellImageExpect {
                role: 1,
                connection_epoch: self.epoch,
                channel_generation: state.config.discovery.channel_generation,
                granted_source_kinds: 1,
                max_width: state.config.limits.max_width,
                max_height: state.config.limits.max_height,
                max_resource_bytes: state.config.limits.max_resource_bytes,
            })
            .map_err(|_| Errno::EINVAL)?;
        let body = metadata.encode().map_err(|_| Errno::EINVAL)?;
        let status = self
            .journal
            .hold_reservation(STATUS_BYTES)
            .ok_or(Errno::EAGAIN)?;
        let Some(release) = self.journal.hold_reservation(RELEASE_BYTES) else {
            self.journal.release_reservation(status);
            return Err(Errno::EAGAIN);
        };
        let admitted = (|| {
            let prepared =
                self.journal
                    .prepare_append(ShellFileKind::ImageAdmitted, &body, true)?;
            let job = jobs
                .reserve(epochs, worker, grant, metadata, now, deadline)
                .map_err(|error| match error {
                    crate::IsbImageAdmissionError::Worker(crate::IsbSubmitFailure::Capacity)
                    | crate::IsbImageAdmissionError::Resource(crate::ContentStoreError::Budget) => {
                        Errno::EAGAIN
                    }
                    crate::IsbImageAdmissionError::Resource(
                        crate::ContentStoreError::Stale | crate::ContentStoreError::Revoked,
                    ) => Errno::ESTALE,
                    _ => Errno::EINVAL,
                })?;
            prepared.commit();
            Ok(job)
        })();
        let job = match admitted {
            Ok(job) => job,
            Err(error) => {
                self.journal.release_reservation(status);
                self.journal.release_reservation(release);
                return Err(error);
            }
        };
        *slot = Some(Offer {
            metadata,
            job: Some(job),
            #[cfg(feature = "experimental-shell-capture")]
            capture: None,
            status: Some(status),
            release: Some(release),
            received: false,
            source: None,
        });
        state.offers.high_water = metadata.offer_id;
        state.offers.dequeue(metadata.offer_id);
        self.inbound.pop_front();
        Ok(job)
    }

    /// Shape and descriptor checks happen before the stale-ID rule. A late
    /// packet is drained by dropping it, without another journal obligation.
    pub(in crate::shell_transport) fn receive_image_source(
        &mut self,
        mut packet: crate::image_channel::ImageDatagram,
    ) -> Result<(), Errno> {
        if crate::owned_ring_protocol::is_source(packet.bytes()) {
            return self.receive_owned_source(packet);
        }
        let state = self.image.as_mut().ok_or(Errno::EPROTO)?;
        let ShellImageReadiness::Ready {
            granted_source_kinds,
        } = state.readiness
        else {
            return Err(Errno::EPROTO);
        };
        let expected = ShellImageExpect {
            role: 1,
            connection_epoch: self.epoch,
            channel_generation: state.config.discovery.channel_generation,
            granted_source_kinds,
            max_width: state.config.limits.max_width,
            max_height: state.config.limits.max_height,
            max_resource_bytes: state.config.limits.max_resource_bytes,
        };
        let ShellImagePacket::Source {
            metadata, planes, ..
        } = ShellImagePacket::decode_with_context(
            packet.bytes(),
            packet.descriptor_count() as u16,
            &expected,
        )
        .map_err(|_| Errno::EPROTO)?
        else {
            return Err(Errno::EPROTO);
        };
        if metadata.offer_id > state.offers.high_water {
            return Err(Errno::EPROTO);
        }
        let Some(offer) = state
            .offers
            .live
            .iter_mut()
            .flatten()
            .find(|offer| offer.metadata.offer_id == metadata.offer_id)
        else {
            return Ok(());
        };
        if offer.release.is_none() {
            return Ok(());
        }
        if metadata != offer.metadata || offer.received {
            return Err(Errno::EPROTO);
        }
        let source = packet.take_descriptor(0).ok_or(Errno::EPROTO)?;
        #[cfg(feature = "experimental-shell-capture")]
        if offer.job.is_none() {
            let limit = state.config.limits.max_dma_source_bytes;
            let others = state
                .offers
                .live
                .iter()
                .flatten()
                .filter(|other| other.metadata.offer_id != metadata.offer_id)
                .filter_map(|other| other.capture.map(|charge| charge.source_bytes))
                .sum::<u64>();
            let offer = state
                .offers
                .live
                .iter_mut()
                .flatten()
                .find(|offer| offer.metadata.offer_id == metadata.offer_id)
                .expect("matched offer");
            let charge = offer.capture.as_mut().ok_or(Errno::EPROTO)?;
            charge.receive(&metadata, planes[0], &source, limit.saturating_sub(others))?;
            offer.source = Some(source);
            offer.received = true;
            return Ok(());
        }
        let _ = planes;
        offer.source = Some(source);
        offer.received = true;
        Ok(())
    }

    pub(in crate::shell_transport) fn take_image_source(&mut self) -> Option<(u64, OwnedFd)> {
        self.image
            .as_mut()?
            .offers
            .live
            .iter_mut()
            .flatten()
            .filter_map(|offer| offer.job.map(|job| (job, offer)))
            .find_map(|(job, offer)| offer.source.take().map(|source| (job, source)))
    }

    pub(super) fn append_image_status(
        &mut self,
        _transaction: TransactionId,
        status: &sophia_protocol::ContentResourceStatus,
        body: &[u8],
    ) -> Result<Option<u64>, Errno> {
        let Some(state) = &mut self.image else {
            return Ok(None);
        };
        // begin_isb consumes a resource generation during jobs.reserve, before
        // admission commits its holds. Two offers cannot both owe a status for
        // this grant/resource identity, even when cancellation changes the tx.
        let owes_status = |offer: &Offer| {
            offer.status.is_some()
                && status.grant.connection_epoch == self.epoch
                && status.grant.content_grant_epoch == offer.metadata.admission_epoch
                && offer.metadata.resource == status.resource.id
                && offer.metadata.resource_generation == status.resource.generation
        };
        debug_assert!(
            state
                .offers
                .live
                .iter()
                .flatten()
                .filter(|offer| owes_status(offer))
                .count()
                <= 1
        );
        let Some(offer) = state
            .offers
            .live
            .iter_mut()
            .flatten()
            .find(|offer| owes_status(offer))
        else {
            return Ok(None);
        };
        if !(2..=4).contains(&status.status) {
            return Err(Errno::EPROTO);
        }
        let sequence = self.journal.append_consuming_reservation(
            offer.status.ok_or(Errno::EINVAL)?,
            ShellFileKind::ResourceStatus,
            body,
        )?;
        offer.status = None;
        state.offers.collect();
        Ok(Some(sequence))
    }

    pub(in crate::shell_transport) fn release_image_source(
        &mut self,
        metadata: ShellImageMetadata,
        disposition: ShellSourceDisposition,
    ) -> Result<u64, Errno> {
        let state = self.image.as_mut().ok_or(Errno::EACCES)?;
        let offer = state
            .offers
            .live
            .iter_mut()
            .flatten()
            .find(|offer| offer.metadata == metadata)
            .ok_or(Errno::ESTALE)?;
        // The independent status hold is consumed only once its record is in
        // the journal. A completed worker alone does not establish that order.
        // Keep the release credit and source custody intact until then.
        if offer.status.is_some() {
            return Err(Errno::EAGAIN);
        }
        let body = ShellSourceReleased {
            offer_id: metadata.offer_id,
            resource: metadata.resource,
            resource_generation: metadata.resource_generation,
            disposition,
        }
        .encode()
        .map_err(|_| Errno::EINVAL)?;
        let sequence = self.journal.append_consuming_reservation(
            offer.release.ok_or(Errno::EINVAL)?,
            ShellFileKind::SourceReleased,
            &body,
        )?;
        offer.release = None;
        offer.source = None;
        state.offers.collect();
        Ok(sequence)
    }
}
