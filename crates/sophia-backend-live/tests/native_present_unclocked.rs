#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
//! t289: an active head whose kernel cannot supply a counter is unclocked,
//! not lost. The answer is definite only for EOPNOTSUPP/ENOTTY on a card with
//! monotonic timestamps; it is cached for that target lifetime under a stable
//! source of its own and invalidated with the target. EINVAL, transient and
//! permission errors stay query failures, and a card without monotonic
//! timestamps stays `UnsupportedClock` with nothing minted.
use sophia_backend_live::*;
use sophia_engine::RenderHeadId;
use std::num::NonZeroU64;
use std::time::Duration;

fn key(head: u64, target: u64) -> LiveNativePresentClockKey {
    LiveNativePresentClockKey {
        head: RenderHeadId::from_raw(head),
        target_generation: target,
        card_group: 0,
        crtc_id: head as u32,
    }
}

fn sample(msc: u64) -> sophia_drm_clock::CrtcSequence {
    sophia_drm_clock::CrtcSequence {
        active: true,
        sequence: msc,
        timestamp_nsec: msc * 1_000_000,
    }
}

fn clocks(owner: u64) -> LiveNativePresentClocks {
    LiveNativePresentClocks::new(NonZeroU64::new(owner).unwrap())
}

const PERIOD: Duration = Duration::from_micros(16_667);
const EOPNOTSUPP: i32 = 95;
const ENOTTY: i32 = 25;

fn unsupported(errno: i32) -> LiveNativeUnclockedReason {
    LiveNativeUnclockedReason::SequenceUnsupported { errno }
}

fn unclocked(observation: LiveNativePresentClockObservation) -> LiveNativeUnclockedPresentClock {
    assert_eq!(observation.current, None, "an unclocked head has no sample");
    match observation.status {
        LiveNativePresentClockStatus::Unclocked(clock) => clock,
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_unsupported_sequence_errnos_are_definite() {
    for errno in [EOPNOTSUPP, ENOTTY] {
        let error = std::io::Error::from_raw_os_error(errno);
        assert_eq!(unsupported_sequence_errno(&error), Some(errno));
    }
    // Permission, transient, stale-object, driver and I/O errors are asked
    // again; so is an error without an errno.
    for errno in [1, 4, 5, 11, 13, 2, 22] {
        let error = std::io::Error::from_raw_os_error(errno);
        assert_eq!(unsupported_sequence_errno(&error), None, "errno {errno}");
    }
    let invalid = std::io::Error::new(std::io::ErrorKind::InvalidData, "decoded badly");
    assert_eq!(unsupported_sequence_errno(&invalid), None);
}

#[test]
fn an_unclocked_head_keeps_its_first_answer_and_a_stable_distinct_source() {
    let mut clocks = clocks(7);
    let clocked = clocks.observe(key(1, 1), sample(10)).current.unwrap();
    let first = unclocked(clocks.observe_unsupported(key(2, 1), unsupported(ENOTTY), PERIOD));
    assert_ne!(first.source, clocked.source);
    assert_eq!(first.source.owner, 7);
    assert_eq!(first.minimum_period_usec, 16_667);
    assert_eq!(first.reason, unsupported(ENOTTY));
    // A later answer for the same lifetime repeats the first, errno included.
    let again = unclocked(clocks.observe_unsupported(
        key(2, 1),
        unsupported(EOPNOTSUPP),
        Duration::from_millis(8),
    ));
    assert_eq!(again, first);
    assert_eq!(clocks.unclocked_for(key(2, 1)).map(unclocked), Some(first));
    assert_eq!(clocks.unclocked_for(key(2, 2)), None, "another lifetime");
    // Unclocked sources never answer as clocks.
    assert_eq!(clocks.last_sample(first.source), None);
    assert_eq!(clocks.source_for(key(2, 1)), None);
    assert_eq!(
        clocks.sources().collect::<Vec<_>>(),
        [(RenderHeadId::from_raw(1), clocked.source)]
    );
    assert_eq!(
        clocks.unclocked().collect::<Vec<_>>(),
        [(RenderHeadId::from_raw(2), first)]
    );
}

#[test]
fn modeset_disable_and_replacement_retire_an_unclocked_source_for_good() {
    let mut clocks = clocks(7);
    let first = unclocked(clocks.observe_unsupported(key(2, 1), unsupported(EOPNOTSUPP), PERIOD));
    // Modeset, including one that rolls back to the same target.
    assert_eq!(clocks.invalidate(), [first.source]);
    let second = unclocked(clocks.observe_unsupported(key(2, 1), unsupported(EOPNOTSUPP), PERIOD));
    assert_ne!(second.source, first.source, "never recycled");
    // Disable.
    let lost = clocks.lose_head(
        RenderHeadId::from_raw(2),
        LiveNativePresentClockStatus::Inactive,
    );
    assert_eq!(lost.lost, Some(second.source));
    assert!(clocks.unclocked().next().is_none());
    // A replaced target loses the old lifetime's source.
    let third = unclocked(clocks.observe_unsupported(key(2, 2), unsupported(EOPNOTSUPP), PERIOD));
    let replaced = clocks.observe_unsupported(key(2, 3), unsupported(ENOTTY), PERIOD);
    assert_eq!(replaced.lost, Some(third.source));
    assert_eq!(unclocked(replaced).reason, unsupported(ENOTTY));
    // A replacement owner shares nothing.
    let mut resumed = LiveNativePresentClocks::new(NonZeroU64::new(8).unwrap());
    let fresh = unclocked(resumed.observe_unsupported(key(2, 3), unsupported(ENOTTY), PERIOD));
    assert_eq!(fresh.source.owner, 8);
    assert!(resumed.unclocked_for(key(2, 1)).is_none());
}

#[test]
fn a_head_moving_between_clocked_and_unclocked_reports_the_old_source_lost() {
    let mut clocks = clocks(7);
    let clocked = clocks.observe(key(1, 1), sample(10)).current.unwrap();
    let observation = clocks.observe_unsupported(key(1, 1), unsupported(EOPNOTSUPP), PERIOD);
    assert_eq!(observation.lost, Some(clocked.source));
    let without = unclocked(observation);
    assert!(clocks.sources().next().is_none());
    let observation = clocks.observe(key(1, 1), sample(20));
    assert_eq!(observation.lost, Some(without.source));
    assert_eq!(observation.status, LiveNativePresentClockStatus::Restarted);
    let regained = observation.current.unwrap();
    assert_ne!(regained.source, clocked.source);
    assert_ne!(regained.source, without.source);
    assert!(clocks.unclocked().next().is_none());
}

#[test]
fn a_clocked_mirror_sibling_wins_over_an_unclocked_primary() {
    let mut clocks = clocks(7);
    let primary = RenderHeadId::from_raw(2);
    let sibling = RenderHeadId::from_raw(1);
    let chosen = query_live_present_clock_candidates(
        [(sibling, true), (primary, true)],
        Some(primary),
        |head| {
            if head == primary {
                clocks.observe_unsupported(key(2, 1), unsupported(EOPNOTSUPP), PERIOD)
            } else {
                clocks.observe(key(1, 1), sample(50))
            }
        },
    );
    assert_eq!(chosen.len(), 2, "querying went past the unclocked primary");
    assert!(matches!(
        chosen[0].1.status,
        LiveNativePresentClockStatus::Unclocked(_)
    ));
    assert_eq!(chosen[1].1.current.unwrap().msc, 50);
    // Without any clock, every member is answered and the primary leads.
    let mut clocks = self::clocks(9);
    let chosen = query_live_present_clock_candidates(
        [(sibling, true), (primary, true)],
        Some(primary),
        |head| clocks.observe_unsupported(key(head.raw(), 1), unsupported(EOPNOTSUPP), PERIOD),
    );
    assert_eq!(
        chosen.iter().map(|(head, _)| *head).collect::<Vec<_>>(),
        [primary, sibling]
    );
    assert!(chosen.iter().all(|(_, observation)| matches!(
        observation.status,
        LiveNativePresentClockStatus::Unclocked(_)
    )));
}

#[test]
fn unclocked_heads_share_the_owner_capacity_and_reject_invalid_targets() {
    let mut clocks = clocks(7);
    let capacity =
        (sophia_engine::MAX_DRM_KMS_OUTPUTS * sophia_engine::MAX_HEADS_PER_OUTPUT) as u64;
    for head in 1..=capacity {
        let observation = if head % 2 == 0 {
            clocks.observe_unsupported(key(head, 1), unsupported(EOPNOTSUPP), PERIOD)
        } else {
            clocks.observe(key(head, 1), sample(head))
        };
        assert_ne!(observation.status, LiveNativePresentClockStatus::Capacity);
    }
    let full = clocks.observe_unsupported(key(capacity + 1, 1), unsupported(EOPNOTSUPP), PERIOD);
    assert_eq!(full.status, LiveNativePresentClockStatus::Capacity);
    assert_eq!(
        clocks.observe(key(capacity + 2, 1), sample(1)).status,
        LiveNativePresentClockStatus::Capacity
    );
    for invalid in [
        key(1, 0),
        LiveNativePresentClockKey {
            crtc_id: 0,
            ..key(3, 1)
        },
    ] {
        assert_eq!(
            clocks
                .observe_unsupported(invalid, unsupported(EOPNOTSUPP), PERIOD)
                .status,
            LiveNativePresentClockStatus::InvalidTarget
        );
    }
}

/// A card without monotonic timestamps: the adapter answers `UnsupportedClock`
/// through `lose_head`, from its per-card capability cache. That mints no
/// source, clocked or unclocked, and retires whatever the head had.
#[test]
fn an_unsupported_clock_mints_nothing() {
    let mut clocks = clocks(7);
    let had = unclocked(clocks.observe_unsupported(key(1, 1), unsupported(EOPNOTSUPP), PERIOD));
    let observation = clocks.lose_head(
        RenderHeadId::from_raw(1),
        LiveNativePresentClockStatus::UnsupportedClock,
    );
    assert_eq!(
        observation.status,
        LiveNativePresentClockStatus::UnsupportedClock
    );
    assert_eq!(observation.current, None);
    assert_eq!(observation.lost, Some(had.source));
    let again = clocks.lose_head(
        RenderHeadId::from_raw(1),
        LiveNativePresentClockStatus::UnsupportedClock,
    );
    assert_eq!(again.lost, None);
    assert!(clocks.unclocked().next().is_none());
    assert!(clocks.sources().next().is_none());
    // Nothing was minted: the next source continues the sequence directly.
    let next = unclocked(clocks.observe_unsupported(key(2, 1), unsupported(ENOTTY), PERIOD));
    assert_eq!(next.source.incarnation, had.source.incarnation + 1);
}

/// The adapter reads DRM_CAP_TIMESTAMP_MONOTONIC through this cache. A card
/// without it answers `UnsupportedClock` on every later query without asking
/// the kernel again; a failed read is asked again.
#[test]
fn a_card_without_monotonic_timestamps_is_asked_once() {
    let mut cache = std::collections::BTreeMap::new();
    let mut reads = 0;
    for _ in 0..3 {
        let supported = cached_monotonic_capability(&mut cache, 0, || {
            reads += 1;
            Ok(0)
        });
        assert_eq!(supported, Some(false));
    }
    assert_eq!(reads, 1, "capability 0 is cached for the card fd");
    // Another card is its own lifetime.
    assert_eq!(
        cached_monotonic_capability(&mut cache, 1, || Ok(1)),
        Some(true)
    );
    assert_eq!(
        cached_monotonic_capability(&mut cache, 1, || panic!("cached")),
        Some(true)
    );
    // A failed read decides nothing and is asked again.
    let mut failures = 0;
    for _ in 0..2 {
        let answer = cached_monotonic_capability(&mut cache, 2, || {
            failures += 1;
            Err(std::io::Error::from_raw_os_error(13))
        });
        assert_eq!(answer, None);
    }
    assert_eq!(failures, 2);
    assert!(!cache.contains_key(&2));
}
