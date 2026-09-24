//! Filled arcs, as spans: `PolyFillArc` as the X server's `mi` draws it.
//!
//! Copyright 1989, 1998 The Open Group. The permission notice is kept in
//! full in `THIRD-PARTY-NOTICES.md`.
//!
//! A port of `mi/mifillarc.c` and `mi/mifillarc.h` (Bob Scheifler, MIT X
//! Consortium). The protocol defines a filled arc's pixels exactly: those
//! whose centres lie inside the ellipse and, for a partial arc, inside the
//! chord or pie slice. `mi` walks the ellipse with an integer error term and
//! clips each row to the slice's edges; this follows it routine by routine,
//! so a filled arc covers the pixels the reference server covers. `mi` keeps
//! an integer walker for arcs small enough not to overflow it and a
//! floating-point one for the rest; here one walker serves both, over `i64`
//! and `f64`, and the choice between them is `mi`'s.
//!
//! This replaces filling the polygon a chord-stepped arc approximation
//! encloses, which disagreed with `mi` on single pixels along the rim.

mod tests;

use super::arc::XArc;
use sophia_protocol::Rect;
use std::ops::{AddAssign, SubAssign};

const FULLCIRCLE: i32 = 360 * 64;
const QUADRANT: i32 = 90 * 64;
const HALFCIRCLE: i32 = 180 * 64;
const QUADRANT3: i32 = 270 * 64;

/// `Dcos`/`Dsin`: an angle in 64ths of a degree, in the same expression
/// order as `mi`'s macros.
fn dcos(angle: i32) -> f64 {
    (f64::from(angle) * (std::f64::consts::PI / 11_520.0)).cos()
}

fn dsin(angle: i32) -> f64 {
    (f64::from(angle) * (std::f64::consts::PI / 11_520.0)).sin()
}

/// What the ellipse walker counts in: `int` or `double` in `mi`.
trait Term: Copy + PartialOrd + AddAssign + SubAssign + From<i32> {}
impl Term for i64 {}
impl Term for f64 {}

/// `miFillArcRec` / `miFillArcDRec`, with the walker's running `x`.
struct Walk<T> {
    xorg: i32,
    yorg: i32,
    y: i32,
    x: i32,
    dx: i32,
    dy: i32,
    e: T,
    ym: T,
    yk: T,
    xm: T,
    xk: T,
}

/// `miFillArcSetup`, for arcs `miCanFillArc` admits.
fn setup_integer(arc: &XArc) -> Walk<i64> {
    let (width, height) = (i64::from(arc.width), i64::from(arc.height));
    let mut y = i32::from(arc.height >> 1);
    let dy = i32::from(arc.height & 1);
    let yorg = i32::from(arc.y) + y;
    let odd = i32::from(arc.width & 1);
    let xorg = i32::from(arc.x) + i32::from(arc.width >> 1) + odd;
    let dx = 1 - odd;
    let (ym, xm, mut yk, xk, e);
    if arc.width == arc.height {
        // (2x - 2xorg)^2 = d^2 - (2y - 2yorg)^2
        ym = 8;
        xm = 8;
        yk = i64::from(y) << 3;
        if dx == 0 {
            xk = 0;
            e = -1;
        } else {
            y += 1;
            yk += 4;
            xk = -4;
            e = -(i64::from(y) << 3);
        }
    } else {
        // h^2 * (2x - 2xorg)^2 = w^2 * h^2 - w^2 * (2y - 2yorg)^2
        ym = (width * width) << 3;
        xm = (height * height) << 3;
        yk = i64::from(y) * ym;
        if dy == 0 {
            yk -= ym >> 1;
        }
        if dx == 0 {
            xk = 0;
            e = -(xm >> 3);
        } else {
            y += 1;
            yk += ym;
            xk = -(xm >> 1);
            e = xk - yk;
        }
    }
    Walk {
        xorg,
        yorg,
        y,
        x: 0,
        dx,
        dy,
        e,
        ym,
        yk,
        xm,
        xk,
    }
}

/// `miFillArcDSetup`, for the arcs too large for the integer walker.
fn setup_double(arc: &XArc) -> Walk<f64> {
    let mut y = i32::from(arc.height >> 1);
    let dy = i32::from(arc.height & 1);
    let yorg = i32::from(arc.y) + y;
    let odd = i32::from(arc.width & 1);
    let xorg = i32::from(arc.x) + i32::from(arc.width >> 1) + odd;
    let dx = 1 - odd;
    let ym = f64::from(arc.width) * f64::from(i32::from(arc.width) * 8);
    let xm = f64::from(arc.height) * f64::from(i32::from(arc.height) * 8);
    let mut yk = f64::from(y) * ym;
    if dy == 0 {
        yk -= ym / 2.0;
    }
    let (xk, e);
    if dx == 0 {
        xk = 0.0;
        e = -(xm / 8.0);
    } else {
        y += 1;
        yk += ym;
        xk = -xm / 2.0;
        e = xk - yk;
    }
    Walk {
        xorg,
        yorg,
        y,
        x: 0,
        dx,
        dy,
        e,
        ym,
        yk,
        xm,
        xk,
    }
}

impl<T: Term> Walk<T> {
    /// `MIFILLARCSTEP`: one row inward, and that row's span width.
    fn step(&mut self) -> i32 {
        self.e += self.yk;
        while self.e >= T::from(0) {
            self.x += 1;
            self.xk -= self.xm;
            self.e += self.xk;
        }
        self.y -= 1;
        self.yk -= self.ym;
        let mut slw = (self.x << 1) + self.dx;
        if self.e == self.xk && slw > 1 {
            slw -= 1;
        }
        slw
    }

    /// `miFillArcLower`: whether the row mirrored below the centre is its
    /// own row and has width.
    fn lower(&self, slw: i32) -> bool {
        (self.y + self.dy) != 0 && (slw > 1 || self.e != self.xk)
    }
}

/// `miSliceEdgeRec`
#[derive(Clone, Copy, Debug, Default)]
struct SliceEdge {
    x: i32,
    stepx: i32,
    deltax: i32,
    e: i32,
    dy: i32,
    dx: i32,
}

impl SliceEdge {
    /// `MIARCSLICESTEP`
    fn step(&mut self) {
        self.x -= self.stepx;
        self.e -= self.dx;
        if self.e <= 0 {
            self.x -= self.deltax;
            self.e += self.dy;
        }
    }
}

/// `miArcSliceRec`
#[derive(Clone, Copy, Debug, Default)]
struct Slice {
    edge1: SliceEdge,
    edge2: SliceEdge,
    min_top_y: i32,
    max_top_y: i32,
    min_bot_y: i32,
    max_bot_y: i32,
    edge1_top: bool,
    edge2_top: bool,
    flip_top: bool,
    flip_bot: bool,
}

/// `miGetArcEdge`
fn arc_edge(arc: &XArc, edge: &mut SliceEdge, k: i32, top: bool, left: bool) {
    let mut y = i32::from(arc.height >> 1);
    if arc.width & 1 == 0 {
        y += 1;
    }
    if !top {
        y = -y;
        if arc.height & 1 == 1 {
            y -= 1;
        }
    }
    let xady = k + y * edge.dx;
    edge.x = if xady <= 0 {
        -((-xady) / edge.dy + 1)
    } else {
        (xady - 1) / edge.dy
    };
    edge.e = xady - edge.x * edge.dy;
    if (top && edge.dx < 0) || (!top && edge.dx > 0) {
        edge.e = edge.dy - edge.e + 1;
    }
    if left {
        edge.x += 1;
    }
    edge.x += i32::from(arc.x) + i32::from(arc.width >> 1);
    if edge.dx > 0 {
        edge.deltax = 1;
        edge.stepx = edge.dx / edge.dy;
        edge.dx %= edge.dy;
    } else {
        edge.deltax = -1;
        edge.stepx = -((-edge.dx) / edge.dy);
        edge.dx = (-edge.dx) % edge.dy;
    }
    if !top {
        edge.deltax = -edge.deltax;
        edge.stepx = -edge.stepx;
    }
}

/// `miEllipseAngleToSlope`, without the unused real-valued outputs.
fn angle_to_slope(angle: i32, width: u16, height: u16) -> (i32, i32) {
    match angle {
        0 => (-1, 0),
        QUADRANT => (0, 1),
        HALFCIRCLE => (1, 0),
        QUADRANT3 => (0, -1),
        _ => {
            let mut d_dx = dcos(angle) * f64::from(width);
            let mut d_dy = dsin(angle) * f64::from(height);
            let negative_dx = d_dx < 0.0;
            if negative_dx {
                d_dx = -d_dx;
            }
            let negative_dy = d_dy < 0.0;
            if negative_dy {
                d_dy = -d_dy;
            }
            let scale = if d_dy > d_dx { d_dy } else { d_dx };
            let mut dx = ((d_dx * 32_768.0) / scale + 0.5).floor() as i32;
            if negative_dx {
                dx = -dx;
            }
            let mut dy = ((d_dy * 32_768.0) / scale + 0.5).floor() as i32;
            if negative_dy {
                dy = -dy;
            }
            (dx, dy)
        }
    }
}

/// `miGetPieEdge`
fn pie_edge(arc: &XArc, angle: i32, edge: &mut SliceEdge, top: bool, left: bool) {
    let (mut dx, mut dy) = angle_to_slope(angle, arc.width, arc.height);
    if dy == 0 {
        edge.x = if left { -65_536 } else { 65_536 };
        edge.stepx = 0;
        edge.e = 0;
        edge.dx = -1;
        return;
    }
    if dx == 0 {
        edge.x = i32::from(arc.x) + i32::from(arc.width >> 1);
        if left && arc.width & 1 == 1 {
            edge.x += 1;
        } else if !left && arc.width & 1 == 0 {
            edge.x -= 1;
        }
        edge.stepx = 0;
        edge.e = 0;
        edge.dx = -1;
        return;
    }
    if dy < 0 {
        dx = -dx;
        dy = -dy;
    }
    let mut k = if arc.height & 1 == 1 { dx } else { 0 };
    if arc.width & 1 == 1 {
        k += dy;
    }
    edge.dx = dx << 1;
    edge.dy = dy << 1;
    arc_edge(arc, edge, k, top, left);
}

/// `miFillArcSliceSetup`
fn slice_setup(arc: &XArc, pie_slice: bool) -> Slice {
    let mut angle1 = i32::from(arc.angle1);
    let mut angle2;
    if arc.angle2 < 0 {
        angle2 = angle1;
        angle1 += i32::from(arc.angle2);
    } else {
        angle2 = angle1 + i32::from(arc.angle2);
    }
    angle1 = angle1.rem_euclid(FULLCIRCLE);
    angle2 = angle2.rem_euclid(FULLCIRCLE);
    let height = i32::from(arc.height);
    let mut slice = Slice {
        min_top_y: 0,
        max_top_y: height >> 1,
        min_bot_y: 1 - (height & 1),
        ..Slice::default()
    };
    slice.max_bot_y = slice.max_top_y - 1;
    if pie_slice {
        slice.edge1_top = angle1 < HALFCIRCLE;
        slice.edge2_top = angle2 <= HALFCIRCLE;
        if angle2 == 0 || angle1 == HALFCIRCLE {
            let top = if angle2 != 0 {
                slice.edge2_top
            } else {
                slice.edge1_top
            };
            slice.min_top_y = if top { slice.min_bot_y } else { height };
            slice.min_bot_y = 0;
        } else if angle1 == 0 || angle2 == HALFCIRCLE {
            slice.min_top_y = slice.min_bot_y;
            let top = if angle1 != 0 {
                slice.edge1_top
            } else {
                slice.edge2_top
            };
            slice.min_bot_y = if top { height } else { 0 };
        } else if slice.edge1_top == slice.edge2_top {
            if angle2 < angle1 {
                slice.flip_top = slice.edge1_top;
                slice.flip_bot = !slice.edge1_top;
            } else if slice.edge1_top {
                slice.min_top_y = 1;
                slice.min_bot_y = height;
            } else {
                slice.min_bot_y = 0;
                slice.min_top_y = height;
            }
        }
        let (edge1_top, edge2_top) = (slice.edge1_top, slice.edge2_top);
        pie_edge(arc, angle1, &mut slice.edge1, edge1_top, !edge1_top);
        pie_edge(arc, angle2, &mut slice.edge2, edge2_top, edge2_top);
        return slice;
    }

    // A chord: both edges lie on the line through the arc's ends.
    let w2 = f64::from(arc.width) / 2.0;
    let h2 = f64::from(arc.height) / 2.0;
    let end = |angle: i32| -> (f64, f64, bool) {
        if angle == 0 || angle == HALFCIRCLE {
            (if angle != 0 { -w2 } else { w2 }, 0.0, true)
        } else if angle == QUADRANT || angle == QUADRANT3 {
            (0.0, if angle == QUADRANT { h2 } else { -h2 }, true)
        } else {
            (dcos(angle) * w2, dsin(angle) * h2, false)
        }
    };
    let (mut x1, mut y1, is_int1) = end(angle1);
    let (mut x2, mut y2, is_int2) = end(angle2);
    let mut dx = x2 - x1;
    let mut dy = y2 - y1;
    if height & 1 == 1 {
        y1 -= 0.5;
        y2 -= 0.5;
    }
    if arc.width & 1 == 1 {
        x1 += 0.5;
        x2 += 0.5;
    }
    let signdy = if dy < 0.0 {
        dy = -dy;
        -1
    } else {
        1
    };
    let signdx = if dx < 0.0 {
        dx = -dx;
        -1
    } else {
        1
    };
    // Doubles assigned to ints: truncated, as C converts them.
    if is_int1 && is_int2 {
        slice.edge1.dx = (dx * 2.0) as i32;
        slice.edge1.dy = (dy * 2.0) as i32;
    } else {
        let scale = if dx > dy { dx } else { dy };
        slice.edge1.dx = ((dx * 32_768.0) / scale + 0.5).floor() as i32;
        slice.edge1.dy = ((dy * 32_768.0) / scale + 0.5).floor() as i32;
    }
    if slice.edge1.dy == 0 {
        if signdx < 0 {
            let y = (y1 + 1.0).floor() as i32;
            if y >= 0 {
                slice.min_top_y = y;
                slice.min_bot_y = height;
            } else {
                slice.max_bot_y = -y - (height & 1);
            }
        } else {
            let y = y1.floor() as i32;
            if y >= 0 {
                slice.max_top_y = y;
            } else {
                slice.min_top_y = height;
                slice.min_bot_y = -y - (height & 1);
            }
        }
        slice.edge1_top = true;
        slice.edge1.x = 65_536;
        slice.edge1.stepx = 0;
        slice.edge1.e = 0;
        slice.edge1.dx = -1;
        slice.edge2 = slice.edge1;
        slice.edge2_top = false;
    } else if slice.edge1.dx == 0 {
        if signdy < 0 {
            x1 -= 1.0;
        }
        slice.edge1.x = x1.ceil() as i32;
        slice.edge1_top = signdy < 0;
        slice.edge1.x += i32::from(arc.x) + i32::from(arc.width >> 1);
        slice.edge1.stepx = 0;
        slice.edge1.e = 0;
        slice.edge1.dx = -1;
        slice.edge2_top = !slice.edge1_top;
        slice.edge2 = slice.edge1;
    } else {
        if signdx < 0 {
            slice.edge1.dx = -slice.edge1.dx;
        }
        if signdy < 0 {
            slice.edge1.dx = -slice.edge1.dx;
        }
        let k = (((x1 + x2) * f64::from(slice.edge1.dy) - (y1 + y2) * f64::from(slice.edge1.dx))
            / 2.0)
            .ceil() as i32;
        slice.edge2.dx = slice.edge1.dx;
        slice.edge2.dy = slice.edge1.dy;
        slice.edge1_top = signdy < 0;
        slice.edge2_top = !slice.edge1_top;
        let (edge1_top, edge2_top) = (slice.edge1_top, slice.edge2_top);
        arc_edge(arc, &mut slice.edge1, k, edge1_top, !edge1_top);
        arc_edge(arc, &mut slice.edge2, k, edge2_top, edge2_top);
    }
    slice
}

fn push(spans: &mut Vec<Rect>, x: i32, y: i32, width: i32) {
    if width > 0 {
        spans.push(Rect {
            x,
            y,
            width,
            height: 1,
        });
    }
}

/// `miFillEllipseI` / `miFillEllipseD`: a whole ellipse.
fn ellipse<T: Term>(mut walk: Walk<T>, spans: &mut Vec<Rect>) {
    while walk.y > 0 {
        let slw = walk.step();
        push(spans, walk.xorg - walk.x, walk.yorg - walk.y, slw);
        if walk.lower(slw) {
            push(spans, walk.xorg - walk.x, walk.yorg + walk.y + walk.dy, slw);
        }
    }
}

/// `ADDSLICESPANS`: a row clipped to the slice, or, where the slice is
/// flipped, the row less the part between its edges.
#[allow(clippy::too_many_arguments)]
fn slice_row(
    spans: &mut Vec<Rect>,
    flip: bool,
    xl: i32,
    xr: i32,
    ya: i32,
    xorg: i32,
    x: i32,
    slw: i32,
) {
    if !flip {
        push(spans, xl, ya, xr - xl + 1);
    } else {
        let mut xc = xorg - x;
        push(spans, xc, ya, xr - xc + 1);
        xc += slw - 1;
        push(spans, xl, ya, xc - xl + 1);
    }
}

/// `miFillArcSliceI` / `miFillArcSliceD`: part of an ellipse, clipped row by
/// row to a chord or a pie slice.
fn slice<T: Term>(mut walk: Walk<T>, mut slice: Slice, spans: &mut Vec<Rect>) {
    while walk.y > 0 {
        let slw = walk.step();
        slice.edge1.step();
        slice.edge2.step();
        let y = walk.y;
        if y >= slice.min_top_y && y <= slice.max_top_y {
            // MIARCSLICEUPPER
            let ya = walk.yorg - y;
            let mut xl = walk.xorg - walk.x;
            let mut xr = xl + slw - 1;
            if slice.edge1_top && slice.edge1.x < xr {
                xr = slice.edge1.x;
            }
            if slice.edge2_top && slice.edge2.x > xl {
                xl = slice.edge2.x;
            }
            slice_row(spans, slice.flip_top, xl, xr, ya, walk.xorg, walk.x, slw);
        }
        if y >= slice.min_bot_y && y <= slice.max_bot_y {
            // MIARCSLICELOWER
            let ya = walk.yorg + y + walk.dy;
            let mut xl = walk.xorg - walk.x;
            let mut xr = xl + slw - 1;
            if !slice.edge1_top && slice.edge1.x > xl {
                xl = slice.edge1.x;
            }
            if !slice.edge2_top && slice.edge2.x < xr {
                xr = slice.edge2.x;
            }
            slice_row(spans, slice.flip_bot, xl, xr, ya, walk.xorg, walk.x, slw);
        }
    }
}

/// `miPolyFillArc`: every arc's spans. `pie_slice` is the GC's arc mode.
pub fn fill(arcs: &[XArc], pie_slice: bool) -> Vec<Rect> {
    let mut spans = Vec::new();
    for arc in arcs {
        // miFillArcEmpty
        if arc.angle2 == 0
            || arc.width == 0
            || arc.height == 0
            || (arc.width == 1 && arc.height & 1 == 1)
        {
            continue;
        }
        // miCanFillArc: the integer walker, unless it could overflow.
        let integer = arc.width == arc.height || (arc.width <= 800 && arc.height <= 800);
        let whole = i32::from(arc.angle2) >= FULLCIRCLE || i32::from(arc.angle2) <= -FULLCIRCLE;
        match (whole, integer) {
            (true, true) => ellipse(setup_integer(arc), &mut spans),
            (true, false) => ellipse(setup_double(arc), &mut spans),
            (false, true) => slice(setup_integer(arc), slice_setup(arc, pie_slice), &mut spans),
            (false, false) => slice(setup_double(arc), slice_setup(arc, pie_slice), &mut spans),
        }
    }
    spans
}
