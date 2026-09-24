//! `miComputeWideEllipse` and `miFillWideEllipse` from `mi/miarc.c`, with
//! `MIWIDEARCSETUP` and the circle and ellipse span tables: a full wide
//! ellipse as spans, without the arc machinery.

use super::*;

/// `MIWIDEARCSETUP`
pub(super) struct WideArcWalk {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) e: i32,
    pub(super) xk: i32,
    pub(super) xm: i32,
    pub(super) yk: i32,
    pub(super) ym: i32,
}

pub(super) fn wide_arc_setup(dy: i32, slw: i32) -> WideArcWalk {
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
    pub(super) fn step(&mut self, dx: i32) -> i32 {
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
pub(super) fn circle_spans(lw: i32, arc: &XArc, sp: &mut SpanData) {
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
pub(super) fn ellipse_spans(lw: i32, arc: &XArc, sp: &mut SpanData) {
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
pub(in super::super) fn wide_ellipse(lw: i32, arc: &XArc) -> SpanData {
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
pub(in super::super) fn fill_wide_ellipse(lw: i32, arc: &XArc, out: &mut Vec<XSpan>) {
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
