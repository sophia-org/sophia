//! ISB images that left the lock's resources while something may still
//! read them. A retired ISB image keeps its live place and its backing
//! bytes against the epoch's budgets until every `SharedPixels` clone of it
//! has gone (Session's lock frames, a candidate, an undelivered event);
//! only then is its `ResourceReleased` journalled, with the credit its
//! retirement held. When the connection ends, its images and those still
//! retiring become orphans, which the next epoch inherits against the same
//! budgets until released. An entry only moves between these states; none
//! is added outside a live place, so the total stays within the
//! live-resource limit. Nothing here waits: owners poll at
//! [`ISB_LEASE_POLL`] while any entry is held.
use std::time::Duration;

use sophia_9p::Errno;
use sophia_protocol::lock_files::*;

use super::{LockFileCustody, reason};
use crate::{SharedPixels, SharedPixelsWatch};

/// How often an owner checks held ISB leases for release.
pub const ISB_LEASE_POLL: Duration = Duration::from_millis(50);

/// The charge of one ISB backing until its last clone goes.
pub(super) struct IsbLease {
    bytes: u64,
    watch: SharedPixelsWatch,
}

impl IsbLease {
    pub(super) fn new(pixels: &SharedPixels) -> Self {
        Self {
            bytes: pixels.len() as u64,
            watch: pixels.watch(),
        }
    }
}

/// A retired ISB image whose `ResourceReleased` waits on its lease.
struct Retiring {
    transaction: u64,
    resource: LockResourceId,
    lease: IsbLease,
}

#[derive(Default)]
pub(super) struct IsbLeases {
    retiring: Vec<Retiring>,
    /// Inherited from earlier epochs; released silently.
    orphans: Vec<IsbLease>,
}

impl IsbLeases {
    pub(super) fn len(&self) -> usize {
        self.retiring.len() + self.orphans.len()
    }

    /// Journal credits held: one `ResourceReleased` per retiring image.
    pub(super) fn credits(&self) -> usize {
        self.retiring.len()
    }

    pub(super) fn bytes(&self) -> u64 {
        self.retiring
            .iter()
            .map(|retiring| &retiring.lease)
            .chain(&self.orphans)
            .map(|lease| lease.bytes)
            .sum()
    }

    pub(super) fn holds(&self, resource: LockResourceId) -> bool {
        self.retiring
            .iter()
            .any(|retiring| retiring.resource == resource)
    }

    pub(super) fn retire(
        &mut self,
        transaction: u64,
        resource: LockResourceId,
        pixels: SharedPixels,
    ) {
        let lease = IsbLease::new(&pixels);
        drop(pixels);
        self.retiring.push(Retiring {
            transaction,
            resource,
            lease,
        });
    }

    /// Removes released orphans and returns the released retiring images,
    /// leaving them in place.
    fn released(&mut self) -> Vec<(u64, LockResourceId)> {
        self.orphans.retain(|lease| !lease.watch.released());
        self.retiring
            .iter()
            .filter(|retiring| retiring.lease.watch.released())
            .map(|retiring| (retiring.transaction, retiring.resource))
            .collect()
    }

    fn remove(&mut self, resource: LockResourceId) {
        self.retiring
            .retain(|retiring| retiring.resource != resource);
    }

    /// Every lease, retiring ones turned orphan.
    pub(super) fn take(&mut self) -> Vec<IsbLease> {
        let mut leases: Vec<_> = self
            .retiring
            .drain(..)
            .map(|retiring| retiring.lease)
            .collect();
        leases.append(&mut self.orphans);
        leases
    }
}

/// Orphan leases held between connections. Each came from a live place of
/// an ended epoch, so their number stays within the live-resource limit.
#[derive(Default)]
pub(super) struct OrphanLeases(Vec<IsbLease>);

impl OrphanLeases {
    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    /// Drops released orphans; `true` while any remain.
    pub(super) fn poll(&mut self) -> bool {
        self.0.retain(|lease| !lease.watch.released());
        !self.0.is_empty()
    }

    /// Takes the ending epoch's leases: its live ISB images, retiring
    /// images and inherited orphans.
    pub(super) fn adopt(&mut self, custody: &mut LockFileCustody) {
        self.0.extend(custody.resources.take_isb_leases());
        self.poll();
    }

    /// Hands every orphan to the next epoch's custody.
    pub(super) fn bequeath(&mut self, custody: &mut LockFileCustody) {
        self.poll();
        custody.resources.leases.orphans.append(&mut self.0);
    }
}

impl LockFileCustody {
    /// Journals `ResourceReleased` for retired ISB images whose last
    /// clone has gone, spending the credits their retirement held, and
    /// drops released orphans. Owners call this every turn and on ACK. On
    /// `EAGAIN` the releases wait for journal room.
    pub fn collect_isb_leases(&mut self) -> Result<bool, Errno> {
        let released = self.resources.leases.released();
        if released.is_empty() || self.live().is_err() {
            return Ok(false);
        }
        let events = released
            .iter()
            .map(|(transaction, resource)| {
                LockResourceReleased {
                    transaction: *transaction,
                    resource: *resource,
                    reason: reason::NONE,
                }
                .encode()
                .map(|body| (LockFileKind::ResourceReleased, body))
                .map_err(|_| Errno::EINVAL)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.journal_events(&events, self.credits() - released.len())?;
        for (_, resource) in released {
            self.resources.leases.remove(resource);
        }
        Ok(true)
    }

    /// ISB images retiring or inherited, still charged to this epoch.
    pub fn isb_leases_pending(&self) -> usize {
        self.resources.leases.len()
    }
}

#[path = "../../tests/support/lock_image_leases.rs"]
mod tests;
