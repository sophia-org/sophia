//! The owned-ring producer plane: shell revision 10's sole CPU image
//! source. One registration per connection fixes the slots and the
//! per-slot capacity; each upload copies its exact bytes into a free
//! slot and an offer is adoptable only after its successful
//! `ResourceStatus` and the matching successful `SlotReleased`.
//! The final status must precede the release; an exact release seen
//! before any status is a Session ordering violation that fails the
//! ring and makes live offers unknown. Status is matched by grant and
//! resource identity, never by transaction alone (a cancellation may
//! carry the cancel transaction); failed control replies are not
//! settlement and never touch slot custody: observed status kinds
//! accumulate and the release's own reason selects the compatible
//! evidence. Only the exact `SlotReleased` frees a slot.
//!
//! Like [`ImageProducer`](crate::ImageProducer), this producer never
//! uses 9P: the role client writes the control bytes this type returns
//! to the `owned-ring-v1` file and forwards journal records back in. All
//! time is caller-supplied monotonic milliseconds.
use std::os::fd::BorrowedFd;

use sophia_shell_protocol::shell_files::{
    OWNED_RING_HEADER_BYTES, RingEvent, RingEventKind, RingIdentity, RingOffer, RingRegistration,
};

use crate::{CloseCause, ProducerError, ResourceStatusKind};

/// Ring construction parameters. Zero `slots` means one slot; zero
/// `capacity` means the bounds' `max_resource_bytes` rounded down to a
/// multiple of four. Capacity is fixed at registration and never grows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingProducerConfig {
    pub identity: RingIdentity,
    pub slots: u8,
    pub capacity: u64,
    pub max_resource_bytes: u64,
    /// The role's transfer deadline; nonzero.
    pub transfer_timeout_ms: u32,
}

/// One upload's identities and dimensions. Offer ids and slot
/// generations are assigned by the producer, never the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingUpload {
    pub resource: u64,
    pub resource_generation: u64,
    pub allocation: u64,
    pub allocation_generation: u64,
    pub output: u64,
    pub output_generation: u64,
    pub topology_generation: u64,
    pub width: u32,
    pub height: u32,
}

/// One ring offer's custody state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingOfferState {
    /// Written to a slot; settlement incomplete.
    Pending,
    /// Successful status and exact successful release both observed:
    /// the image is settled and adoptable.
    Settled,
    /// Terminally refused or cancelled; never adoptable.
    Refused,
    /// Journal loss ended custody proof; never adoptable or reusable.
    Unknown,
}

/// A snapshot of one ring offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingOfferInfo {
    pub offer_id: u64,
    pub resource: u64,
    pub resource_generation: u64,
    pub width: u32,
    pub height: u32,
    pub slot: u32,
    pub state: RingOfferState,
    pub status: Option<ResourceStatusKind>,
    /// Local expiry only; custody is unchanged by it.
    pub deadline_passed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    /// Registration bytes not yet handed out.
    New,
    /// Registration written; awaiting RingAdmitted.
    Registering,
    /// RingAdmitted observed; source packet not yet sent.
    Admitted,
    /// Source packet sent; awaiting RingReady.
    Sourcing,
    /// Offers may begin.
    Ready,
    Closed,
}

#[derive(Clone, Copy, Default)]
struct RingSlot {
    generation: u64,
    offer: Option<u64>,
    acquired: bool,
}

#[derive(Clone, Copy, Default)]
struct Offer {
    upload: Option<RingUpload>,
    offer_id: u64,
    slot: u32,
    generation: u64,
    state: Option<RingOfferState>,
    status: Option<ResourceStatusKind>,
    /// Bounded evidence set of observed terminal status kinds
    /// (bit per kind; only three kinds exist). Settlement at the
    /// exact release picks the evidence compatible with the
    /// release's own reason, because failed control replies share
    /// the status kind and carry no store provenance.
    statuses: u8,
    released: bool,
    deadline_ms: u64,
    deadline_passed: bool,
}

const STATUS_ACCEPTED: u8 = 1;
fn reported_status(offer: &Offer) -> Option<ResourceStatusKind> {
    // While pending, an observed Accepted outranks later failed
    // control replies. Once the release has selected the terminal
    // evidence, the chosen status is authoritative.
    if offer.state == Some(RingOfferState::Pending) && offer.statuses & STATUS_ACCEPTED != 0 {
        Some(ResourceStatusKind::Accepted)
    } else {
        offer.status
    }
}

const STATUS_REJECTED: u8 = 2;
const STATUS_CANCELLED: u8 = 4;

fn status_bit(kind: ResourceStatusKind) -> u8 {
    match kind {
        ResourceStatusKind::Accepted => STATUS_ACCEPTED,
        ResourceStatusKind::Rejected => STATUS_REJECTED,
        ResourceStatusKind::Cancelled => STATUS_CANCELLED,
    }
}

/// The owned-ring producer state machine. See the module documentation.
pub struct RingProducer {
    config: RingProducerConfig,
    registration: RingRegistration,
    phase: Phase,
    cause: CloseCause,
    control_error: Option<u32>,
    ring: Option<sophia_isb::MappedRing>,
    slots: [RingSlot; 4],
    offers: [Offer; 4],
    next_offer: u64,
    last_now: u64,
}

impl RingProducer {
    pub fn new(config: RingProducerConfig, now_ms: u64) -> Result<Self, ProducerError> {
        let slots = if config.slots == 0 { 1 } else { config.slots };
        let capacity = if config.capacity == 0 {
            config.max_resource_bytes & !3
        } else {
            config.capacity
        };
        if config.identity.connection_epoch == 0
            || config.identity.content_grant_epoch == 0
            || config.identity.channel_generation == 0
            || config.identity.ring == 0
            || config.identity.generation == 0
            || !(1..=4).contains(&slots)
            || !(4..=sophia_shell_protocol::shell_files::OWNED_RING_MAX_CAPACITY_BYTES)
                .contains(&capacity)
            || !capacity.is_multiple_of(4)
            || capacity > config.max_resource_bytes
            || config.transfer_timeout_ms == 0
        {
            return Err(ProducerError::Argument);
        }
        let registration = RingRegistration {
            identity: config.identity,
            slots: u32::from(slots),
            capacity,
        };
        Ok(Self {
            config,
            registration,
            phase: Phase::New,
            cause: CloseCause::None,
            control_error: None,
            ring: None,
            slots: [RingSlot::default(); 4],
            offers: [Offer::default(); 4],
            next_offer: 1,
            last_now: now_ms,
        })
    }

    fn clock(&mut self, now_ms: u64) -> Result<(), ProducerError> {
        if now_ms < self.last_now {
            return Err(ProducerError::Argument);
        }
        self.last_now = now_ms;
        Ok(())
    }

    fn fail(&mut self, cause: CloseCause) {
        if self.phase != Phase::Closed || (self.cause.is_clean() && !cause.is_clean()) {
            self.cause = cause;
        }
        self.phase = Phase::Closed;
    }

    fn fault(&mut self) -> ProducerError {
        self.fail(CloseCause::Fault);
        ProducerError::Closed
    }

    fn fault_unknown(&mut self) -> ProducerError {
        for offer in &mut self.offers {
            if offer.state == Some(RingOfferState::Pending) {
                offer.state = Some(RingOfferState::Unknown);
            }
        }
        self.fault()
    }

    /// The registration in force for this connection.
    pub fn registration_info(&self) -> RingRegistration {
        self.registration
    }

    /// The 72-byte D4C1 Register record, handed out exactly once. The
    /// caller writes it to the `owned-ring-v1` control file. A budget
    /// refusal arrives later as `RingRefused` and is final.
    pub fn registration(&mut self) -> Option<Vec<u8>> {
        if self.phase != Phase::New {
            return None;
        }
        self.phase = Phase::Registering;
        Some(self.registration.encode())
    }

    /// The 72-byte D4F1 source packet and its descriptor, available once
    /// after `RingAdmitted`. The caller sends both on the image channel
    /// (exactly one `SCM_RIGHTS` fd), then calls
    /// [`source_sent`](Self::source_sent).
    pub fn source_packet(&self) -> Option<(Vec<u8>, BorrowedFd<'_>)> {
        if self.phase != Phase::Admitted {
            return None;
        }
        let ring = self.ring.as_ref()?;
        Some((self.registration.source_packet(), ring.source_fd()))
    }

    pub fn source_sent(&mut self) -> Result<(), ProducerError> {
        if self.phase != Phase::Admitted {
            return Err(ProducerError::Refused);
        }
        self.phase = Phase::Sourcing;
        Ok(())
    }

    /// Forward one D4E1 journal event. Identity must match the
    /// registered ring exactly; anything else is a channel fault.
    pub fn ring_event(&mut self, event: RingEvent, now_ms: u64) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        if event.identity != self.config.identity {
            return Err(self.fault());
        }
        match event.kind {
            RingEventKind::RingAdmitted => {
                if self.phase != Phase::Registering {
                    return Err(self.fault());
                }
                let bytes = self
                    .registration
                    .source_bytes()
                    .ok_or(ProducerError::Refused)?;
                match sophia_isb::MappedRing::create(bytes) {
                    Ok(ring) => {
                        self.ring = Some(ring);
                        self.phase = Phase::Admitted;
                        Ok(())
                    }
                    Err(_) => Err(self.fault()),
                }
            }
            RingEventKind::RingReady => {
                if self.phase != Phase::Sourcing {
                    return Err(self.fault());
                }
                self.phase = Phase::Ready;
                Ok(())
            }
            RingEventKind::RingRefused => {
                if !matches!(
                    self.phase,
                    Phase::Registering | Phase::Admitted | Phase::Sourcing
                ) {
                    return Err(self.fault());
                }
                self.fail(CloseCause::Unavailable);
                Ok(())
            }
            RingEventKind::SlotReleased => self.slot_released(event),
        }
    }

    fn slot_released(&mut self, event: RingEvent) -> Result<(), ProducerError> {
        let Some(index) = self.offers.iter().position(|offer| {
            offer.state == Some(RingOfferState::Pending) && offer.offer_id == event.offer_id
        }) else {
            return Err(self.fault_unknown());
        };
        let offer = self.offers[index];
        let upload = offer.upload.ok_or(ProducerError::Closed)?;
        if event.slot != offer.slot
            || event.slot_generation != offer.generation
            || event.resource != upload.resource
            || event.resource_generation != upload.resource_generation
        {
            return Err(self.fault_unknown());
        }
        // The contract's settlement order is final status, then release.
        // An otherwise exact release with no observed status is therefore
        // a Session ordering violation, not an early settlement phase.
        if offer.statuses == 0 {
            return Err(self.fault_unknown());
        }
        // The release is authoritative for custody; its reason selects
        // the compatible observed evidence, because the runtime pins
        // the terminal status kind to the release reason: success
        // (reason 0, no changed generation) requires an observed
        // Accepted, a cancellation release (reason
        // `ContentReason::Cancelled`) requires an observed Cancelled,
        // and every other refusal release requires an observed
        // Rejected. No compatible evidence is a Session ordering
        // violation, never a guessed settlement.
        let cancelled_reason =
            event.reason == sophia_shell_protocol::ContentReason::Cancelled as u16;
        let (settled, chosen) = if event.reason == 0 && !event.changed_generation {
            if offer.statuses & STATUS_ACCEPTED == 0 {
                return Err(self.fault_unknown());
            }
            (true, ResourceStatusKind::Accepted)
        } else if cancelled_reason {
            // A changed generation is a read-failure flag the runtime
            // sets independently of the stopped reason, so a cancelled
            // offer can legitimately carry it; it is never adoptable
            // either way, but still requires Cancelled evidence.
            if offer.statuses & STATUS_CANCELLED == 0 {
                return Err(self.fault_unknown());
            }
            (false, ResourceStatusKind::Cancelled)
        } else {
            if offer.statuses & STATUS_REJECTED == 0 {
                return Err(self.fault_unknown());
            }
            (false, ResourceStatusKind::Rejected)
        };
        self.offers[index].released = true;
        self.free_slot(offer.slot, offer.offer_id);
        // Report the evidence the release selected, not whatever
        // status happened to be observed last: a failed control
        // reply after the genuine Accepted must not leave a Settled
        // offer reporting Rejected.
        self.offers[index].status = Some(chosen);
        self.offers[index].state = Some(if settled {
            RingOfferState::Settled
        } else {
            RingOfferState::Refused
        });
        Ok(())
    }

    fn free_slot(&mut self, slot: u32, offer_id: u64) {
        let slot = &mut self.slots[slot as usize];
        if slot.offer == Some(offer_id) {
            slot.offer = None;
        }
    }

    /// Acquire a free slot for direct fill: the caller writes pixels
    /// into the mapped slot itself (no producer copy) and commits with
    /// the offer identities and exact byte count. Dropping or aborting
    /// the guard frees the slot without spending a generation. This is
    /// the direct-fill counterpart of [`upload`](Self::upload); the
    /// benchmark's producer-side measurement uses this path, while
    /// `upload` costs one slice-to-slot copy.
    pub fn acquire_slot(&mut self, now_ms: u64) -> Result<RingSlotFill<'_>, ProducerError> {
        self.clock(now_ms)?;
        if self.phase == Phase::Closed {
            return Err(ProducerError::Closed);
        }
        if self.phase != Phase::Ready {
            return Err(ProducerError::Again);
        }
        let Some(slot_index) = self.slots[..self.registration.slots as usize]
            .iter()
            .position(|slot| slot.offer.is_none() && !slot.acquired)
        else {
            return Err(ProducerError::Busy);
        };
        self.slots[slot_index].acquired = true;
        Ok(RingSlotFill {
            producer: self,
            slot: slot_index as u32,
            committed: false,
        })
    }

    /// Copy `pixels` into a free slot, write its generation word, and
    /// return the assigned offer id plus the 152-byte D4C1 Offer record
    /// for the control file. The exact byte count must fit the fixed
    /// capacity; over-capacity uploads are refused before any write.
    /// The slot does not change again until its exact `SlotReleased`.
    /// This convenience path costs one producer copy; direct fill via
    /// [`acquire_slot`](Self::acquire_slot) costs none.
    pub fn upload(
        &mut self,
        upload: RingUpload,
        pixels: &[u8],
        now_ms: u64,
    ) -> Result<(u64, Vec<u8>), ProducerError> {
        self.clock(now_ms)?;
        if self.phase == Phase::Closed {
            return Err(ProducerError::Closed);
        }
        if self.phase != Phase::Ready {
            return Err(ProducerError::Again);
        }
        let bytes = u64::from(upload.width)
            .checked_mul(u64::from(upload.height))
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or(ProducerError::Argument)?;
        if upload.width == 0
            || upload.height == 0
            || bytes != pixels.len() as u64
            || upload.resource == 0
            || upload.resource_generation == 0
            || upload.allocation == 0
            || upload.allocation_generation == 0
            || upload.output == 0
            || upload.output_generation == 0
            || upload.topology_generation == 0
        {
            return Err(ProducerError::Argument);
        }
        if bytes > self.registration.capacity {
            return Err(ProducerError::Refused);
        }
        let slot_index = self.free_slot_index().ok_or(ProducerError::Busy)?;
        {
            let ring = self.ring.as_mut().ok_or(ProducerError::Closed)?;
            let extent = usize::try_from(self.registration.slot_extent()).unwrap();
            let offset = slot_index * extent;
            let slot = &mut ring.bytes_mut()[offset..offset + extent];
            slot[OWNED_RING_HEADER_BYTES as usize..OWNED_RING_HEADER_BYTES as usize + pixels.len()]
                .copy_from_slice(pixels);
        }
        self.commit_slot(slot_index, upload, bytes, now_ms)
    }

    fn free_slot_index(&self) -> Option<usize> {
        self.slots[..self.registration.slots as usize]
            .iter()
            .position(|slot| slot.offer.is_none() && !slot.acquired)
    }

    /// Shared commit for [`upload`](Self::upload) and direct fill:
    /// pixels are already in the slot; validate the offer, assign the
    /// next even generation, write the generation word last, and mark
    /// the slot busy until its exact `SlotReleased`.
    fn commit_slot(
        &mut self,
        slot_index: usize,
        upload: RingUpload,
        byte_len: u64,
        now_ms: u64,
    ) -> Result<(u64, Vec<u8>), ProducerError> {
        let bytes = u64::from(upload.width)
            .checked_mul(u64::from(upload.height))
            .and_then(|bytes| bytes.checked_mul(4))
            .filter(|bytes| *bytes == byte_len)
            .ok_or(ProducerError::Argument)?;
        if upload.width == 0
            || upload.height == 0
            || upload.resource == 0
            || upload.resource_generation == 0
            || upload.allocation == 0
            || upload.allocation_generation == 0
            || upload.output == 0
            || upload.output_generation == 0
            || upload.topology_generation == 0
        {
            return Err(ProducerError::Argument);
        }
        if bytes > self.registration.capacity {
            return Err(ProducerError::Refused);
        }
        if self.next_offer == 0 {
            self.fail(CloseCause::Exhausted);
            return Err(ProducerError::Exhausted);
        }
        if self.offers.iter().any(|offer| {
            offer.state == Some(RingOfferState::Pending)
                && offer.upload.is_some_and(|live| {
                    live.resource == upload.resource
                        && live.resource_generation == upload.resource_generation
                })
        }) {
            return Err(ProducerError::Refused);
        }
        let Some(offer_index) = self.offers.iter().position(|offer| offer.state.is_none()) else {
            return Err(ProducerError::Busy);
        };
        // Checked before any mutation: an exhausted per-slot
        // generation (or the offer-id sentinel) closes the ring
        // instead of wrapping.
        let Some(generation) = self.slots[slot_index]
            .generation
            .checked_add(2)
            .filter(|generation| *generation != 0 && generation.is_multiple_of(2))
        else {
            self.fail(CloseCause::Exhausted);
            return Err(ProducerError::Exhausted);
        };
        let offer_id = self.next_offer;
        {
            let ring = self.ring.as_mut().ok_or(ProducerError::Closed)?;
            let extent = usize::try_from(self.registration.slot_extent()).unwrap();
            let offset = slot_index * extent;
            ring.bytes_mut()[offset..offset + 8].copy_from_slice(&generation.to_le_bytes());
        }
        // Wraps to the zero sentinel at the last id; the check at
        // the top of this function refuses the next commit.
        self.next_offer = offer_id.wrapping_add(1);
        self.slots[slot_index].generation = generation;
        self.slots[slot_index].offer = Some(offer_id);
        self.slots[slot_index].acquired = false;
        self.offers[offer_index] = Offer {
            upload: Some(upload),
            offer_id,
            slot: slot_index as u32,
            generation,
            state: Some(RingOfferState::Pending),
            status: None,
            statuses: 0,
            released: false,
            deadline_ms: now_ms.saturating_add(u64::from(self.config.transfer_timeout_ms)),
            deadline_passed: false,
        };
        let offer = RingOffer {
            identity: self.config.identity,
            offer_id,
            slot: slot_index as u32,
            slot_generation: generation,
            resource: upload.resource,
            resource_generation: upload.resource_generation,
            allocation: upload.allocation,
            allocation_generation: upload.allocation_generation,
            output: upload.output,
            output_generation: upload.output_generation,
            topology_generation: upload.topology_generation,
            width: upload.width,
            height: upload.height,
        };
        Ok((offer_id, offer.encode()))
    }

    /// The control file definitively refused an offer write (EINVAL,
    /// ESTALE or EALREADY). The offer id and slot generation stay spent;
    /// the offer is refused and its slot frees, since nothing was
    /// accepted by the control file. EAGAIN is not reported here: the
    /// caller retries the identical bytes.
    pub fn begin_refused(&mut self, offer_id: u64, now_ms: u64) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        let Some(index) = self.offers.iter().position(|offer| {
            offer.state == Some(RingOfferState::Pending) && offer.offer_id == offer_id
        }) else {
            return Err(ProducerError::Refused);
        };
        let offer = self.offers[index];
        self.offers[index].state = Some(RingOfferState::Refused);
        self.free_slot(offer.slot, offer_id);
        Ok(())
    }

    /// Forward a `ResourceStatus` for an offer's resource, matched by
    /// grant and resource identity only. A status never settles on its
    /// own. Failed control replies (for example a refused
    /// `ResourceRetire` or `ResourceCancel`) share this record kind
    /// but carry no store provenance on the wire, and they can arrive
    /// before or after the genuine final status. Observed kinds are
    /// therefore accumulated (a bounded three-kind set), and the
    /// exact `SlotReleased` that must follow selects the evidence
    /// compatible with its own reason: success requires an observed
    /// Accepted, a cancellation release an observed Cancelled, and
    /// any other refusal release an observed Rejected. A release
    /// seen before any status at all, or with no compatible
    /// evidence, faults in [`ring_event`](Self::ring_event) instead.
    pub fn resource_status(
        &mut self,
        resource: u64,
        resource_generation: u64,
        status: ResourceStatusKind,
        now_ms: u64,
    ) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        let Some(index) = self.offers.iter().position(|offer| {
            offer.state == Some(RingOfferState::Pending)
                && offer.upload.is_some_and(|upload| {
                    upload.resource == resource && upload.resource_generation == resource_generation
                })
        }) else {
            return Ok(());
        };
        // Accumulate evidence; settlement at the exact release
        // picks the kind compatible with the release reason, so a
        // failed control reply recorded here can neither spend the
        // offer's settlement nor poison a genuine success.
        self.offers[index].status = Some(status);
        self.offers[index].statuses |= status_bit(status);
        Ok(())
    }

    /// The pending offer holding this resource identity, if any.
    /// Status routing matches on this, never on a transaction alone.
    pub fn offer_for_resource(
        &self,
        resource: u64,
        resource_generation: u64,
    ) -> Option<RingOfferInfo> {
        self.offers
            .iter()
            .find(|offer| {
                offer.state == Some(RingOfferState::Pending)
                    && offer.upload.is_some_and(|upload| {
                        upload.resource == resource
                            && upload.resource_generation == resource_generation
                    })
            })
            .map(|offer| RingOfferInfo {
                offer_id: offer.offer_id,
                resource,
                resource_generation,
                width: offer.upload.map_or(0, |upload| upload.width),
                height: offer.upload.map_or(0, |upload| upload.height),
                slot: offer.slot,
                state: RingOfferState::Pending,
                status: reported_status(offer),
                deadline_passed: offer.deadline_passed,
            })
    }

    /// The image channel closed under this ring: no offer settles
    /// afterward on this connection, and live offers become Unknown
    /// (Session's reset clears its credits without journaling
    /// `SlotReleased`).
    pub fn channel_closed(&mut self) {
        for offer in &mut self.offers {
            if offer.state == Some(RingOfferState::Pending) {
                offer.state = Some(RingOfferState::Unknown);
            }
        }
        self.fail(CloseCause::Fault);
    }

    /// A definitive control-write refusal of the registration itself
    /// (the offer form is [`begin_refused`](Self::begin_refused)).
    /// The refusing errno is retained in
    /// [`control_error`](Self::control_error).
    pub fn control_refused(&mut self, errno: u32) {
        self.control_error = Some(errno);
        self.fail(CloseCause::Fault);
    }

    /// The errno of the definitive control refusal that closed the
    /// ring, when the control file produced one.
    pub fn control_error(&self) -> Option<u32> {
        self.control_error
    }

    /// A snapshot of one offer, including whether it is adoptable
    /// (`state == Settled`).
    pub fn offer(&self, offer_id: u64) -> Option<RingOfferInfo> {
        self.offers
            .iter()
            .find(|offer| offer.state.is_some() && offer.offer_id == offer_id)
            .map(|offer| RingOfferInfo {
                offer_id: offer.offer_id,
                resource: offer.upload.map_or(0, |upload| upload.resource),
                resource_generation: offer.upload.map_or(0, |upload| upload.resource_generation),
                width: offer.upload.map_or(0, |upload| upload.width),
                height: offer.upload.map_or(0, |upload| upload.height),
                slot: offer.slot,
                state: offer.state.unwrap_or(RingOfferState::Refused),
                status: reported_status(offer),
                deadline_passed: offer.deadline_passed,
            })
    }

    /// Live offers in slot order.
    pub fn offers(&self) -> impl Iterator<Item = RingOfferInfo> + '_ {
        self.offers
            .iter()
            .filter(|offer| offer.state.is_some())
            .map(|offer| RingOfferInfo {
                offer_id: offer.offer_id,
                resource: offer.upload.map_or(0, |upload| upload.resource),
                resource_generation: offer.upload.map_or(0, |upload| upload.resource_generation),
                width: offer.upload.map_or(0, |upload| upload.width),
                height: offer.upload.map_or(0, |upload| upload.height),
                slot: offer.slot,
                state: offer.state.unwrap_or(RingOfferState::Refused),
                status: reported_status(offer),
                deadline_passed: offer.deadline_passed,
            })
    }

    /// Record local deadline passage for pending offers. Custody is
    /// unchanged; settlement still requires status and release.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        if self.clock(now_ms).is_err() {
            return false;
        }
        let mut changed = false;
        for offer in &mut self.offers {
            if offer.state == Some(RingOfferState::Pending)
                && !offer.deadline_passed
                && now_ms >= offer.deadline_ms
            {
                offer.deadline_passed = true;
                changed = true;
            }
        }
        changed
    }

    /// Free a Refused, Settled or Unknown offer record.
    pub fn forget(&mut self, offer_id: u64) -> Result<(), ProducerError> {
        let Some(index) = self
            .offers
            .iter()
            .position(|offer| offer.state.is_some() && offer.offer_id == offer_id)
        else {
            return Err(ProducerError::Refused);
        };
        if !matches!(
            self.offers[index].state,
            Some(RingOfferState::Refused | RingOfferState::Settled | RingOfferState::Unknown)
        ) {
            return Err(ProducerError::Refused);
        }
        self.offers[index] = Offer::default();
        Ok(())
    }

    /// The 9P journal ended: pending offers become Unknown and no slot
    /// may be reused on this connection.
    pub fn journal_lost(&mut self) {
        for offer in &mut self.offers {
            if offer.state == Some(RingOfferState::Pending) {
                offer.state = Some(RingOfferState::Unknown);
            }
        }
        self.fail(CloseCause::JournalLost);
    }

    pub fn close(&mut self) {
        self.fail(CloseCause::Caller);
    }

    /// Whether offers may begin.
    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    pub fn closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    pub fn cause(&self) -> CloseCause {
        self.cause
    }
}

/// A directly filled slot, from [`RingProducer::acquire_slot`].
/// `pixels_mut` is the slot's payload (exactly the registered
/// capacity); the header generation word is the producer's, written
/// at commit after the caller's pixels. Dropping the guard without
/// committing frees the slot and spends nothing.
pub struct RingSlotFill<'a> {
    producer: &'a mut RingProducer,
    slot: u32,
    committed: bool,
}

impl RingSlotFill<'_> {
    pub fn slot(&self) -> u32 {
        self.slot
    }

    /// The slot payload, writable in place: registered capacity bytes.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let extent = usize::try_from(self.producer.registration.slot_extent()).unwrap();
        let capacity = usize::try_from(self.producer.registration.capacity).unwrap();
        let offset = self.slot as usize * extent + OWNED_RING_HEADER_BYTES as usize;
        let ring = self.producer.ring.as_mut().expect("ready ring is mapped");
        &mut ring.bytes_mut()[offset..offset + capacity]
    }

    /// Commit exactly `byte_len` filled bytes as one offer. The byte
    /// count must equal `width * height * 4` and fit the capacity.
    pub fn commit(
        mut self,
        upload: RingUpload,
        byte_len: usize,
        now_ms: u64,
    ) -> Result<(u64, Vec<u8>), ProducerError> {
        let result = self.producer.commit_slot(
            self.slot as usize,
            upload,
            u64::try_from(byte_len).map_err(|_| ProducerError::Argument)?,
            now_ms,
        );
        if result.is_ok() {
            self.committed = true;
        }
        result
    }

    /// Free the slot without offering it.
    pub fn abort(self) {}
}

impl Drop for RingSlotFill<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.producer.slots[self.slot as usize].acquired = false;
        }
    }
}

#[cfg(test)]
#[path = "../tests/support/ring_producer.rs"]
mod tests;
