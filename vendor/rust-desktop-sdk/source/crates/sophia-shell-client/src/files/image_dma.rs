//! Optional r10 DMA source custody, sharing the staged discovery reader.
use super::*;
use sophia_image_producer::{
    ImageProducer, ImageProducerConfig, ImageRole, OfferInfo, ProducerMetadata,
};

struct TicketSlot {
    ticket: u64,
    offer: u64,
}

/// The adapter state for one image-enabled connection. Queries are
/// on this type; the connection drives it from its session drives.
pub struct DmaAdapter {
    config: ShellImageConfig,
    producer: Option<ImageProducer>,
    origin: Instant,
    origin_ms: u64,
    /// The highest caller millisecond accepted so far: drives and
    /// image calls share one monotonic watermark, so a time that
    /// follows the origin but precedes an earlier accepted call is
    /// still rejected before anything mutates.
    last_ms: u64,
    stage: Stage,
    tickets: [Option<TicketSlot>; 4],
    offer_metadata: [Option<(u64, ProducerMetadata)>; 4],
    pass: u64,
    retry_at_ms: u64,
    retry_delay_ms: u64,
    discovery_text: Vec<u8>,
    lost: bool,
    final_notified: bool,
}

impl DmaAdapter {
    pub(crate) fn new(
        config: ShellImageConfig,
        connection_epoch: u64,
        _capabilities: u64,
        origin: Instant,
        origin_ms: u64,
    ) -> Result<Self, ShellClientError> {
        let producer_config = ImageProducerConfig {
            role: ImageRole::Shell,
            revision: 10,
            connection_epoch,
            granted_kinds: 6,
            bounds: config.bounds,
            transfer_timeout_ms: config.transfer_timeout_ms,
            startup_deadline_ms: config.startup_deadline_ms,
        };
        // An invalid producer configuration is refused here, at
        // construction: no adapter exists to swallow it into.
        let producer = ImageProducer::new(producer_config, origin_ms)
            .map_err(|_| ShellClientError::InvalidArgument("image producer config"))?;
        let producer = Some(producer);
        Ok(Self {
            config,
            producer,
            origin,
            origin_ms,
            last_ms: origin_ms,
            stage: Stage::Discovering,
            tickets: Default::default(),
            offer_metadata: Default::default(),
            pass: 0,
            retry_at_ms: origin_ms,
            retry_delay_ms: 0,
            discovery_text: Vec::new(),
            lost: false,
            final_notified: false,
        })
    }

    /// Validate `now` against the origin and the shared watermark,
    /// and advance the watermark. Called before any drive or queue
    /// mutation that would act on this time.
    pub(crate) fn advance_clock(&mut self, now: Instant) -> Result<u64, ImageAdapterError> {
        let now_ms = self.now_ms(now)?;
        if now_ms < self.last_ms {
            return Err(ImageAdapterError::Argument);
        }
        self.last_ms = now_ms;
        Ok(now_ms)
    }

    /// Convert a session instant to the caller's millisecond domain.
    pub(crate) fn now_ms(&self, now: Instant) -> Result<u64, ImageAdapterError> {
        let elapsed = now
            .checked_duration_since(self.origin)
            .ok_or(ImageAdapterError::Argument)?;
        let elapsed_ms =
            u64::try_from(elapsed.as_millis()).map_err(|_| ImageAdapterError::Argument)?;
        self.origin_ms
            .checked_add(elapsed_ms)
            .ok_or(ImageAdapterError::Argument)
    }

    fn back_off(&mut self, now_ms: u64) {
        self.retry_delay_ms = match self.retry_delay_ms {
            0 => 4,
            delay if delay < 256 => delay * 2,
            _ => 256,
        };
        self.retry_at_ms = now_ms.saturating_add(self.retry_delay_ms);
    }

    /// Drive one adapter turn: routed records and statuses, ticket
    /// outcomes, discovery/connect stages, then producer dispatch
    /// with real readiness polled (zero wait) on its socket.
    pub(crate) fn tick(
        &mut self,
        wire: &mut FileWire,
        ledger: &crate::custody::Ledger,
        now: Instant,
    ) -> Result<(), ShellClientError> {
        // The clock conversion is validated before anything is
        // drained or mutated: a backwards or overflowing instant is
        // an argument error the session drive surfaces.
        let now_ms = self
            .advance_clock(now)
            .map_err(|_| ShellClientError::InvalidArgument("image clock"))?;
        if wire.peer_closed() {
            self.on_session_final();
        }
        {
            let Some(producer) = &mut self.producer else {
                return Ok(());
            };
            while let Some(routed) = wire.image_routed.pop_front() {
                let result = match routed {
                    Routed::Event(event) => producer.record(wire_epoch(wire), event, now_ms),
                    Routed::Status(route) => {
                        // Only the offer the status names, with the
                        // exact resource identity, is forwarded; any
                        // other status is the application's alone and
                        // never mutates image custody.
                        let named = producer.offer(route.transaction).is_some_and(|info| {
                            info.resource == route.resource
                                && info.resource_generation == route.generation
                        });
                        if named {
                            producer.resource_status(
                                route.resource,
                                route.generation,
                                route.status,
                                route.unavailable,
                                now_ms,
                            )
                        } else {
                            Ok(())
                        }
                    }
                };
                if let Err(ProducerError::Argument) = result {
                    return Err(ShellClientError::InvalidArgument("image clock"));
                }
            }
        }
        self.settle_tickets(ledger, now_ms);
        if self.lost {
            if let Some(producer) = &mut self.producer {
                producer.journal_lost();
            }
            self.lost = false;
        }
        match self.stage {
            Stage::Discovering => self.discovery(wire, now_ms),
            Stage::Connecting => self.connect(now_ms),
            Stage::Running => {}
        }
        let Some(producer) = &mut self.producer else {
            return Ok(());
        };
        if let Some(fd) = producer.poll_fd() {
            let wanted = producer.poll_events();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::from_bits_retain(wanted.0),
            )];
            let zero = rustix::event::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            if rustix::event::poll(&mut fds, Some(&zero)).is_ok() {
                let revents = ReadyEvents(fds[0].revents().bits());
                if let Err(ProducerError::Argument) = producer.dispatch(revents, now_ms) {
                    return Err(ShellClientError::InvalidArgument("image clock"));
                }
            }
        } else if let Err(ProducerError::Argument) = producer.dispatch(ReadyEvents::NONE, now_ms) {
            return Err(ShellClientError::InvalidArgument("image clock"));
        }
        Ok(())
    }

    fn settle_tickets(&mut self, ledger: &crate::custody::Ledger, now_ms: u64) {
        for slot in &mut self.tickets {
            let Some(entry) = slot.as_ref() else { continue };
            let (ticket, offer) = (entry.ticket, entry.offer);
            match ledger.get(crate::Ticket(ticket)) {
                Some(crate::Custody::Submitted) => *slot = None,
                Some(crate::Custody::Refused(_) | crate::Custody::DroppedUnsent) => {
                    if let Some(producer) = &mut self.producer {
                        let _ = producer.begin_refused(offer, now_ms);
                    }
                    *slot = None;
                }
                // Unknown fate, or local history evicted: the C
                // adapter treats both as journal loss.
                Some(crate::Custody::Unknown) | None => {
                    self.lost = true;
                    *slot = None;
                }
                _ => {}
            }
        }
    }

    fn discovery(&mut self, wire: &mut FileWire, now_ms: u64) {
        let Some(producer) = &self.producer else {
            return;
        };
        if producer.channel() != sophia_image_producer::ChannelState::Idle {
            self.stage = Stage::Running;
            return;
        }
        if wire.image_fetch.is_none() && wire.image_discovery.is_none() {
            if now_ms < self.retry_at_ms {
                return;
            }
            wire.start_image_fetch(self.config.formats_capacity);
            return;
        }
        let Some(outcome) = wire.image_discovery.take() else {
            return;
        };
        self.pass += 1;
        let result = match outcome {
            DiscoveryOutcome::Set {
                text,
                limits,
                formats,
            } => {
                self.discovery_text.clone_from(&text);
                // The staged reader fetched whole records; the
                // producer's discovery set carries bodies, with the
                // header epochs supplied separately.
                let parsed = (|| {
                    use sophia_shell_protocol::shell_files::{
                        ShellFileClass, ShellFileKind, decode_shell_file_record_at_revision,
                    };
                    let limits_record =
                        decode_shell_file_record_at_revision(&limits, ShellFileClass::Object, 10)
                            .ok()?;
                    let formats_record =
                        decode_shell_file_record_at_revision(&formats, ShellFileClass::Object, 10)
                            .ok()?;
                    if limits_record.header.kind != ShellFileKind::ImageLimits
                        || formats_record.header.kind != ShellFileKind::ImageFormats
                    {
                        return None;
                    }
                    Some((
                        limits_record.body.to_vec(),
                        formats_record.body.to_vec(),
                        limits_record.header.connection_epoch,
                        formats_record.header.connection_epoch,
                    ))
                })();
                let Some((limits_body, formats_body, limits_epoch, formats_epoch)) = parsed else {
                    match &mut self.producer {
                        Some(producer) => {
                            let _ = producer.discover(
                                sophia_image_producer::DiscoveryRead::Failed,
                                self.pass,
                                now_ms,
                            );
                            self.stage = Stage::Running;
                            return;
                        }
                        None => return,
                    }
                };
                let read = sophia_image_producer::DiscoveryRead::Present {
                    text: &text,
                    limits_body: &limits_body,
                    formats_body: &formats_body,
                    limits_connection_epoch: limits_epoch,
                    formats_connection_epoch: formats_epoch,
                };
                match &mut self.producer {
                    Some(producer) => producer.discover(read, self.pass, now_ms),
                    None => return,
                }
            }
            DiscoveryOutcome::Absent => match &mut self.producer {
                Some(producer) => producer.discover(
                    sophia_image_producer::DiscoveryRead::Absent,
                    self.pass,
                    now_ms,
                ),
                None => return,
            },
            DiscoveryOutcome::Failed => match &mut self.producer {
                Some(producer) => producer.discover(
                    sophia_image_producer::DiscoveryRead::Failed,
                    self.pass,
                    now_ms,
                ),
                None => return,
            },
        };
        match result {
            Ok(()) => {
                self.stage = Stage::Connecting;
                self.retry_delay_ms = 0;
                self.retry_at_ms = now_ms;
            }
            Err(ProducerError::Again) => self.back_off(now_ms),
            Err(_) => self.stage = Stage::Running,
        }
    }

    fn connect(&mut self, now_ms: u64) {
        if now_ms < self.retry_at_ms {
            return;
        }
        let endpoint = sophia_shell_protocol::shell_files::ShellImageDiscovery::decode(
            std::str::from_utf8(&self.discovery_text).unwrap_or(""),
        )
        .map(|discovery| discovery.endpoint);
        let Ok(endpoint) = endpoint else {
            if let Some(producer) = &mut self.producer {
                producer.close();
            }
            self.stage = Stage::Running;
            return;
        };
        match sophia_image_producer::connect(&self.config.endpoint_dir, &endpoint) {
            Ok(fd) => {
                let result = match &mut self.producer {
                    Some(producer) => producer.attach(fd, now_ms).map_err(|(error, fd)| {
                        drop(fd);
                        error
                    }),
                    None => Err(ProducerError::Closed),
                };
                if result.is_err()
                    && let Some(producer) = &mut self.producer
                {
                    producer.close();
                }
            }
            Err(ProducerError::Again) => {
                self.back_off(now_ms);
                return;
            }
            Err(_) => {
                if let Some(producer) = &mut self.producer {
                    producer.close();
                }
            }
        }
        self.stage = Stage::Running;
    }

    /// The session ended: unsettled offers become Unknown, once.
    pub(crate) fn on_session_final(&mut self) {
        if self.final_notified {
            return;
        }
        self.final_notified = true;
        if let Some(producer) = &mut self.producer {
            producer.journal_lost();
        }
    }

    /// The next instant the caller should drive the connection for
    /// the adapter's sake, if any.
    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let now_ms = self.now_ms(now).ok()?;
        let mut due: Option<u64> = None;
        if matches!(self.stage, Stage::Discovering | Stage::Connecting) && self.producer.is_some() {
            due = Some(self.retry_at_ms);
        }
        if let Some(producer) = &self.producer
            && let Some(timeout) = producer.timeout(now_ms)
        {
            let at = now_ms.saturating_add(timeout);
            due = Some(due.map_or(at, |current| current.min(at)));
        }
        due.map(|due_ms| self.origin + Duration::from_millis(due_ms.saturating_sub(self.origin_ms)))
    }

    pub fn channel(&self) -> Option<ChannelState> {
        self.producer.as_ref().map(ImageProducer::channel)
    }

    pub fn cause(&self) -> Option<CloseCause> {
        self.producer.as_ref().map(ImageProducer::cause)
    }

    pub fn path(&self) -> Option<ImagePath> {
        self.producer.as_ref().map(ImageProducer::path)
    }

    pub fn offer(&self, offer: u64) -> Option<OfferInfo> {
        self.producer.as_ref().and_then(|p| p.offer(offer))
    }

    pub fn offers(&self) -> Vec<OfferInfo> {
        self.producer
            .as_ref()
            .map(|p| p.offers().collect())
            .unwrap_or_default()
    }

    pub fn poll_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        self.producer.as_ref().and_then(ImageProducer::poll_fd)
    }

    pub fn poll_events(&self) -> ReadyEvents {
        self.producer
            .as_ref()
            .map_or(ReadyEvents::NONE, ImageProducer::poll_events)
    }

    pub(crate) fn producer_mut(&mut self) -> Option<&mut ImageProducer> {
        self.producer.as_mut()
    }

    pub(crate) fn has_free_ticket(&self) -> bool {
        self.tickets.iter().any(Option::is_none)
    }

    pub(crate) fn register_ticket(
        &mut self,
        ticket: u64,
        offer: u64,
        metadata: ProducerMetadata,
    ) -> Result<(), ImageAdapterError> {
        let Some(slot) = self.tickets.iter_mut().find(|slot| slot.is_none()) else {
            return Err(ImageAdapterError::Busy);
        };
        *slot = Some(TicketSlot { ticket, offer });
        if let Some(entry) = self.offer_metadata.iter_mut().find(|entry| entry.is_none()) {
            *entry = Some((offer, metadata));
        }
        Ok(())
    }

    pub(crate) fn ticket_metadata(&self, offer: u64) -> Option<ProducerMetadata> {
        self.offer_metadata
            .iter()
            .flatten()
            .find(|(id, _)| *id == offer)
            .map(|(_, metadata)| *metadata)
    }

    pub(crate) fn forget_offer(&mut self, offer: u64) {
        for entry in &mut self.offer_metadata {
            if entry.is_some_and(|(id, _)| id == offer) {
                *entry = None;
            }
        }
    }
}

fn wire_epoch(wire: &FileWire) -> u64 {
    wire.epoch
}
