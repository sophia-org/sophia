//! Owned-ring file custody. Only the persistent owner accepts resources.
use super::super::journal::JournalReservation;
use super::*;
use crate::owned_ring_protocol::{
    Control, EVENT_BYTES, Event, EventKind, Identity, Offer, Registration,
};
use crate::{
    ContentEpochRegistry, OwnedCopyImageSettlement, OwnedRing, OwnedRingSourceReservation,
};
use sophia_protocol::{ContentReason, ContentResourceStatus};
use std::os::fd::OwnedFd;

mod release;
mod setup;

#[cfg(test)]
#[path = "../../../../tests/support/owned_ring_export.rs"]
mod tests;

pub(super) struct OwnedRingExport {
    registration: Option<Setup>,
    last: Option<Control>,
    pending: Option<(Offer, Instant)>,
    live: [Option<Credits>; 4],
    generations: [u64; 4],
    published: u64,
}
impl OwnedRingExport {
    pub(super) fn new() -> Self {
        Self {
            registration: None,
            last: None,
            pending: None,
            live: std::array::from_fn(|_| None),
            generations: [0; 4],
            published: 0,
        }
    }
}
struct Setup {
    request: Registration,
    deadline: Instant,
    phase: Phase,
}
enum Phase {
    AwaitingBudget {
        admitted: JournalReservation,
        terminal: JournalReservation,
    },
    AwaitingFd {
        // FD must close before its storage charge is returned.
        fd: Option<OwnedFd>,
        reservation: OwnedRingSourceReservation,
        terminal: JournalReservation,
    },
    PublishingReady {
        ring: OwnedRing,
        terminal: JournalReservation,
    },
    Ready,
    Refusing {
        reason: u16,
        admitted: Option<JournalReservation>,
        terminal: JournalReservation,
    },
    Refused,
}
struct Credits {
    offer: Offer,
    status: Option<JournalReservation>,
    release: JournalReservation,
}

impl ShellFiles {
    pub(super) fn owned_disclosed(&self) -> bool {
        !self.revoked
            && self.content
            && self.negotiated
            && self.image_disclosed()
            && matches!(
                self.image_readiness(),
                ShellImageReadiness::Ready { granted_source_kinds }
                    if granted_source_kinds & crate::owned_ring_protocol::SOURCE_KIND != 0
            )
    }
    pub(super) fn owned_mode(&self) -> bool {
        self.owned.registration.is_some()
    }
    pub(in crate::shell_transport) fn owned_current_identity(&self) -> Option<Identity> {
        let setup = self.owned.registration.as_ref()?;
        (self.owned_disclosed() && matches!(setup.phase, Phase::Ready))
            .then_some(setup.request.identity)
    }
    pub(in crate::shell_transport) fn owned_pending_offer(&self) -> Option<(Offer, Instant)> {
        self.owned.pending
    }
    pub(in crate::shell_transport) fn owned_take_offer(&mut self) {
        self.owned.pending = None;
    }
    pub(in crate::shell_transport) fn owned_admission_available(
        &self,
        epochs: &ContentEpochRegistry,
        worker_pending: usize,
    ) -> Result<bool, Errno> {
        let (offer, _) = self.owned.pending.ok_or(Errno::ESTALE)?;
        if self.owned_current_identity() != Some(offer.identity) {
            return Err(Errno::ESTALE);
        }
        let config = &self.image.as_ref().ok_or(Errno::ESTALE)?.config.limits;
        let geometry = self
            .owned
            .registration
            .as_ref()
            .ok_or(Errno::ESTALE)?
            .request
            .geometry()
            .map_err(|_| Errno::EINVAL)?;
        let usage = epochs
            .resources(offer.identity.grant)
            .ok_or(Errno::ESTALE)?
            .usage();
        Ok(worker_pending < config.max_validation_jobs as usize
            && usage
                .backing
                .checked_add(geometry.source_bytes() as u64)
                .and_then(|n| n.checked_add(offer.request.pixel_bytes()? as u64))
                .is_some_and(|n| n <= config.max_sealed_backing_bytes))
    }
    pub(in crate::shell_transport) fn owned_publication_sequence(&self) -> u64 {
        self.owned.published
    }
    pub(in crate::shell_transport) fn owned_deadline(&self) -> Option<Instant> {
        self.owned
            .registration
            .as_ref()
            .filter(|s| {
                matches!(
                    s.phase,
                    Phase::AwaitingBudget { .. }
                        | Phase::AwaitingFd { .. }
                        | Phase::PublishingReady { .. }
                )
            })
            .map(|s| s.deadline)
    }
    pub(super) fn write_owned_ring(&mut self, bytes: &[u8]) -> Result<u32, Errno> {
        if !self.owned_disclosed() {
            return Err(Errno::EACCES);
        }
        let control = Control::decode(bytes).ok_or(Errno::EINVAL)?;
        let identity = control.identity();
        let image = self.image.as_ref().ok_or(Errno::EACCES)?;
        if self.content_limits.as_ref().map(|l| l.grant) != Some(identity.grant)
            || identity.channel != image.config.discovery.channel_generation
        {
            return Err(Errno::ESTALE);
        }
        if self.owned.last == Some(control) {
            return Ok(bytes.len() as u32);
        }
        let deadline = Instant::now()
            .checked_add(self.image_validation_timeout().ok_or(Errno::EACCES)?)
            .ok_or(Errno::EINVAL)?;
        match control {
            Control::Register(request) => {
                if self.owned_mode() {
                    return Err(EALREADY);
                }
                #[cfg(feature = "experimental-draw")]
                if self.has_draw_work() {
                    return Err(EALREADY);
                }
                if !self.inbound_is_empty() {
                    return Err(Errno::EAGAIN);
                }
                if request.pixels as u64 > image.config.limits.max_resource_bytes
                    || (request
                        .geometry()
                        .map_err(|_| Errno::EINVAL)?
                        .source_bytes() as u64)
                        .checked_add(request.pixels as u64)
                        .is_none_or(|n| n > image.config.limits.max_sealed_backing_bytes)
                {
                    return Err(Errno::EINVAL);
                }
                let admitted = self
                    .journal
                    .hold_reservation(EVENT_BYTES)
                    .ok_or(Errno::EAGAIN)?;
                let Some(terminal) = self.journal.hold_reservation(EVENT_BYTES) else {
                    self.journal.release_reservation(admitted);
                    return Err(Errno::EAGAIN);
                };
                self.owned.registration = Some(Setup {
                    request,
                    deadline,
                    phase: Phase::AwaitingBudget { admitted, terminal },
                });
            }
            Control::Offer(offer) => {
                if self.owned_current_identity() != Some(identity) {
                    return Err(Errno::ESTALE);
                }
                if self.owned.pending.is_some() {
                    return Err(Errno::EAGAIN);
                }
                let request = self
                    .owned
                    .registration
                    .as_ref()
                    .ok_or(Errno::ESTALE)?
                    .request;
                let r = offer.request;
                if r.slot.slot >= request.slots
                    || r.width > image.config.limits.max_width
                    || r.height > image.config.limits.max_height
                    || r.pixel_bytes().is_none_or(|bytes| bytes > request.pixels)
                {
                    return Err(Errno::EINVAL);
                }
                if matches!(self.owned.last, Some(Control::Offer(old)) if r.slot.offer <= old.request.slot.offer)
                    || r.slot.generation <= self.owned.generations[r.slot.slot]
                {
                    return Err(EALREADY);
                }
                if self.owned.live.iter().flatten().any(|c| {
                    c.offer.request.slot.slot == r.slot.slot
                        || c.offer.request.resource == r.resource
                }) {
                    return Err(Errno::EAGAIN);
                }
                let slot = self.owned.live[..image.config.limits.max_offers.min(4) as usize]
                    .iter_mut()
                    .find(|s| s.is_none())
                    .ok_or(Errno::EAGAIN)?;
                let status = self
                    .journal
                    .hold_reservation(SHELL_FILE_HEADER_BYTES + 56)
                    .ok_or(Errno::EAGAIN)?;
                let Some(release) = self.journal.hold_reservation(EVENT_BYTES) else {
                    self.journal.release_reservation(status);
                    return Err(Errno::EAGAIN);
                };
                *slot = Some(Credits {
                    offer,
                    status: Some(status),
                    release,
                });
                self.owned.generations[r.slot.slot] = r.slot.generation;
                self.owned.pending = Some((offer, deadline));
            }
        }
        self.owned.last = Some(control);
        Ok(bytes.len() as u32)
    }
    pub(super) fn reset_owned_ring(&mut self) {
        if let Some(setup) = self.owned.registration.take() {
            match setup.phase {
                Phase::AwaitingBudget { admitted, terminal } => {
                    self.journal.release_reservation(admitted);
                    self.journal.release_reservation(terminal);
                }
                Phase::AwaitingFd { terminal, .. } | Phase::PublishingReady { terminal, .. } => {
                    self.journal.release_reservation(terminal);
                }
                Phase::Refusing {
                    admitted, terminal, ..
                } => {
                    if let Some(hold) = admitted {
                        self.journal.release_reservation(hold);
                    }
                    self.journal.release_reservation(terminal);
                }
                Phase::Ready | Phase::Refused => {}
            }
        }
        for credit in &mut self.owned.live {
            if let Some(c) = credit.take() {
                if let Some(status) = c.status {
                    self.journal.release_reservation(status);
                }
                self.journal.release_reservation(c.release);
            }
        }
        self.owned.pending = None;
        self.owned.last = None;
    }
}
