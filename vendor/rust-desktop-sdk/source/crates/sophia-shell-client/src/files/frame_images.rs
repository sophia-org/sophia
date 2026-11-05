//! Join a complete frame's queue admission to its copied-source reservation.
use super::{ImageAdapter, ImageAdapterError, Routed};
use crate::{Admission, Custody, FrameStream, ShellClientError, ShellConnection, Ticket};
use sophia_image_producer::{OwnedRingSlotFill, ProducerError, RingOfferState, RingUpload};
use sophia_shell_protocol::shell_files::{ShellFileKind, ShellFrameRecord};
use sophia_shell_protocol::{ContentAllocationId, ContentGrant, ContentOutputId, TransactionId};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameRefusal {
    transaction: TransactionId,
    grant: ContentGrant,
    output: ContentOutputId,
    stream: u64,
    generation: u64,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FrameSource {
    request: FrameRefusal,
    ticket: Ticket,
    offer: u64,
    admitted: bool,
    allocation: ContentAllocationId,
    facts: u64,
}

pub(crate) fn route_frame_source(
    item: &crate::wire::Inbound,
    routes: &mut std::collections::VecDeque<Routed>,
) {
    if let crate::wire::Inbound::Frame(ShellFrameRecord::Refused {
        transaction,
        grant,
        output,
        stream_id,
        candidate_generation,
        request_kind: ShellFileKind::FrameCommit,
        ..
    }) = item
    {
        routes.push_back(Routed::FrameRefused(FrameRefusal {
            transaction: *transaction,
            grant: *grant,
            output: *output,
            stream: *stream_id,
            generation: *candidate_generation,
        }));
    }
    if let crate::wire::Inbound::Frame(ShellFrameRecord::Outcome {
        transaction,
        outcome,
        stream_id,
        allocation,
        facts_generation,
        ..
    }) = item
    {
        routes.push_back(Routed::FrameAdmitted {
            request: FrameRefusal {
                transaction: *transaction,
                grant: outcome.grant,
                output: outcome.output,
                stream: *stream_id,
                generation: outcome.candidate_generation,
            },
            allocation: *allocation,
            facts: *facts_generation,
        });
    }
}

fn image_error(error: ProducerError) -> ShellClientError {
    ShellClientError::Image(error.into())
}

impl FrameStream {
    /// Queue a legacy image offer for this binding after frame and source work drain.
    /// This reuses the registration and accepted resources; it neither reconnects
    /// nor grants a frame credit. Follow ordinary image settlement, demand/permit
    /// and candidate handling. Keep this output binding on that legacy path;
    /// reconsider SFT only on a new binding. The caller freshly checks render identity.
    ///
    /// # Errors
    /// Refuses an undrained/unknown stream, wrong binding, unsettled SFT source,
    /// closed connection or invalid loan. Refusal retains the filled bytes.
    /// Success has the custody rules of [`ShellConnection::image_commit_ring_fill`].
    pub fn commit_legacy_ring_fill(
        &self,
        connection: &mut ShellConnection,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now: Instant,
    ) -> Result<u64, ShellClientError> {
        connection.wire.require_frame()?;
        let key = self.key().filter(|_| self.frame_work_drained()).ok_or(
            ShellClientError::InvalidArgument("frame stream has not drained"),
        )?;
        if key.grant.connection_epoch != connection.connection_epoch()
            || (upload.output, upload.output_generation) != (key.output.id, key.output.generation)
            || (upload.allocation, upload.allocation_generation)
                != (key.allocation.id, key.allocation.generation)
            || upload.topology_generation != key.facts_generation
        {
            return Err(ShellClientError::InvalidArgument(
                "legacy image does not match drained binding",
            ));
        }
        let adapter = connection
            .image
            .as_mut()
            .ok_or(ShellClientError::Image(ImageAdapterError::NotEnabled))?;
        if adapter.config.content_grant_epoch != key.grant.content_grant_epoch {
            return Err(ShellClientError::InvalidArgument(
                "legacy image has a replaced grant",
            ));
        }
        if let Some(ring) = &adapter.ring {
            for source in adapter.frame_sources.iter().flatten().filter(|source| {
                source.request.grant == key.grant && source.request.output == key.output
            }) {
                match ring.offer(source.offer).map(|offer| offer.state) {
                    Some(RingOfferState::Pending) => {
                        return Err(ShellClientError::Image(ImageAdapterError::Busy));
                    }
                    Some(RingOfferState::Unknown) => {
                        return Err(ShellClientError::Image(ImageAdapterError::Closed));
                    }
                    _ => {}
                }
            }
        }
        let offer = adapter
            .commit_returned_fill(&connection.wire, fill, upload, now)
            .map_err(ShellClientError::Image)?;
        let _ = connection.poll_io();
        Ok(offer)
    }

    /// Queue a frame using a completed worker-owned ring fill without copying pixels.
    /// The caller must freshly validate its render/output binding before submitting.
    /// No I/O occurs; source custody is identical to [`Self::submit_ring`].
    /// # Errors
    /// Refusal retains the exact loan, record and frame credit for retry or abort.
    /// Success makes the loan inert. Dropping an unreturned live loan faults the ring.
    pub fn submit_ring_fill(
        &mut self,
        connection: &mut ShellConnection,
        record: &mut ShellFrameRecord,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now: Instant,
    ) -> Result<(Admission, u64), ShellClientError> {
        self.check_submit(record)?;
        let admission = connection.enqueue_frame_ring_fill(record, fill, upload, now)?;
        self.submitted(record);
        Ok(admission)
    }

    /// Fill and queue one image-bearing frame, reserving source, queue and credit together.
    /// `record` must be a Commit without an image; its fields are restored before
    /// `fill` runs. Pixels are filled directly in a free slot, with no staging copy.
    /// No I/O occurs. The returned offer stays busy until exact source release;
    /// Prepared, frame completion and ACKs do not release it. Keep driving the
    /// connection and route frame events through `observe` as for `submit`.
    /// # Errors
    /// Invalid input, unavailable source/credit or queue capacity sends nothing
    /// and spends no identities. A panicking fill also leaves the slot unpublished.
    pub fn submit_ring(
        &mut self,
        connection: &mut ShellConnection,
        record: &mut ShellFrameRecord,
        upload: RingUpload,
        now: Instant,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<(Admission, u64), ShellClientError> {
        self.check_submit(record)?;
        let admission = connection.enqueue_frame_ring(record, upload, now, fill)?;
        self.submitted(record);
        Ok(admission)
    }
}

impl ShellConnection {
    fn enqueue_frame_ring_fill(
        &mut self,
        record: &mut ShellFrameRecord,
        fill: &mut OwnedRingSlotFill,
        upload: RingUpload,
        now: Instant,
    ) -> Result<(Admission, u64), ShellClientError> {
        self.wire.require_frame()?;
        if self.wire.peer_closed() {
            return Err(ShellClientError::PeerClosed);
        }
        let ShellFrameRecord::Commit(value) = record else {
            return Err(ShellClientError::WrongDirection);
        };
        if value.image.is_some() {
            return Err(ShellClientError::InvalidArgument(
                "frame already has an image",
            ));
        }
        let request = FrameRefusal {
            transaction: value.transaction,
            grant: value.candidate.grant,
            output: value.candidate.output,
            stream: value.candidate.pacing_permit,
            generation: value.candidate.candidate_generation,
        };
        let adapter = self
            .image
            .as_mut()
            .ok_or(ShellClientError::Image(ImageAdapterError::NotEnabled))?;
        let now_ms = adapter
            .advance_clock(now)
            .map_err(ShellClientError::Image)?;
        if adapter.pending.is_some() {
            return Err(ShellClientError::Image(ImageAdapterError::Again));
        }
        if upload.width > adapter.config.bounds.max_width
            || upload.height > adapter.config.bounds.max_height
        {
            return Err(ShellClientError::Image(ImageAdapterError::Refused));
        }
        let ring = adapter
            .ring
            .as_mut()
            .ok_or(ShellClientError::Image(ImageAdapterError::Again))?;
        let offer = ring
            .owned_offer(fill, upload, now_ms)
            .map_err(image_error)?;
        value.image = Some(offer);
        let encoded = self.wire.encode_frame(record);
        let ShellFrameRecord::Commit(value) = record else {
            unreachable!("frame kind unchanged")
        };
        value.image = None;
        let units = encoded?;
        self.wire.reserve_encoded()?;
        let result = self
            .output
            .enqueue_after(units, false, &mut self.ledger, || {
                ring.commit_owned_offer(fill, upload, now_ms)
                    .map_err(image_error)?;
                Ok(())
            });
        let admission = match result {
            Ok(admission) => admission,
            Err(error) => {
                self.wire.cancel_encoded();
                return Err(error);
            }
        };
        self.wire.commit_encoded();
        adapter.frame_sources[offer.slot as usize] = Some(FrameSource {
            request,
            ticket: admission.first,
            offer: offer.offer_id,
            admitted: false,
            allocation: ContentAllocationId {
                id: upload.allocation,
                generation: upload.allocation_generation,
            },
            facts: upload.topology_generation,
        });
        Ok((admission, offer.offer_id))
    }

    fn enqueue_frame_ring(
        &mut self,
        record: &mut ShellFrameRecord,
        upload: RingUpload,
        now: Instant,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<(Admission, u64), ShellClientError> {
        self.wire.require_frame()?;
        if self.wire.peer_closed() {
            return Err(ShellClientError::PeerClosed);
        }
        let ShellFrameRecord::Commit(value) = record else {
            return Err(ShellClientError::WrongDirection);
        };
        if value.image.is_some() {
            return Err(ShellClientError::InvalidArgument(
                "frame already has an image",
            ));
        }
        let request = FrameRefusal {
            transaction: value.transaction,
            grant: value.candidate.grant,
            output: value.candidate.output,
            stream: value.candidate.pacing_permit,
            generation: value.candidate.candidate_generation,
        };
        let adapter = self
            .image
            .as_mut()
            .ok_or(ShellClientError::Image(ImageAdapterError::NotEnabled))?;
        let now_ms = adapter
            .advance_clock(now)
            .map_err(ShellClientError::Image)?;
        if adapter.pending.is_some() {
            return Err(ShellClientError::Image(ImageAdapterError::Again));
        }
        if upload.width > adapter.config.bounds.max_width
            || upload.height > adapter.config.bounds.max_height
        {
            return Err(ShellClientError::Image(ImageAdapterError::Refused));
        }
        let bytes = u64::from(upload.width)
            .checked_mul(u64::from(upload.height))
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(ShellClientError::Image(ImageAdapterError::Argument))?;
        let ring = adapter
            .ring
            .as_mut()
            .ok_or(ShellClientError::Image(ImageAdapterError::Again))?;
        let mut slot = ring.acquire_slot(now_ms).map_err(image_error)?;
        let offer = slot.offer(upload, bytes).map_err(image_error)?;
        value.image = Some(offer);
        let encoded = self.wire.encode_frame(record);
        let ShellFrameRecord::Commit(value) = record else {
            unreachable!("frame kind unchanged")
        };
        value.image = None;
        let units = encoded?;
        self.wire.reserve_encoded()?;
        let result = self
            .output
            .enqueue_after(units, false, &mut self.ledger, || {
                fill(&mut slot.pixels_mut()[..bytes]);
                slot.commit_offer(upload, bytes, now_ms)
                    .map_err(image_error)?;
                Ok(())
            });
        let admission = match result {
            Ok(admission) => admission,
            Err(error) => {
                self.wire.cancel_encoded();
                return Err(error);
            }
        };
        self.wire.commit_encoded();
        adapter.frame_sources[offer.slot as usize] = Some(FrameSource {
            request,
            ticket: admission.first,
            offer: offer.offer_id,
            admitted: false,
            allocation: ContentAllocationId {
                id: upload.allocation,
                generation: upload.allocation_generation,
            },
            facts: upload.topology_generation,
        });
        Ok((admission, offer.offer_id))
    }
}

impl ImageAdapter {
    fn refuse_frame_source(&mut self, source: FrameSource, now: u64) -> Result<(), ProducerError> {
        let Some(ring) = &mut self.ring else {
            return Err(ProducerError::Closed);
        };
        // A refusal cannot undo a source whose acceptance or release was observed.
        if source.admitted
            || ring
                .offer(source.offer)
                .is_none_or(|o| o.state != RingOfferState::Pending || o.status.is_some())
        {
            ring.journal_lost();
            return Err(ProducerError::Refused);
        }
        ring.begin_refused(source.offer, now)
    }

    pub(super) fn frame_refused(
        &mut self,
        refusal: FrameRefusal,
        now: u64,
    ) -> Result<(), ProducerError> {
        let Some(source) = self
            .frame_sources
            .iter()
            .flatten()
            .find(|s| s.request.transaction == refusal.transaction)
            .copied()
        else {
            return Ok(());
        };
        if source.request != refusal {
            if let Some(ring) = &mut self.ring {
                ring.journal_lost();
            }
            return Err(ProducerError::Refused);
        }
        self.refuse_frame_source(source, now)
    }

    pub(super) fn frame_admitted(
        &mut self,
        request: FrameRefusal,
        allocation: ContentAllocationId,
        facts: u64,
    ) -> Result<(), ProducerError> {
        let Some(index) = self.frame_sources.iter().position(|source| {
            source.is_some_and(|s| s.request.transaction == request.transaction)
        }) else {
            return Ok(());
        };
        let source = self.frame_sources[index]
            .as_mut()
            .expect("located frame source");
        if source.request != request
            || source.allocation != allocation
            || source.facts != facts
            || self
                .ring
                .as_ref()
                .and_then(|r| r.offer(source.offer))
                .is_some_and(|o| o.state == RingOfferState::Refused)
        {
            if let Some(ring) = &mut self.ring {
                ring.journal_lost();
            }
            return Err(ProducerError::Refused);
        }
        source.admitted = true;
        Ok(())
    }

    pub(super) fn frame_custody(&mut self, ledger: &crate::custody::Ledger, now: u64) {
        for source in self.frame_sources.into_iter().flatten() {
            if self
                .ring
                .as_ref()
                .and_then(|r| r.offer(source.offer))
                .is_none_or(|o| o.state != RingOfferState::Pending)
            {
                continue;
            }
            match ledger.get(source.ticket) {
                Some(Custody::Refused(_) | Custody::DroppedUnsent) => {
                    let _ = self.refuse_frame_source(source, now);
                }
                None | Some(Custody::Unknown) => {
                    if let Some(ring) = &mut self.ring {
                        ring.journal_lost();
                    }
                }
                _ => {}
            }
        }
    }
}
