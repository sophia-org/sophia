//! DMA-BUF entry points preserve the ordinary ticket and ResourceCancel queue ordering.
use crate::*;
impl ShellConnection {
    /// Offer a completed LINEAR XRGB8888 DMA-BUF. Keep the backing allocation immutable
    /// until the matching SourceReleased; disconnect leaves completion unknown.
    pub fn image_begin_dma_buf(
        &mut self,
        metadata: ProducerMetadata,
        source: std::os::fd::OwnedFd,
        offset: u32,
        stride: u32,
        now: std::time::Instant,
    ) -> Result<ImageBeginAdmission, ImageBeginError> {
        let refuse = |error: ImageAdapterError, source| ImageBeginError {
            error,
            source: Some(source),
        };
        if self.image.as_ref().and_then(|a| a.dma.as_ref()).is_none() {
            return Err(refuse(ImageAdapterError::NotEnabled, source));
        }
        if self.wire.peer_closed() {
            return Err(refuse(ImageAdapterError::Closed, source));
        }
        // Room and a free adapter ticket first: once the producer
        // owns the descriptor, admission cannot be allowed to fail
        // for local queue capacity.
        if !self.output.has_room(1, 128, false)
            || !self
                .image
                .as_ref()
                .unwrap()
                .dma
                .as_ref()
                .unwrap()
                .has_free_ticket()
        {
            return Err(refuse(ImageAdapterError::Busy, source));
        }
        let now_ms = match self
            .image
            .as_mut()
            .unwrap()
            .dma
            .as_mut()
            .unwrap()
            .advance_clock(now)
        {
            Ok(now_ms) => now_ms,
            Err(error) => return Err(refuse(error, source)),
        };
        let adapter = self.image.as_mut().unwrap().dma.as_mut().unwrap();
        let producer = match adapter.producer_mut() {
            Some(producer) => producer,
            None => return Err(refuse(ImageAdapterError::Closed, source)),
        };
        let (offer, body, assigned) =
            match producer.begin_dma_buf(metadata, source, offset, stride, now_ms) {
                Ok(begun) => begun,
                Err((error, source)) => return Err(refuse(error.into(), source)),
            };
        let admission = match self.admit(Outbound::ImageBegin(body), false) {
            Ok(admission) => admission,
            Err(_) => {
                // Unreachable after the room check; keep custody
                // honest rather than leak the offer.
                if let Some(producer) = self
                    .image
                    .as_mut()
                    .and_then(|a| a.dma.as_mut())
                    .and_then(|a| a.producer_mut())
                {
                    let _ = producer.begin_refused(offer, now_ms);
                }
                return Err(ImageBeginError {
                    error: ImageAdapterError::Refused,
                    source: None,
                });
            }
        };
        let ticket = admission.first;
        if let Some(adapter) = self.image.as_mut().and_then(|a| a.dma.as_mut()) {
            let _ = adapter.register_ticket(ticket.0, offer, assigned);
        }
        let _ = self.poll_io();
        Ok(ImageBeginAdmission { offer, ticket })
    }
    pub(crate) fn image_cancel_dma_buf(
        &mut self,
        offer: u64,
        now: std::time::Instant,
    ) -> Result<Ticket, ShellClientError> {
        if self.image.is_none() {
            return Err(ShellClientError::InvalidArgument(
                "image adapter not enabled",
            ));
        }
        let now_ms = self
            .image
            .as_mut()
            .expect("adapter checked above")
            .dma
            .as_mut()
            .expect("DMA adapter")
            .advance_clock(now)
            .map_err(|_| ShellClientError::InvalidArgument("image clock"))?;
        let adapter = self
            .image
            .as_ref()
            .and_then(|a| a.dma.as_ref())
            .expect("DMA adapter");
        let metadata = adapter
            .ticket_metadata(offer)
            .ok_or(ShellClientError::InvalidArgument("unknown image offer"))?;
        let record =
            ShellContentRecord::ResourceCancel(sophia_shell_protocol::ContentResourceCancel {
                grant: sophia_shell_protocol::ContentGrant {
                    connection_epoch: self.welcome.connection_epoch,
                    content_grant_epoch: metadata.admission_epoch,
                },
                resource: sophia_shell_protocol::ContentResourceId {
                    id: metadata.resource,
                    generation: metadata.resource_generation,
                },
            });
        // Prepare the queue slot before marking the offer: a
        // saturated queue leaves the offer unmarked and sendable.
        let outbound = Outbound::Content(TransactionId::from_raw(offer), record);
        let control = outbound.is_control();
        let units = self.wire.encode(outbound)?;
        let prepared = match self.output.prepare(units, control, &mut self.ledger) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.wire.cancel_encoded();
                return Err(error);
            }
        };
        if let Some(producer) = self
            .image
            .as_mut()
            .and_then(|a| a.dma.as_mut())
            .and_then(|a| a.producer_mut())
            && let Err(error) = producer.cancel(offer, now_ms)
        {
            drop(prepared);
            self.wire.cancel_encoded();
            return Err(match error {
                sophia_image_producer::ProducerError::Argument => {
                    ShellClientError::InvalidArgument("unknown image offer")
                }
                _ => ShellClientError::QueueSaturated,
            });
        }
        let admission = self.output.commit(prepared, &mut self.ledger);
        self.wire.commit_encoded();
        Ok(admission.first)
    }
}
