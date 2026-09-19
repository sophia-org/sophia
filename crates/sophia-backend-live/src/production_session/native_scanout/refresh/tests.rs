#![cfg(test)]

//! The kernel's mode wins; the sysfs record is only ever a fallback.
//!
//! These pin the precedence rather than the arithmetic. The arithmetic is one
//! multiplication; the precedence is what was wrong, and what silently paced a
//! 120Hz head at 60Hz for a whole session.

use super::{fallback_cadence_index, head_refresh_interval, head_refresh_millihz};

#[test]
fn a_discovered_mode_supplies_the_refresh() {
    // The case that was broken: the record says sixty because sysfs always
    // says sixty, and the head actually runs at a hundred and twenty.
    assert_eq!(head_refresh_millihz(Some(120), 60_000), 120_000);
    assert_eq!(head_refresh_millihz(Some(60), 60_000), 60_000);
    assert_eq!(head_refresh_millihz(Some(144), 60_000), 144_000);
}

#[test]
fn a_selection_without_a_mode_keeps_the_record() {
    // A caller composing a selection by hand supplies no mode, and a record
    // that something later corrected is better than nothing.
    assert_eq!(head_refresh_millihz(None, 60_000), 60_000);
    assert_eq!(head_refresh_millihz(None, 120_000), 120_000);
}

#[test]
fn a_zero_mode_refresh_falls_back_rather_than_pacing_on_nothing() {
    // The kernel reporting zero is not a claim that the head never scans out,
    // and a zero interval divides by zero in the pacer.
    assert_eq!(head_refresh_millihz(Some(0), 60_000), 60_000);
}

#[test]
fn the_cadence_falls_back_to_the_lowest_enabled_head_not_the_lowest() {
    // The bug this replaces: `heads.first()` filtered nothing, so a disabled
    // head at index zero paced the desktop from a display that had stopped
    // scanning out, using whatever refresh it last carried.
    assert_eq!(fallback_cadence_index(&[false, true, true]), Some(1));
    assert_eq!(fallback_cadence_index(&[true, true]), Some(0));
    assert_eq!(fallback_cadence_index(&[false, false, true]), Some(2));
}

#[test]
fn no_enabled_head_selects_nothing_rather_than_index_zero() {
    // With every head disabled there is no rate to pace at. The caller keeps
    // its previous interval; it must not be handed a disabled head's.
    assert_eq!(fallback_cadence_index(&[false, false]), None);
    assert_eq!(fallback_cadence_index(&[]), None);
}

#[test]
fn an_implausible_mode_refresh_does_not_overflow() {
    // Saturating rather than wrapping: a nonsense refresh should clamp, not
    // wrap to a tiny interval and spin the compositor.
    assert_eq!(head_refresh_millihz(Some(u32::MAX), 60_000), u32::MAX);
}

#[test]
fn an_interval_is_one_refresh_period() {
    // The arithmetic every pacing decision divides by, in one place, so a
    // caller cannot pick its own rounding.
    assert_eq!(
        head_refresh_interval(60_000),
        std::time::Duration::from_micros(16_666)
    );
    assert_eq!(
        head_refresh_interval(120_000),
        std::time::Duration::from_micros(8_333)
    );
    assert_eq!(
        head_refresh_interval(144_000),
        std::time::Duration::from_micros(6_944)
    );
}

#[test]
fn an_unknown_refresh_paces_at_sixty_rather_than_not_at_all() {
    // Zero reaches here from a head whose mode was never resolved. A zero
    // interval is not a faster cadence, it is no cadence: it would make every
    // deadline already due and every wait zero.
    assert_eq!(
        head_refresh_interval(0),
        head_refresh_interval(60_000),
        "an unknown refresh takes the same fallback the millihertz rule does"
    );
    assert!(!head_refresh_interval(0).is_zero());
    assert!(!head_refresh_interval(u32::MAX).is_zero());
}
