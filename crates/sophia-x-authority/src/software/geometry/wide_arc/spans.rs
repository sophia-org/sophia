//! The spans of one wide arc: the inner and outer edges of an ellipse swept
//! by a line, quadrant by quadrant, and the accumulator that merges them.
//!
//! From `mi/miarc.c` (Keith Packard and Bob Scheifler): `miComputeWideEllipse`
//! with its circle and ellipse span tables, `tailX`, `miFillWideEllipse`,
//! `drawArc`, `drawQuadrant`, `computeAcc`, `computeBound`, the hook and tail
//! span routines, `drawZeroArc`, and `newFinalSpan`. The quartic that gives an
//! ellipse's offset edge is solved as `mi` solves it, expression by
//! expression, because the pixels depend on its rounding.

use super::super::wide_line::{XSpan, iceil};
use super::{Face, LEFT_END, RIGHT_END, Spp, mi_dcos, mi_dsin};
use crate::software::geometry::arc::XArc;

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

/// `MIWIDEARCSETUP`
struct WideArcWalk {
    x: i32,
    y: i32,
    e: i32,
    xk: i32,
    xm: i32,
    yk: i32,
    ym: i32,
}

fn wide_arc_setup(dy: i32, slw: i32) -> WideArcWalk {
    let x = 0;
    let mut y = slw >> 1;
    let mut yk = y << 3;
    let xm = 8;
    let ym = 8;
    let (xk, e);
    if dy != 0 {
        xk = 0;
        e = if slw & 1 != 0 { -1 } else { -(y << 2) - 2 };
    } else {
        y += 1;
        yk += 4;
        xk = -4;
        e = if slw & 1 != 0 {
            -(y << 2) - 3
        } else {
            -(y << 3)
        };
    }
    WideArcWalk {
        x,
        y,
        e,
        xk,
        xm,
        yk,
        ym,
    }
}

impl WideArcWalk {
    /// `MIFILLARCSTEP` / `MIFILLINARCSTEP`
    fn step(&mut self, dx: i32) -> i32 {
        self.e += self.yk;
        while self.e >= 0 {
            self.x += 1;
            self.xk -= self.xm;
            self.e += self.xk;
        }
        self.y -= 1;
        self.yk -= self.ym;
        let mut slw = (self.x << 1) + dx;
        if self.e == self.xk && slw > 1 {
            slw -= 1;
        }
        slw
    }
}

/// `miComputeCircleSpans`
fn circle_spans(lw: i32, arc: &XArc, sp: &mut SpanData) {
    let width = i32::from(arc.width);
    let height = i32::from(arc.height);
    let mut doinner = -lw;
    let slw = width - doinner;
    let dy = height & 1;
    let dx = 1 - dy;
    let mut outer = wide_arc_setup(dy, slw);
    let inslw = width + doinner;
    let mut inner = None;
    if inslw > 0 {
        sp.hole = sp.top;
        inner = Some(wide_arc_setup(dy, inslw));
    } else {
        sp.hole = false;
        doinner = -outer.y;
    }
    sp.count1 = -doinner - i32::from(sp.top);
    sp.count2 = outer.y + doinner;
    let mut index = 0usize;
    while outer.y != 0 {
        let slw = outer.step(dx);
        let x = outer.x;
        let mut span = ArcSpan {
            lx: dy - x,
            ..ArcSpan::default()
        };
        doinner += 1;
        if doinner <= 0 {
            span.lw = slw;
            span.rx = 0;
            span.rw = span.lx + slw;
        } else if let Some(inner) = inner.as_mut() {
            let inslw = inner.step(dx);
            let inx = inner.x;
            span.lw = x - inx;
            span.rx = dy - inx + inslw;
            span.rw = inx - x + slw - inslw;
        }
        if let Some(slot) = sp.spans.get_mut(index) {
            *slot = span;
        }
        index += 1;
    }
    if sp.bot {
        if sp.count2 != 0 {
            sp.count2 -= 1;
        } else {
            if let Some(last) = index.checked_sub(1).and_then(|i| sp.spans.get_mut(i)) {
                if lw > height {
                    let v = -((lw - height) >> 1);
                    last.rx = v;
                    last.rw = v;
                } else {
                    last.rw = 0;
                }
            }
            sp.count1 -= 1;
        }
    }
}

/// `miComputeEllipseSpans`
fn ellipse_spans(lw: i32, arc: &XArc, sp: &mut SpanData) {
    let w = f64::from(arc.width) / 2.0;
    let h = f64::from(arc.height) / 2.0;
    let r = f64::from(lw) / 2.0;
    let rs = r * r;
    let hs = h * h;
    let wh = w * w - hs;
    let mut nk = w * r;
    let vk = (nk * hs) / (wh + wh);
    let hf = hs * hs;
    nk = (hf - nk * nk) / wh;
    let fk = hf / wh;
    let hepp = h + EPSILON;
    let hepm = h - EPSILON;
    let mut k = h + f64::from((lw - 1) >> 1);
    let mut index = 0usize;
    let xorg = if arc.width & 1 != 0 { 0.5 } else { 0.0 };
    if sp.top {
        sp.spans[index].lx = 0;
        sp.spans[index].lw = 1;
        index += 1;
    }
    sp.count1 = 0;
    sp.count2 = 0;
    sp.hole = sp.top
        && i32::from(arc.height) * lw <= i32::from(arc.width) * i32::from(arc.width)
        && lw < i32::from(arc.height);
    let mut inx;
    let mut outx = 0.0;
    while k > 0.0 {
        let n = (k * k + nk) / 6.0;
        let nc = n * n * n;
        let vr = vk * k;
        let mut t = nc + vr * vr;
        let mut d = nc + t;
        let z;
        let flip;
        if d < 0.0 {
            d = nc;
            let mut b = n;
            if (b < 0.0) == (t < 0.0) {
                b = -b;
                d = -d;
            }
            z = n - 2.0 * b * ((-t / d).acos() / 3.0).cos();
            flip = if (z < 0.0) == (vr < 0.0) { 2 } else { 1 };
        } else {
            d = vr * d.sqrt();
            z = n + cbrt(t + d) + cbrt(t - d);
            flip = 0;
        }
        let a = ((z + z) - nk).sqrt();
        let tt = (fk - z) * k / a;
        inx = 0.0;
        let mut solution = false;
        let mut b = -a + k;
        d = b * b - 4.0 * (z + tt);
        if d >= 0.0 {
            d = d.sqrt();
            let mut y = (b + d) / 2.0;
            if y >= 0.0 && y < hepp {
                solution = true;
                if y > hepm {
                    y = h;
                }
                t = y / h;
                let x = w * (1.0 - (t * t)).sqrt();
                t = k - y;
                t = if rs - (t * t) >= 0.0 {
                    (rs - (t * t)).sqrt()
                } else {
                    0.0
                };
                if flip == 2 {
                    inx = x - t;
                } else {
                    outx = x + t;
                }
            }
        }
        b = a + k;
        d = b * b - 4.0 * (z - tt);
        // mi: near the axis precision can push d below zero where it should
        // not be; treated as zero.
        if d < 0.0 && !solution {
            d = 0.0;
        }
        if d >= 0.0 {
            d = d.sqrt();
            let mut y = (b + d) / 2.0;
            if y < hepp {
                if y > hepm {
                    y = h;
                }
                t = y / h;
                let x = w * (1.0 - (t * t)).sqrt();
                t = k - y;
                inx = if rs - (t * t) >= 0.0 {
                    x - (rs - (t * t)).sqrt()
                } else {
                    x
                };
            }
            y = (b - d) / 2.0;
            if y >= 0.0 {
                if y > hepm {
                    y = h;
                }
                t = y / h;
                let x = w * (1.0 - (t * t)).sqrt();
                t = k - y;
                t = if rs - (t * t) >= 0.0 {
                    (rs - (t * t)).sqrt()
                } else {
                    0.0
                };
                if flip == 1 {
                    inx = x - t;
                } else {
                    outx = x + t;
                }
            }
        }
        let span = &mut sp.spans[index];
        span.lx = iceil(xorg - outx);
        if inx <= 0.0 {
            sp.count1 += 1;
            span.lw = iceil(xorg + outx) - span.lx;
            span.rx = iceil(xorg + inx);
            span.rw = -iceil(xorg - inx);
        } else {
            sp.count2 += 1;
            span.lw = iceil(xorg - inx) - span.lx;
            span.rx = iceil(xorg + inx);
            span.rw = iceil(xorg + outx) - span.rx;
        }
        index += 1;
        k -= 1.0;
    }
    if sp.bot {
        outx = w + r;
        if r >= h && r <= w {
            inx = 0.0;
        } else if nk < 0.0 && -nk < hs {
            inx = w * (1.0 + nk / hs).sqrt() - (rs + nk).sqrt();
            if inx > w - r {
                inx = w - r;
            }
        } else {
            inx = w - r;
        }
        let span = &mut sp.spans[index];
        span.lx = iceil(xorg - outx);
        if inx <= 0.0 {
            span.lw = iceil(xorg + outx) - span.lx;
            span.rx = iceil(xorg + inx);
            span.rw = -iceil(xorg - inx);
        } else {
            span.lw = iceil(xorg - inx) - span.lx;
            span.rx = iceil(xorg + inx);
            span.rw = iceil(xorg + outx) - span.rx;
        }
    }
    if sp.hole {
        let at = usize::try_from(sp.count1).unwrap_or(0);
        let span = &mut sp.spans[at];
        span.lw = -span.lx;
        span.rx = 1;
        span.rw = span.lw;
        sp.count1 -= 1;
        sp.count2 += 1;
    }
}

/// `miComputeWideEllipse`
pub(super) fn wide_ellipse(lw: i32, arc: &XArc) -> SpanData {
    let lw = if lw == 0 { 1 } else { lw };
    let k = i32::from(arc.height >> 1) + ((lw - 1) >> 1);
    let mut sp = SpanData {
        spans: vec![ArcSpan::default(); usize::try_from(k + 2).unwrap_or(2)],
        k,
        top: lw & 1 == 0 && arc.width & 1 == 0,
        bot: arc.height & 1 == 0,
        ..SpanData::default()
    };
    if arc.width == arc.height {
        circle_spans(lw, arc, &mut sp);
    } else {
        ellipse_spans(lw, arc, &mut sp);
    }
    sp
}

/// `miFillWideEllipse`: a whole solid ellipse, its spans as `mi` hands them
/// to `FillSpans`, in order and not merged.
pub(super) fn fill_wide_ellipse(lw: i32, arc: &XArc, out: &mut Vec<XSpan>) {
    let sp = wide_ellipse(lw, arc);
    let mut push = |x: i32, y: i32, width: i32| {
        out.push(XSpan { x, y, width });
    };
    let mut index = 0usize;
    let xorg = i32::from(arc.x) + i32::from(arc.width >> 1);
    let mut yorgu = i32::from(arc.y) + i32::from(arc.height >> 1);
    let mut yorgl = yorgu + i32::from(arc.height & 1);
    yorgu -= sp.k;
    yorgl += sp.k;
    if sp.top {
        push(xorg, yorgu - 1, 1);
        index += 1;
    }
    for _ in 0..sp.count1.max(0) {
        let span = sp.spans[index];
        push(xorg + span.lx, yorgu, span.lw);
        push(xorg + span.lx, yorgl, span.lw);
        yorgu += 1;
        yorgl -= 1;
        index += 1;
    }
    if sp.hole {
        push(xorg, yorgl, 1);
    }
    for _ in 0..sp.count2.max(0) {
        let span = sp.spans[index];
        push(xorg + span.lx, yorgu, span.lw);
        push(xorg + span.rx, yorgu, span.rw);
        push(xorg + span.lx, yorgl, span.lw);
        push(xorg + span.rx, yorgl, span.rw);
        yorgu += 1;
        yorgl -= 1;
        index += 1;
    }
    if sp.bot {
        let span = sp.spans[index];
        push(xorg + span.lx, yorgu, span.lw);
        if span.rw > 0 {
            push(xorg + span.rx, yorgu, span.rw);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Bound {
    min: f64,
    max: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct IBound {
    min: i32,
    max: i32,
}

fn bounded(value: f64, bound: Bound) -> bool {
    bound.min <= value && value <= bound.max
}

fn ibounded(value: i32, bound: IBound) -> bool {
    bound.min <= value && value <= bound.max
}

#[derive(Clone, Copy, Debug, Default)]
struct Line {
    m: f64,
    b: f64,
    valid: bool,
}

impl Line {
    fn at(&self, y: f64) -> f64 {
        self.m * y + self.b
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ArcBound {
    ellipse: Bound,
    inner: Bound,
    outer: Bound,
    right: Bound,
    left: Bound,
    inneri: IBound,
    outeri: IBound,
}

#[derive(Clone, Copy, Debug, Default)]
struct Accelerators {
    tail_y: f64,
    h2: f64,
    w2: f64,
    h4: f64,
    w4: f64,
    h2mw2: f64,
    h2l: f64,
    w2l: f64,
    from_int_x: f64,
    from_int_y: f64,
    left: Line,
    right: Line,
    yorgu: i32,
    yorgl: i32,
    xorg: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ArcDef {
    w: f64,
    h: f64,
    l: f64,
    a0: f64,
    a1: f64,
}

/// `tailX`
fn tail_x(k: f64, def: &ArcDef, bounds: &ArcBound, acc: &Accelerators) -> f64 {
    let w = def.w;
    let h = def.h;
    let r = def.l;
    let rs = r * r;
    let hs = acc.h2;
    let wh = -acc.h2mw2;
    let mut nk = def.w * r;
    let vk = (nk * hs) / (wh + wh);
    let hf = acc.h4;
    nk = (hf - nk * nk) / wh;
    let mut xs = [0.0f64; 2];
    let pick = |xs: &[f64; 2]| {
        if acc.left.valid
            && bounded(k, bounds.left)
            && !bounded(k, bounds.outer)
            && xs[0] >= 0.0
            && xs[1] >= 0.0
        {
            return Some(xs[1]);
        }
        if acc.right.valid
            && bounded(k, bounds.right)
            && !bounded(k, bounds.inner)
            && xs[0] <= 0.0
            && xs[1] <= 0.0
        {
            return Some(xs[1]);
        }
        None
    };
    if k == 0.0 {
        if nk < 0.0 && -nk < hs {
            xs[0] = w * (1.0 + nk / hs).sqrt() - (rs + nk).sqrt();
            xs[1] = w - r;
            return pick(&xs).unwrap_or(xs[0]);
        }
        return w - r;
    }
    let fk = hf / wh;
    let hepp = h + EPSILON;
    let hepm = h - EPSILON;
    let n = (k * k + nk) / 6.0;
    let nc = n * n * n;
    let vr = vk * k;
    let mut count = 0usize;
    let mut push = |xs: &mut [f64; 2], value: f64| {
        if let Some(slot) = xs.get_mut(count) {
            *slot = value;
        }
        count += 1;
    };
    let mut t = nc + vr * vr;
    let mut d = nc + t;
    let z;
    let flip;
    if d < 0.0 {
        d = nc;
        let mut b = n;
        if (b < 0.0) == (t < 0.0) {
            b = -b;
            d = -d;
        }
        z = n - 2.0 * b * ((-t / d).acos() / 3.0).cos();
        flip = if (z < 0.0) == (vr < 0.0) { 2 } else { 1 };
    } else {
        d = vr * d.sqrt();
        z = n + cbrt(t + d) + cbrt(t - d);
        flip = 0;
    }
    let a = ((z + z) - nk).sqrt();
    let tt = (fk - z) * k / a;
    let mut solution = false;
    let mut b = -a + k;
    d = b * b - 4.0 * (z + tt);
    if d >= 0.0 && flip == 2 {
        d = d.sqrt();
        let mut y = (b + d) / 2.0;
        if y >= 0.0 && y < hepp {
            solution = true;
            if y > hepm {
                y = h;
            }
            t = y / h;
            let x = w * (1.0 - (t * t)).sqrt();
            t = k - y;
            t = if rs - (t * t) >= 0.0 {
                (rs - (t * t)).sqrt()
            } else {
                0.0
            };
            push(&mut xs, x - t);
        }
    }
    b = a + k;
    d = b * b - 4.0 * (z - tt);
    if d < 0.0 && !solution {
        d = 0.0;
    }
    if d >= 0.0 {
        d = d.sqrt();
        let mut y = (b + d) / 2.0;
        if y < hepp {
            if y > hepm {
                y = h;
            }
            t = y / h;
            let x = w * (1.0 - (t * t)).sqrt();
            t = k - y;
            let value = if rs - (t * t) >= 0.0 {
                x - (rs - (t * t)).sqrt()
            } else {
                x
            };
            push(&mut xs, value);
        }
        y = (b - d) / 2.0;
        if y >= 0.0 && flip == 1 {
            if y > hepm {
                y = h;
            }
            t = y / h;
            let x = w * (1.0 - (t * t)).sqrt();
            t = k - y;
            t = if rs - (t * t) >= 0.0 {
                (rs - (t * t)).sqrt()
            } else {
                0.0
            };
            push(&mut xs, x - t);
        }
    }
    if count > 1
        && let Some(x) = pick(&xs)
    {
        return x;
    }
    xs[0]
}

/// `computeAcc`, with `tailEllipseY`.
fn compute_acc(arc: &XArc, lw: i32) -> (ArcDef, Accelerators) {
    let def = ArcDef {
        w: f64::from(arc.width) / 2.0,
        h: f64::from(arc.height) / 2.0,
        l: f64::from(lw) / 2.0,
        a0: 0.0,
        a1: 0.0,
    };
    let mut acc = Accelerators {
        h2: def.h * def.h,
        w2: def.w * def.w,
        ..Accelerators::default()
    };
    acc.h4 = acc.h2 * acc.h2;
    acc.w4 = acc.w2 * acc.w2;
    acc.h2l = acc.h2 * def.l;
    acc.w2l = acc.w2 * def.l;
    acc.h2mw2 = acc.h2 - acc.w2;
    acc.from_int_x = if arc.width & 1 != 0 { 0.5 } else { 0.0 };
    acc.from_int_y = if arc.height & 1 != 0 { 0.5 } else { 0.0 };
    acc.xorg = i32::from(arc.x) + i32::from(arc.width >> 1);
    acc.yorgu = i32::from(arc.y) + i32::from(arc.height >> 1);
    acc.yorgl = acc.yorgu + i32::from(arc.height & 1);
    // tailEllipseY
    acc.tail_y = 0.0;
    if def.w != def.h {
        let mut t = def.l * def.w;
        let skip = if def.w > def.h {
            t < acc.h2
        } else {
            t > acc.h2
        };
        if !skip {
            t *= 2.0 * def.h;
            t = (CUBED_ROOT_4 * acc.h2 - cbrt(t * t)) / acc.h2mw2;
            if t > 0.0 {
                acc.tail_y = def.h / CUBED_ROOT_2 * t.sqrt();
            }
        }
    }
    (def, acc)
}

fn outer_x(x: f64, y: f64, acc: &Accelerators) -> f64 {
    x + (x * acc.h2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

fn outer_y(x: f64, y: f64, acc: &Accelerators) -> f64 {
    y + (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

fn inner_x(x: f64, y: f64, acc: &Accelerators) -> f64 {
    x - (x * acc.h2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

fn inner_y(x: f64, y: f64, acc: &Accelerators) -> f64 {
    y - (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

fn inner_y_from_y(y: f64, def: &ArcDef, acc: &Accelerators) -> f64 {
    let x = (def.w / def.h) * (acc.h2 - y * y).sqrt();
    y - (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

fn compute_line(x1: f64, y1: f64, x2: f64, y2: f64) -> Line {
    if y1 == y2 {
        Line::default()
    } else {
        let m = (x1 - x2) / (y1 - y2);
        Line {
            m,
            b: x1 - y1 * m,
            valid: true,
        }
    }
}

/// `Dsin`/`Dcos` of `miarc.c`'s quadrant code, in degrees.
fn dsin(d: f64) -> f64 {
    if d == 0.0 {
        0.0
    } else if d == 90.0 {
        1.0
    } else {
        (d * std::f64::consts::PI / 180.0).sin()
    }
}

fn dcos(d: f64) -> f64 {
    if d == 0.0 {
        1.0
    } else if d == 90.0 {
        0.0
    } else {
        (d * std::f64::consts::PI / 180.0).cos()
    }
}

/// `computeBound`: the y extents of the ellipse and its inner and outer
/// edges over this quadrant's angles, and the faces at its ends.
fn compute_bound(
    def: &ArcDef,
    acc: &mut Accelerators,
    faces: Option<&mut [Face; 2]>,
    right: Option<usize>,
    left: Option<usize>,
) -> ArcBound {
    let mut bound = ArcBound::default();
    bound.ellipse.min = dsin(def.a0) * def.h;
    bound.ellipse.max = dsin(def.a1) * def.h;
    let ellipse_x_min = if def.a0 == 45.0 && def.w == def.h {
        bound.ellipse.min
    } else {
        dcos(def.a0) * def.w
    };
    let ellipse_x_max = if def.a1 == 45.0 && def.w == def.h {
        bound.ellipse.max
    } else {
        dcos(def.a1) * def.w
    };
    bound.outer.min = outer_y(ellipse_x_min, bound.ellipse.min, acc);
    bound.outer.max = outer_y(ellipse_x_max, bound.ellipse.max, acc);
    bound.inner.min = inner_y(ellipse_x_min, bound.ellipse.min, acc);
    bound.inner.max = inner_y(ellipse_x_max, bound.ellipse.max, acc);
    let outer_x_min = outer_x(ellipse_x_min, bound.ellipse.min, acc);
    let outer_x_max = outer_x(ellipse_x_max, bound.ellipse.max, acc);
    let inner_x_min = inner_x(ellipse_x_min, bound.ellipse.min, acc);
    let inner_x_max = inner_x(ellipse_x_max, bound.ellipse.max, acc);
    // The faces keep cartesian coordinates here (y up); drawArc mirrors them.
    let mut faces = faces;
    if let (Some(pair), Some(right)) = (faces.as_deref_mut(), right) {
        let right = &mut pair[right];
        right.counter_clock = Spp {
            x: outer_x_min,
            y: bound.outer.min,
        };
        right.center = Spp {
            x: ellipse_x_min,
            y: bound.ellipse.min,
        };
        right.clock = Spp {
            x: inner_x_min,
            y: bound.inner.min,
        };
    }
    if let (Some(pair), Some(left)) = (faces, left) {
        let left = &mut pair[left];
        left.clock = Spp {
            x: outer_x_max,
            y: bound.outer.max,
        };
        left.center = Spp {
            x: ellipse_x_max,
            y: bound.ellipse.max,
        };
        left.counter_clock = Spp {
            x: inner_x_max,
            y: bound.inner.max,
        };
    }
    bound.left = Bound {
        min: bound.inner.max,
        max: bound.outer.max,
    };
    bound.right = Bound {
        min: bound.inner.min,
        max: bound.outer.min,
    };
    acc.right = compute_line(inner_x_min, bound.inner.min, outer_x_min, bound.outer.min);
    acc.left = compute_line(inner_x_max, bound.inner.max, outer_x_max, bound.outer.max);
    if bound.inner.min > bound.inner.max {
        std::mem::swap(&mut bound.inner.min, &mut bound.inner.max);
    }
    let tail_y = if acc.tail_y > bound.ellipse.max {
        bound.ellipse.max
    } else if acc.tail_y < bound.ellipse.min {
        bound.ellipse.min
    } else {
        acc.tail_y
    };
    let inner_tail_y = inner_y_from_y(tail_y, def, acc);
    if bound.inner.min > inner_tail_y {
        bound.inner.min = inner_tail_y;
    }
    if bound.inner.max < inner_tail_y {
        bound.inner.max = inner_tail_y;
    }
    bound.inneri = IBound {
        min: iceil(bound.inner.min - acc.from_int_y),
        max: (bound.inner.max - acc.from_int_y).floor() as i32,
    };
    bound.outeri = IBound {
        min: iceil(bound.outer.min - acc.from_int_y),
        max: (bound.outer.max - acc.from_int_y).floor() as i32,
    };
    bound
}

/// `hookEllipseY`
fn hook_ellipse_y(scan_y: f64, bound: &ArcBound, acc: &Accelerators, left: bool) -> f64 {
    if acc.h2mw2 == 0.0 {
        if (scan_y > 0.0 && !left) || (scan_y < 0.0 && left) {
            return bound.ellipse.min;
        }
        return bound.ellipse.max;
    }
    let ret = (acc.h4 * scan_y) / acc.h2mw2;
    if ret >= 0.0 { cbrt(ret) } else { -cbrt(-ret) }
}

/// `hookX`
fn hook_x(scan_y: f64, def: &ArcDef, bound: &ArcBound, acc: &Accelerators, left: bool) -> f64 {
    if def.w != def.h {
        let ellipse_y = hook_ellipse_y(scan_y, bound, acc, left);
        if bounded(ellipse_y, bound.ellipse) {
            let max_min = ellipse_y * ellipse_y * ellipse_y * acc.h2mw2
                - acc.h2 * scan_y * (3.0 * ellipse_y * ellipse_y - 2.0 * acc.h2);
            if (left && max_min > 0.0) || (!left && max_min < 0.0) {
                if ellipse_y == 0.0 {
                    return def.w + if left { -def.l } else { def.l };
                }
                return (acc.h2 * scan_y - ellipse_y * acc.h2mw2)
                    * (acc.h2 - ellipse_y * ellipse_y).sqrt()
                    / (def.h * def.w * ellipse_y);
            }
        }
    }
    if left {
        if acc.left.valid && bounded(scan_y, bound.left) {
            acc.left.at(scan_y)
        } else if acc.right.valid {
            acc.right.at(scan_y)
        } else {
            def.w - def.l
        }
    } else if acc.right.valid && bounded(scan_y, bound.right) {
        acc.right.at(scan_y)
    } else if acc.left.valid {
        acc.left.at(scan_y)
    } else {
        def.w - def.l
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
