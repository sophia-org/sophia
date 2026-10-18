//! The lock image channel beside one authenticated 9P connection. A listener
//! in the lock endpoint directory accepts connectors from the assigned
//! provider only; the first one's Hello must name this epoch and channel
//! generation before its deadline, and ChannelReady is journalled before the
//! peer is kept. After Ready, at most one packet per turn goes to the
//! validation worker's owner; a packet it does not owe closes the channel
//! as a protocol fault.
use std::time::{Duration, Instant};

use sophia_9p::connection::ConnectionId;
use sophia_protocol::lock_files::{
    LOCK_IMAGE_SOURCE_SEALED_MEMORY, LockChannelClosedReason as Reason, LockFileLimits,
    LockImageDiscovery, LockImageFormats, LockImageLimits,
};

use super::image_worker::ImageWorker;
use super::{LockFileExport, LockImageReadiness, LockImageSettings};
use crate::PolicyPeerIdentity;
use crate::authenticated_files::AuthenticatedFiles;
use crate::image_channel::{
    IMAGE_ENDPOINT_BASENAME, ImageChannelListener, ImageChannelPeer, ImageChannelRole,
    ImageHelloPending, receive_image_datagram,
};

/// What Session grants a protected provider's ISB image channel. Each
/// epoch's discovery, limits and formats derive from this and the lock
/// limits; legacy connections never receive a channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockImagePolicy {
    pub hello_timeout_ms: u32,
    pub validation_timeout_ms: u32,
    pub validation_chunk_bytes: u32,
    pub capture_interest_timeout_ms: u32,
    pub max_sealed_backing_bytes: u64,
}

impl LockImagePolicy {
    /// The channel generation is the connection epoch, so no epoch can
    /// resume another's channel. Formats follow the lock topology.
    pub(super) fn settings(
        &self,
        epoch: u64,
        limits: &LockFileLimits,
        topology_generation: u64,
    ) -> LockImageSettings {
        let offers = u32::from(limits.upload_slots).min(4);
        LockImageSettings {
            discovery: LockImageDiscovery {
                connection_epoch: epoch,
                channel_generation: epoch,
                endpoint: IMAGE_ENDPOINT_BASENAME.into(),
                source_kinds: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
                max_offers: offers,
                max_packets_per_turn: 1,
                hello_timeout_ms: self.hello_timeout_ms,
                validation_timeout_ms: self.validation_timeout_ms,
                validation_chunk_bytes: self.validation_chunk_bytes,
                capture_interest_timeout_ms: self.capture_interest_timeout_ms,
                formats_generation: topology_generation,
                limits_generation: 1,
            },
            limits: LockImageLimits {
                channel_generation: epoch,
                limits_generation: 1,
                max_resource_bytes: limits.max_resource_bytes,
                max_sealed_backing_bytes: self.max_sealed_backing_bytes,
                max_dma_source_bytes: 0,
                max_renderer_allocation_bytes: 0,
                max_renderer_entries: 0,
                max_offers: offers,
                max_validation_jobs: offers,
                max_capture_jobs: 0,
                max_admitted_outputs: u32::from(limits.max_outputs),
                max_format_rows_per_output: 1,
                max_width: limits.max_width_px,
                max_height: limits.max_height_px,
                validation_timeout_ms: self.validation_timeout_ms,
                capture_interest_timeout_ms: self.capture_interest_timeout_ms,
                validation_chunk_bytes: self.validation_chunk_bytes,
                max_packets_per_turn: 1,
            },
            formats: LockImageFormats {
                channel_generation: epoch,
                formats_generation: topology_generation,
                limits_generation: 1,
                rows: Vec::new(),
            },
        }
    }
}

pub(super) struct ImageService {
    listener: ImageChannelListener,
    expected: PolicyPeerIdentity,
    connection: ConnectionId,
    epoch: u64,
    generation: u64,
    hello_timeout: Duration,
    accept_not_before: Option<Instant>,
    state: State,
}

enum State {
    Waiting,
    Hello(ImageHelloPending),
    Ready(ImageChannelPeer),
    Closing(Reason),
    Closed,
}

impl ImageService {
    pub(super) fn new(
        listener: ImageChannelListener,
        expected: PolicyPeerIdentity,
        connection: ConnectionId,
        settings: &LockImageSettings,
    ) -> Self {
        Self {
            listener,
            expected,
            connection,
            epoch: settings.discovery.connection_epoch,
            generation: settings.discovery.channel_generation,
            hello_timeout: Duration::from_millis(settings.discovery.hello_timeout_ms.into()),
            accept_not_before: None,
            state: State::Waiting,
        }
    }

    /// Journal pressure while closing clears on ACK progress, which the
    /// 9P socket already wakes; only the Hello and accept retry are timed.
    pub(super) fn deadline(&self) -> Option<Instant> {
        let hello = match &self.state {
            State::Hello(hello) => Some(hello.deadline()),
            _ => None,
        };
        hello.into_iter().chain(self.accept_not_before).min()
    }

    pub(super) fn visit_poll_fds<'a>(
        &'a self,
        now: Instant,
        visit: &mut impl FnMut(rustix::event::PollFd<'a>),
    ) {
        use rustix::event::{PollFd, PollFlags};
        if self.accept_not_before.is_none_or(|at| now >= at) {
            visit(PollFd::from_borrowed_fd(
                self.listener.readiness(),
                PollFlags::IN,
            ));
        }
        match &self.state {
            State::Hello(hello) => visit(PollFd::from_borrowed_fd(hello.socket(), PollFlags::IN)),
            State::Ready(peer) => visit(PollFd::from_borrowed_fd(peer.socket(), PollFlags::IN)),
            _ => {}
        }
    }

    /// One owner turn. Returns whether an event was journalled, so the
    /// caller wakes blocked readers.
    pub(super) fn service(
        &mut self,
        files: &mut AuthenticatedFiles<LockFileExport>,
        worker: Option<&mut ImageWorker>,
        now: Instant,
    ) -> bool {
        // One accept per turn also drains unauthorized and duplicate
        // connectors; refusing them never touches the channel in service.
        if self.accept_not_before.is_none_or(|at| now >= at) {
            self.accept_not_before = None;
            match self
                .listener
                .accept_authenticated(self.expected, self.connection, files, now)
            {
                Ok(Some(peer)) if matches!(self.state, State::Waiting) => {
                    self.state = match ImageHelloPending::new(
                        peer,
                        ImageChannelRole::Lock,
                        self.epoch,
                        self.generation,
                        now + self.hello_timeout,
                    ) {
                        Ok(hello) => State::Hello(hello),
                        Err(_) => State::Closing(Reason::ProtocolFault),
                    };
                }
                Ok(_) => {}
                // Exhaustion can leave the listener readable; back off
                // briefly rather than spin or close a working channel.
                Err(_) => self.accept_not_before = Some(now + Duration::from_millis(10)),
            }
        }
        let mut journalled = false;
        self.state = match std::mem::replace(&mut self.state, State::Closed) {
            State::Hello(mut hello) => match hello.poll(files, now) {
                Ok(false) => State::Hello(hello),
                Ok(true) => match files
                    .files_mut()
                    .custody_mut()
                    .image_channel_ready(self.generation, LOCK_IMAGE_SOURCE_SEALED_MEMORY)
                {
                    Ok(_) => {
                        journalled = true;
                        match hello.into_peer() {
                            Ok(peer) => State::Ready(peer),
                            Err(_) => State::Closing(Reason::ProtocolFault),
                        }
                    }
                    // No journal room yet: the checked Hello waits within
                    // its original deadline.
                    Err(sophia_9p::Errno::EAGAIN) => State::Hello(hello),
                    Err(_) => State::Closing(Reason::GrantRevoked),
                },
                Err(error) => State::Closing(match error.kind() {
                    std::io::ErrorKind::TimedOut => Reason::HelloTimeout,
                    std::io::ErrorKind::BrokenPipe => Reason::PeerDisconnect,
                    std::io::ErrorKind::PermissionDenied => Reason::GrantRevoked,
                    _ => Reason::ProtocolFault,
                }),
            },
            State::Ready(peer) => {
                if peer.check(files, now).is_err() {
                    State::Closing(Reason::GrantRevoked)
                } else {
                    match (receive_image_datagram(peer.socket()), worker) {
                        (Ok(None), _) => State::Ready(peer),
                        (Err(rustix::io::Errno::PIPE), _) => State::Closing(Reason::PeerDisconnect),
                        (Ok(Some(datagram)), Some(worker)) => {
                            match worker.source(files.files_mut().custody_mut(), datagram) {
                                Ok(()) => State::Ready(peer),
                                Err(_) => State::Closing(Reason::ProtocolFault),
                            }
                        }
                        (Ok(Some(_)) | Err(_), _) => State::Closing(Reason::ProtocolFault),
                    }
                }
            }
            other => other,
        };
        if let State::Closing(reason) = self.state {
            let custody = files.files_mut().custody_mut();
            let closed = if custody.image_readiness() == LockImageReadiness::AwaitingHello {
                custody.image_channel_hello_failed(self.generation, reason)
            } else {
                custody
                    .image_channel_closed(self.generation, reason)
                    .map(|_| ())
            };
            if closed.is_ok() {
                journalled = true;
                self.state = State::Closed;
            }
        }
        journalled
    }
}
