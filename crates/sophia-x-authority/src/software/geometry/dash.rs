//! Walking a dash pattern along a line.
//!
//! Derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes),
//! `crates/yserver/src/kms/render/stroke.rs`, the dash iterator.
//!
//! A pattern alternates on and off runs, starting on. The offset says how far
//! into the pattern the first line begins, and the walk continues across a
//! polyline's segments rather than restarting at each vertex, so a dashed
//! outline is dashed evenly around its corners.

mod tests;

use crate::XPoint;

/// One drawn piece of a dashed line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XDashRun {
    pub from: XPoint,
    pub to: XPoint,
    /// Whether this run is an `on` run. `LineDoubleDash` paints the off runs
    /// in the background colour; `LineOnOffDash` leaves them alone.
    pub on: bool,
}

/// Where the walk stands, so it can continue across segments.
#[derive(Clone, Copy, Debug)]
pub struct XDashState {
    index: usize,
    /// How much of the current run is still to be spent.
    remaining: u32,
    on: bool,
}

impl XDashState {
    /// Begin a pattern at `offset` units in.
    pub fn new(dashes: &[u8], offset: u16) -> Self {
        let total: u32 = dashes.iter().map(|length| u32::from(*length)).sum();
        if dashes.is_empty() || total == 0 {
            return Self {
                index: 0,
                remaining: u32::MAX,
                on: true,
            };
        }
        let mut skip = u32::from(offset) % total;
        let mut index = 0usize;
        loop {
            let length = u32::from(dashes[index % dashes.len()]);
            if skip < length {
                return Self {
                    index,
                    remaining: length - skip,
                    on: index.is_multiple_of(2),
                };
            }
            skip -= length;
            index += 1;
        }
    }

    fn advance(&mut self, dashes: &[u8]) {
        self.index += 1;
        self.on = self.index.is_multiple_of(2);
        self.remaining = u32::from(dashes[self.index % dashes.len()]).max(1);
    }
}

/// Split one segment into its dash runs, continuing `state`.
///
/// A solid pattern returns the segment whole, so the caller needs no special
/// case for an undashed line.
pub fn split(from: XPoint, to: XPoint, dashes: &[u8], state: &mut XDashState) -> Vec<XDashRun> {
    if dashes.is_empty() || dashes.iter().all(|length| *length == 0) {
        return vec![XDashRun { from, to, on: true }];
    }
    let delta_x = f64::from(to.x) - f64::from(from.x);
    let delta_y = f64::from(to.y) - f64::from(from.y);
    let length = delta_x.hypot(delta_y);
    if length < 1.0 {
        return vec![XDashRun {
            from,
            to,
            on: state.on,
        }];
    }

    let mut runs = Vec::new();
    let mut walked = 0.0_f64;
    // Bounded by the segment's own length: every iteration spends at least one
    // unit of pattern or ends the segment.
    while walked < length {
        let take = f64::from(state.remaining).min(length - walked);
        let start = walked / length;
        let end = (walked + take) / length;
        runs.push(XDashRun {
            from: XPoint {
                x: interpolate(from.x, delta_x, start),
                y: interpolate(from.y, delta_y, start),
            },
            to: XPoint {
                x: interpolate(from.x, delta_x, end),
                y: interpolate(from.y, delta_y, end),
            },
            on: state.on,
        });
        walked += take;
        // The pattern advances only when this run is actually spent; a run cut
        // short by the segment's end resumes on the next segment.
        if take >= f64::from(state.remaining) {
            state.advance(dashes);
        } else {
            state.remaining -= take.round() as u32;
        }
    }
    runs
}

fn interpolate(origin: i16, delta: f64, fraction: f64) -> i16 {
    let value = f64::from(origin) + delta * fraction;
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
