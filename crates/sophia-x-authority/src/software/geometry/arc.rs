//! Arcs, approximated by chords.
//!
//! Derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes),
//! `crates/yserver/src/kms/render/stroke.rs:33-96`.
//!
//! An arc is walked as a polyline whose step is chosen so the chord never
//! departs from the true curve by more than half a pixel. That keeps a filled
//! arc and a stroked one consistent, since both are built from the same
//! points.

mod tests;

use crate::XPoint;

/// An X11 arc: a bounding box, a start angle and an extent, both in
/// sixty-fourths of a degree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XArc {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub angle1: i16,
    pub angle2: i16,
}

/// The most chords one arc may be split into.
///
/// A full ellipse of a thousand pixels across needs a few hundred; this bounds
/// the work a client can ask for with one request.
const MAX_CHORDS: usize = 512;

/// Walk an arc as a polyline.
///
/// Returns at least two points for any non-empty arc. A negative extent is
/// canonicalised to a positive one starting at the other end, so the two
/// spellings of the same arc produce identical points rather than merely
/// similar ones.
pub fn polyline(arc: XArc) -> Vec<XPoint> {
    if arc.width == 0 || arc.height == 0 || arc.angle2 == 0 {
        return Vec::new();
    }
    let (start, extent) = if arc.angle2 < 0 {
        (
            f64::from(arc.angle1) + f64::from(arc.angle2),
            -f64::from(arc.angle2),
        )
    } else {
        (f64::from(arc.angle1), f64::from(arc.angle2))
    };
    // Sixty-fourths of a degree to radians.
    let to_radians = std::f64::consts::PI / (180.0 * 64.0);
    let start = start * to_radians;
    let extent = (extent * to_radians).min(std::f64::consts::TAU);

    let radius_x = f64::from(arc.width) / 2.0;
    let radius_y = f64::from(arc.height) / 2.0;
    let centre_x = f64::from(arc.x) + radius_x;
    let centre_y = f64::from(arc.y) + radius_y;

    // A chord of angle d departs from a circle of radius r by about r*d*d/8.
    // Points are rounded to whole pixels, which can add most of another half,
    // so the chord itself is held to a quarter: d <= sqrt(2/r).
    let radius = radius_x.max(radius_y).max(1.0);
    let step = (2.0_f64 / radius)
        .sqrt()
        .clamp(0.001, std::f64::consts::FRAC_PI_8);
    let chords = ((extent / step).ceil() as usize).clamp(1, MAX_CHORDS);

    (0..=chords)
        .map(|index| {
            let angle = start + extent * (index as f64) / (chords as f64);
            // X's y axis grows downward while the angle convention does not.
            XPoint {
                x: clamp_i16(centre_x + radius_x * angle.cos()),
                y: clamp_i16(centre_y - radius_y * angle.sin()),
            }
        })
        .collect()
}

fn clamp_i16(value: f64) -> i16 {
    if value.is_nan() {
        return 0;
    }
    value
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}
