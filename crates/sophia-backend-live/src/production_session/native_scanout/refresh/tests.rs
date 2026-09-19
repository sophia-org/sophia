#![cfg(test)]

//! The kernel's mode wins; the sysfs record is only ever a fallback.
//!
//! These pin the precedence rather than the arithmetic. The arithmetic is one
//! multiplication; the precedence is what was wrong, and what silently paced a
//! 120Hz head at 60Hz for a whole session.

use super::head_refresh_millihz;

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
fn an_implausible_mode_refresh_does_not_overflow() {
    // Saturating rather than wrapping: a nonsense refresh should clamp, not
    // wrap to a tiny interval and spin the compositor.
    assert_eq!(head_refresh_millihz(Some(u32::MAX), 60_000), u32::MAX);
}
