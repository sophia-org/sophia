//! Borrow readiness from the sole transport reader. No independent socket read.
use rustix::event::{PollFd, PollFlags};
use std::time::{Duration, Instant};

use super::{LockFileTransport, LockFileTransportError};

impl LockFileTransport {
    pub(in crate::lock_files) fn wait_for_work(
        &self,
        wake: &sophia_wake::Wake,
        service_io: bool,
    ) -> Result<(), LockFileTransportError> {
        let now = Instant::now();
        let expiry = self
            .export()
            .map_or(Duration::MAX, |export| export.wait(now, Duration::MAX));
        let deadline = now
            .checked_add(expiry)
            .into_iter()
            .chain(self.negotiation_deadline)
            .min();
        let mut fds = vec![PollFd::new(wake, PollFlags::IN)];
        // A full owner handoff cannot consume more inbound events. Waiting on
        // a permanently readable peer there would spin; owner drains wake us.
        if service_io {
            if let Some(server) = &self.server {
                fds.extend(server.poll_fds());
            } else if self.assignee_alive()?
                && let Some(fd) = self.endpoint.accept_readiness()
            {
                fds.push(PollFd::new(&fd, PollFlags::IN));
                sophia_wake::wait(&mut fds, deadline)?;
                return Ok(());
            }
        }
        sophia_wake::wait(&mut fds, deadline)?;
        Ok(())
    }
}
