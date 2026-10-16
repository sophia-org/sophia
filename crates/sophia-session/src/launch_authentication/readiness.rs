//! Borrowed relay subscriptions. Launch records retain each actual wait once;
//! immediate owner work uses one aggregate readiness sample instead.
use super::{Error, LaunchBook};
use rustix::event::{PollFd, Timespec, poll};
use smallvec::SmallVec;
use sophia_9p::unix::{Observation, Subscription};
use sophia_factotum::launch::MAX_DELEGATIONS;
use std::{cell::Cell, time::Instant};

pub struct RelayWait<'a> {
    start: usize,
    subscription: Subscription,
    target: &'a Cell<Option<Observation>>,
}

impl RelayWait<'_> {
    pub fn observe(self, fds: &[PollFd<'_>]) {
        self.target.set(
            fds.get(self.start..self.start + 2)
                .and_then(|fds| self.subscription.observe(fds)),
        );
    }
}

impl LaunchBook {
    pub fn relay_deadline(&self) -> Option<Instant> {
        self.records
            .iter()
            .filter_map(|record| record.relay.as_ref()?.deadline())
            .min()
    }

    pub fn subscribe_relays<'a>(
        &'a self,
        start: usize,
        visit: impl FnMut(PollFd<'a>),
    ) -> SmallVec<[RelayWait<'a>; MAX_DELEGATIONS]> {
        self.relay_subscriptions(start, visit, false)
    }

    fn relay_subscriptions<'a>(
        &'a self,
        mut start: usize,
        mut visit: impl FnMut(PollFd<'a>),
        missing_only: bool,
    ) -> SmallVec<[RelayWait<'a>; MAX_DELEGATIONS]> {
        let mut waits = SmallVec::new();
        for record in &self.records {
            let Some(relay) = &record.relay else {
                continue;
            };
            if let Some(observed) = record.readiness.take()
                && missing_only
                && relay.observation_is_current(&observed)
            {
                record.readiness.set(Some(observed));
                continue;
            }
            let Some(subscription) = relay.subscription() else {
                continue;
            };
            let first = start;
            relay.visit_poll_fds(|fd| {
                visit(fd);
                start += 1;
            });
            waits.push(RelayWait {
                start: first,
                subscription,
                target: &record.readiness,
            });
        }
        waits
    }

    pub(super) fn sample_missing_readiness(&self) -> Result<(), Error> {
        let mut fds = SmallVec::<[PollFd<'_>; 128]>::new();
        let waits = self.relay_subscriptions(0, |fd| fds.push(fd), true);
        if !fds.is_empty() {
            poll(&mut fds, Some(&Timespec::default())).map_err(|e| Error::Io(e.into()))?;
            for wait in waits {
                wait.observe(&fds);
            }
        }
        Ok(())
    }
}
