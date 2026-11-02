//! Single-shell compatibility owner. Independent components retain their own
//! ShellComponentTransport and borrow one Session-owned ContentEpochRegistry.
use std::cell::Cell;
use std::path::Path;

use super::{
    ShellComponentTransport, ShellReadinessObservation, ShellReadinessSubscription,
    ShellTransportError,
};
use crate::ContentEpochRegistry;

#[path = "legacy_methods.rs"]
mod legacy_methods;

pub struct ShellSessionTransport {
    pub(super) state: ShellComponentTransport,
    images: Option<super::ShellIsbImageOwner>,
    pub(super) content_epochs: ContentEpochRegistry,
    /// Readiness the owner's last wait observed for the wire, consumed by the
    /// next owner turn or replaced by the next wait.
    readiness: Cell<Option<ShellReadinessObservation>>,
}

/// One owner wait's registration of the wire's descriptors, observed once
/// after the poll that included them.
pub struct ShellSessionWait<'a> {
    start: usize,
    end: usize,
    subscription: ShellReadinessSubscription,
    target: &'a Cell<Option<ShellReadinessObservation>>,
}

impl ShellSessionWait<'_> {
    /// Call once after the wait polled the slice this registration appended.
    /// A changed slice records nothing, and the next turn probes instead.
    pub fn observe(self, fds: &[rustix::event::PollFd<'_>]) {
        self.target.set(
            fds.get(self.start..self.end)
                .and_then(|fds| self.subscription.observe(fds)),
        );
    }
}

impl ShellSessionTransport {
    pub fn bind_for_supervised_uid(
        directory: impl AsRef<Path>,
        expected_uid: u32,
    ) -> Result<Self, ShellTransportError> {
        Ok(Self {
            state: ShellComponentTransport::bind_for_supervised_uid(directory, expected_uid)?,
            content_epochs: ContentEpochRegistry::new(64 * 1024 * 1024)?,
            images: None,
            readiness: Cell::new(None),
        })
    }

    /// See [`ShellComponentTransport::poll_fds`].
    pub fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        let mut fds = self.state.poll_fds();
        self.visit_image_worker(|fd| fds.push(fd));
        fds
    }
    pub fn visit_poll_fds<'a>(&'a self, mut visit: impl FnMut(rustix::event::PollFd<'a>)) {
        self.state.visit_poll_fds(&mut visit);
        self.visit_image_worker(visit);
    }

    /// Registers the wire's descriptors from index `start` of the caller's
    /// poll set, then any retained image worker, for one owner wait. An
    /// earlier wait's unconsumed observation is dropped. The next
    /// [`Self::service_owner_turn`] consumes the observation; with none, or
    /// after an intervening turn, the wire probes its descriptors itself.
    pub fn subscribe_wait<'a>(
        &'a self,
        start: usize,
        mut visit: impl FnMut(rustix::event::PollFd<'a>),
    ) -> Option<ShellSessionWait<'a>> {
        self.readiness.take();
        let mut end = start;
        self.state.visit_poll_fds(|fd| {
            end += 1;
            visit(fd);
        });
        self.visit_image_worker(visit);
        Some(ShellSessionWait {
            start,
            end,
            subscription: self.state.readiness_subscription()?,
            target: &self.readiness,
        })
    }

    /// Drops a wait's observation when the pass does not turn the wire.
    pub fn discard_readiness(&self) {
        self.readiness.take();
    }

    /// One owner pass, consuming the last wait's observation of the wire.
    pub fn service_owner_turn(&mut self) -> Result<(), ShellTransportError> {
        self.state
            .service_owner_observed(&mut self.content_epochs, self.readiness.take())
    }

    /// See [`ShellComponentTransport::output_pending`].
    pub fn output_pending(&self) -> bool {
        self.state.output_pending()
    }

    /// Authentication deadlines belong to the same owner as wire readiness.
    pub fn authentication_wait(&self, now: std::time::Instant) -> std::time::Duration {
        self.state
            .authentication_wait(now)
            .min(self.image_wait(now))
    }

    /// Worker deadlines remain relevant after wire admission is suspended.
    pub fn image_wait(&self, now: std::time::Instant) -> std::time::Duration {
        self.images
            .as_ref()
            .and_then(|images| images.deadline())
            .map_or(std::time::Duration::MAX, |at| {
                at.saturating_duration_since(now)
            })
    }

    /// Includes returned completions awaiting source-release publication.
    pub fn pending_images(&self) -> usize {
        self.images
            .as_ref()
            .map_or(0, super::ShellIsbImageOwner::pending)
    }

    /// Retained worker readiness remains relevant while presentation is paused.
    pub fn visit_image_worker<'a>(&'a self, mut visit: impl FnMut(rustix::event::PollFd<'a>)) {
        if let Some(images) = &self.images
            && images.pending() != 0
        {
            visit(rustix::event::PollFd::from_borrowed_fd(
                images.readiness(),
                rustix::event::PollFlags::IN,
            ));
        }
    }

    /// Configure the same authenticated image channel as a component owner.
    /// The retained worker survives reconnects and reduced grants.
    pub fn configure_image_channel(
        &mut self,
        discovery: sophia_protocol::shell_files::ShellImageDiscovery,
        limits: sophia_protocol::shell_files::ShellImageLimits,
        formats: sophia_protocol::shell_files::ShellImageFormats,
        bounds: sophia_protocol::shell_files::ShellImageRoleBounds,
    ) -> Result<(), ShellTransportError> {
        self.state
            .configure_image_channel(discovery, limits, formats, bounds)?;
        if self.images.is_none() {
            let maximum =
                sophia_protocol::ContentLimits::prototype(Default::default()).max_resource_bytes;
            self.images = Some(
                super::ShellIsbImageOwner::new(maximum as usize, 65_536)
                    .map_err(|error| ShellTransportError::Io(error.to_string()))?,
            );
        }
        Ok(())
    }

    /// Visit even after disconnect while worker custody remains. `now_msec`
    /// must use the same content clock as resource and candidate services.
    pub fn service_images(
        &mut self,
        now_msec: u64,
        now: std::time::Instant,
    ) -> Result<(), ShellTransportError> {
        if let Some(images) = self.images.as_mut() {
            images.service(&mut self.state, &mut self.content_epochs, now_msec, now)?;
        }
        Ok(())
    }
}
