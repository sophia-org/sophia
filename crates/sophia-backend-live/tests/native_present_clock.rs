#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use sophia_backend_live::*;
use sophia_engine::RenderHeadId;
use std::num::NonZeroU64;

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
fn clocks() -> LiveNativePresentClocks {
    LiveNativePresentClocks::new(NonZeroU64::new(7).unwrap())
}

#[test]
fn native_present_clocks_keep_real_head_counters_independent_and_never_extrapolate() {
    let mut clocks = clocks();
    let a = clocks.observe(key(1, 1), sample(1 << 33)).current.unwrap();
    let b = clocks.observe(key(2, 1), sample(300)).current.unwrap();
    assert_ne!(a.source, b.source);
    assert_eq!(a.msc, 1 << 33); // no 32-bit page-flip truncation
    assert_eq!(clocks.last_sample(a.source), Some(a));
    for _ in 0..100 {
        let observation = clocks.observe(key(1, 1), sample(1 << 33));
        assert_eq!(observation.current, Some(a));
        assert_eq!(observation.lost, None);
    }
    assert_eq!(clocks.last_sample(b.source), Some(b));
}

#[test]
fn native_present_clock_regression_invalidates_only_that_incarnation() {
    let mut clocks = clocks();
    let first = clocks.observe(key(1, 1), sample(100)).current.unwrap();
    let other = clocks.observe(key(2, 1), sample(200)).current.unwrap();
    let reset = clocks.observe(key(1, 1), sample(1));
    assert_eq!(reset.lost, Some(first.source));
    assert_eq!(reset.status, LiveNativePresentClockStatus::Restarted);
    let new = reset.current.unwrap();
    assert_eq!(new.msc, 1);
    assert_ne!(new.source, first.source);
    assert_eq!(clocks.last_sample(first.source), None);
    assert_eq!(clocks.last_sample(other.source), Some(other));
    // Same-field timestamp jitter is not a counter restart. Keep the prior
    // accepted observation, including its sub-microsecond comparison anchor.
    let stable = clocks
        .observe(
            key(1, 1),
            sophia_drm_clock::CrtcSequence {
                timestamp_nsec: 1_000_999,
                ..sample(1)
            },
        )
        .current
        .unwrap();
    let repeated = clocks.observe(
        key(1, 1),
        sophia_drm_clock::CrtcSequence {
            timestamp_nsec: 1_000_998,
            ..sample(1)
        },
    );
    assert_eq!(repeated.lost, None);
    assert_eq!(repeated.current, Some(stable));
    let reset = clocks.observe(
        key(1, 1),
        sophia_drm_clock::CrtcSequence {
            timestamp_nsec: 1_000_998,
            ..sample(2)
        },
    );
    assert_eq!(reset.lost, Some(stable.source));
    assert_ne!(reset.current.unwrap().source, stable.source);
}

#[test]
fn native_present_clock_modeset_rollback_disable_and_new_owner_never_recycle() {
    let mut clocks = clocks();
    let original = clocks.observe(key(1, 1), sample(100)).current.unwrap();
    assert_eq!(clocks.invalidate(), vec![original.source]);
    // Modeset B failed and restored A before the next query. Same target,
    // sequence and timestamp still name a distinct physical clock lifetime.
    let restored = clocks.observe(key(1, 1), sample(100)).current.unwrap();
    assert_ne!(original.source, restored.source);
    let changed = clocks.observe(key(1, 2), sample(101));
    assert_eq!(changed.lost, Some(restored.source));
    let changed = changed.current.unwrap();
    let disabled = clocks.observe(
        key(1, 2),
        sophia_drm_clock::CrtcSequence {
            active: false,
            ..sample(101)
        },
    );
    assert_eq!(disabled.lost, Some(changed.source));
    assert_eq!(disabled.current, None);
    assert_eq!(
        clocks
            .observe(
                key(1, 2),
                sophia_drm_clock::CrtcSequence {
                    active: false,
                    ..sample(101)
                }
            )
            .lost,
        None
    );
    let resumed = clocks.observe(key(1, 2), sample(101)).current.unwrap();
    assert_ne!(changed.source, resumed.source);
    let mut owner = LiveNativePresentClocks::new(NonZeroU64::new(8).unwrap());
    assert_ne!(
        owner
            .observe(key(1, 2), sample(101))
            .current
            .unwrap()
            .source,
        original.source
    );
}

#[test]
fn native_present_clock_loss_and_capacity_do_not_invent_a_fallback_sample() {
    let mut clocks = clocks();
    for head in 1..=64 {
        assert!(clocks.observe(key(head, 1), sample(head)).current.is_some());
    }
    let full = clocks.observe(key(65, 1), sample(65));
    assert_eq!(full.status, LiveNativePresentClockStatus::Capacity);
    assert_eq!(full.current, None);
    let old = clocks
        .sources()
        .find(|(head, _)| head.raw() == 1)
        .unwrap()
        .1;
    let failed = clocks.lose_head(
        RenderHeadId::from_raw(1),
        LiveNativePresentClockStatus::QueryFailed,
    );
    assert_eq!(failed.lost, Some(old));
    assert_eq!(failed.current, None);
    assert_eq!(clocks.last_sample(old), None);
    assert!(clocks.observe(key(65, 1), sample(65)).current.is_some());
}

#[test]
fn native_present_clock_mirror_fallback_selects_an_active_member_and_one_counter() {
    let mut clocks = clocks();
    let primary = RenderHeadId::from_raw(2);
    let sibling = RenderHeadId::from_raw(1);
    let old = clocks.observe(key(2, 1), sample(100)).current.unwrap();
    let chosen = query_live_present_clock_candidates(
        [(sibling, true), (primary, true)],
        Some(primary),
        |head| {
            if head == primary {
                clocks.observe(
                    key(2, 1),
                    sophia_drm_clock::CrtcSequence {
                        active: false,
                        ..sample(100)
                    },
                )
            } else {
                clocks.observe(key(1, 1), sample(50))
            }
        },
    );
    assert_eq!(chosen.len(), 2);
    assert_eq!(chosen[0].0, primary);
    assert_eq!(chosen[0].1.lost, Some(old.source));
    assert_eq!(chosen[1].0, sibling);
    let selected = chosen[1].1.current.unwrap();
    assert_eq!(selected.msc, 50);
    // The disabled head is never queried. Its preferred status cannot win
    // over a live member, and no further sibling query follows success.
    let chosen = query_live_present_clock_candidates(
        [(primary, false), (sibling, true)],
        Some(primary),
        |head| {
            assert_eq!(head, sibling);
            clocks.observe(key(1, 1), sample(51))
        },
    );
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].1.current.unwrap().source, selected.source);
    // Independent refresh/counter progress never advances the chosen clock.
    clocks.observe(key(2, 1), sample(300));
    assert_eq!(clocks.last_sample(selected.source).unwrap().msc, 51);
    assert!(
        query_live_present_clock_candidates(
            [(primary, false), (sibling, false)],
            Some(primary),
            |_| panic!("no clock query without an active candidate")
        )
        .is_empty()
    );
}

#[test]
fn real_flip_samples_advance_only_an_anchored_clock_and_preserve_wrap() {
    let mut clocks = clocks();
    let support = LiveNativePresentClockEventSupport {
        monotonic: true,
        crtc_id: true,
    };
    assert_eq!(clocks.observe_page_flip(key(1, 1), 0, 1_000, support), None);
    let anchor = clocks
        .observe(key(1, 1), sample((1 << 33) - 1))
        .current
        .unwrap();
    let wrapped = clocks
        .observe_page_flip(key(1, 1), 0, anchor.ust_usec + 1_000, support)
        .unwrap();
    assert_eq!(wrapped.source, anchor.source);
    assert_eq!(wrapped.msc, 1 << 33);
    assert_eq!(clocks.last_sample(anchor.source), Some(wrapped));
    // Neither an old event already superseded by GET_SEQUENCE nor an event
    // from a different target/head may rewind or restart this source.
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), u32::MAX, anchor.ust_usec, support),
        None
    );
    assert_eq!(
        clocks.observe_page_flip(key(1, 2), 1, wrapped.ust_usec + 1_000, support),
        None
    );
    assert_eq!(
        clocks.observe_page_flip(key(2, 1), 1, wrapped.ust_usec + 1_000, support),
        None
    );
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), 1 << 31, wrapped.ust_usec + 1_000, support),
        None
    );
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), 1, u64::MAX, support),
        None
    );
    assert_eq!(clocks.last_sample(anchor.source), Some(wrapped));
    let queried = clocks
        .observe(key(1, 1), sample((1 << 33) + 5))
        .current
        .unwrap();
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), 3, queried.ust_usec - 1_000, support),
        None
    );
    assert_eq!(clocks.last_sample(anchor.source), Some(queried));
    clocks.invalidate();
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), 6, queried.ust_usec + 1_000, support),
        None
    );
    let restored = clocks
        .observe(
            key(1, 1),
            sophia_drm_clock::CrtcSequence {
                active: true,
                sequence: 10,
                timestamp_nsec: (queried.ust_usec + 2_000) * 1_000,
            },
        )
        .current
        .unwrap();
    assert_ne!(restored.source, queried.source);
    // A late event from before invalidation cannot enter the replacement
    // anchor, even when rollback restored the same head/CRTC/target key.
    assert_eq!(
        clocks.observe_page_flip(key(1, 1), 6, queried.ust_usec + 1_000, support),
        None
    );
    assert_eq!(clocks.last_sample(restored.source), Some(restored));
}

#[test]
fn flip_observations_require_both_card_capabilities_and_match_each_crtc() {
    let mut clocks = clocks();
    let a = clocks.observe(key(1, 1), sample(100)).current.unwrap();
    let b = clocks.observe(key(2, 1), sample(1_000)).current.unwrap();
    for (monotonic, crtc_id) in [(false, false), (false, true), (true, false)] {
        assert_eq!(
            clocks.observe_page_flip(
                key(1, 1),
                101,
                101_000,
                LiveNativePresentClockEventSupport { monotonic, crtc_id }
            ),
            None
        );
        assert_eq!(clocks.last_sample(a.source), Some(a));
        assert_eq!(clocks.last_sample(b.source), Some(b));
    }
    let support = LiveNativePresentClockEventSupport {
        monotonic: true,
        crtc_id: true,
    };
    // Even a matching head id cannot accept a counter from another card/CRTC.
    assert_eq!(
        clocks.observe_page_flip(
            LiveNativePresentClockKey {
                crtc_id: 2,
                ..key(1, 1)
            },
            101,
            101_000,
            support
        ),
        None
    );
    assert_eq!(
        clocks.observe_page_flip(
            LiveNativePresentClockKey {
                card_group: 1,
                ..key(1, 1)
            },
            101,
            101_000,
            support
        ),
        None
    );
    clocks
        .observe_page_flip(key(2, 1), 1_001, 1_001_000, support)
        .unwrap();
    assert_eq!(clocks.last_sample(a.source), Some(a));
    assert_eq!(clocks.last_sample(b.source).unwrap().msc, 1_001);
    // Lack of event attribution never disables the separate real-query path.
    assert_eq!(
        clocks.observe(key(1, 1), sample(101)).current.unwrap().msc,
        101
    );
}

#[test]
fn retirement_keeps_its_old_incarnation_after_clock_invalidation() {
    let mut clocks = clocks();
    let old = clocks.observe(key(1, 1), sample(20)).current.unwrap();
    let evidence = LiveNativeRetirementClocks::from_samples([old]);
    clocks.invalidate();
    let new = clocks.observe(key(1, 2), sample(30)).current.unwrap();
    assert_ne!(old.source, new.source);
    assert_eq!(evidence.sample(new.source), None);
    assert_eq!(evidence.sample(old.source), Some(old));
    assert!(LiveNativeRetirementClocks::default().evidence().is_empty());
}

#[test]
fn old_submission_event_cannot_advance_a_new_incarnation_of_the_same_head() {
    let mut clocks = clocks();
    let old = clocks.observe(key(1, 1), sample(20)).current.unwrap();
    let support = LiveNativePresentClockEventSupport {
        monotonic: true,
        crtc_id: true,
    };
    let event = clocks
        .observe_submitted_page_flip(key(1, 1), Some(old.source), 21, 21_000, support)
        .unwrap();
    let evidence = LiveNativeRetirementClocks::from_evidence([event]);
    clocks.invalidate();
    let new = clocks.observe(key(1, 1), sample(30)).current.unwrap();
    assert_ne!(old.source, new.source);
    assert_eq!(
        clocks.observe_submitted_page_flip(key(1, 1), Some(old.source), 31, 31_000, support),
        None
    );
    assert_eq!(clocks.last_sample(new.source), Some(new));
    assert_eq!(evidence.sample(old.source), Some(event.sample));
    assert_eq!(evidence.sample(new.source), None);
    assert_eq!(
        clocks.observe_submitted_page_flip(key(1, 1), None, 31, 31_000, support),
        None
    );
}

#[test]
fn historical_decode_crosses_wrap_without_advancing_the_query_anchor() {
    let mut clocks = clocks();
    let anchor = clocks
        .observe(key(1, 1), sample((1 << 33) + 2))
        .current
        .unwrap();
    let support = LiveNativePresentClockEventSupport {
        monotonic: true,
        crtc_id: true,
    };
    let event = clocks
        .observe_submitted_page_flip(
            key(1, 1),
            Some(anchor.source),
            u32::MAX,
            anchor.ust_usec - 3_000,
            support,
        )
        .unwrap();
    assert!(event.historical);
    assert_eq!(event.sample.msc, (1 << 33) - 1);
    assert_eq!(event.sample.ust_usec, anchor.ust_usec - 3_000);
    assert_eq!(event.sample.source, anchor.source);
    assert_eq!(clocks.last_sample(anchor.source), Some(anchor));
    // Repeated decode is inert, including for a same-field earlier UST.
    let repeated = clocks
        .observe_submitted_page_flip(
            key(1, 1),
            Some(anchor.source),
            2,
            anchor.ust_usec - 1,
            support,
        )
        .unwrap();
    assert!(repeated.historical);
    assert_eq!(repeated.sample.msc, anchor.msc);
    assert_eq!(repeated.sample.ust_usec, anchor.ust_usec - 1);
    assert_eq!(clocks.last_sample(anchor.source), Some(anchor));
    let evidence = LiveNativeRetirementClocks::from_evidence([event, repeated]);
    assert_eq!(evidence.clone().evidence(), &[event, repeated]);
    let forward = clocks
        .observe_submitted_page_flip(
            key(1, 1),
            Some(anchor.source),
            3,
            anchor.ust_usec + 1_000,
            support,
        )
        .unwrap();
    assert!(!forward.historical);
    assert_eq!(clocks.last_sample(anchor.source), Some(forward.sample));
}

#[test]
fn submitted_decode_rejects_direction_ambiguity_and_changed_incarnations() {
    let mut clocks = clocks();
    let anchor = clocks
        .observe(key(1, 1), sample((1 << 33) + 2))
        .current
        .unwrap();
    let support = LiveNativePresentClockEventSupport {
        monotonic: true,
        crtc_id: true,
    };
    for (sequence, ust) in [
        (1, anchor.ust_usec + 1),
        (1, anchor.ust_usec),
        (3, anchor.ust_usec - 1),
        (3, anchor.ust_usec),
        (2 + (1 << 31), anchor.ust_usec + 1),
        (2 + (1 << 31), anchor.ust_usec - 1),
    ] {
        assert_eq!(
            clocks.observe_submitted_page_flip(
                key(1, 1),
                Some(anchor.source),
                sequence,
                ust,
                support
            ),
            None
        );
        assert_eq!(clocks.last_sample(anchor.source), Some(anchor));
    }
    // Same sequence is not a direction mismatch: the event keeps its own UST.
    let equal = clocks
        .decode_submitted_page_flip(
            key(1, 1),
            Some(anchor.source),
            2,
            anchor.ust_usec + 1,
            support,
        )
        .unwrap();
    assert!(!equal.historical);
    assert_eq!(equal.sample.ust_usec, anchor.ust_usec + 1);
    assert_eq!(clocks.last_sample(anchor.source), Some(anchor));
    clocks.invalidate();
    let replacement = clocks
        .observe(key(1, 1), sample(anchor.msc + 10))
        .current
        .unwrap();
    assert_eq!(
        clocks.decode_submitted_page_flip(
            key(1, 1),
            Some(anchor.source),
            2,
            anchor.ust_usec,
            support
        ),
        None
    );
    assert_eq!(clocks.last_sample(replacement.source), Some(replacement));
    // Nearest extension cannot underflow the full-width counter.
    let mut fresh = LiveNativePresentClocks::new(NonZeroU64::new(99).unwrap());
    let zero = fresh
        .observe(
            key(1, 1),
            sophia_drm_clock::CrtcSequence {
                timestamp_nsec: 1_000_000,
                ..sample(0)
            },
        )
        .current
        .unwrap();
    assert_eq!(
        fresh.decode_submitted_page_flip(key(1, 1), Some(zero.source), u32::MAX, 999, support),
        None
    );
}
