//! Status publication and source release are separate, ordered obligations.
use super::*;

impl ShellFiles {
    pub(in crate::shell_transport) fn append_owned_status(
        &mut self,
        _transaction: TransactionId,
        status: &ContentResourceStatus,
        body: &[u8],
    ) -> Result<Option<u64>, Errno> {
        let Some(credit) = self.owned.live.iter_mut().flatten().find(|c| {
            // Cancellation keeps its own transaction id. Resource custody is
            // unique among live offers, so consume the exact owed status once.
            c.status.is_some()
                && c.offer.identity.grant == status.grant
                && c.offer.request.resource == status.resource
        }) else {
            return Ok(None);
        };
        if status.grant != credit.offer.identity.grant {
            return Err(Errno::ESTALE);
        }
        if !(2..=4).contains(&status.status) {
            return Err(Errno::EPROTO);
        }
        let sequence = self.journal.append_consuming_reservation(
            credit.status.ok_or(Errno::EINVAL)?,
            ShellFileKind::ResourceStatus,
            body,
        )?;
        credit.status = None;
        self.owned.published = sequence;
        Ok(Some(sequence))
    }

    pub(in crate::shell_transport) fn owned_refuse_offer(
        &mut self,
        reason: ContentReason,
    ) -> Result<(), Errno> {
        let (offer, _) = self.owned.pending.ok_or(Errno::ESTALE)?;
        let needs_status = self
            .owned
            .live
            .iter()
            .flatten()
            .find(|c| c.offer == offer)
            .ok_or(Errno::ESTALE)?
            .status
            .is_some();
        if needs_status {
            let status = ContentResourceStatus {
                grant: offer.identity.grant,
                resource: offer.request.resource,
                status: if reason == ContentReason::Cancelled {
                    4
                } else {
                    3
                },
                reason: reason as u16,
                next_ordinal: 0,
                admitted_bytes: 0,
            };
            let transaction = TransactionId::from_raw(offer.request.slot.offer);
            let record = ShellFileTransactionRecord {
                transaction,
                record: ShellContentRecord::ResourceStatus(status.clone()),
            };
            let bytes = encode_shell_file_resource_status_at_revision(
                ShellFileHeader {
                    kind: ShellFileKind::ResourceStatus,
                    connection_epoch: self.epoch,
                    submission_id: 0,
                    sequence: 1,
                },
                &record,
                crate::owned_ring_protocol::REVISION,
            )
            .map_err(|_| Errno::EINVAL)?;
            self.append_owned_status(transaction, &status, &bytes[SHELL_FILE_HEADER_BYTES..])?
                .ok_or(Errno::ESTALE)?;
        }
        self.publish_owned_release(offer, reason as u16, false)?;
        self.owned.pending = None;
        Ok(())
    }

    pub(in crate::shell_transport) fn owned_release(
        &mut self,
        settlement: &OwnedCopyImageSettlement,
    ) -> Result<(), Errno> {
        let request = *settlement.admission.metadata.request();
        let offer = self
            .owned
            .live
            .iter()
            .flatten()
            .find(|c| {
                c.offer.request == request && c.offer.identity.grant == settlement.admission.grant
            })
            .ok_or(Errno::ESTALE)?
            .offer;
        if self.owned_current_identity() != Some(offer.identity) {
            return Err(Errno::ESTALE);
        }
        let reason = settlement.stopped.map(|r| r as u16).unwrap_or_else(|| {
            if settlement.outcome.is_ok() {
                0
            } else {
                ContentReason::Malformed as u16
            }
        });
        let changed =
            settlement.validation_failure == Some(crate::IsbValidationFailure::GenerationChanged);
        self.publish_owned_release(offer, reason, changed)
    }

    fn publish_owned_release(
        &mut self,
        offer: Offer,
        reason: u16,
        changed_generation: bool,
    ) -> Result<(), Errno> {
        let slot = self
            .owned
            .live
            .iter_mut()
            .find(|c| c.as_ref().is_some_and(|c| c.offer == offer))
            .ok_or(Errno::ESTALE)?;
        let credit = slot.as_ref().ok_or(Errno::ESTALE)?;
        if credit.status.is_some() {
            return Err(Errno::EAGAIN);
        }
        let sequence = self.journal.append_owned_event(
            credit.release,
            Event {
                kind: EventKind::SlotReleased,
                identity: offer.identity,
                sequence: 0,
                slot: offer.request.slot,
                resource: offer.request.resource,
                reason,
                changed_generation,
            },
        )?;
        self.owned.published = sequence;
        *slot = None;
        Ok(())
    }
}
