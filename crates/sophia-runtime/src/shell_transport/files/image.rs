//! Side-channel operations borrow the same authentication owner as 9P fids.
//! Neither cached identity nor a legacy export can authorize a descriptor.
use super::ShellFileWire;
use crate::PolicyPeerIdentity;
use crate::image_channel::{ImageChannelListener, ImageChannelPeer, ImageHelloPending};
use crate::image_channel::{ImageChannelRole, receive_image_datagram};
use sophia_protocol::shell_files::ShellChannelClosedReason as Reason;
use std::os::fd::{AsRawFd, RawFd};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ListenerSubscription {
    pub(super) descriptor: RawFd,
    epoch: u64,
    generation: u64,
}

pub(super) struct ImageService {
    listener: ImageChannelListener,
    expected: PolicyPeerIdentity,
    epoch: u64,
    generation: u64,
    kinds: u32,
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
    fn probe_idle_ready_listener(&self) -> bool {
        if !matches!(self.state, State::Ready(_)) || self.accept_not_before.is_some() {
            return false;
        }
        // Even an empty accept allocates kernel socket/file objects on Linux.
        // Without a borrowed observation, probe only this established listener.
        // Errors and nonempty readiness retain the ordinary accept path; a late
        // connector remains level-ready for the next service. No authority is
        // inferred here, and the live peer is still checked on every visit.
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let mut fds = [PollFd::from_borrowed_fd(
            self.listener.readiness(),
            PollFlags::IN,
        )];
        matches!(
            poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: 0,
                    tv_nsec: 0
                })
            ),
            Ok(0)
        )
    }

    pub(super) fn listener_subscription(&self) -> Option<ListenerSubscription> {
        // Handshake and backoff transitions always retain their ordinary probe.
        (matches!(self.state, State::Ready(_)) && self.accept_not_before.is_none()).then(|| {
            ListenerSubscription {
                descriptor: self.listener.readiness().as_raw_fd(),
                epoch: self.epoch,
                generation: self.generation,
            }
        })
    }
    pub(super) fn wait(&self, now: Instant) -> Duration {
        let channel_wait = match &self.state {
            State::Hello(hello) => hello.deadline().saturating_duration_since(now),
            // Journal pressure clears on 9P ACK progress; do not busy-poll it.
            State::Closing(_) => Duration::MAX,
            _ => Duration::MAX,
        };
        channel_wait.min(
            self.accept_not_before
                .map_or(Duration::MAX, |at| at.saturating_duration_since(now)),
        )
    }

    pub(super) fn visit_poll_fds<'a>(&'a self, mut visit: impl FnMut(rustix::event::PollFd<'a>)) {
        use rustix::event::{PollFd, PollFlags};
        if self.accept_not_before.is_none_or(|at| Instant::now() >= at) {
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

    fn service(
        &mut self,
        wire: &mut ShellFileWire,
        now: Instant,
        listener_idle: Option<ListenerSubscription>,
    ) {
        // One accept per owner turn also drains unauthorized/duplicate joins;
        // their refusal never changes the channel already serving this epoch.
        // A late join stays level-ready for the next visit. Missing or stale
        // observations use a fresh probe when Ready; handshake/backoff states
        // retain the ordinary duplicate/unauthorized-join drain.
        let skip_accept = (listener_idle.is_some()
            && listener_idle == self.listener_subscription())
            || self.probe_idle_ready_listener();
        let accepted = if !skip_accept && self.accept_not_before.is_none_or(|at| now >= at) {
            self.accept_not_before = None;
            match wire.accept_image_peer(&self.listener, self.expected, now) {
                Ok(peer) => peer,
                Err(_) => {
                    // Resource exhaustion can leave the listener readable.
                    // Suppress its poll interest until a bounded retry; never
                    // revoke an existing channel for another connector's error.
                    self.accept_not_before = Some(now + Duration::from_millis(10));
                    None
                }
            }
        } else {
            None
        };
        if let Some(peer) = accepted
            && matches!(self.state, State::Waiting)
        {
            match ImageHelloPending::new(
                peer,
                ImageChannelRole::Shell,
                self.epoch,
                self.generation,
                now + self.hello_timeout,
            ) {
                Ok(hello) => self.state = State::Hello(hello),
                Err(_) => self.state = State::Closing(Reason::ProtocolFault),
            }
        }
        let state = std::mem::replace(&mut self.state, State::Closed);
        self.state = match state {
            State::Hello(mut hello) => match wire.poll_image_hello(&mut hello, now) {
                Ok(false) => State::Hello(hello),
                Ok(true) => match wire
                    .export_mut()
                    .image_channel_ready(self.generation, self.kinds)
                {
                    Ok(_) => {
                        wire.server.wake().wake();
                        match hello.into_peer() {
                            Ok(peer) => State::Ready(peer),
                            Err(_) => State::Closing(Reason::ProtocolFault),
                        }
                    }
                    Err(sophia_9p::Errno::EAGAIN) => State::Hello(hello),
                    Err(_) => State::Closing(Reason::GrantRevoked),
                },
                Err(error) => State::Closing(if error.kind() == std::io::ErrorKind::TimedOut {
                    Reason::HelloTimeout
                } else if error.kind() == std::io::ErrorKind::BrokenPipe {
                    Reason::PeerDisconnect
                } else if error.kind() == std::io::ErrorKind::PermissionDenied {
                    Reason::GrantRevoked
                } else {
                    Reason::ProtocolFault
                }),
            },
            State::Ready(peer) => {
                if wire.check_image_peer(&peer, now).is_err() {
                    State::Closing(Reason::GrantRevoked)
                } else {
                    match receive_image_datagram(peer.socket()) {
                        Ok(None) => State::Ready(peer),
                        Err(rustix::io::Errno::PIPE) => State::Closing(Reason::PeerDisconnect),
                        Ok(Some(packet)) => match wire.export_mut().receive_image_source(packet) {
                            Ok(()) => State::Ready(peer),
                            Err(_) => State::Closing(Reason::ProtocolFault),
                        },
                        Err(_) => State::Closing(Reason::ProtocolFault),
                    }
                }
            }
            other => other,
        };
        if let State::Closing(reason) = self.state {
            let closed = if wire.export().image_readiness()
                == super::export::ImageReadiness::AwaitingHello
            {
                wire.export_mut()
                    .image_channel_hello_failed(self.generation, reason)
            } else {
                wire.export_mut()
                    .image_channel_closed(self.generation, reason)
                    .map(|_| ())
            };
            if closed.is_ok() {
                wire.server.wake().wake();
                self.state = State::Closed;
            }
        }
        if matches!(
            wire.export_mut().retry_pending_image_publications(),
            Ok(true)
        ) {
            wire.server.wake().wake();
        }
    }
}

impl ShellFileWire {
    pub(in crate::shell_transport) fn install_image_transport(
        &mut self,
        listener: ImageChannelListener,
        expected: PolicyPeerIdentity,
    ) -> Result<(), sophia_9p::Errno> {
        let access = self
            .export()
            .image_channel_access()
            .ok_or(sophia_9p::Errno::EACCES)?;
        if self.image.is_some()
            || access.hello_timeout_ms == 0
            || access.readiness != super::export::ImageReadiness::AwaitingHello
            || access.granted_source_kinds == 0
        {
            return Err(sophia_9p::Errno::EINVAL);
        }
        self.image = Some(ImageService {
            listener,
            expected,
            epoch: access.connection_epoch,
            generation: access.channel_generation,
            kinds: access.granted_source_kinds,
            hello_timeout: Duration::from_millis(u64::from(access.hello_timeout_ms)),
            accept_not_before: None,
            state: State::Waiting,
        });
        Ok(())
    }
    pub(super) fn service_image_channel(
        &mut self,
        now: Instant,
        listener_idle: Option<ListenerSubscription>,
    ) {
        if let Some(mut image) = self.image.take() {
            image.service(self, now, listener_idle);
            self.image = Some(image);
        }
    }
    pub(in crate::shell_transport) fn accept_image_peer(
        &mut self,
        listener: &ImageChannelListener,
        expected: PolicyPeerIdentity,
        now: Instant,
    ) -> std::io::Result<Option<ImageChannelPeer>> {
        listener.accept_authenticated(expected, self.connection, self.server.export_mut(), now)
    }

    pub(in crate::shell_transport) fn poll_image_hello(
        &mut self,
        hello: &mut ImageHelloPending,
        now: Instant,
    ) -> std::io::Result<bool> {
        hello.poll(self.server.export_mut(), now)
    }

    pub(in crate::shell_transport) fn check_image_peer(
        &mut self,
        peer: &ImageChannelPeer,
        now: Instant,
    ) -> std::io::Result<()> {
        peer.check(self.server.export_mut(), now)
    }
}
