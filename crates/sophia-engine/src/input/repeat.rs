use crate::prelude::*;

pub const KEY_REPEAT_SEAT_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyRepeatConfig {
    pub delay_msec: u64,
    pub interval_msec: u64,
}

impl KeyRepeatConfig {
    pub const fn new(delay_msec: u64, interval_msec: u64) -> Option<Self> {
        if delay_msec == 0 || interval_msec == 0 {
            return None;
        }
        Some(Self {
            delay_msec,
            interval_msec,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyRepeatTarget {
    pub surface: SurfaceId,
    pub seat: SeatId,
    pub device: DeviceId,
    pub keycode: u32,
    pub source_time_msec: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyRepeatPulse {
    pub target: KeyRepeatTarget,
    pub time_msec: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRepeatArmOutcome {
    Armed,
    NotRepeatable,
    SeatCapacityExhausted,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyRepeatMetrics {
    pub armed: u64,
    pub pulses: u64,
    pub coalesced: u64,
    pub cancelled: u64,
    pub seat_capacity_exhausted: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KeyRepeatSeatState {
    seat: SeatId,
    target: Option<KeyRepeatTarget>,
    armed_at_msec: u64,
    next_due_msec: u64,
}

#[derive(Debug)]
pub struct KeyRepeatState {
    config: KeyRepeatConfig,
    seats: [Option<KeyRepeatSeatState>; KEY_REPEAT_SEAT_CAPACITY],
    metrics: KeyRepeatMetrics,
}

impl KeyRepeatState {
    pub const fn new(config: KeyRepeatConfig) -> Self {
        Self {
            config,
            seats: [None; KEY_REPEAT_SEAT_CAPACITY],
            metrics: KeyRepeatMetrics {
                armed: 0,
                pulses: 0,
                coalesced: 0,
                cancelled: 0,
                seat_capacity_exhausted: 0,
            },
        }
    }

    pub fn arm(
        &mut self,
        target: KeyRepeatTarget,
        now_msec: u64,
        repeatable: bool,
    ) -> KeyRepeatArmOutcome {
        if !repeatable {
            return KeyRepeatArmOutcome::NotRepeatable;
        }
        let delay_msec = self.config.delay_msec;
        let Some(slot) = self.seat_slot_or_insert(target.seat) else {
            self.metrics.seat_capacity_exhausted =
                self.metrics.seat_capacity_exhausted.saturating_add(1);
            return KeyRepeatArmOutcome::SeatCapacityExhausted;
        };
        slot.target = Some(target);
        slot.armed_at_msec = now_msec;
        slot.next_due_msec = now_msec.saturating_add(delay_msec);
        self.metrics.armed = self.metrics.armed.saturating_add(1);
        KeyRepeatArmOutcome::Armed
    }

    pub fn release(&mut self, seat: SeatId, device: DeviceId, keycode: u32) -> bool {
        let Some(slot) = self.seat_slot_mut(seat) else {
            return false;
        };
        if !slot
            .target
            .is_some_and(|target| target.device == device && target.keycode == keycode)
        {
            return false;
        }
        slot.target = None;
        self.metrics.cancelled = self.metrics.cancelled.saturating_add(1);
        true
    }

    pub fn cancel_all(&mut self) {
        for slot in self.seats.iter_mut().flatten() {
            if slot.target.take().is_some() {
                self.metrics.cancelled = self.metrics.cancelled.saturating_add(1);
            }
        }
    }

    pub fn cancel_seat(&mut self, seat: SeatId) -> bool {
        let Some(slot) = self.seat_slot_mut(seat) else {
            return false;
        };
        let cancelled = slot.target.take().is_some();
        if cancelled {
            self.metrics.cancelled = self.metrics.cancelled.saturating_add(1);
        }
        cancelled
    }

    /// The device left the seat; whatever it was repeating stops. A seat
    /// holds one repeating key at a time, so this is at most one per seat
    /// and touches no other device's key.
    pub fn cancel_device(&mut self, device: DeviceId) -> usize {
        let mut cancelled = 0usize;
        for slot in self.seats.iter_mut().flatten() {
            if slot.target.is_some_and(|target| target.device == device) {
                slot.target = None;
                cancelled = cancelled.saturating_add(1);
            }
        }
        self.metrics.cancelled = self
            .metrics
            .cancelled
            .saturating_add(u64::try_from(cancelled).unwrap_or(u64::MAX));
        cancelled
    }

    pub fn cancel_surface(&mut self, surface: SurfaceId) -> usize {
        let mut cancelled = 0usize;
        for slot in self.seats.iter_mut().flatten() {
            if slot.target.is_some_and(|target| target.surface == surface) {
                slot.target = None;
                cancelled = cancelled.saturating_add(1);
            }
        }
        self.metrics.cancelled = self
            .metrics
            .cancelled
            .saturating_add(u64::try_from(cancelled).unwrap_or(u64::MAX));
        cancelled
    }

    pub fn active_target(&self, seat: SeatId) -> Option<KeyRepeatTarget> {
        self.seats
            .iter()
            .flatten()
            .find(|slot| slot.seat == seat)
            .and_then(|slot| slot.target)
    }

    pub fn take_due(&mut self, seat: SeatId, now_msec: u64) -> Option<KeyRepeatPulse> {
        let interval_msec = self.config.interval_msec;
        let (target, armed_at_msec, intervals_elapsed) = {
            let slot = self.seat_slot_mut(seat)?;
            let target = slot.target?;
            if now_msec < slot.next_due_msec {
                return None;
            }
            let intervals_elapsed = now_msec
                .saturating_sub(slot.next_due_msec)
                .checked_div(interval_msec)
                .unwrap_or(0);
            slot.next_due_msec = slot.next_due_msec.saturating_add(
                intervals_elapsed
                    .saturating_add(1)
                    .saturating_mul(interval_msec),
            );
            (target, slot.armed_at_msec, intervals_elapsed)
        };
        self.metrics.coalesced = self.metrics.coalesced.saturating_add(intervals_elapsed);
        self.metrics.pulses = self.metrics.pulses.saturating_add(1);
        Some(KeyRepeatPulse {
            target,
            time_msec: target
                .source_time_msec
                .saturating_add(now_msec.saturating_sub(armed_at_msec)),
        })
    }

    pub fn active_seats(&self) -> usize {
        self.seats
            .iter()
            .flatten()
            .filter(|slot| slot.target.is_some())
            .count()
    }

    pub const fn metrics(&self) -> KeyRepeatMetrics {
        self.metrics
    }

    fn seat_slot_mut(&mut self, seat: SeatId) -> Option<&mut KeyRepeatSeatState> {
        self.seats
            .iter_mut()
            .flatten()
            .find(|slot| slot.seat == seat)
    }

    fn seat_slot_or_insert(&mut self, seat: SeatId) -> Option<&mut KeyRepeatSeatState> {
        if let Some(index) = self
            .seats
            .iter()
            .position(|slot| slot.is_some_and(|slot| slot.seat == seat))
        {
            return self.seats[index].as_mut();
        }
        let index = self.seats.iter().position(Option::is_none)?;
        self.seats[index] = Some(KeyRepeatSeatState {
            seat,
            target: None,
            armed_at_msec: 0,
            next_due_msec: 0,
        });
        self.seats[index].as_mut()
    }
}
