use sophia_x_authority::*;

fn head(domain: u64, incarnation: u64, ust: u64, msc: u64) -> XPresentClockSample {
    XPresentClockSample {
        source: XPresentClockSource::Hardware {
            domain,
            incarnation,
        },
        ust,
        msc,
    }
}

#[test]
fn future_targets_win_and_notify_modulus_selects_a_later_matching_field() {
    assert_eq!(
        XPresentMscTiming::new(11, 4, 0, false)
            .unwrap()
            .fields_after(10),
        1
    );
    assert_eq!(
        XPresentMscTiming::new(0, 0, 0, false)
            .unwrap()
            .fields_after(10),
        1
    );
    assert_eq!(
        XPresentMscTiming::new(0, 0, 0, true)
            .unwrap()
            .fields_after(10),
        0
    );
    assert_eq!(
        XPresentMscTiming::notify(0, 0, 0).unwrap().fields_after(10),
        0
    );
    assert_eq!(
        XPresentMscTiming::notify(0, 5, 0).unwrap().fields_after(10),
        5
    );
    assert_eq!(
        XPresentMscTiming::notify(11, 5, 0)
            .unwrap()
            .fields_after(10),
        1
    );
    for (divisor, remainder) in [(0, 1), (4, 4), (4, 5), (u64::MAX, u64::MAX)] {
        assert_eq!(
            XPresentMscTiming::notify(0, divisor, remainder),
            Err(XPresentTimingError::InvalidRemainder)
        );
    }
}

#[test]
fn modulo_matches_a_field_by_field_oracle_including_counter_wrap() {
    for current in (0..128).chain(u64::MAX - 32..=u64::MAX) {
        for divisor in 1..18 {
            for remainder in 0..divisor {
                for asynchronous in [false, true] {
                    let timing =
                        XPresentMscTiming::new(current, divisor, remainder, asynchronous).unwrap();
                    let first = u64::from(!asynchronous);
                    let expected = (first..=2 * divisor)
                        .find(|distance| current.wrapping_add(*distance) % divisor == remainder)
                        .unwrap();
                    assert_eq!(
                        timing.fields_after(current),
                        expected,
                        "current={current} divisor={divisor} remainder={remainder} async={asynchronous}"
                    );
                }
            }
        }
    }
    assert_eq!(
        XPresentMscTiming::new(1, 0, 0, false)
            .unwrap()
            .fields_after(u64::MAX - 1),
        3
    );
}

#[test]
fn window_counter_is_continuous_across_heads_and_a_return_to_the_first_head() {
    let mut clock = XPresentWindowClock::default();
    assert_eq!(clock.observe(head(1, 1, 1000, 500), None), Ok((1000, 500)));
    assert_eq!(
        clock.observe(head(2, 1, 2000, 40), Some(head(1, 1, 1900, 510))),
        Ok((2000, 510))
    );
    assert_eq!(clock.observe(head(2, 1, 2100, 41), None), Ok((2100, 511)));
    assert_eq!(
        clock.observe(head(1, 1, 3000, 900), Some(head(2, 1, 2900, 48))),
        Ok((3000, 518))
    );
    assert_eq!(clock.observe(head(1, 1, 3100, 901), None), Ok((3100, 519)));
}

#[test]
fn queued_binding_keeps_its_offset_after_the_window_changes_source() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(head(1, 1, 1000, 500), None).unwrap();
    clock.observe(head(2, 1, 2000, 40), None).unwrap();
    let old_request = clock.binding().unwrap();
    let target = clock
        .target(XPresentMscTiming::new(505, 0, 0, false).unwrap())
        .unwrap();
    clock.observe(head(1, 1, 3000, 900), None).unwrap();
    assert!(!clock.target_reached(target));
    // A result from the earlier head uses the binding owned by that request,
    // rather than the offset now installed for the window on head 1.
    assert_eq!(
        old_request.window_sample(head(2, 1, 3100, 45)),
        Ok((3100, 505))
    );
    assert_eq!(
        clock.binding().unwrap().window_sample(head(2, 1, 3100, 45)),
        Err(XPresentTimingError::WrongSource)
    );
    clock.observe(head(1, 1, 3200, 905), None).unwrap();
    assert!(clock.target_reached(target));
}

#[test]
fn fake_clock_advances_unseen_windows_and_preserves_a_target_across_visibility() {
    let mut clock = XPresentWindowClock::default();
    assert_eq!(
        clock.observe(XPresentClockSample::background(50_250_000), None),
        Ok((50_250_000, 50))
    );
    let target = clock
        .target(XPresentMscTiming::notify(51, 0, 0).unwrap())
        .unwrap();
    assert_eq!(clock.background_deadline(target), Some(51_000_000));
    clock
        .observe(XPresentClockSample::background(50_999_999), None)
        .unwrap();
    assert!(!clock.target_reached(target));
    clock
        .observe(XPresentClockSample::background(51_000_000), None)
        .unwrap();
    assert!(clock.target_reached(target));
    let next = clock
        .target(XPresentMscTiming::new(54, 0, 0, false).unwrap())
        .unwrap();
    assert_eq!(
        clock.observe(head(1, 1, 51_100_000, 800), None),
        Ok((51_100_000, 51))
    );
    assert_eq!(clock.background_deadline(next), None);
    clock.observe(head(1, 1, 51_125_000, 803), None).unwrap();
    assert!(clock.target_reached(next));
}

#[test]
fn reset_needs_a_new_incarnation_and_failed_observations_are_atomic() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(head(1, 1, 1000, 500), None).unwrap();
    let before = clock.clone();
    for (new, old, error) in [
        (head(1, 1, 1100, 10), None, XPresentTimingError::StaleSample),
        (
            head(2, 1, 1200, 10),
            Some(head(3, 1, 1100, 510)),
            XPresentTimingError::WrongSource,
        ),
    ] {
        assert_eq!(clock.observe(new, old), Err(error));
        assert_eq!(clock, before);
    }
    assert_eq!(clock.observe(head(1, 2, 1200, 10), None), Ok((1200, 500)));
    assert_eq!(clock.observe(head(1, 2, 1300, 11), None), Ok((1300, 501)));
}

#[test]
fn another_heads_last_vblank_can_be_older_without_regressing_its_clock() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(head(1, 1, 1100, 500), None).unwrap();
    assert_eq!(clock.observe(head(2, 1, 1000, 20), None), Ok((1000, 500)));
    assert_eq!(clock.observe(head(2, 1, 1200, 21), None), Ok((1200, 501)));
    assert_eq!(
        clock.observe(head(2, 1, 900, 22), None),
        Err(XPresentTimingError::StaleSample)
    );
}

#[test]
fn a_large_modulus_cannot_turn_into_an_immediate_or_negative_wait() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(head(1, 1, 1000, 0), None).unwrap();
    let target = clock
        .target(XPresentMscTiming::new(0, u64::MAX, u64::MAX - 1, false).unwrap())
        .unwrap();
    assert_eq!(target.window_msc, u64::MAX - 1);
    assert!(!clock.target_reached(target));
    clock.observe(head(1, 1, 1100, 1), None).unwrap();
    assert!(!clock.target_reached(target));
}

#[test]
fn requests_and_clock_progress_agree_across_card64_wrap() {
    let mut clock = XPresentWindowClock::default();
    clock.observe(head(1, 1, 1000, u64::MAX - 1), None).unwrap();
    let target = clock
        .target(XPresentMscTiming::new(1, 0, 0, false).unwrap())
        .unwrap();
    clock.observe(head(1, 1, 1100, 0), None).unwrap();
    assert!(!clock.target_reached(target));
    clock.observe(head(1, 1, 1200, 1), None).unwrap();
    assert!(clock.target_reached(target));
}
