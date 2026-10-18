//! ISB image offers of one connection epoch, from the `ImageBegin`
//! receipt to their terminal status and source release. Custody decides and
//! journals; the owner holds the validation worker and reports its
//! reservations and results here. Each live offer holds two journal
//! credits, for its final status and its source release, taken with its
//! receipt and spent or returned only when it settles. An offer whose
//! source the worker holds settles only on that job's completion; the
//! first cause to stop it (a cancel, channel loss) is kept until then.
use std::time::{Duration, Instant};

use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::{LockFileCustody, LockImageReadiness, LockInbound, reason};
use crate::{IsbValidationFailure, SharedPixels};

/// The contract's `topology` reason: the offer names an earlier topology.
const TOPOLOGY: u16 = 7;

/// Credits each live offer holds: final status and source release.
pub(super) const OFFER_CREDITS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    /// Receipted; the owner has yet to admit or refuse it.
    Pending,
    /// `ImageAdmitted` journalled; the owner holds worker credit `job`.
    /// `sourced` marks a matched Source, so a second one faults.
    Admitted { job: u64, sourced: bool },
    /// The worker holds the source; only its completion settles the offer.
    Validating { job: u64 },
}

/// The first cause to stop an offer whose source the worker holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stop {
    Cancelled,
    ChannelLost,
}

#[derive(Clone, Copy, Debug)]
struct Offer {
    metadata: LockImageMetadata,
    stage: Stage,
    /// Fixed at receipt for a pending offer and at admission after that;
    /// neither a Source nor a retry renews it.
    deadline: Instant,
    stop: Option<Stop>,
}

impl Offer {
    fn resource(&self) -> LockResourceId {
        LockResourceId {
            id: self.metadata.resource,
            generation: self.metadata.resource_generation,
        }
    }

    fn job(&self) -> Option<u64> {
        match self.stage {
            Stage::Pending => None,
            Stage::Admitted { job, .. } | Stage::Validating { job } => Some(job),
        }
    }
}

#[derive(Default)]
pub(super) struct Offers {
    /// In receipt order; at most the channel's `max_offers`.
    live: Vec<Offer>,
    /// The highest offer ID admitted; a Source above it faults the channel.
    admitted_high_water: u64,
    /// Work the owner must do on its worker, in order.
    actions: Vec<LockImageWorkerAction>,
}

impl Offers {
    pub(super) fn len(&self) -> usize {
        self.live.len()
    }

    pub(super) fn resources(&self) -> impl Iterator<Item = LockResourceId> + '_ {
        self.live.iter().map(Offer::resource)
    }

    /// Declared bytes of offers that hold ISB backing or a reservation.
    fn reserved_bytes(&self) -> u64 {
        self.live
            .iter()
            .filter(|offer| offer.stage != Stage::Pending)
            .map(|offer| offer.metadata.declared_bytes)
            .sum()
    }

    fn index(&self, offer_id: u64) -> Option<usize> {
        self.live
            .iter()
            .position(|offer| offer.metadata.offer_id == offer_id)
    }

    fn index_of_job(&self, job: u64) -> Option<usize> {
        self.live.iter().position(|offer| offer.job() == Some(job))
    }
}

/// The oldest pending offer, checked against the lock, ready for the
/// owner's worker reservation of `bytes` under `deadline`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImageAdmission {
    pub offer_id: u64,
    pub bytes: usize,
    pub deadline: Instant,
}

/// What the owner does with a Source packet custody has matched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockImageSource {
    /// Hand the descriptor to the worker under this reserved job, then
    /// report [`LockFileCustody::image_source_submitted`].
    Submit { job: u64 },
    /// A late packet for an offer that has settled: close its descriptors.
    Stale,
}

/// Worker work custody owes the owner, taken with
/// [`LockFileCustody::take_image_worker_action`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockImageWorkerAction {
    /// The reservation of an unsent source is no longer needed.
    Release(u64),
    /// Stop the job; its completion still settles the offer.
    Cancel(u64),
}

fn status(
    offer: &Offer,
    state: LockResourceState,
    reason: u16,
) -> Result<(LockFileKind, Vec<u8>), Errno> {
    LockResourceStatus {
        transaction: offer.metadata.offer_id,
        resource: offer.resource(),
        status: state,
        reason,
        admitted_bytes: 0,
    }
    .encode()
    .map(|body| (LockFileKind::ResourceStatus, body))
    .map_err(|_| Errno::EINVAL)
}

fn released(
    offer: &Offer,
    disposition: LockSourceDisposition,
) -> Result<(LockFileKind, Vec<u8>), Errno> {
    LockSourceReleased {
        offer_id: offer.metadata.offer_id,
        resource: offer.metadata.resource,
        resource_generation: offer.metadata.resource_generation,
        disposition,
    }
    .encode()
    .map(|body| (LockFileKind::SourceReleased, body))
    .map_err(|_| Errno::EINVAL)
}

/// How an offer ends: its events, then the worker action it leaves.
struct Settlement {
    events: Vec<(LockFileKind, Vec<u8>)>,
    action: Option<LockImageWorkerAction>,
}

/// An offer stopped before its source reached the worker.
fn unsent(
    offer: &Offer,
    state: LockResourceState,
    reason: u16,
    expired: bool,
) -> Result<Settlement, Errno> {
    let mut events = vec![status(offer, state, reason)?];
    let mut action = None;
    if let Stage::Admitted { job, .. } = offer.stage {
        // Expiry is the contract's expired-before-use; a stop is a
        // cancellation before import.
        let disposition = if expired {
            LockSourceDisposition::RefusedBeforeUse
        } else {
            LockSourceDisposition::CancelledBeforeImport
        };
        events.push(released(offer, disposition)?);
        action = Some(LockImageWorkerAction::Release(job));
    }
    Ok(Settlement { events, action })
}

impl LockFileCustody {
    fn offers(&self) -> Option<&Offers> {
        self.image.as_ref().map(|image| &image.offers)
    }

    fn offers_mut(&mut self) -> &mut Offers {
        &mut self.image.as_mut().expect("image offers").offers
    }

    fn image_timeout(&self) -> Duration {
        Duration::from_millis(
            self.image
                .as_ref()
                .map_or(0, |image| image.settings().limits.validation_timeout_ms)
                .into(),
        )
    }

    /// Journals the settlements of the offers at `indices`, removing them
    /// and returning their unspent credits, beside `before` events.
    fn settle(
        &mut self,
        before: Vec<(LockFileKind, Vec<u8>)>,
        settlements: Vec<(usize, Settlement)>,
        spent_beside: usize,
    ) -> Result<u64, Errno> {
        let mut events = before;
        let mut actions = Vec::new();
        for (_, settlement) in &settlements {
            events.extend(settlement.events.iter().cloned());
            actions.extend(settlement.action);
        }
        let credits = self.credits() - spent_beside - OFFER_CREDITS * settlements.len();
        let sequence = self.journal_events(&events, credits)?;
        let mut indices: Vec<usize> = settlements.iter().map(|(index, _)| *index).collect();
        indices.sort_unstable();
        let offers = self.offers_mut();
        for index in indices.into_iter().rev() {
            offers.live.remove(index);
        }
        offers.actions.extend(actions);
        Ok(sequence)
    }

    /// `ImageBegin` on a ready channel. Identity and capability errors fail
    /// the write without a receipt. Both terminal credits are taken with the
    /// receipt, so no receipted offer can lack room for its end. With no
    /// offer or live-resource room left the receipt is followed at once by
    /// a budget refusal, which needs no held credit.
    pub(super) fn offer_image(
        &mut self,
        receipt: (LockFileKind, Vec<u8>),
        metadata: LockImageMetadata,
        now: Instant,
    ) -> Result<(), Errno> {
        if metadata.source_kind != LOCK_IMAGE_SOURCE_SEALED_MEMORY
            || metadata.source_format != LOCK_IMAGE_SEALED_FORMAT
        {
            return Err(Errno::EINVAL);
        }
        let resource = LockResourceId {
            id: metadata.resource,
            generation: metadata.resource_generation,
        };
        let offers = self.offers().expect("ready image");
        if self.resources.holds(resource) || offers.resources().any(|held| held == resource) {
            return Err(Errno::EINVAL);
        }
        let max_offers = self
            .image
            .as_ref()
            .expect("ready image")
            .settings()
            .limits
            .max_offers;
        let full = offers.len() >= max_offers as usize
            || self.resources.live() + offers.len() >= usize::from(self.limits.max_live_resources);
        let offer = Offer {
            metadata,
            stage: Stage::Pending,
            deadline: now + self.image_timeout(),
            stop: None,
        };
        if full {
            let refusal = status(&offer, LockResourceState::Rejected, reason::BUDGET)?;
            self.journal_events(&[receipt, refusal], self.credits())?;
        } else {
            self.journal_events(&[receipt], self.credits() + OFFER_CREDITS)?;
            self.offers_mut().live.push(offer);
        }
        Ok(())
    }

    /// Refuses pending offers that expired or no longer fit the lock, and
    /// returns the oldest that does, for the owner to reserve worker credit
    /// and call [`Self::admit_image`] or [`Self::refuse_image`].
    pub fn image_admission(&mut self, now: Instant) -> Result<Option<LockImageAdmission>, Errno> {
        self.live()?;
        if !matches!(self.image_readiness(), LockImageReadiness::Ready { .. }) {
            return Ok(None);
        }
        loop {
            let Some(offers) = self.offers() else {
                return Ok(None);
            };
            let Some(index) = offers
                .live
                .iter()
                .position(|offer| offer.stage == Stage::Pending)
            else {
                return Ok(None);
            };
            let offer = offers.live[index];
            let metadata = offer.metadata;
            let image = self.image.as_ref().expect("ready image").settings();
            let fits_lock = |lock: &LockObject| {
                lock.allocations.iter().any(|allocation| {
                    allocation.output_id == metadata.output_id
                        && allocation.allocation_id == metadata.allocation_id
                })
            };
            let sealed = self.resources.isb_bytes() + offers.reserved_bytes();
            let refusal = if now >= offer.deadline {
                Some(LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT)
            } else if metadata.admission_epoch != self.lock.lock_epoch {
                Some(reason::STALE_LOCK)
            } else if metadata.topology_generation != self.lock.topology_generation {
                Some(TOPOLOGY)
            } else if !fits_lock(&self.lock) {
                Some(reason::STALE_ALLOCATION)
            } else if metadata.width > image.limits.max_width
                || metadata.height > image.limits.max_height
                || metadata.declared_bytes > image.limits.max_resource_bytes
                || sealed + metadata.declared_bytes > image.limits.max_sealed_backing_bytes
            {
                Some(reason::BUDGET)
            } else {
                None
            };
            match refusal {
                Some(reason) => {
                    let settlement = unsent(&offer, LockResourceState::Rejected, reason, true)?;
                    self.settle(Vec::new(), vec![(index, settlement)], 0)?;
                }
                None => {
                    return Ok(Some(LockImageAdmission {
                        offer_id: metadata.offer_id,
                        bytes: usize::try_from(metadata.declared_bytes)
                            .map_err(|_| Errno::EINVAL)?,
                        deadline: now + self.image_timeout(),
                    }));
                }
            }
        }
    }

    /// Journals `ImageAdmitted` for a pending offer whose worker credit
    /// `job` the owner reserved under `deadline`. On `EAGAIN` the journal
    /// has no room yet: the offer stays pending and the owner releases
    /// `job`.
    pub fn admit_image(
        &mut self,
        offer_id: u64,
        job: u64,
        deadline: Instant,
    ) -> Result<u64, Errno> {
        self.live()?;
        let offers = self.offers().ok_or(Errno::EACCES)?;
        let index = offers.index(offer_id).ok_or(Errno::ESTALE)?;
        let offer = offers.live[index];
        if offer.stage != Stage::Pending || offers.index_of_job(job).is_some() {
            return Err(Errno::EINVAL);
        }
        let body = offer.metadata.encode().map_err(|_| Errno::EINVAL)?;
        let sequence = self.journal_events(
            &[(LockFileKind::ImageAdmitted, body.to_vec())],
            self.credits(),
        )?;
        let offers = self.offers_mut();
        offers.live[index].stage = Stage::Admitted {
            job,
            sourced: false,
        };
        offers.live[index].deadline = deadline;
        offers.admitted_high_water = offers.admitted_high_water.max(offer_id);
        Ok(sequence)
    }

    /// Refuses a pending offer the owner cannot admit, such as for want of
    /// worker credit (`budget`).
    pub fn refuse_image(&mut self, offer_id: u64, reason: u16) -> Result<u64, Errno> {
        self.live()?;
        let offers = self.offers().ok_or(Errno::EACCES)?;
        let index = offers.index(offer_id).ok_or(Errno::ESTALE)?;
        let offer = offers.live[index];
        if offer.stage != Stage::Pending || reason == reason::NONE {
            return Err(Errno::EINVAL);
        }
        let settlement = unsent(&offer, LockResourceState::Rejected, reason, true)?;
        self.settle(Vec::new(), vec![(index, settlement)], 0)
    }

    /// Matches a well-formed Source packet carrying `descriptors` received
    /// references. `Err` faults the channel: a foreign identity, an offer
    /// above the admitted mark, a second Source, or metadata that differs
    /// from the admission. A settled offer at or below the mark is stale.
    pub fn image_source(
        &mut self,
        packet: &LockImagePacket,
        descriptors: usize,
    ) -> Result<LockImageSource, Errno> {
        self.live()?;
        let LockImagePacket::Source {
            connection_epoch,
            channel_generation,
            metadata,
            ..
        } = packet
        else {
            return Err(Errno::EPROTO);
        };
        let image = self.image.as_ref().ok_or(Errno::EPROTO)?;
        if *connection_epoch != self.epoch
            || *channel_generation != image.settings().discovery.channel_generation
            || descriptors != usize::from(metadata.descriptor_count)
        {
            return Err(Errno::EPROTO);
        }
        let offers = &image.offers;
        if metadata.offer_id > offers.admitted_high_water {
            return Err(Errno::EPROTO);
        }
        let Some(index) = offers.index(metadata.offer_id) else {
            return Ok(LockImageSource::Stale);
        };
        let offer = offers.live[index];
        let Stage::Admitted {
            job,
            sourced: false,
        } = offer.stage
        else {
            return Err(Errno::EPROTO);
        };
        if offer.metadata != *metadata {
            return Err(Errno::EPROTO);
        }
        self.offers_mut().live[index].stage = Stage::Admitted { job, sourced: true };
        Ok(LockImageSource::Submit { job })
    }

    /// The worker accepted the source for `job`. A refused hand-over
    /// leaves the offer admitted with its reservation, to settle at its
    /// deadline; the owner reports nothing then.
    pub fn image_source_submitted(&mut self, job: u64) -> Result<(), Errno> {
        let offers = self.offers().ok_or(Errno::ESTALE)?;
        let index = offers.index_of_job(job).ok_or(Errno::ESTALE)?;
        if offers.live[index].stage != (Stage::Admitted { job, sourced: true }) {
            return Err(Errno::EINVAL);
        }
        self.offers_mut().live[index].stage = Stage::Validating { job };
        Ok(())
    }

    /// Settles the offer of a completed `job`. The first stop wins over the
    /// result; otherwise validated pixels become the resource, and a
    /// refused source or timeout rejects it: a wrong backing length as
    /// size mismatch, any other invalid backing as an invalid source.
    /// `ESTALE` names no live job.
    pub fn image_completed(
        &mut self,
        job: u64,
        result: Result<SharedPixels, IsbValidationFailure>,
    ) -> Result<u64, Errno> {
        self.live()?;
        let offers = self.offers().ok_or(Errno::ESTALE)?;
        let index = offers.index_of_job(job).ok_or(Errno::ESTALE)?;
        let offer = offers.live[index];
        if offer.stage != (Stage::Validating { job }) {
            return Err(Errno::EINVAL);
        }
        use LockResourceState::*;
        use LockSourceDisposition::*;
        let (state, reason, disposition) = match (offer.stop, &result) {
            (Some(Stop::Cancelled), _) => (Cancelled, reason::NONE, CancelledBeforeImport),
            (Some(Stop::ChannelLost), _) => (
                Rejected,
                LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE,
                CancelledBeforeImport,
            ),
            (None, Ok(_)) => (Accepted, reason::NONE, Retained),
            (None, Err(IsbValidationFailure::Malformed)) => (
                Rejected,
                LOCK_RESOURCE_REASON_INVALID_SOURCE,
                RefusedBeforeUse,
            ),
            (None, Err(IsbValidationFailure::SizeMismatch)) => {
                (Rejected, reason::SIZE_MISMATCH, RefusedBeforeUse)
            }
            (None, Err(IsbValidationFailure::Timeout | IsbValidationFailure::Cancelled)) => (
                Rejected,
                LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT,
                RefusedBeforeUse,
            ),
        };
        let settlement = Settlement {
            events: vec![
                status(&offer, state, reason)?,
                released(&offer, disposition)?,
            ],
            action: None,
        };
        let sequence = self.settle(Vec::new(), vec![(index, settlement)], 0)?;
        if let (Accepted, Ok(pixels)) = (state, result) {
            let resource = offer.resource();
            let (width_px, height_px) = (offer.metadata.width, offer.metadata.height);
            self.resources
                .insert_isb(resource, width_px, height_px, pixels.clone());
            self.inbound.push_back(LockInbound::ResourceReady {
                resource,
                width_px,
                height_px,
                pixels,
            });
        }
        Ok(sequence)
    }

    /// `ResourceCancel` naming a live offer: its offer ID as transaction
    /// and its exact resource. An unsent offer settles now; one with the
    /// worker records the cancel and settles on completion. A second stop
    /// fails the write.
    pub(super) fn cancel_image(
        &mut self,
        receipt: (LockFileKind, Vec<u8>),
        index: usize,
    ) -> Result<(), Errno> {
        let offer = self.offers().expect("live offer").live[index];
        if offer.stop.is_some() {
            return Err(Errno::EINVAL);
        }
        match offer.stage {
            Stage::Validating { job } => {
                self.journal_events(&[receipt], self.credits())?;
                let offers = self.offers_mut();
                offers.live[index].stop = Some(Stop::Cancelled);
                offers.actions.push(LockImageWorkerAction::Cancel(job));
            }
            _ => {
                let settlement = unsent(&offer, LockResourceState::Cancelled, reason::NONE, false)?;
                self.settle(vec![receipt], vec![(index, settlement)], 0)?;
            }
        }
        Ok(())
    }

    /// The live offer a `ResourceCancel` step names, if any.
    pub(super) fn image_offer_for(
        &self,
        transaction: u64,
        resource: LockResourceId,
    ) -> Option<usize> {
        let offers = self.offers()?;
        offers
            .index(transaction)
            .filter(|index| offers.live[*index].resource() == resource)
    }

    /// Channel loss: `ChannelClosed`, then every unsent offer rejected with
    /// reason 9; offers with the worker keep it until completion. Spends
    /// the channel's held credit with the offers'.
    pub(super) fn close_image_offers(
        &mut self,
        closed: (LockFileKind, Vec<u8>),
        close_credit: usize,
    ) -> Result<u64, Errno> {
        let offers = self.offers().map_or(&[][..], |offers| &offers.live[..]);
        let mut settlements = Vec::new();
        let mut cancels = Vec::new();
        for (index, offer) in offers.iter().enumerate() {
            match offer.stage {
                Stage::Validating { job } => {
                    if offer.stop.is_none() {
                        cancels.push((index, job));
                    }
                }
                _ => settlements.push((
                    index,
                    unsent(
                        offer,
                        LockResourceState::Rejected,
                        LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE,
                        false,
                    )?,
                )),
            }
        }
        let sequence = self.settle(vec![closed], settlements, close_credit)?;
        // Removal shifted indices; the stopped offers are found by job.
        let offers = self.offers_mut();
        for (_, job) in cancels {
            let index = offers.index_of_job(job).expect("validating offer");
            offers.live[index].stop = Some(Stop::ChannelLost);
            offers.actions.push(LockImageWorkerAction::Cancel(job));
        }
        Ok(sequence)
    }

    /// Settles offers past their deadline: pending ones and admitted ones
    /// whose source never reached the worker, as validation timeouts.
    /// Validating offers end by the worker's own deadline. Owners drain
    /// delivered completions before calling this.
    pub fn expire_images(&mut self, now: Instant) -> Result<bool, Errno> {
        self.live()?;
        let Some(offers) = self.offers() else {
            return Ok(false);
        };
        let settlements = offers
            .live
            .iter()
            .enumerate()
            .filter(|(_, offer)| {
                now >= offer.deadline && !matches!(offer.stage, Stage::Validating { .. })
            })
            .map(|(index, offer)| {
                unsent(
                    offer,
                    LockResourceState::Rejected,
                    LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT,
                    true,
                )
                .map(|settlement| (index, settlement))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if settlements.is_empty() {
            return Ok(false);
        }
        self.settle(Vec::new(), settlements, 0)?;
        Ok(true)
    }

    /// The earliest deadline custody itself enforces.
    pub fn image_deadline(&self) -> Option<Instant> {
        self.offers()?
            .live
            .iter()
            .filter(|offer| !matches!(offer.stage, Stage::Validating { .. }))
            .map(|offer| offer.deadline)
            .min()
    }

    /// Worker reservations to release and jobs to cancel, in order.
    pub fn take_image_worker_action(&mut self) -> Option<LockImageWorkerAction> {
        let offers = &mut self.image.as_mut()?.offers;
        (!offers.actions.is_empty()).then(|| offers.actions.remove(0))
    }
}
