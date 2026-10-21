use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use crate::io;
use crate::offers::Slot;
use crate::role::{self, Expect};
use crate::{
    ChannelState, CloseCause, DiscoveryRead, DispatchChanged, ImageClosedReason, ImageEvent,
    ImagePath, ImageProducerConfig, ImageRole, KIND_BIT_SEALED, MAX_OFFERS, OfferState,
    ProducerError, ReadyEvents,
};

/// The producer state machine. See the crate documentation.
pub struct ImageProducer {
    pub(crate) config: ImageProducerConfig,
    pub(crate) expect: Option<Expect>,
    pub(crate) slots: [Slot; MAX_OFFERS],
    pub(crate) last_now: u64,
    pub(crate) last_pass: Option<u64>,
    pub(crate) next_offer: u64,
    pub(crate) hello_deadline_ms: u64,
    pub(crate) hello_timeout_ms: u32,
    pub(crate) max_offers: u32,
    pub(crate) ignored: u32,
    pub(crate) fd: Option<OwnedFd>,
    pub(crate) channel: ChannelState,
    pub(crate) cause: CloseCause,
}

impl ImageProducer {
    pub fn new(config: ImageProducerConfig, now_ms: u64) -> Result<Self, ProducerError> {
        if config.connection_epoch == 0
            || config.transfer_timeout_ms == 0
            || now_ms >= config.startup_deadline_ms
            || config.granted_kinds == 0
            || config.granted_kinds & !0b11 != 0
        {
            return Err(ProducerError::Argument);
        }
        // Exactly the known negotiated revisions; nothing is inferred.
        let known = match config.role {
            ImageRole::Shell => config.revision == 9,
            ImageRole::Lock => config.revision == 2,
        };
        if !known {
            return Err(ProducerError::Argument);
        }
        Ok(Self {
            config,
            expect: None,
            slots: Default::default(),
            last_now: now_ms,
            last_pass: None,
            next_offer: 1,
            hello_deadline_ms: 0,
            hello_timeout_ms: 0,
            max_offers: 0,
            ignored: 0,
            fd: None,
            channel: ChannelState::Idle,
            cause: CloseCause::None,
        })
    }

    pub(crate) fn clock(&mut self, now_ms: u64) -> Result<(), ProducerError> {
        if now_ms < self.last_now {
            return Err(ProducerError::Argument);
        }
        self.last_now = now_ms;
        Ok(())
    }

    pub(crate) fn fail(&mut self, cause: CloseCause) {
        self.fd = None;
        if self.channel != ChannelState::Closed || (self.cause.is_clean() && !cause.is_clean()) {
            self.cause = cause;
        }
        self.channel = ChannelState::Closed;
    }

    pub(crate) fn slot_mut(&mut self, offer: u64) -> Option<&mut Slot> {
        if offer == 0 {
            return None;
        }
        self.slots
            .iter_mut()
            .find(|slot| slot.state.is_some() && slot.metadata.offer_id == offer)
    }

    pub(crate) fn fault(&mut self) -> ProducerError {
        self.fail(CloseCause::Fault);
        ProducerError::Closed
    }

    /// Consume one discovery set. Only a generation mismatch between
    /// the three objects is retryable (`Again` on a later pass);
    /// malformed bytes, wrong epochs or roles, incoherence beyond
    /// generations, an ungranted source kind, or a failed read are
    /// fatal to the channel. A set without the sealed kind closes
    /// cleanly as unavailable.
    pub fn discover(
        &mut self,
        read: DiscoveryRead<'_>,
        pass: u64,
        now_ms: u64,
    ) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        if self.channel != ChannelState::Idle {
            return Err(if self.channel == ChannelState::Closed {
                ProducerError::Closed
            } else {
                ProducerError::Refused
            });
        }
        if now_ms >= self.config.startup_deadline_ms {
            self.fail(CloseCause::Timeout);
            return Err(ProducerError::Closed);
        }
        if self.last_pass == Some(pass) {
            return Err(ProducerError::Again);
        }
        self.last_pass = Some(pass);
        let DiscoveryRead::Present {
            text,
            limits_body,
            formats_body,
            limits_connection_epoch,
            formats_connection_epoch,
        } = read
        else {
            self.fail(if matches!(read, DiscoveryRead::Absent) {
                CloseCause::Absent
            } else {
                CloseCause::Discovery
            });
            return Err(ProducerError::Closed);
        };
        let (expect, hello_timeout_ms, max_offers) = match role::decode_discovery(
            self.config.role,
            self.config.connection_epoch,
            &self.config.bounds,
            role::DiscoverySet {
                text,
                limits_body,
                formats_body,
                limits_connection_epoch,
                formats_connection_epoch,
            },
        ) {
            Ok(decoded) => decoded,
            Err(ProducerError::Again) => return Err(ProducerError::Again),
            Err(_) => {
                self.fail(CloseCause::Discovery);
                return Err(ProducerError::Closed);
            }
        };
        if expect.source_kinds() & !self.config.granted_kinds != 0 {
            self.fail(CloseCause::Discovery);
            return Err(ProducerError::Closed);
        }
        if expect.source_kinds() & KIND_BIT_SEALED == 0 {
            self.fail(CloseCause::Unavailable);
            return Err(ProducerError::Closed);
        }
        self.expect = Some(expect);
        self.hello_timeout_ms = hello_timeout_ms;
        self.max_offers = max_offers;
        self.channel = ChannelState::Discovered;
        Ok(())
    }

    /// Take ownership of a connected SEQPACKET socket in Discovered
    /// and queue Hello; the descriptor type is verified because a
    /// stream socket would silently change packet semantics. On any
    /// error the caller keeps the descriptor.
    pub fn attach(&mut self, fd: OwnedFd, now_ms: u64) -> Result<(), (ProducerError, OwnedFd)> {
        if let Err(error) = self.clock(now_ms) {
            return Err((error, fd));
        }
        if self.channel != ChannelState::Discovered {
            return Err((
                if self.channel == ChannelState::Closed {
                    ProducerError::Closed
                } else {
                    ProducerError::Refused
                },
                fd,
            ));
        }
        if now_ms >= self.config.startup_deadline_ms {
            self.fail(CloseCause::Timeout);
            return Err((ProducerError::Closed, fd));
        }
        if !io::is_seqpacket(fd.as_fd()) {
            return Err((ProducerError::Argument, fd));
        }
        self.fd = Some(fd);
        self.hello_deadline_ms = now_ms
            .saturating_add(u64::from(self.hello_timeout_ms))
            .min(self.config.startup_deadline_ms);
        self.channel = ChannelState::HelloQueued;
        Ok(())
    }

    pub fn poll_fd(&self) -> Option<BorrowedFd<'_>> {
        self.fd.as_ref().map(|fd| fd.as_fd())
    }

    pub fn poll_events(&self) -> ReadyEvents {
        if self.fd.is_none() {
            return ReadyEvents::NONE;
        }
        let mut events = ReadyEvents::IN;
        if self.channel == ChannelState::HelloQueued || self.source_queued() {
            events = events | ReadyEvents::OUT;
        }
        events
    }

    fn source_queued(&self) -> bool {
        self.channel == ChannelState::Ready
            && self
                .slots
                .iter()
                .any(|slot| slot.state == Some(OfferState::Admitted) && !slot.cancelled)
    }

    /// Milliseconds until the earliest live deadline, or None.
    pub fn timeout(&self, now_ms: u64) -> Option<u64> {
        let mut due = u64::MAX;
        if !matches!(self.channel, ChannelState::Ready | ChannelState::Closed) {
            due = self.config.startup_deadline_ms;
        }
        if matches!(
            self.channel,
            ChannelState::HelloQueued | ChannelState::HelloSent
        ) {
            due = due.min(self.hello_deadline_ms);
        }
        for slot in &self.slots {
            if matches!(slot.state, Some(OfferState::Begun | OfferState::Admitted))
                && !slot.deadline_passed
            {
                due = due.min(slot.deadline_ms);
            }
        }
        if due == u64::MAX {
            None
        } else {
            Some(due.saturating_sub(now_ms))
        }
    }

    fn expire(&mut self, now_ms: u64) -> bool {
        let mut changed = false;
        for slot in &mut self.slots {
            if !matches!(slot.state, Some(OfferState::Begun | OfferState::Admitted))
                || slot.deadline_passed
                || now_ms < slot.deadline_ms
            {
                continue;
            }
            slot.deadline_passed = true;
            if slot.state == Some(OfferState::Admitted) {
                slot.state = Some(OfferState::Expired);
            }
            changed = true;
        }
        changed
    }

    /// One bounded nonblocking pass: expire deadlines, detect hangup,
    /// send a queued Hello, then admitted sources in offer order. A
    /// non-EAGAIN send failure marks that offer Ambiguous and closes
    /// the channel; a sent or failed descriptor is never sent again.
    pub fn dispatch(
        &mut self,
        revents: ReadyEvents,
        now_ms: u64,
    ) -> Result<DispatchChanged, ProducerError> {
        self.clock(now_ms)?;
        let before = self.channel;
        let mut changed = DispatchChanged {
            channel: false,
            offers: self.expire(now_ms),
        };
        if !matches!(self.channel, ChannelState::Ready | ChannelState::Closed)
            && (now_ms >= self.config.startup_deadline_ms
                || (matches!(
                    self.channel,
                    ChannelState::HelloQueued | ChannelState::HelloSent
                ) && now_ms >= self.hello_deadline_ms))
        {
            self.fail(CloseCause::Timeout);
        }
        if self.fd.is_some() && !self.hung_up(revents) {
            if self.channel == ChannelState::HelloQueued {
                self.send_hello();
            }
            if self.send_sources() {
                changed.offers = true;
            }
        }
        changed.channel = self.channel != before;
        Ok(changed)
    }

    fn hung_up(&mut self, revents: ReadyEvents) -> bool {
        if !revents.contains(ReadyEvents::IN)
            && !revents.contains(ReadyEvents::HUP)
            && !revents.contains(ReadyEvents::ERR)
        {
            return false;
        }
        let Some(fd) = self.fd.as_ref() else {
            return false;
        };
        match io::peek_disconnect(fd.as_fd()) {
            io::Peek::Open => false,
            io::Peek::Data => {
                self.fail(CloseCause::Fault);
                true
            }
            io::Peek::Closed => {
                self.fail(CloseCause::Unavailable);
                true
            }
        }
    }

    fn send_hello(&mut self) {
        let Some(expect) = &self.expect else { return };
        let bytes = role::hello_bytes(expect);
        let Ok(bytes) = bytes else {
            self.fail(CloseCause::Fault);
            return;
        };
        let Some(fd) = self.fd.as_ref() else { return };
        match io::send_packet(fd.as_fd(), &bytes, None) {
            io::SendOutcome::Sent => self.channel = ChannelState::HelloSent,
            io::SendOutcome::Again => {}
            io::SendOutcome::Failed(disconnect) => {
                self.fail(if disconnect {
                    CloseCause::Unavailable
                } else {
                    CloseCause::Fault
                });
            }
        }
    }

    fn send_sources(&mut self) -> bool {
        let mut changed = false;
        loop {
            if self.channel != ChannelState::Ready {
                return changed;
            }
            let mut next: Option<usize> = None;
            for (index, slot) in self.slots.iter().enumerate() {
                if slot.state == Some(OfferState::Admitted) && !slot.cancelled {
                    let better = next.is_none_or(|current| {
                        slot.metadata.offer_id < self.slots[current].metadata.offer_id
                    });
                    if better {
                        next = Some(index);
                    }
                }
            }
            let Some(index) = next else { return changed };
            let metadata = self.slots[index].metadata;
            let bytes = match &self.expect {
                Some(expect) => role::source_bytes(expect, metadata),
                None => Err(()),
            };
            let Ok(bytes) = bytes else {
                self.fail(CloseCause::Fault);
                return true;
            };
            if self.slots[index].fd.is_none() {
                self.fail(CloseCause::Fault);
                return true;
            }
            let outcome = {
                let (Some(channel), Some(source)) =
                    (self.fd.as_ref(), self.slots[index].fd.as_ref())
                else {
                    return changed;
                };
                io::send_packet(channel.as_fd(), &bytes, Some(source.as_fd()))
            };
            match outcome {
                // EAGAIN consumed nothing: the offer stays Admitted
                // with its descriptor and may be sent on a later pass.
                io::SendOutcome::Again => return changed,
                io::SendOutcome::Sent => {
                    self.slots[index].fd = None;
                    self.slots[index].state = Some(OfferState::Sent);
                    changed = true;
                }
                io::SendOutcome::Failed(disconnect) => {
                    self.slots[index].fd = None;
                    self.slots[index].state = Some(OfferState::Ambiguous);
                    self.fail(if disconnect {
                        CloseCause::Unavailable
                    } else {
                        CloseCause::Fault
                    });
                    return true;
                }
            }
        }
    }

    /// Forward one journal image record. Records of another epoch
    /// are counted and ignored. Contradictions close the channel
    /// with Fault and keep every offer's custody.
    pub fn record(
        &mut self,
        connection_epoch: u64,
        event: ImageEvent,
        now_ms: u64,
    ) -> Result<(), ProducerError> {
        self.clock(now_ms)?;
        if connection_epoch != self.config.connection_epoch {
            self.ignored = self.ignored.saturating_add(1);
            return Ok(());
        }
        match event {
            // Ring events belong to RingProducer; routing one here is a
            // caller error, not a channel fault.
            ImageEvent::Ring(_) => Err(ProducerError::Argument),
            ImageEvent::ChannelReady {
                channel_generation,
                granted_source_kinds,
            } => self.channel_record(channel_generation, Some(granted_source_kinds)),
            ImageEvent::ChannelClosed {
                channel_generation,
                reason,
            } => self.channel_closed(channel_generation, reason),
            ImageEvent::ImageAdmitted { metadata } => {
                let Some(slot) = self.slot_mut(metadata.offer_id) else {
                    return self.unknown_offer(metadata.offer_id);
                };
                if slot.state != Some(OfferState::Begun) || slot.metadata != metadata {
                    return Err(self.fault());
                }
                slot.state = Some(if slot.deadline_passed {
                    OfferState::Expired
                } else {
                    OfferState::Admitted
                });
                Ok(())
            }
            ImageEvent::SourceReleased {
                offer_id,
                resource,
                resource_generation,
                disposition,
            } => {
                if !(1..=5).contains(&disposition) {
                    return Err(ProducerError::Argument);
                }
                let Some(slot) = self.slot_mut(offer_id) else {
                    return self.unknown_offer(offer_id);
                };
                if slot.metadata.resource != resource
                    || slot.metadata.resource_generation != resource_generation
                {
                    return Err(self.fault());
                }
                if !matches!(
                    slot.state,
                    Some(
                        OfferState::Admitted
                            | OfferState::Sent
                            | OfferState::Ambiguous
                            | OfferState::Expired
                    )
                ) {
                    return Err(self.fault());
                }
                slot.fd = None;
                slot.state = Some(OfferState::Settled);
                slot.disposition = disposition;
                Ok(())
            }
        }
    }

    fn unknown_offer(&mut self, offer: u64) -> Result<(), ProducerError> {
        if offer >= self.next_offer && offer != 0 {
            return Err(self.fault());
        }
        self.ignored = self.ignored.saturating_add(1);
        Ok(())
    }

    fn channel_generation_ok(&self, generation: u64) -> bool {
        self.expect
            .as_ref()
            .is_some_and(|expect| expect.channel_generation() == generation)
    }

    fn channel_record(&mut self, generation: u64, ready: Option<u32>) -> Result<(), ProducerError> {
        if matches!(self.channel, ChannelState::Idle | ChannelState::Discovered)
            || !self.channel_generation_ok(generation)
        {
            return Err(self.fault());
        }
        let Some(granted) = ready else {
            return Err(ProducerError::Argument);
        };
        // Ready may trail a local closure; it never reopens it.
        if self.channel == ChannelState::Closed {
            return Ok(());
        }
        let Some(expect) = &self.expect else {
            return Err(self.fault());
        };
        if self.channel != ChannelState::HelloSent || granted & !expect.source_kinds() != 0 {
            return Err(self.fault());
        }
        if let Some(expect) = &mut self.expect {
            expect.set_source_kinds(granted);
        }
        if granted & KIND_BIT_SEALED == 0 {
            self.fail(CloseCause::Unavailable);
            return Err(ProducerError::Closed);
        }
        self.channel = ChannelState::Ready;
        Ok(())
    }

    fn channel_closed(
        &mut self,
        generation: u64,
        reason: ImageClosedReason,
    ) -> Result<(), ProducerError> {
        if matches!(self.channel, ChannelState::Idle | ChannelState::Discovered)
            || !self.channel_generation_ok(generation)
        {
            return Err(self.fault());
        }
        self.fail(match reason {
            ImageClosedReason::PeerDisconnect | ImageClosedReason::GrantRevoked => {
                CloseCause::Unavailable
            }
            ImageClosedReason::ProtocolFault | ImageClosedReason::HelloTimeout => CloseCause::Fault,
        });
        Ok(())
    }

    pub fn close(&mut self) {
        self.fail(CloseCause::Caller);
    }

    /// The role adapter has validated a negotiation without a usable sealed
    /// grant. This closes the side channel cleanly; it never settles offers or
    /// invents journal release. Image admission stays closed for this epoch.
    pub fn channel_unavailable(&mut self) {
        self.fail(CloseCause::Unavailable);
    }

    pub fn channel(&self) -> ChannelState {
        self.channel
    }

    pub fn cause(&self) -> CloseCause {
        self.cause
    }

    pub fn ignored_records(&self) -> u32 {
        self.ignored
    }

    pub fn path(&self) -> ImagePath {
        match self.channel {
            ChannelState::Ready => ImagePath::Descriptor,
            ChannelState::Closed => ImagePath::Error,
            _ => ImagePath::Wait,
        }
    }
}
