use sophia_protocol::TransactionId;
use sophia_x_authority::*;
use std::num::NonZeroUsize;

fn hardware(msc: u64) -> XPresentClockSample {
    XPresentClockSample {
        source: XPresentClockSource::Hardware {
            domain: 1,
            incarnation: 1,
        },
        ust: msc * 1000,
        msc,
    }
}

fn request(
    clock: &XPresentWindowClock,
    id: u64,
    target: u64,
    kind: XPresentScheduledKind,
) -> XPresentScheduledRequest {
    XPresentScheduledRequest {
        request: TransactionId::from_raw(id),
        kind,
        binding: clock.binding().unwrap(),
        replaces_contents: kind == XPresentScheduledKind::Pixmap,
        target: clock
            .target(XPresentMscTiming::new(target, 0, 0, false).unwrap())
            .unwrap(),
    }
}

#[test]
fn earlier_targets_pass_a_later_owner_and_only_equal_pixmaps_are_superseded() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
    let later = request(&clock, 1, 30, XPresentScheduledKind::Pixmap);
    let earlier = request(&clock, 2, 20, XPresentScheduledKind::Pixmap);
    let notify = request(&clock, 3, 20, XPresentScheduledKind::NotifyMsc);
    for item in [later, earlier, notify] {
        assert_eq!(queue.insert(item), Ok(vec![]));
    }
    let replacement = request(&clock, 4, 20, XPresentScheduledKind::Pixmap);
    assert_eq!(queue.insert(replacement), Ok(vec![earlier]));
    assert_eq!(queue.len(), 4);
    assert!(queue.ready().next().is_none());
    clock.observe(hardware(19), None).unwrap();
    queue.observe(hardware(19)).unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![replacement]);
    // Merely considering a request must not lose it when an acquire fence or
    // downstream queue is not ready. Execution takes it only after admission.
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![replacement]);
    assert_eq!(queue.take(replacement.request), Some(replacement));
    clock.observe(hardware(20), None).unwrap();
    queue.observe(hardware(20)).unwrap();
    let skipped = XPresentScheduledRequest {
        kind: XPresentScheduledKind::Skip,
        ..earlier
    };
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![skipped, notify]);
    assert_eq!(queue.take(skipped.request), Some(skipped));
    assert_eq!(queue.take(notify.request), Some(notify));
    clock.observe(hardware(29), None).unwrap();
    queue.observe(hardware(29)).unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![later]);
    assert_eq!(queue.take(later.request), Some(later));
    assert!(queue.is_empty());
}

#[test]
fn capacity_and_duplicate_refusals_preserve_all_obligations() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(2).unwrap());
    let a = request(&clock, 1, 20, XPresentScheduledKind::Pixmap);
    let b = request(&clock, 2, 30, XPresentScheduledKind::Pixmap);
    queue.insert(a).unwrap();
    queue.insert(b).unwrap();
    assert_eq!(
        queue.insert(request(&clock, 3, 40, XPresentScheduledKind::Pixmap)),
        Err(XPresentScheduleError::Capacity)
    );
    assert_eq!(
        queue.insert(request(&clock, 2, 20, XPresentScheduledKind::Pixmap)),
        Err(XPresentScheduleError::DuplicateRequest)
    );
    assert_eq!(
        queue.insert(request(&clock, 0, 20, XPresentScheduledKind::Pixmap)),
        Err(XPresentScheduleError::InvalidRequest)
    );
    assert_eq!(queue.cancel().collect::<Vec<_>>(), vec![a, b]);
    assert!(queue.is_empty());
}

#[test]
fn partial_updates_accumulate_and_full_replacement_keeps_every_completion() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(3).unwrap());
    let partials = [1, 2].map(|id| XPresentScheduledRequest {
        replaces_contents: false,
        ..request(&clock, id, 20, XPresentScheduledKind::Pixmap)
    });
    for partial in partials {
        assert!(queue.insert(partial).unwrap().is_empty());
    }
    clock.observe(hardware(19), None).unwrap();
    queue.observe(hardware(19)).unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), partials);
    let full = request(&clock, 3, 20, XPresentScheduledKind::Pixmap);
    assert_eq!(queue.insert(full).unwrap(), partials);
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![full]);
    assert_eq!(
        queue.insert(request(&clock, 4, 20, XPresentScheduledKind::Pixmap)),
        Err(XPresentScheduleError::Capacity)
    );
    clock.observe(hardware(20), None).unwrap();
    queue.observe(hardware(20)).unwrap();
    let mut expected = partials
        .map(|p| XPresentScheduledRequest {
            kind: XPresentScheduledKind::Skip,
            ..p
        })
        .to_vec();
    expected.push(full);
    assert_eq!(queue.ready().collect::<Vec<_>>(), expected);
}

#[test]
fn equal_window_targets_on_different_clock_bindings_do_not_scrap_each_other() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
    let original = request(&clock, 1, 20, XPresentScheduledKind::Pixmap);
    queue.insert(original).unwrap();
    clock
        .observe(
            XPresentClockSample {
                source: XPresentClockSource::Hardware {
                    domain: 2,
                    incarnation: 1,
                },
                ust: 11_000,
                msc: 900,
            },
            None,
        )
        .unwrap();
    let other_head = request(&clock, 2, 20, XPresentScheduledKind::Pixmap);
    assert_eq!(other_head.target.window_msc, original.target.window_msc);
    assert!(queue.insert(other_head).unwrap().is_empty());
    clock
        .observe(
            XPresentClockSample {
                source: XPresentClockSource::Hardware {
                    domain: 1,
                    incarnation: 2,
                },
                ust: 12_000,
                msc: 1,
            },
            None,
        )
        .unwrap();
    let reset = request(&clock, 3, 20, XPresentScheduledKind::Pixmap);
    assert!(queue.insert(reset).unwrap().is_empty());
    // Even returning to the same source under a changed counter offset must
    // not turn an old physical target into the new request's target.
    clock.observe(hardware(13), None).unwrap();
    let rebound = request(&clock, 4, 20, XPresentScheduledKind::Pixmap);
    assert_eq!(rebound.binding.source, original.binding.source);
    assert_ne!(rebound.binding, original.binding);
    assert!(queue.insert(rebound).unwrap().is_empty());
    assert_eq!(
        queue.cancel().collect::<Vec<_>>(),
        vec![original, other_head, reset, rebound]
    );
}

#[test]
fn hidden_requests_do_not_execute_a_fake_field_early_and_survive_a_source_change() {
    let mut clock = XPresentWindowClock::default();
    clock
        .observe(XPresentClockSample::background(10_500_000), None)
        .unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(2).unwrap());
    let pixmap = request(&clock, 1, 11, XPresentScheduledKind::Pixmap);
    let notify = request(&clock, 2, 11, XPresentScheduledKind::NotifyMsc);
    queue.insert(pixmap).unwrap();
    queue.insert(notify).unwrap();
    assert!(queue.ready().next().is_none());
    clock
        .observe(XPresentClockSample::background(10_999_999), None)
        .unwrap();
    queue
        .observe(XPresentClockSample::background(10_999_999))
        .unwrap();
    assert!(queue.ready().next().is_none());
    clock
        .observe(XPresentClockSample::background(11_000_000), None)
        .unwrap();
    queue
        .observe(XPresentClockSample::background(11_000_000))
        .unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![pixmap, notify]);
    let new_head = XPresentClockSample {
        source: XPresentClockSource::Hardware {
            domain: 2,
            incarnation: 1,
        },
        ust: 11_100_000,
        msc: 800,
    };
    clock.observe(new_head, None).unwrap();
    queue.observe(new_head).unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![pixmap, notify]);
}

#[test]
fn queued_requests_keep_their_source_even_when_new_requests_choose_another() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
    let old = request(&clock, 1, 20, XPresentScheduledKind::Pixmap);
    queue.insert(old).unwrap();
    let replacement = request(&clock, 2, 20, XPresentScheduledKind::Pixmap);
    queue.insert(replacement).unwrap();
    queue.take(replacement.request);
    let fake = XPresentClockSample::background(15_500_000);
    clock.observe(fake, None).unwrap();
    let background = request(&clock, 3, 11, XPresentScheduledKind::Pixmap);
    queue.insert(background).unwrap();
    clock
        .observe(
            XPresentClockSample {
                source: XPresentClockSource::Hardware {
                    domain: 2,
                    incarnation: 1,
                },
                ust: 15_510_000,
                msc: 800,
            },
            None,
        )
        .unwrap();
    // Moving the window and advancing another head does not execute either
    // old obligation. The fake one still arms its original one-second field.
    queue.observe(clock.sample().unwrap()).unwrap();
    assert!(queue.ready().next().is_none());
    assert_eq!(queue.background_deadline(), Some(16_000_000));
    queue
        .observe(XPresentClockSample::background(16_000_000))
        .unwrap();
    assert_eq!(queue.ready().collect::<Vec<_>>(), vec![background]);
    queue.take(background.request);
    queue.observe(hardware(19)).unwrap();
    assert!(queue.ready().next().is_none());
    assert_eq!(queue.completion_sample(old.request), None);
    queue.observe(hardware(20)).unwrap();
    assert_eq!(queue.completion_sample(old.request), Some((20_000, 20)));
    assert_eq!(
        queue.ready().next().unwrap().kind,
        XPresentScheduledKind::Skip
    );
}

#[test]
fn a_lost_source_settles_once_from_its_last_observation_without_migration() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(3).unwrap());
    let pixmap = request(&clock, 1, 20, XPresentScheduledKind::Pixmap);
    let notify = request(&clock, 2, 30, XPresentScheduledKind::NotifyMsc);
    queue.insert(pixmap).unwrap();
    queue.insert(notify).unwrap();
    queue.observe(hardware(12)).unwrap();
    assert_eq!(queue.lose_source(hardware(12).source), vec![pixmap]);
    assert!(queue.lose_source(hardware(12).source).is_empty());
    assert!(queue.clock_sources().next().is_none());
    queue.observe(hardware(40)).unwrap();
    assert_eq!(queue.completion_sample(pixmap.request), Some((12_000, 12)));
    assert_eq!(queue.completion_sample(notify.request), Some((12_000, 12)));
    assert_eq!(
        queue.ready().map(|p| p.kind).collect::<Vec<_>>(),
        vec![
            XPresentScheduledKind::Skip,
            XPresentScheduledKind::NotifyMsc
        ]
    );
}

#[test]
fn ready_but_blocked_requests_keep_loss_bindings_without_query_demand() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(10), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(2).unwrap());
    let pixmap = request(&clock, 1, 20, XPresentScheduledKind::Pixmap);
    let notify = request(&clock, 2, 20, XPresentScheduledKind::NotifyMsc);
    queue.insert(pixmap).unwrap();
    queue.insert(notify).unwrap();
    assert_eq!(
        queue.clock_demands().collect::<Vec<_>>(),
        vec![(hardware(10).source, 9), (hardware(10).source, 10)]
    );
    queue.observe(hardware(19)).unwrap();
    assert_eq!(
        queue.clock_demands().collect::<Vec<_>>(),
        vec![(hardware(10).source, 1)]
    );
    queue.observe(hardware(20)).unwrap();
    assert!(queue.clock_demands().next().is_none());
    assert_eq!(queue.clock_sources().count(), 2);
    assert_eq!(queue.lose_source(hardware(20).source), vec![pixmap]);
    assert!(queue.lose_source(hardware(20).source).is_empty());
    assert_eq!(
        queue.ready().map(|p| p.kind).collect::<Vec<_>>(),
        vec![
            XPresentScheduledKind::Skip,
            XPresentScheduledKind::NotifyMsc
        ]
    );
}
