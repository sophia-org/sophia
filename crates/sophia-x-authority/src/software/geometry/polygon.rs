//! Filling a polygon, by scanline, as the X server's `mi` does.
//!
//! Copyright 1987, 1998 The Open Group. Copyright 1987 by Digital Equipment
//! Corporation, Maynard, Massachusetts. Both permission notices are kept in
//! full in `THIRD-PARTY-NOTICES.md`.
//!
//! A port of `mi/mipoly.c` (Brian Kelleher) with the Bresenham edge macros of
//! `mi/miscanfill.h`. The protocol's rule is that a pixel is inside when its
//! centre is, pixel centres lie on integer coordinates, and a centre on the
//! boundary counts only when the interior is immediately to its right (and,
//! on a horizontal edge, below). `mi` meets that with integer edge walkers
//! sampled at each integer y and spans half open on the right; this follows
//! it routine by routine, so that a polygon fills the pixels the reference
//! server fills. `general` is `miFillGeneralPoly`, with both fill rules;
//! `convex` is `miFillConvexPoly`, which a client chooses by declaring its
//! polygon Convex.
//!
//! This replaces a filler derived from yserver that sampled each row at its
//! middle in floating point; it disagreed with `mi` on single pixels along
//! slanted edges (t172).

mod tests;

use crate::XPoint;
use sophia_protocol::Rect;

/// `BRESINFO`: one edge's walk down the scanlines.
#[derive(Clone, Copy, Debug, Default)]
struct Bres {
    minor: i32,
    d: i32,
    m: i32,
    m1: i32,
    incr1: i32,
    incr2: i32,
}

impl Bres {
    /// `BRESINITPGON`, for an edge that is not horizontal.
    fn new(dy: i32, x1: i32, x2: i32) -> Self {
        let dx = x2 - x1;
        let m = dx / dy;
        if dx < 0 {
            let m1 = m - 1;
            Self {
                minor: x1,
                m,
                m1,
                incr1: -2 * dx + 2 * dy * m1,
                incr2: -2 * dx + 2 * dy * m,
                d: 2 * m * dy - 2 * dx - 2 * dy,
            }
        } else {
            let m1 = m + 1;
            Self {
                minor: x1,
                m,
                m1,
                incr1: 2 * dx - 2 * dy * m1,
                incr2: 2 * dx - 2 * dy * m,
                d: -2 * m * dy + 2 * dx,
            }
        }
    }

    /// `BRESINCRPGON`: one scanline down.
    fn step(&mut self) {
        let take_m1 = if self.m1 > 0 { self.d > 0 } else { self.d >= 0 };
        if take_m1 {
            self.minor += self.m1;
            self.d += self.incr1;
        } else {
            self.minor += self.m;
            self.d += self.incr2;
        }
    }
}

/// `EdgeTableEntry`, less its links: the active edge table is a vector kept
/// in the order `mi`'s list would be.
#[derive(Clone, Copy, Debug)]
struct Edge {
    /// The last scanline this edge is on.
    ymax: i32,
    bres: Bres,
    clockwise: bool,
    /// Whether the winding rule's chain (`nextWETE`) passes through this
    /// edge. Recomputed exactly when `mi` recomputes the chain, so a flag
    /// stands in for its links.
    winding: bool,
}

fn span(x: i32, y: i32, width: i32) -> Option<Rect> {
    (width > 0).then_some(Rect {
        x,
        y,
        width,
        height: 1,
    })
}

/// `micomputeWAET`: mark the edges where the winding count leaves zero and
/// where it returns.
fn compute_winding(aet: &mut [Edge]) {
    let mut inside = true;
    let mut is_inside = 0;
    for edge in aet {
        is_inside += if edge.clockwise { 1 } else { -1 };
        edge.winding = (!inside && is_inside == 0) || (inside && is_inside != 0);
        if edge.winding {
            inside = !inside;
        }
    }
}

/// `miInsertionSort`: a stable sort by x, and whether anything moved.
fn sort_active(aet: &mut [Edge]) -> bool {
    let sorted = aet
        .windows(2)
        .all(|pair| pair[0].bres.minor <= pair[1].bres.minor);
    if !sorted {
        aet.sort_by_key(|edge| edge.bres.minor);
    }
    !sorted
}

/// `miFillGeneralPoly`: any polygon, self-intersecting or not, under the
/// even-odd or the winding rule.
pub fn general(points: &[XPoint], winding: bool) -> Vec<Rect> {
    let count = points.len();
    if count < 3 {
        return Vec::new();
    }
    // `miCreateETandAET`: every edge that is not horizontal, bucketed by the
    // scanline it starts on, each bucket ordered by x with a newcomer placed
    // before an equal one. The table's extent comes from each such edge's
    // first point, as `mi` takes it.
    let mut buckets: Vec<(i32, Vec<Edge>)> = Vec::new();
    let (mut et_ymin, mut et_ymax) = (i32::MAX, i32::MIN);
    let mut previous = points[count - 1];
    for &current in points {
        let (top, bottom, clockwise) = if previous.y > current.y {
            (current, previous, false)
        } else {
            (previous, current, true)
        };
        if bottom.y != top.y {
            let dy = i32::from(bottom.y) - i32::from(top.y);
            let edge = Edge {
                ymax: i32::from(bottom.y) - 1,
                bres: Bres::new(dy, i32::from(top.x), i32::from(bottom.x)),
                clockwise,
                winding: false,
            };
            let scanline = i32::from(top.y);
            let bucket = match buckets.binary_search_by_key(&scanline, |(line, _)| *line) {
                Ok(index) => index,
                Err(index) => {
                    buckets.insert(index, (scanline, Vec::new()));
                    index
                }
            };
            let list = &mut buckets[bucket].1;
            let slot = list
                .iter()
                .position(|other| other.bres.minor >= edge.bres.minor)
                .unwrap_or(list.len());
            list.insert(slot, edge);
            et_ymax = et_ymax.max(i32::from(previous.y));
            et_ymin = et_ymin.min(i32::from(previous.y));
        }
        previous = current;
    }

    let mut spans = Vec::new();
    let mut aet: Vec<Edge> = Vec::new();
    let mut buckets = buckets.into_iter().peekable();
    let mut fix_winding = false;
    let mut y = et_ymin;
    while y < et_ymax {
        // `miloadAET`: merge the edges starting here, each after every
        // active edge strictly left of it.
        let mut loaded = false;
        if buckets.peek().is_some_and(|(line, _)| *line == y)
            && let Some((_, entering)) = buckets.next()
        {
            let mut at = 0;
            for edge in entering {
                while at < aet.len() && aet[at].bres.minor < edge.bres.minor {
                    at += 1;
                }
                aet.insert(at, edge);
                at += 1;
            }
            loaded = true;
        }
        if winding {
            if loaded {
                compute_winding(&mut aet);
            }
            // A span runs from an edge where the count leaves zero to the
            // next marked edge, where it returns.
            let marked: Vec<i32> = aet
                .iter()
                .filter(|edge| edge.winding)
                .map(|edge| edge.bres.minor)
                .collect();
            for pair in marked.chunks_exact(2) {
                spans.extend(span(pair[0], y, pair[1] - pair[0]));
            }
        } else {
            for pair in aet.chunks_exact(2) {
                spans.extend(span(
                    pair[0].bres.minor,
                    y,
                    pair[1].bres.minor - pair[0].bres.minor,
                ));
            }
        }
        // `EVALUATEEDGE*`: retire the edges ending on this scanline and step
        // the rest.
        let before = aet.len();
        aet.retain(|edge| edge.ymax != y);
        if aet.len() != before {
            fix_winding = true;
        }
        for edge in &mut aet {
            edge.bres.step();
        }
        let resorted = sort_active(&mut aet);
        if winding && (resorted || fix_winding) {
            compute_winding(&mut aet);
            fix_winding = false;
        }
        y += 1;
    }
    spans
}

/// `miFillConvexPoly`: a polygon its client declared convex, walked down one
/// left and one right chain of edges. As in `mi`, a polygon that proves not
/// to be convex fills nothing.
pub fn convex(points: &[XPoint]) -> Vec<Rect> {
    let count = points.len();
    if count < 3 {
        return Vec::new();
    }
    let at = |index: usize| (i32::from(points[index].x), i32::from(points[index].y));
    // `getPolyYBounds`
    let mut imin = 0;
    let (mut ymin, mut ymax) = (at(0).1, at(0).1);
    for index in 1..count {
        let y = at(index).1;
        if y < ymin {
            imin = index;
            ymin = y;
        }
        if y > ymax {
            ymax = y;
        }
    }
    let mut spans = Vec::new();
    let (mut nextleft, mut nextright) = (imin, imin);
    let mut y = at(nextleft).1;
    let mut left = Bres::default();
    let mut right = Bres::default();
    // `mi` trusts the client's word that the polygon is convex. Each pass
    // either takes a new edge on a side or fills rows, so a polygon that is
    // what it says ends well inside this bound; one that lies is refused
    // rather than allowed to spin.
    let bound = 2 * count + usize::try_from(ymax - ymin).unwrap_or(0) + 2;
    for _ in 0..bound {
        if at(nextleft).1 == y {
            let from = nextleft;
            nextleft = if nextleft + 1 >= count {
                0
            } else {
                nextleft + 1
            };
            let dy = at(nextleft).1 - at(from).1;
            if dy != 0 {
                left = Bres::new(dy, at(from).0, at(nextleft).0);
            }
        }
        if at(nextright).1 == y {
            let from = nextright;
            nextright = if nextright == 0 {
                count - 1
            } else {
                nextright - 1
            };
            let dy = at(nextright).1 - at(from).1;
            if dy != 0 {
                right = Bres::new(dy, at(from).0, at(nextright).0);
            }
        }
        let rows = at(nextleft).1.min(at(nextright).1) - y;
        // Not convex after all: `mi` drops what it had and fills nothing.
        if rows < 0 {
            return Vec::new();
        }
        for _ in 0..rows {
            let (x, width) = if left.minor < right.minor {
                (left.minor, right.minor - left.minor)
            } else {
                (right.minor, left.minor - right.minor)
            };
            spans.extend(span(x, y, width));
            y += 1;
            left.step();
            right.step();
        }
        if y == ymax {
            return spans;
        }
    }
    Vec::new()
}
