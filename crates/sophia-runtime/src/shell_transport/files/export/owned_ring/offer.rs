//! Reserve both source responses before advancing an offer or slot identity.
use super::*;

#[must_use = "commit or cancel the source reservation before returning to wire I/O"]
pub(in crate::shell_transport) struct OwnedOfferReservation {
    slot: usize,
    offer: Offer,
}

impl ShellFiles {
    /// None is a busy source; EAGAIN is journal pressure that may defer input.
    pub(in crate::shell_transport) fn reserve_owned_offer(
        &mut self,
        offer: Offer,
    ) -> Result<Option<OwnedOfferReservation>, Errno> {
        if self.owned_current_identity() != Some(offer.identity) {
            return Err(Errno::ESTALE);
        }
        if self.owned.pending.is_some() {
            return Ok(None);
        }
        let image = self.image.as_ref().ok_or(Errno::ESTALE)?;
        let registration = self
            .owned
            .registration
            .as_ref()
            .ok_or(Errno::ESTALE)?
            .request;
        let r = offer.request;
        if r.slot.slot >= registration.slots
            || r.width > image.config.limits.max_width
            || r.height > image.config.limits.max_height
            || r.pixel_bytes()
                .is_none_or(|bytes| bytes > registration.pixels)
        {
            return Err(Errno::EINVAL);
        }
        if matches!(self.owned.last, Some(Control::Offer(old)) if r.slot.offer <= old.request.slot.offer)
            || r.slot.generation <= self.owned.generations[r.slot.slot]
        {
            return Err(EALREADY);
        }
        if self.owned.live.iter().flatten().any(|c| {
            c.offer.request.slot.slot == r.slot.slot || c.offer.request.resource == r.resource
        }) {
            return Ok(None);
        }
        let Some(slot) = self.owned.live[..image.config.limits.max_offers.min(4) as usize]
            .iter()
            .position(Option::is_none)
        else {
            return Ok(None);
        };
        let status = self
            .journal
            .hold_reservation(SHELL_FILE_HEADER_BYTES + 56)
            .ok_or(Errno::EAGAIN)?;
        let Some(release) = self.journal.hold_reservation(EVENT_BYTES) else {
            self.journal.release_reservation(status);
            return Err(Errno::EAGAIN);
        };
        self.owned.live[slot] = Some(Credits {
            offer,
            status: Some(status),
            release,
        });
        Ok(Some(OwnedOfferReservation { slot, offer }))
    }

    pub(in crate::shell_transport) fn commit_owned_offer(&mut self, held: OwnedOfferReservation) {
        assert!(
            self.owned.live[held.slot]
                .as_ref()
                .is_some_and(|c| c.offer == held.offer),
            "exact reserved source remains owned"
        );
        self.owned.generations[held.offer.request.slot.slot] = held.offer.request.slot.generation;
        self.owned.last = Some(Control::Offer(held.offer));
    }

    #[cfg(feature = "experimental-native-frame-transaction")]
    pub(in crate::shell_transport) fn cancel_owned_offer(&mut self, held: OwnedOfferReservation) {
        let credit = self.owned.live[held.slot]
            .take()
            .expect("reserved source remains owned");
        assert_eq!(
            credit.offer, held.offer,
            "cancel only the exact provisional source"
        );
        self.journal
            .release_reservation(credit.status.expect("source status not yet published"));
        self.journal.release_reservation(credit.release);
    }
}
