use sophia_engine::{
    KEY_REPEAT_SEAT_CAPACITY, KeyRepeatArmOutcome, KeyRepeatConfig, KeyRepeatState, KeyRepeatTarget,
};
use sophia_protocol::{DeviceId, SeatId, SurfaceId};

fn target(seat: u64, surface: u32, keycode: u32) -> KeyRepeatTarget {
    KeyRepeatTarget {
        surface: SurfaceId::new(surface, 1),
        seat: SeatId::from_raw(seat),
        device: DeviceId::from_raw(seat),
        keycode,
        source_time_msec: 10_000,
    }
}

#[test]
fn repeat_waits_for_delay_then_keeps_bounded_cadence() {
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(500, 40).unwrap());
    let key = target(1, 2, 14);

    assert_eq!(repeat.arm(key, 100, true), KeyRepeatArmOutcome::Armed);
    assert_eq!(repeat.take_due(key.seat, 599), None);
    assert_eq!(repeat.take_due(key.seat, 600).unwrap().target, key);
    assert_eq!(repeat.take_due(key.seat, 639), None);
    assert_eq!(repeat.take_due(key.seat, 640).unwrap().time_msec, 10_540);

    let delayed = repeat.take_due(key.seat, 800).unwrap();
    assert_eq!(delayed.target, key);
    assert_eq!(repeat.metrics().pulses, 3);
    assert_eq!(repeat.metrics().coalesced, 3);
}

#[test]
fn only_latest_repeatable_key_owns_a_seat_until_its_release() {
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(500, 40).unwrap());
    let first = target(1, 2, 14);
    let second = target(1, 2, 105);

    repeat.arm(first, 0, true);
    assert_eq!(
        repeat.arm(target(1, 2, 42), 10, false),
        KeyRepeatArmOutcome::NotRepeatable
    );
    assert_eq!(repeat.active_target(first.seat), Some(first));
    repeat.arm(second, 20, true);
    assert!(!repeat.release(first.seat, first.device, first.keycode));
    assert_eq!(repeat.active_target(first.seat), Some(second));
    assert!(repeat.release(second.seat, second.device, second.keycode));
    assert_eq!(repeat.active_target(first.seat), None);
}

#[test]
fn focus_and_seat_barriers_cancel_bound_repeat_targets() {
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(500, 40).unwrap());
    let first = target(1, 2, 14);
    let second = target(2, 3, 105);
    repeat.arm(first, 0, true);
    repeat.arm(second, 0, true);

    assert_eq!(repeat.cancel_surface(first.surface), 1);
    assert_eq!(repeat.active_target(first.seat), None);
    assert_eq!(repeat.active_target(second.seat), Some(second));
    assert!(repeat.cancel_seat(second.seat));
    assert_eq!(repeat.active_seats(), 0);
}

#[test]
fn repeat_seat_storage_fails_closed_at_fixed_capacity() {
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(500, 40).unwrap());
    for index in 0..KEY_REPEAT_SEAT_CAPACITY {
        assert_eq!(
            repeat.arm(target(index as u64 + 1, index as u32, 14), 0, true),
            KeyRepeatArmOutcome::Armed
        );
    }

    assert_eq!(
        repeat.arm(target(KEY_REPEAT_SEAT_CAPACITY as u64 + 1, 9, 14), 0, true),
        KeyRepeatArmOutcome::SeatCapacityExhausted
    );
    assert_eq!(repeat.metrics().seat_capacity_exhausted, 1);
}

#[test]
fn a_departed_device_cancels_only_its_own_repeat() {
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(500, 40).unwrap());
    let mut held = target(1, 2, 30);
    held.device = DeviceId::from_raw(258);
    let other = target(2, 3, 30);
    repeat.arm(held, 0, true);
    repeat.arm(other, 0, true);

    assert_eq!(repeat.cancel_device(DeviceId::from_raw(257)), 0);
    assert_eq!(repeat.active_target(held.seat), Some(held));
    assert_eq!(repeat.cancel_device(DeviceId::from_raw(258)), 1);
    assert_eq!(repeat.active_target(held.seat), None);
    assert_eq!(repeat.active_target(other.seat), Some(other));
    assert_eq!(repeat.metrics().cancelled, 1);
}
