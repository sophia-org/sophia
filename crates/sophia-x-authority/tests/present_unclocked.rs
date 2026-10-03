use sophia_protocol::TransactionId;
use sophia_x_authority::*;
use std::num::NonZeroUsize;

fn unclocked(ust: u64) -> XPresentClockSample {
    XPresentClockSample {
        source: XPresentClockSource::Unclocked {
            domain: 1,
            incarnation: 2,
            minimum_period_usec: 16_667,
        },
        ust,
        msc: 0,
    }
}

fn hardware(ust: u64, msc: u64) -> XPresentClockSample {
    XPresentClockSample {
        source: XPresentClockSource::Hardware {
            domain: 1,
            incarnation: 1,
        },
        ust,
        msc,
    }
}

#[test]
fn unclocked_pixmaps_ignore_future_targets_without_advancing_the_counter() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(hardware(900, 500), None).unwrap();
    assert_eq!(clock.observe(unclocked(1_000), None), Ok((1_000, 500)));
    for (i, target) in [0, 501, 9_000_000].into_iter().enumerate() {
        let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
        let request = XPresentScheduledRequest {
            request: TransactionId::from_raw(i as u64 + 1),
            kind: XPresentScheduledKind::Pixmap,
            target: clock
                .target(XPresentMscTiming::new(target, 0, 0, false).unwrap())
                .unwrap(),
            binding: clock.binding().unwrap(),
            replaces_contents: true,
        };
        queue.insert(request).unwrap();
        assert_eq!(queue.ready().collect::<Vec<_>>(), vec![request]);
        assert_eq!(queue.clock_demands().count(), 0);
        assert_eq!(queue.completion_sample(request.request), Some((1_000, 500)));
    }
}

#[test]
fn unclocked_notify_has_one_mode_field_deadline_and_never_invents_an_msc() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(unclocked(100), None).unwrap();
    let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
    let request = XPresentScheduledRequest {
        request: TransactionId::from_raw(1),
        kind: XPresentScheduledKind::NotifyMsc,
        target: clock
            .target(XPresentMscTiming::notify(9000, 13, 12).unwrap())
            .unwrap(),
        binding: clock.binding().unwrap(),
        replaces_contents: false,
    };
    queue.insert(request).unwrap();
    assert_eq!(queue.background_deadline(), Some(16_767));
    assert_eq!(queue.ready().count(), 0);
    queue.observe(unclocked(16_766)).unwrap();
    assert_eq!(queue.completion_sample(request.request), None);
    queue.observe(unclocked(16_767)).unwrap();
    assert_eq!(queue.ready().count(), 1);
    assert_eq!(queue.completion_sample(request.request), Some((16_767, 0)));
    queue.take(request.request).unwrap();
    assert_eq!(queue.background_deadline(), None);
}

#[test]
fn clocked_unclocked_hidden_and_back_switches_keep_the_window_continuous() {
    let mut clock = XPresentWindowClock::default();
    assert_eq!(clock.observe(hardware(10, 700), None), Ok((10, 700)));
    assert_eq!(clock.observe(unclocked(20), None), Ok((20, 700)));
    assert_eq!(clock.observe(unclocked(30), None), Ok((30, 700)));
    assert_eq!(
        clock.observe(XPresentClockSample::background(1_000_000), None),
        Ok((1_000_000, 700))
    );
    assert_eq!(
        clock.observe(XPresentClockSample::background(2_000_000), None),
        Ok((2_000_000, 701))
    );
    assert_eq!(
        clock.observe(unclocked(2_000_010), None),
        Ok((2_000_010, 701))
    );
    assert_eq!(
        clock.observe(hardware(2_000_020, 5000), None),
        Ok((2_000_020, 701))
    );
    assert_eq!(
        clock.observe(hardware(2_000_030, 5001), None),
        Ok((2_000_030, 702))
    );
}
