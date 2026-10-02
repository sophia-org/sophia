//! Read custody for immutable images that stay in their original store. The
//! guards travel with queued frames and frozen sources, including worker moves.
use super::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Debug)]
pub struct LiveRendererImageRead(Arc<LiveRendererImageId>);

#[derive(Clone, Debug, Default)]
pub struct LiveRendererImageReads(Arc<Mutex<Reads>>);

#[derive(Debug, Default)]
struct Reads {
    readers: BTreeMap<LiveRendererImageId, Weak<LiveRendererImageId>>,
    evictions: BTreeSet<LiveRendererImageId>,
}

impl LiveRendererImageReads {
    pub fn same_registry(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn has_readers(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .readers
            .values()
            .any(|read| read.strong_count() != 0)
    }

    pub fn has_pending_evictions(&self) -> bool {
        !self
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .evictions
            .is_empty()
    }

    pub fn acquire(&self, image: LiveRendererImageId) -> LiveRendererImageRead {
        let mut reads = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let guard = reads
            .readers
            .get(&image)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let guard = Arc::new(image);
                reads.readers.insert(image, Arc::downgrade(&guard));
                guard
            });
        LiveRendererImageRead(guard)
    }

    pub fn contains(&self, image: LiveRendererImageId) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .readers
            .get(&image)
            .is_some_and(|read| read.strong_count() != 0)
    }

    /// Returns true only when a broadcast eviction can no longer invalidate
    /// an admitted reader. Requests are idempotent and discharged once.
    pub fn request_eviction(&self, image: LiveRendererImageId) -> bool {
        let mut reads = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if reads
            .readers
            .get(&image)
            .is_some_and(|read| read.strong_count() != 0)
        {
            reads.evictions.insert(image);
            false
        } else {
            reads.evictions.remove(&image);
            true
        }
    }

    pub fn ready_evictions(&self) -> Vec<LiveRendererImageId> {
        let reads = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        reads
            .evictions
            .iter()
            .copied()
            .filter(|image| {
                reads
                    .readers
                    .get(image)
                    .is_none_or(|read| read.strong_count() == 0)
            })
            .collect()
    }

    /// Device loss destroys the stores regardless of readers. It must not
    /// replay stale eviction requests into a replacement context.
    pub fn clear_evictions(&self) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .evictions
            .clear();
    }

    pub fn prune(&self) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .readers
            .retain(|_, read| read.strong_count() != 0);
    }
}

impl LiveRendererImageRead {
    pub fn image_id(&self) -> LiveRendererImageId {
        *self.0
    }
}
