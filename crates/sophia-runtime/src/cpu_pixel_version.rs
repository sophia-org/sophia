//! Process-local identity for trusted immutable CPU pixel revisions.
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// Exhaustion disables reuse; identities never wrap or alias a previous revision.
pub fn next_cpu_pixel_version() -> Option<NonZeroU64> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .ok()
        .and_then(NonZeroU64::new)
}
