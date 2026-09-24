//! The geometry `miArcSpan` walks, from `mi/miarc.c`: `computeAcc` and
//! `computeBound`, `tailX` and the hook routines, the edge lines and the
//! accelerators an arc's quadrant is drawn with.

use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Bound {
    pub(super) min: f64,
    pub(super) max: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct IBound {
    pub(super) min: i32,
    pub(super) max: i32,
}

pub(super) fn bounded(value: f64, bound: Bound) -> bool {
    bound.min <= value && value <= bound.max
}

pub(super) fn ibounded(value: i32, bound: IBound) -> bool {
    bound.min <= value && value <= bound.max
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Line {
    pub(super) m: f64,
    pub(super) b: f64,
    pub(super) valid: bool,
}

impl Line {
    pub(super) fn at(&self, y: f64) -> f64 {
        self.m * y + self.b
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ArcBound {
    pub(super) ellipse: Bound,
    pub(super) inner: Bound,
    pub(super) outer: Bound,
    pub(super) right: Bound,
    pub(super) left: Bound,
    pub(super) inneri: IBound,
    pub(super) outeri: IBound,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Accelerators {
    pub(super) tail_y: f64,
    pub(super) h2: f64,
    pub(super) w2: f64,
    pub(super) h4: f64,
    pub(super) w4: f64,
    pub(super) h2mw2: f64,
    pub(super) h2l: f64,
    pub(super) w2l: f64,
    pub(super) from_int_x: f64,
    pub(super) from_int_y: f64,
    pub(super) left: Line,
    pub(super) right: Line,
    pub(super) yorgu: i32,
    pub(super) yorgl: i32,
    pub(super) xorg: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ArcDef {
    pub(super) w: f64,
    pub(super) h: f64,
    pub(super) l: f64,
    pub(super) a0: f64,
    pub(super) a1: f64,
}

/// `tailX`
pub(super) fn tail_x(k: f64, def: &ArcDef, bounds: &ArcBound, acc: &Accelerators) -> f64 {
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
pub(super) fn compute_acc(arc: &XArc, lw: i32) -> (ArcDef, Accelerators) {
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

pub(super) fn outer_x(x: f64, y: f64, acc: &Accelerators) -> f64 {
    x + (x * acc.h2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

pub(super) fn outer_y(x: f64, y: f64, acc: &Accelerators) -> f64 {
    y + (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

pub(super) fn inner_x(x: f64, y: f64, acc: &Accelerators) -> f64 {
    x - (x * acc.h2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

pub(super) fn inner_y(x: f64, y: f64, acc: &Accelerators) -> f64 {
    y - (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

pub(super) fn inner_y_from_y(y: f64, def: &ArcDef, acc: &Accelerators) -> f64 {
    let x = (def.w / def.h) * (acc.h2 - y * y).sqrt();
    y - (y * acc.w2l) / (x * x * acc.h4 + y * y * acc.w4).sqrt()
}

pub(super) fn compute_line(x1: f64, y1: f64, x2: f64, y2: f64) -> Line {
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
pub(super) fn dsin(d: f64) -> f64 {
    if d == 0.0 {
        0.0
    } else if d == 90.0 {
        1.0
    } else {
        (d * std::f64::consts::PI / 180.0).sin()
    }
}

pub(super) fn dcos(d: f64) -> f64 {
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
pub(super) fn compute_bound(
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
pub(super) fn hook_ellipse_y(scan_y: f64, bound: &ArcBound, acc: &Accelerators, left: bool) -> f64 {
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
pub(super) fn hook_x(
    scan_y: f64,
    def: &ArcDef,
    bound: &ArcBound,
    acc: &Accelerators,
    left: bool,
) -> f64 {
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
