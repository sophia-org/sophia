use super::*;
use sophia_backend_live::{LiveNativeUnclockedPresentClock, LiveNativeUnclockedReason};

fn clock() -> LiveNativeUnclockedPresentClock {
    LiveNativeUnclockedPresentClock {
        source: LiveNativePresentClockSource {
            owner: 8,
            incarnation: 3,
        },
        minimum_period_usec: 16_667,
        reason: LiveNativeUnclockedReason::SequenceUnsupported { errno: 95 },
    }
}

fn unsupported() -> LiveNativePresentClockObservation {
    LiveNativePresentClockObservation {
        lost: None,
        current: None,
        status: LiveNativePresentClockStatus::Unclocked(clock()),
    }
}

#[test]
fn visible_unsupported_head_binds_unclocked_while_hidden_still_binds_fake() {
    for visible in [true, false] {
        let frontend = AdmissionFrontend::new(true);
        frontend.prepare(3, 1_000_000);
        let mut bridge = SessionPresentClocks::default();
        bridge
            .admit_with(
                &frontend,
                100_000,
                |_| BTreeMap::from([(SURFACE, visible.then_some(OutputId::from_raw(1)))]),
                |_| {
                    assert!(visible, "a hidden request must not query hardware");
                    vec![(RenderHeadId::from_raw(1), unsupported())]
                },
            )
            .unwrap();
        let sample = frontend.bound.borrow()[0].1;
        assert_eq!(
            sample.source,
            if visible {
                unclocked_source(clock())
            } else {
                XPresentClockSource::Fake
            }
        );
        assert_eq!(sample.msc, 0);
        assert!(frontend.admissions().unwrap().is_empty());
    }
}

#[test]
fn a_clocked_mirror_member_wins_over_an_unsupported_primary() {
    let frontend = AdmissionFrontend::new(true);
    frontend.prepare(3, 30);
    let supported = sophia_backend_live::LiveNativePresentClockSample {
        source: LiveNativePresentClockSource {
            owner: 8,
            incarnation: 4,
        },
        ust_usec: 100_000,
        msc: 30,
    };
    SessionPresentClocks::default()
        .admit_with(
            &frontend,
            100_000,
            |_| BTreeMap::from([(SURFACE, Some(OutputId::from_raw(1)))]),
            |_| {
                vec![
                    (RenderHeadId::from_raw(1), unsupported()),
                    (
                        RenderHeadId::from_raw(2),
                        LiveNativePresentClockObservation {
                            lost: None,
                            current: Some(supported),
                            status: LiveNativePresentClockStatus::Observed,
                        },
                    ),
                ]
            },
        )
        .unwrap();
    assert_eq!(
        frontend.bound.borrow()[0].1.source,
        source(supported.source)
    );
    assert_eq!(frontend.bound.borrow()[0].1.msc, 30);
}

#[test]
fn inactive_or_failed_queries_never_claim_an_active_unclocked_head() {
    for status in [
        LiveNativePresentClockStatus::Inactive,
        LiveNativePresentClockStatus::UnsupportedClock,
        LiveNativePresentClockStatus::QueryFailed,
    ] {
        let frontend = AdmissionFrontend::new(true);
        frontend.prepare(3, 0);
        SessionPresentClocks::default()
            .admit_with(
                &frontend,
                100_000,
                |_| BTreeMap::from([(SURFACE, Some(OutputId::from_raw(1)))]),
                |_| {
                    vec![(
                        RenderHeadId::from_raw(1),
                        LiveNativePresentClockObservation {
                            lost: None,
                            current: None,
                            status,
                        },
                    )]
                },
            )
            .unwrap();
        assert_eq!(
            frontend.bound.borrow()[0].1.source,
            XPresentClockSource::Fake
        );
    }
}
