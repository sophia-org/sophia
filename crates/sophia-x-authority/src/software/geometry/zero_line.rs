//! Zero-width lines and arcs, as the X server's `mi` draws them.
//!
//! Copyright 1987, 1998 The Open Group and copyright 1987 by Digital
//! Equipment Corporation, Maynard, Massachusetts (`mizerline.c`); copyright
//! 1994, 1998 The Open Group (`miline.h`); copyright 1989, 1998 The Open
//! Group (`mizerarc.c`). The permission notices are kept in full in
//! `THIRD-PARTY-NOTICES.md`.
//!
//! The protocol leaves a zero-width line's pixels to the server, so this is
//! fidelity rather than conformance: the pixels the reference server draws,
//! and the ones `fb`'s fast paths draw too, since they share `mi`'s bias and
//! arc setup. `polyline` is `miZeroLine` (Ken Whaley) with the default
//! zero-line bias of `miscrinit.c`; `arcs` is `miZeroPolyArc`'s solid path,
//! `miZeroArcSetup` and `miZeroArcPts` (Bob Scheifler, after Pitteway's
//! plotter algorithm).
//!
//! Not ported: `mizerclip.c`, which only lets `mi` clip a line early while
//! keeping the pixels it would have drawn unclipped. The store clips pixel by
//! pixel, which keeps them by construction. Arcs `miCanZeroArc` refuses,
//! which `mi` hands to `miarc.c`, stay with the caller. Dashed lines and arcs
//! are in `dash`.

use crate::XPoint;

pub(super) mod dash;
mod tests;

/// `miline.h`'s octant bits.
const XDECREASING: u32 = 4;
const YDECREASING: u32 = 2;
const YMAJOR: u32 = 1;

/// `DEFAULTZEROLINEBIAS` from `miscrinit.c`: octants 2, 3, 4 and 5 round
/// the other way at a tie. Every `fb` screen keeps it.
const BIAS: u32 = (1 << (YDECREASING | YMAJOR))
    | (1 << (XDECREASING | YDECREASING | YMAJOR))
    | (1 << (XDECREASING | YDECREASING))
    | (1 << XDECREASING);

/// `miZeroLine` for points in `CoordModeOrigin`: the pixels of one
/// connected thin polyline, in drawing order. Each segment stops short of
/// its end, so a joint is drawn once; the polyline's last point is drawn
/// unless the cap style is NotLast, or the polyline closes on its start.
pub fn polyline(points: &[XPoint], cap_not_last: bool) -> Vec<(i32, i32)> {
    let mut pixels = Vec::new();
    let Some(first) = points.first() else {
        return pixels;
    };
    let (xstart, ystart) = (i32::from(first.x), i32::from(first.y));
    let (mut x2, mut y2) = (xstart, ystart);
    let (mut x, mut y) = (0, 0);
    for point in &points[1..] {
        let (x1, y1) = (x2, y2);
        x2 = i32::from(point.x);
        y2 = i32::from(point.y);
        // CalcLineDeltas
        let mut octant = 0;
        let (mut adx, mut ady) = (x2 - x1, y2 - y1);
        let (mut signdx, mut signdy) = (1, 1);
        if adx < 0 {
            adx = -adx;
            signdx = -1;
            octant |= XDECREASING;
        }
        if ady < 0 {
            ady = -ady;
            signdy = -1;
            octant |= YDECREASING;
        }
        x = x1;
        y = y1;
        if adx > ady {
            let e1 = ady << 1;
            let e2 = e1 - (adx << 1);
            let mut e = e1 - adx;
            // FIXUP_ERROR
            e -= ((BIAS >> octant) & 1) as i32;
            let e3 = e2 - e1;
            e -= e1;
            for _ in 0..adx {
                pixels.push((x, y));
                e += e1;
                if e >= 0 {
                    y += signdy;
                    e += e3;
                }
                x += signdx;
            }
        } else {
            let e1 = adx << 1;
            let e2 = e1 - (ady << 1);
            let mut e = e1 - ady;
            octant |= YMAJOR;
            e -= ((BIAS >> octant) & 1) as i32;
            let e3 = e2 - e1;
            e -= e1;
            for _ in 0..ady {
                pixels.push((x, y));
                e += e1;
                if e >= 0 {
                    x += signdx;
                    e += e3;
                }
                y += signdy;
            }
        }
    }
    // The last point, unless NotLast, or the path closes on its start --
    // though a single segment of no length is still a point.
    if !cap_not_last && ((xstart != x2 || ystart != y2) || points.len() == 2) {
        pixels.push((x, y));
    }
    pixels
}

const FULLCIRCLE: i32 = 360 * 64;
const OCTANT: i32 = 45 * 64;
const QUADRANT: i32 = 90 * 64;
const HALFCIRCLE: i32 = 180 * 64;
const QUADRANT3: i32 = 270 * 64;
const EPSILON45: i32 = 64;

/// `mizerarc.c`'s `Dsin`/`Dcos`, exact at the quadrants.
fn dsin(angle: i32) -> f64 {
    match angle {
        0 | HALFCIRCLE => 0.0,
        QUADRANT => 1.0,
        QUADRANT3 => -1.0,
        _ => (f64::from(angle) * (std::f64::consts::PI / 11_520.0)).sin(),
    }
}

fn dcos(angle: i32) -> f64 {
    match angle {
        0 => 1.0,
        QUADRANT | QUADRANT3 => 0.0,
        HALFCIRCLE => -1.0,
        _ => (f64::from(angle) * (std::f64::consts::PI / 11_520.0)).cos(),
    }
}

/// `miZeroArcPtRec`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ArcPoint {
    x: i32,
    y: i32,
    mask: i32,
}

const OOB: ArcPoint = ArcPoint {
    x: 65_536,
    y: 65_536,
    mask: 0,
};

/// `miZeroArcRec`
#[derive(Clone, Copy, Debug)]
struct ZeroArc {
    x: i32,
    y: i32,
    k1: i32,
    k3: i32,
    a: i32,
    b: i32,
    d: i32,
    dx: i32,
    dy: i32,
    xorg: i32,
    yorg: i32,
    xorgo: i32,
    yorgo: i32,
    w: i32,
    h: i32,
    initial_mask: i32,
    start: ArcPoint,
    altstart: ArcPoint,
    end: ArcPoint,
    altend: ArcPoint,
    /// The arc's normalised angles and its first endpoint as `setup` found
    /// it, which only the dashed walk reads.
    start_angle: i32,
    end_angle: i32,
    first_x: i32,
    first_y: i32,
}

/// `miCanZeroArc`: whether the zero-width arc walker can take an arc without
/// overflowing.
pub fn can_zero_arc(arc: &super::arc::XArc) -> bool {
    arc.width == arc.height || (arc.width <= 800 && arc.height <= 800)
}

/// `miZeroArcSetup`; the flag is whether a full circle may take the
/// unmasked path.
fn setup(arc: &super::arc::XArc, ok360: bool) -> (ZeroArc, bool) {
    let width = i32::from(arc.width);
    let height = i32::from(arc.height);
    let l = width & 1;
    let (mut k1, mut k3, mut a, mut b, mut d);
    if arc.width == arc.height {
        k1 = -8;
        k3 = -16;
        b = 12;
        a = (width << 2) - 12;
        d = 17 - (width << 1);
        if l != 0 {
            b -= 4;
            a += 4;
            d -= 7;
        }
    } else if width == 0 || height == 0 {
        k1 = 0;
        k3 = 0;
        a = -height;
        b = 0;
        d = -1;
    } else {
        // initial conditions
        let alpha = (width * width) << 2;
        let beta = (height * height) << 2;
        k1 = beta << 1;
        k3 = k1 + (alpha << 1);
        b = if l != 0 { 0 } else { -beta };
        a = alpha * height;
        d = b - (a >> 1) - (alpha >> 2);
        if l != 0 {
            d -= beta >> 2;
        }
        a -= b;
        // take first step, d < 0 always
        b -= k1;
        a += k1;
        d += b;
        // octant change, b < 0 always
        k1 = -k1;
        k3 = -k3;
        b = -b;
        d = b - a - d;
        a -= b << 1;
    }
    let xorg = i32::from(arc.x) + (width >> 1);
    let yorg = i32::from(arc.y);
    let mut info = ZeroArc {
        x: 1,
        y: 0,
        k1,
        k3,
        a,
        b,
        d,
        dx: 1,
        dy: 0,
        xorg,
        yorg,
        xorgo: xorg + l,
        yorgo: yorg + height,
        w: (width + 1) >> 1,
        h: height >> 1,
        initial_mask: 0,
        start: OOB,
        altstart: OOB,
        end: OOB,
        altend: OOB,
        start_angle: 0,
        end_angle: 0,
        first_x: 0,
        first_y: 0,
    };
    if width == 0 {
        if height == 0 {
            info.x = 0;
            info.y = 0;
            return (info, false);
        }
        info.x = 0;
        info.y = 1;
    }
    let angle1 = i32::from(arc.angle1);
    let mut angle2 = i32::from(arc.angle2);
    let (mut start_angle, mut end_angle);
    if angle1 == 0 && angle2 >= FULLCIRCLE {
        start_angle = 0;
        end_angle = 0;
    } else {
        angle2 = angle2.clamp(-FULLCIRCLE, FULLCIRCLE);
        if angle2 < 0 {
            start_angle = angle1 + angle2;
            end_angle = angle1;
        } else {
            start_angle = angle1;
            end_angle = angle1 + angle2;
        }
        if start_angle < 0 {
            start_angle = FULLCIRCLE - (-start_angle) % FULLCIRCLE;
        }
        if start_angle >= FULLCIRCLE {
            start_angle %= FULLCIRCLE;
        }
        if end_angle < 0 {
            end_angle = FULLCIRCLE - (-end_angle) % FULLCIRCLE;
        }
        if end_angle >= FULLCIRCLE {
            end_angle %= FULLCIRCLE;
        }
    }
    info.start_angle = start_angle;
    info.end_angle = end_angle;
    if ok360 && start_angle == end_angle && arc.angle2 != 0 && width != 0 && height != 0 {
        info.initial_mask = 0xf;
        return (info, true);
    }
    // An endpoint is found by x on one side of 45 degrees and by y on the
    // other. Doubles assigned to ints truncate, as in C.
    let h = info.h;
    let endpoint = |angle: i32| -> ArcPoint {
        let segment = angle / OCTANT;
        if height == 0 || (((segment + 1) & 2) != 0 && width != 0) {
            let x = (dcos(angle) * (f64::from(width + 1) / 2.0)) as i32;
            ArcPoint {
                x: x.abs(),
                y: -1,
                mask: 0,
            }
        } else {
            let y = (dsin(angle) * (f64::from(height) / 2.0)) as i32;
            ArcPoint {
                x: 65_536,
                y: h - y.abs(),
                mask: 0,
            }
        }
    };
    let mut start = endpoint(start_angle);
    let mut end = endpoint(end_angle);
    info.first_x = start.x;
    info.first_y = start.y;
    let mut startseg = start_angle / OCTANT;
    let mut endseg = end_angle / OCTANT;
    info.initial_mask = 0;
    let mut overlap = arc.angle2 != 0 && end_angle <= start_angle;
    for i in 0..4 {
        let covered = if overlap {
            i * QUADRANT <= end_angle || (i + 1) * QUADRANT > start_angle
        } else {
            i * QUADRANT <= end_angle && (i + 1) * QUADRANT > start_angle
        };
        if covered {
            info.initial_mask |= 1 << i;
        }
    }
    start.mask = info.initial_mask;
    end.mask = info.initial_mask;
    startseg >>= 1;
    endseg >>= 1;
    overlap = overlap && endseg == startseg;
    if start.x != end.x || start.y != end.y || !overlap {
        if startseg & 1 != 0 {
            if !overlap {
                info.initial_mask &= !(1 << startseg);
            }
            if start.x > end.x || start.y > end.y {
                end.mask &= !(1 << startseg);
            }
        } else {
            start.mask &= !(1 << startseg);
            if ((start.x < end.x || start.y < end.y)
                || (start.x == end.x && start.y == end.y && (endseg & 1) != 0))
                && !overlap
            {
                end.mask &= !(1 << startseg);
            }
        }
        if endseg & 1 != 0 {
            end.mask &= !(1 << endseg);
            if ((start.x > end.x || start.y > end.y)
                || (start.x == end.x && start.y == end.y && (startseg & 1) == 0))
                && !overlap
            {
                start.mask &= !(1 << endseg);
            }
        } else {
            if !overlap {
                info.initial_mask &= !(1 << endseg);
            }
            if start.x < end.x || start.y < end.y {
                start.mask &= !(1 << endseg);
            }
        }
    }
    // take care of case when start and stop are both near 45
    if start_angle != 0 && ((start.y < 0 && end.y >= 0) || (start.y >= 0 && end.y < 0)) {
        let near = |angle: i32| {
            let i = (angle + OCTANT) % OCTANT;
            !(EPSILON45..=OCTANT - EPSILON45).contains(&i)
        };
        if near(start_angle) && near(end_angle) {
            if start.y < 0 {
                let i = ((dsin(start_angle) * (f64::from(height) / 2.0)) as i32).abs();
                if info.h - i == end.y {
                    start.mask = end.mask;
                }
            } else {
                let i = ((dsin(end_angle) * (f64::from(height) / 2.0)) as i32).abs();
                if info.h - i == start.y {
                    end.mask = start.mask;
                }
            }
        }
    }
    if startseg & 1 != 0 {
        info.start = start;
        info.end = OOB;
    } else {
        info.end = start;
        info.start = OOB;
    }
    if endseg & 1 != 0 {
        info.altend = end;
        if info.altend.x < info.end.x || info.altend.y < info.end.y {
            std::mem::swap(&mut info.altend, &mut info.end);
        }
        info.altstart = OOB;
    } else {
        info.altstart = end;
        if info.altstart.x < info.start.x || info.altstart.y < info.start.y {
            std::mem::swap(&mut info.altstart, &mut info.start);
        }
        info.altend = OOB;
    }
    if info.start.x == 0 || info.start.y == 0 {
        info.initial_mask = info.start.mask;
        info.start = info.altstart;
    }
    if width == 0 && height == 1 {
        // mi's "kludge!"
        info.initial_mask |= info.end.mask;
        info.initial_mask |= info.initial_mask << 1;
        info.end.x = 0;
        info.end.mask = 0;
    }
    (info, false)
}

/// The walker's registers, as `MIARCSETUP` loads them.
struct Walker {
    x: i32,
    y: i32,
    k1: i32,
    k3: i32,
    a: i32,
    b: i32,
    d: i32,
    dx: i32,
    dy: i32,
}

impl Walker {
    fn new(info: &ZeroArc) -> Self {
        Self {
            x: info.x,
            y: info.y,
            k1: info.k1,
            k3: info.k3,
            a: info.a,
            b: info.b,
            d: info.d,
            dx: info.dx,
            dy: info.dy,
        }
    }

    /// `MIARCOCTANTSHIFT`
    fn octant_shift(&mut self, h: i32) {
        if self.a < 0 {
            if self.y == h {
                self.d = -1;
                self.a = 0;
                self.b = 0;
                self.k1 = 0;
            } else {
                self.dx = (self.k1 << 1) - self.k3;
                self.k1 = self.dx - self.k1;
                self.k3 = -self.k3;
                self.b = self.b + self.a - (self.k1 >> 1);
                self.d = self.b + ((-self.a) >> 1) - self.d + (self.k3 >> 3);
                self.a = if self.dx < 0 {
                    -((-self.dx) >> 1) - self.a
                } else {
                    (self.dx >> 1) - self.a
                };
                self.dx = 0;
                self.dy = 1;
            }
        }
    }

    /// `MIARCSTEP`
    fn step(&mut self) {
        self.b -= self.k1;
        if self.d < 0 {
            self.x += self.dx;
            self.y += self.dy;
            self.a += self.k1;
            self.d += self.b;
        } else {
            self.x += 1;
            self.y += 1;
            self.a += self.k3;
            self.d -= self.a;
        }
    }

    /// `MIARCCIRCLESTEP`
    fn circle_step(&mut self) {
        self.b -= self.k1;
        self.x += 1;
        if self.d < 0 {
            self.a += self.k1;
            self.d += self.b;
        } else {
            self.y += 1;
            self.a += self.k3;
            self.d -= self.a;
        }
    }
}

/// `miZeroArcPts`: one solid zero-width arc's pixels, in `mi`'s order.
fn arc_points(arc: &super::arc::XArc, out: &mut Vec<(i32, i32)>) {
    let (mut info, do360) = setup(arc, true);
    let mut walk = Walker::new(&info);
    let mut mask = info.initial_mask;
    let pix = |out: &mut Vec<(i32, i32)>, mask: i32, index: i32, x: i32, y: i32| {
        if mask & (1 << index) != 0 {
            out.push((x, y));
        }
    };
    if arc.width & 1 == 0 {
        pix(out, mask, 1, info.xorgo, info.yorg);
        pix(out, mask, 3, info.xorgo, info.yorgo);
    }
    if info.end.x == 0 || info.end.y == 0 {
        mask = info.end.mask;
        info.end = info.altend;
    }
    if do360 && arc.width == arc.height && arc.width & 1 == 0 {
        let yorgh = info.yorg + info.h;
        let xorghp = info.xorg + info.h;
        let xorghn = info.xorg - info.h;
        let begin = out.len();
        loop {
            let (x, y) = (walk.x, walk.y);
            out.push((info.xorg + x, info.yorg + y));
            out.push((info.xorg - x, info.yorg + y));
            out.push((info.xorg - x, info.yorgo - y));
            out.push((info.xorg + x, info.yorgo - y));
            if walk.a < 0 {
                break;
            }
            out.push((xorghp - y, yorgh - x));
            out.push((xorghn + y, yorgh - x));
            out.push((xorghn + y, yorgh + x));
            out.push((xorghp - y, yorgh + x));
            walk.circle_step();
        }
        // The last octant's four points may repeat the previous four.
        let n = out.len();
        if walk.x > 1 && n >= begin + 5 && out[n - 1] == out[n - 5] {
            out.truncate(n - 4);
        }
        walk.x = info.w;
        walk.y = info.h;
    } else if do360 {
        while walk.y < info.h || walk.x < info.w {
            walk.octant_shift(info.h);
            let (x, y) = (walk.x, walk.y);
            out.push((info.xorg + x, info.yorg + y));
            out.push((info.xorgo - x, info.yorg + y));
            out.push((info.xorgo - x, info.yorgo - y));
            out.push((info.xorg + x, info.yorgo - y));
            walk.step();
        }
    } else {
        while walk.y < info.h || walk.x < info.w {
            walk.octant_shift(info.h);
            let (x, y) = (walk.x, walk.y);
            if x == info.start.x || y == info.start.y {
                mask = info.start.mask;
                info.start = info.altstart;
            }
            pix(out, mask, 0, info.xorg + x, info.yorg + y);
            pix(out, mask, 1, info.xorgo - x, info.yorg + y);
            pix(out, mask, 2, info.xorgo - x, info.yorgo - y);
            pix(out, mask, 3, info.xorg + x, info.yorgo - y);
            if x == info.end.x || y == info.end.y {
                mask = info.end.mask;
                info.end = info.altend;
            }
            walk.step();
        }
    }
    let (x, y) = (walk.x, walk.y);
    if x == info.start.x || y == info.start.y {
        mask = info.start.mask;
    }
    pix(out, mask, 0, info.xorg + x, info.yorg + y);
    pix(out, mask, 2, info.xorgo - x, info.yorgo - y);
    if arc.height & 1 == 1 {
        pix(out, mask, 1, info.xorgo - x, info.yorg + y);
        pix(out, mask, 3, info.xorg + x, info.yorgo - y);
    }
}

/// `miZeroPolyArc`'s solid path: every arc `miCanZeroArc` admits, as the
/// pixels `PolyPoint` would be given, in order.
pub fn arcs(arcs: &[super::arc::XArc]) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for arc in arcs.iter().filter(|arc| can_zero_arc(arc)) {
        arc_points(arc, &mut out);
    }
    out
}
