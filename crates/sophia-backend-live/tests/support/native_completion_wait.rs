use super::*;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;

#[test]
fn mirror_heads_share_one_card_wait_and_cards_keep_the_earliest_watchdog() {
    let (a, _a_kernel) = UnixStream::pair().unwrap();
    let (b, _b_kernel) = UnixStream::pair().unwrap();
    let start = Instant::now();
    let mut wait = LiveNativeCompletionWait::default();
    wait.observe_group(
        a.as_fd(),
        [start, start + Duration::from_millis(20)].into_iter(),
        false,
    );
    wait.observe_group(
        b.as_fd(),
        [start + Duration::from_millis(10)].into_iter(),
        false,
    );
    assert_eq!(
        wait.descriptors
            .iter()
            .map(AsRawFd::as_raw_fd)
            .collect::<Vec<_>>(),
        [a.as_raw_fd(), b.as_raw_fd()]
    );
    assert!(wait.submissions);
    assert!(!wait.short_service);
    assert_eq!(
        wait.deadline,
        Some(start + LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL)
    );
    let mut drained = LiveNativeCompletionWait::default();
    drained.observe_group(a.as_fd(), std::iter::empty(), false);
    drained.observe_group(b.as_fd(), std::iter::empty(), false);
    assert!(drained.descriptors.is_empty());
    assert!(!drained.submissions);
    assert!(!drained.short_service);
    assert!(drained.deadline.is_none());
}

#[test]
fn callbacks_already_read_from_the_card_need_service_without_another_fd_event() {
    let (card, _kernel) = UnixStream::pair().unwrap();
    let mut wait = LiveNativeCompletionWait::default();
    wait.observe_group(card.as_fd(), std::iter::empty(), true);
    assert!(wait.short_service);
    assert!(wait.descriptors.is_empty());
}

#[test]
fn new_or_pending_fences_keep_readiness_after_a_predecessor_signalled() {
    let (fence, _signal) = UnixStream::pair().unwrap();
    for status in [
        LibdrmNativeCompletionFenceStatus::Unsupported,
        LibdrmNativeCompletionFenceStatus::Pending,
    ] {
        let mut wait = LiveNativeCompletionWait::default();
        wait.observe_fence(fence.as_fd(), status);
        assert_eq!(wait.descriptors.len(), 1);
        assert!(!wait.short_service);
    }
}
