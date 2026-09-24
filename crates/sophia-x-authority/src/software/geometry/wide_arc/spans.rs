//! The spans of one wide arc: the inner and outer edges of an ellipse swept
//! by a line, quadrant by quadrant, and the accumulator that merges them.
//!
//! From `mi/miarc.c` (Keith Packard and Bob Scheifler): `miComputeWideEllipse`
//! with its circle and ellipse span tables, `tailX`, `miFillWideEllipse`,
//! `drawArc`, `drawQuadrant`, `computeAcc`, `computeBound`, the hook and tail
//! span routines, `drawZeroArc`, and `newFinalSpan`. The quartic that gives an
//! ellipse's offset edge is solved as `mi` solves it, expression by
//! expression, because the pixels depend on its rounding.
//!
//! The full wide ellipse is in `ellipse`, and the bounds and accelerators
//! `arcSpan` walks are in `bounds`; this file keeps the quadrant drawing and
//! the span accumulator they feed.

use super::super::wide_line::{XSpan, iceil};
use super::{Face, LEFT_END, RIGHT_END, Spp, mi_dcos, mi_dsin};
use crate::software::geometry::arc::XArc;

mod bounds;
mod ellipse;

use bounds::*;
pub(super) use ellipse::{fill_wide_ellipse, wide_ellipse};

const EPSILON: f64 = 0.000_001;
const CUBED_ROOT_2: f64 = 1.259_921_049_894_873_2;
const CUBED_ROOT_4: f64 = 1.587_401_051_968_199_4;
pub(super) const FULLCIRCLE: i32 = 360 * 64;

fn cbrt(x: f64) -> f64 {
    x.cbrt()
}

/// `miArcSpan`: one row of a quadrant, as offsets from the arc's centre.
#[derive(Clone, Copy, Debug, Default)]
struct ArcSpan {
    lx: i32,
    lw: i32,
    rx: i32,
    rw: i32,
}

/// `miArcSpanData`
#[derive(Clone, Debug, Default)]
pub(super) struct SpanData {
    spans: Vec<ArcSpan>,
    count1: i32,
    count2: i32,
    k: i32,
    top: bool,
    bot: bool,
    hole: bool,
}

/// The pixels one render step has produced so far: `mi`'s final spans, and
/// the caps, joins and flat arcs it paints in the same step.
#[derive(Default)]
pub(super) struct Canvas {
    spans: Vec<XSpan>,
}

impl Canvas {
    /// `newFinalSpan` for `[xmin, xmax)`. The union is taken when the step is
    /// painted; an empty or inverted interval adds no pixel there either.
    pub(super) fn span(&mut self, y: i32, xmin: i32, xmax: i32) {
        if xmax > xmin {
            self.spans.push(XSpan {
                x: xmin,
                y,
                width: xmax - xmin,
            });
        }
    }

    /// The union of everything drawn, row by row.
    pub(super) fn take_union(&mut self) -> Vec<XSpan> {
        let mut all = std::mem::take(&mut self.spans);
        all.sort_by_key(|span| (span.y, span.x));
        let mut merged: Vec<XSpan> = Vec::with_capacity(all.len());
        for span in all {
            match merged.last_mut() {
                Some(current) if current.y == span.y && span.x <= current.x + current.width => {
                    let end = (span.x + span.width).max(current.x + current.width);
                    current.width = end - current.x;
                }
                _ => merged.push(span),
            }
        }
        merged
    }
}

/// `arcSpan`
#[allow(clippy::too_many_arguments)]
fn arc_span(
    canvas: &mut Canvas,
    y: i32,
    lx: i32,
    lw: i32,
    rx: i32,
    rw: i32,
    def: &ArcDef,
    bounds: &ArcBound,
    acc: &Accelerators,
    mask: i32,
) {
    let (linx, rinx);
    let yy = f64::from(y) + acc.from_int_y;
    if ibounded(y, bounds.inneri) {
        linx = -(lx + lw);
        rinx = rx;
    } else {
        // intersection with left face
        let mut x = hook_x(yy, def, bounds, acc, true);
        if acc.right.valid && bounded(yy, bounds.right) {
            let altx = acc.right.at(yy);
            if altx < x {
                x = altx;
            }
        }
        linx = -iceil(acc.from_int_x - x);
        rinx = iceil(acc.from_int_x + x);
    }
    let (loutx, routx);
    if ibounded(y, bounds.outeri) {
        loutx = -lx;
        routx = rx + rw;
    } else {
        // intersection with right face
        let mut x = hook_x(yy, def, bounds, acc, false);
        if acc.left.valid && bounded(yy, bounds.left) {
            let altx = x;
            x = acc.left.at(yy);
            if x < altx {
                x = altx;
            }
        }
        loutx = -iceil(acc.from_int_x - x);
        routx = iceil(acc.from_int_x + x);
    }
    if routx > rinx {
        if mask & 1 != 0 {
            canvas.span(acc.yorgu - y, acc.xorg + rinx, acc.xorg + routx);
        }
        if mask & 8 != 0 {
            canvas.span(acc.yorgl + y, acc.xorg + rinx, acc.xorg + routx);
        }
    }
    if loutx > linx {
        if mask & 2 != 0 {
            canvas.span(acc.yorgu - y, acc.xorg - loutx, acc.xorg - linx);
        }
        if mask & 4 != 0 {
            canvas.span(acc.yorgl + y, acc.xorg - loutx, acc.xorg - linx);
        }
    }
}

/// `arcSpan0`
#[allow(clippy::too_many_arguments)]
fn arc_span0(
    canvas: &mut Canvas,
    lx: i32,
    mut lw: i32,
    mut rx: i32,
    mut rw: i32,
    def: &ArcDef,
    bounds: &ArcBound,
    acc: &Accelerators,
    mask: i32,
) {
    if ibounded(0, bounds.inneri) && acc.left.valid && bounded(0.0, bounds.left) && acc.left.b > 0.0
    {
        let mut x = def.w - def.l;
        if acc.left.b < x {
            x = acc.left.b;
        }
        lw = iceil(acc.from_int_x - x) - lx;
        rw += rx;
        rx = iceil(acc.from_int_x + x);
        rw -= rx;
    }
    arc_span(canvas, 0, lx, lw, rx, rw, def, bounds, acc, mask);
}

/// `tailSpan`
#[allow(clippy::too_many_arguments)]
fn tail_span(
    canvas: &mut Canvas,
    y: i32,
    lw: i32,
    rw: i32,
    def: &ArcDef,
    bounds: &ArcBound,
    acc: &Accelerators,
    mask: i32,
) {
    if ibounded(y, bounds.outeri) {
        arc_span(canvas, y, 0, lw, -rw, rw, def, bounds, acc, mask);
    } else if def.w != def.h {
        let yy = f64::from(y) + acc.from_int_y;
        let x = tail_x(yy, def, bounds, acc);
        if yy == 0.0 && x == f64::from(-rw) - acc.from_int_x {
            return;
        }
        if acc.right.valid && bounded(yy, bounds.right) {
            let mut rx = x;
            let lx = -x;
            let xalt = acc.right.at(yy);
            if xalt >= f64::from(-rw) - acc.from_int_x && xalt <= rx {
                rx = xalt;
            }
            let mut n = iceil(acc.from_int_x + lx);
            if lw > n {
                if mask & 2 != 0 {
                    canvas.span(acc.yorgu - y, acc.xorg + n, acc.xorg + lw);
                }
                if mask & 4 != 0 {
                    canvas.span(acc.yorgl + y, acc.xorg + n, acc.xorg + lw);
                }
            }
            n = iceil(acc.from_int_x + rx);
            if n > -rw {
                if mask & 1 != 0 {
                    canvas.span(acc.yorgu - y, acc.xorg - rw, acc.xorg + n);
                }
                if mask & 8 != 0 {
                    canvas.span(acc.yorgl + y, acc.xorg - rw, acc.xorg + n);
                }
            }
        }
        arc_span(
            canvas,
            y,
            iceil(acc.from_int_x - x),
            0,
            iceil(acc.from_int_x + x),
            0,
            def,
            bounds,
            acc,
            mask,
        );
    }
}

/// `drawQuadrant`
#[allow(clippy::too_many_arguments)]
fn draw_quadrant(
    canvas: &mut Canvas,
    def: &mut ArcDef,
    acc: &mut Accelerators,
    a0: i32,
    a1: i32,
    mask: i32,
    faces: Option<&mut [Face; 2]>,
    right: Option<usize>,
    left: Option<usize>,
    sp: &SpanData,
) {
    def.a0 = f64::from(a0) / 64.0;
    def.a1 = f64::from(a1) / 64.0;
    let bound = compute_bound(def, acc, faces, right, left);
    let def = &*def;
    let acc = &*acc;
    let mut yy = bound.inner.min;
    if bound.outer.min < yy {
        yy = bound.outer.min;
    }
    let miny = iceil(yy - acc.from_int_y);
    yy = bound.inner.max;
    if bound.outer.max > yy {
        yy = bound.outer.max;
    }
    let maxy = (yy - acc.from_int_y).floor() as i32;
    let mut y = sp.k;
    let mut index = 0usize;
    if sp.top {
        if a1 == 90 * 64 && mask & 1 != 0 {
            canvas.span(acc.yorgu - y - 1, acc.xorg, acc.xorg + 1);
        }
        index += 1;
    }
    for _ in 0..sp.count1.max(0) {
        if y < miny {
            return;
        }
        let span = sp.spans[index];
        if y <= maxy {
            arc_span(
                canvas,
                y,
                span.lx,
                -span.lx,
                0,
                span.lx + span.lw,
                def,
                &bound,
                acc,
                mask,
            );
            if span.rw + span.rx != 0 {
                tail_span(canvas, y, -span.rw, -span.rx, def, &bound, acc, mask);
            }
        }
        y -= 1;
        index += 1;
    }
    if y < miny {
        return;
    }
    if sp.hole && y <= maxy {
        arc_span(canvas, y, 0, 0, 0, 1, def, &bound, acc, mask & 0xc);
    }
    for _ in 0..sp.count2.max(0) {
        if y < miny {
            return;
        }
        let span = sp.spans[index];
        if y <= maxy {
            arc_span(
                canvas, y, span.lx, span.lw, span.rx, span.rw, def, &bound, acc, mask,
            );
        }
        y -= 1;
        index += 1;
    }
    if sp.bot && miny <= y && y <= maxy {
        let span = sp.spans[index];
        let mut n = mask;
        if y == miny {
            n &= 0xc;
        }
        if span.rw <= 0 {
            arc_span0(
                canvas,
                span.lx,
                -span.lx,
                0,
                span.lx + span.lw,
                def,
                &bound,
                acc,
                n,
            );
            if span.rw + span.rx != 0 {
                tail_span(canvas, y, -span.rw, -span.rx, def, &bound, acc, n);
            }
        } else {
            arc_span0(
                canvas, span.lx, span.lw, span.rx, span.rw, def, &bound, acc, n,
            );
        }
        y -= 1;
    }
    while y >= miny {
        let yy = f64::from(y) + acc.from_int_y;
        let x = if def.w == def.h {
            let xalt = def.w - def.l;
            -(xalt * xalt - yy * yy).sqrt()
        } else {
            let mut x = tail_x(yy, def, &bound, acc);
            if acc.left.valid && bounded(yy, bound.left) {
                let xalt = acc.left.at(yy);
                if xalt < x {
                    x = xalt;
                }
            }
            if acc.right.valid && bounded(yy, bound.right) {
                let xalt = acc.right.at(yy);
                if xalt < x {
                    x = xalt;
                }
            }
            x
        };
        arc_span(
            canvas,
            y,
            iceil(acc.from_int_x - x),
            0,
            iceil(acc.from_int_x + x),
            0,
            def,
            &bound,
            acc,
            mask,
        );
        y -= 1;
    }
}

/// `mirrorSppPoint`: from a first-quadrant point to its quadrant, and to X
/// coordinates (y down).
fn mirror(quadrant: i32, point: &mut Spp) {
    match quadrant {
        1 => point.x = -point.x,
        2 => {
            point.x = -point.x;
            point.y = -point.y;
        }
        3 => point.y = -point.y,
        _ => {}
    }
    point.y = -point.y;
}

/// Which of an arc's two faces a quadrant pass writes, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    None,
    Face(usize),
}

/// `drawArc`: split the arc into first-quadrant sweeps, scan-convert each,
/// and record the arc's end faces. `right` and `left` index `faces`, and are
/// swapped by the caller for a clockwise arc, as `miArcSegment` swaps them.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_arc(
    canvas: &mut Canvas,
    arc: &XArc,
    l: i32,
    mut a0: i32,
    mut a1: i32,
    faces: Option<&mut [Face; 2]>,
    right: usize,
    left: usize,
    sp: &SpanData,
) {
    #[derive(Clone, Copy, Default)]
    struct Band {
        a0: i32,
        a1: i32,
        mask: i32,
    }
    if a1 < a0 {
        a1 += 360 * 64;
    }
    let startq = a0 / (90 * 64);
    let mut endq = if a0 == a1 {
        startq
    } else {
        (a1 - 1) / (90 * 64)
    };
    let mut bands: Vec<Band> = Vec::with_capacity(5);
    let mut curq = startq;
    let mut rightq = -1;
    let (mut leftq, mut righta, mut lefta) = (0, 0, 0);
    loop {
        let (q0, q1);
        match curq {
            0 => {
                q0 = if a0 > 90 * 64 { 0 } else { a0 };
                q1 = if a1 < 360 * 64 {
                    a1.min(90 * 64)
                } else {
                    90 * 64
                };
                if curq == startq && a0 == q0 && rightq < 0 {
                    righta = q0;
                    rightq = curq;
                }
                if curq == endq && a1 == q1 {
                    lefta = q1;
                    leftq = curq;
                }
            }
            1 => {
                q0 = if a1 < 90 * 64 {
                    0
                } else {
                    180 * 64 - a1.min(180 * 64)
                };
                q1 = if a0 > 180 * 64 {
                    90 * 64
                } else {
                    180 * 64 - a0.max(90 * 64)
                };
                if curq == startq && 180 * 64 - a0 == q1 {
                    righta = q1;
                    rightq = curq;
                }
                if curq == endq && 180 * 64 - a1 == q0 {
                    lefta = q0;
                    leftq = curq;
                }
            }
            2 => {
                q0 = if a0 > 270 * 64 {
                    0
                } else {
                    a0.max(180 * 64) - 180 * 64
                };
                q1 = if a1 < 180 * 64 {
                    90 * 64
                } else {
                    a1.min(270 * 64) - 180 * 64
                };
                if curq == startq && a0 - 180 * 64 == q0 {
                    righta = q0;
                    rightq = curq;
                }
                if curq == endq && a1 - 180 * 64 == q1 {
                    lefta = q1;
                    leftq = curq;
                }
            }
            _ => {
                q0 = if a1 < 270 * 64 {
                    0
                } else {
                    360 * 64 - a1.min(360 * 64)
                };
                q1 = 360 * 64 - a0.max(270 * 64);
                if curq == startq && 360 * 64 - a0 == q1 {
                    righta = q1;
                    rightq = curq;
                }
                if curq == endq && 360 * 64 - a1 == q0 {
                    lefta = q0;
                    leftq = curq;
                }
            }
        }
        bands.push(Band {
            a0: q0,
            a1: q1,
            mask: 1 << curq,
        });
        if curq == endq {
            break;
        }
        curq += 1;
        if curq == 4 {
            a0 = 0;
            a1 -= 360 * 64;
            curq = 0;
            endq -= 4;
        }
    }
    let mut sweeps: Vec<Band> = Vec::with_capacity(20);
    loop {
        let mut q0 = 90 * 64;
        let mut q1 = 0;
        let mut mask = 0;
        // find left-most point
        for band in &bands {
            if band.a0 <= q0 {
                q0 = band.a0;
                q1 = band.a1;
                mask = band.mask;
            }
        }
        if mask == 0 {
            break;
        }
        // locate next point of change
        for band in &bands {
            if mask & band.mask == 0 {
                if band.a0 == q0 {
                    if band.a1 < q1 {
                        q1 = band.a1;
                    }
                    mask |= band.mask;
                } else if band.a0 < q1 {
                    q1 = band.a0;
                }
            }
        }
        sweeps.push(Band {
            a0: q0,
            a1: q1,
            mask,
        });
        // subtract the sweep from the affected bands
        for band in &mut bands {
            if band.a0 == q0 {
                band.a0 = q1;
                if band.a0 == band.a1 {
                    band.a0 = 90 * 64 + 1;
                    band.a1 = 90 * 64 + 1;
                }
            }
        }
    }
    let (mut def, mut acc) = compute_acc(arc, l);
    let mut faces = faces;
    let (mut flip_right, mut flip_left, mut copy_end) = (false, false, false);
    for sweep in &sweeps {
        let mask = sweep.mask;
        let mut pass_right = Pass::None;
        let mut pass_left = Pass::None;
        if faces.is_some() {
            if rightq >= 0 && mask & (1 << rightq) != 0 {
                if sweep.a0 == righta {
                    pass_right = Pass::Face(right);
                } else if sweep.a1 == righta {
                    pass_left = Pass::Face(right);
                    flip_right = true;
                }
            }
            if mask & (1 << leftq) != 0 {
                if sweep.a1 == lefta {
                    if pass_left != Pass::None {
                        copy_end = true;
                    }
                    pass_left = Pass::Face(left);
                } else if sweep.a0 == lefta {
                    if pass_right != Pass::None {
                        copy_end = true;
                    }
                    pass_right = Pass::Face(left);
                    flip_left = true;
                }
            }
        }
        let index = |pass: Pass| match pass {
            Pass::None => None,
            Pass::Face(face) => Some(face),
        };
        draw_quadrant(
            canvas,
            &mut def,
            &mut acc,
            sweep.a0,
            sweep.a1,
            mask,
            faces.as_deref_mut(),
            index(pass_right),
            index(pass_left),
            sp,
        );
    }
    let Some(pair) = faces else {
        return;
    };
    // When both ends were computed in one pass, only the left face holds
    // them; copy it.
    if copy_end {
        pair[right] = pair[left];
    }
    let rq = rightq;
    let face = &mut pair[right];
    mirror(rq, &mut face.clock);
    mirror(rq, &mut face.center);
    mirror(rq, &mut face.counter_clock);
    if flip_right {
        std::mem::swap(&mut face.clock, &mut face.counter_clock);
    }
    let face = &mut pair[left];
    mirror(leftq, &mut face.counter_clock);
    mirror(leftq, &mut face.center);
    mirror(leftq, &mut face.clock);
    if flip_left {
        std::mem::swap(&mut face.clock, &mut face.counter_clock);
    }
}

/// `drawZeroArc`: an arc with no width or no height is a bar as wide as the
/// line; its faces are recorded as for any arc.
pub(super) fn draw_zero_arc(
    canvas: &mut Canvas,
    arc: &XArc,
    lw: i32,
    faces: Option<&mut [Face; 2]>,
) {
    let (right, left) = (RIGHT_END, LEFT_END);
    let l = f64::from(lw) / 2.0;
    let a0 = i32::from(arc.angle1);
    let a1 = i32::from(arc.angle2).clamp(-FULLCIRCLE, FULLCIRCLE);
    let w = f64::from(arc.width) / 2.0;
    let h = f64::from(arc.height) / 2.0;
    // play in X coordinates right away
    let start_angle = -(f64::from(a0) / 64.0);
    let end_angle = -(f64::from(a0 + a1) / 64.0);
    let (mut xmax, mut xmin, mut ymax, mut ymin) = (-w, w, -h, h);
    let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, 0.0, 0.0);
    let mut a = start_angle;
    loop {
        let x = w * mi_dcos(a);
        let y = h * mi_dsin(a);
        if a == start_angle {
            x0 = x;
            y0 = y;
        }
        if a == end_angle {
            x1 = x;
            y1 = y;
        }
        xmax = xmax.max(x);
        xmin = xmin.min(x);
        ymax = ymax.max(y);
        ymin = ymin.min(y);
        if a == end_angle {
            break;
        }
        if a1 < 0 {
            // clockwise
            if (a / 90.0).floor() == (end_angle / 90.0).floor() {
                a = end_angle;
            } else {
                a = 90.0 * ((a / 90.0).floor() + 1.0);
            }
        } else if (a / 90.0).ceil() == (end_angle / 90.0).ceil() {
            a = end_angle;
        } else {
            a = 90.0 * ((a / 90.0).ceil() - 1.0);
        }
    }
    let (mut lx, mut ly) = (l, l);
    if (x1 - x0) + (y1 - y0) < 0.0 {
        lx = -l;
        ly = -l;
    }
    if h != 0.0 {
        ly = 0.0;
        lx = -lx;
    } else {
        lx = 0.0;
    }
    if let Some(pair) = faces {
        pair[right].center = Spp { x: x0, y: y0 };
        pair[right].clock = Spp {
            x: x0 - lx,
            y: y0 - ly,
        };
        pair[right].counter_clock = Spp {
            x: x0 + lx,
            y: y0 + ly,
        };
        pair[left].center = Spp { x: x1, y: y1 };
        pair[left].clock = Spp {
            x: x1 + lx,
            y: y1 + ly,
        };
        pair[left].counter_clock = Spp {
            x: x1 - lx,
            y: y1 - ly,
        };
    }
    let y1 = ymax;
    if ymin != y1 {
        xmin = -l;
        xmax = l;
    } else {
        ymin = -l;
        ymax = l;
    }
    if xmax != xmin && ymax != ymin {
        let minx = iceil(xmin + w) + i32::from(arc.x);
        let maxx = iceil(xmax + w) + i32::from(arc.x);
        let miny = iceil(ymin + h) + i32::from(arc.y);
        let maxy = iceil(ymax + h) + i32::from(arc.y);
        for y in miny..maxy {
            canvas.span(y, minx, maxx);
        }
    }
}
