//! Filling a polygon, by scanline.
//!
//! Derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes),
//! `crates/yserver/src/kms/backend.rs`, the even-odd scanline fill.
//!
//! The winding rule is Sophia's own. yserver stores `fill_rule`, copies it in
//! `CopyGC`, and threads it into its draw state, but no renderer ever reads
//! it, so a self-intersecting polygon is always even-odd there. The two rules
//! differ exactly where a polygon crosses itself, which is where a client
//! chose one deliberately.

mod tests;

use crate::XPoint;
use sophia_protocol::Rect;

/// Spans are half-open in y: a scanline at the lower edge is filled and one at
/// the upper edge is not, so abutting polygons neither gap nor overlap.
pub fn fill(points: &[XPoint], winding: bool) -> Vec<Rect> {
    if points.len() < 3 {
        return Vec::new();
    }
    let top = points
        .iter()
        .map(|point| i32::from(point.y))
        .min()
        .unwrap_or(0);
    let bottom = points
        .iter()
        .map(|point| i32::from(point.y))
        .max()
        .unwrap_or(0);

    let mut spans = Vec::new();
    for scanline in top..bottom {
        // Sample the middle of the row, which avoids deciding whether a vertex
        // exactly on the boundary is inside or outside.
        let sample = f64::from(scanline) + 0.5;
        let mut crossings: Vec<(f64, i32)> = Vec::new();
        for index in 0..points.len() {
            let from = points[index];
            let to = points[(index + 1) % points.len()];
            let (y0, y1) = (f64::from(from.y), f64::from(to.y));
            if (y0 <= sample) == (y1 <= sample) {
                continue;
            }
            let t = (sample - y0) / (y1 - y0);
            let x = f64::from(from.x) + t * (f64::from(to.x) - f64::from(from.x));
            // Which way the edge crosses decides a winding count; even-odd
            // ignores the direction entirely.
            crossings.push((x, if y1 > y0 { 1 } else { -1 }));
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));

        if winding {
            let mut depth = 0;
            for pair in crossings.windows(2) {
                depth += pair[0].1;
                if depth != 0 {
                    push_span(&mut spans, pair[0].0, pair[1].0, scanline);
                }
            }
        } else {
            for pair in crossings.chunks_exact(2) {
                push_span(&mut spans, pair[0].0, pair[1].0, scanline);
            }
        }
    }
    spans
}

fn push_span(spans: &mut Vec<Rect>, left: f64, right: f64, scanline: i32) {
    let left = left.ceil() as i32;
    let right = right.ceil() as i32;
    if right <= left {
        return;
    }
    spans.push(Rect {
        x: left,
        y: scanline,
        width: right - left,
        height: 1,
    });
}
