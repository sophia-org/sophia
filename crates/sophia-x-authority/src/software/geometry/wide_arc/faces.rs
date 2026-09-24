//! Where wide arcs meet: caps, joins, and the sub-pixel polygon filler they
//! share.
//!
//! From `mi/miarc.c`: `miArcJoin`, `miArcCap`, `miRoundCap`, `miFillSppPoly`
//! (Todd Newman), `miGetArcPts`, `GetFPolyYBounds`, `angleBetween` and
//! `translateBounds`.

use super::super::wide_line::iceil;
use super::spans::Canvas;
use super::{Face, Spp, mi_dasin, mi_datan2, mi_dcos, mi_dsin};
use crate::{X_CAP_PROJECTING, X_CAP_ROUND, X_JOIN_BEVEL, X_JOIN_MITER, X_JOIN_ROUND};

const EPSILON: f64 = 0.000_001;

fn is_equal(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPSILON
}

/// `SppArcRec`: an arc at sub-pixel position, angles in degrees.
#[derive(Clone, Copy, Debug, Default)]
struct SppArc {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    angle1: f64,
    angle2: f64,
}

/// `miFillSppPoly`: fill a convex polygon given at sub-pixel positions,
/// translated by `(x_trans, y_trans)` after rounding and by
/// `(x_ftrans, y_ftrans)` before it, so a cap or join meets the arc it ends.
fn fill_spp_poly(
    canvas: &mut Canvas,
    points: &[Spp],
    x_trans: i32,
    y_trans: i32,
    x_ftrans: f64,
    y_ftrans: f64,
) {
    let count = points.len();
    if count < 3 {
        return;
    }
    // GetFPolyYBounds
    let mut imin = 0;
    let (mut ymin_f, mut ymax_f) = (points[0].y, points[0].y);
    for (index, point) in points.iter().enumerate().skip(1) {
        if point.y < ymin_f {
            imin = index;
            ymin_f = point.y;
        }
        if point.y > ymax_f {
            ymax_f = point.y;
        }
    }
    let ymin = iceil(ymin_f + y_ftrans);
    let ymax = iceil(ymax_f + y_ftrans - 1.0);
    if ymax < ymin {
        return;
    }
    let mut marked = vec![0i32; count];
    let (mut nextleft, mut nextright) = (imin, imin);
    marked[imin] = -1;
    let mut y = iceil(points[nextleft].y + y_ftrans);
    let (mut xl, mut xr, mut ml, mut mr) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    // mi trusts its callers' polygons to be convex; a bound keeps one that
    // is not from spinning.
    let bound = 4 * count + usize::try_from(ymax - ymin).unwrap_or(0) + 4;
    for _ in 0..bound {
        // add a left edge if we need to
        let fy = f64::from(y);
        if (fy > points[nextleft].y + y_ftrans || is_equal(fy, points[nextleft].y + y_ftrans))
            && marked[nextleft] != 1
        {
            marked[nextleft] += 1;
            let left = nextleft;
            nextleft += 1;
            if nextleft >= count {
                nextleft = 0;
            }
            let dy = points[nextleft].y - points[left].y;
            if dy != 0.0 {
                ml = (points[nextleft].x - points[left].x) / dy;
                let dy = fy - (points[left].y + y_ftrans);
                xl = (points[left].x + x_ftrans) + ml * dy.max(0.0);
            }
        }
        // add a right edge if we need to
        if fy > points[nextright].y + y_ftrans
            || (is_equal(fy, points[nextright].y + y_ftrans) && marked[nextright] != 1)
        {
            marked[nextright] += 1;
            let right = nextright;
            nextright = if nextright == 0 {
                count - 1
            } else {
                nextright - 1
            };
            let dy = points[nextright].y - points[right].y;
            if dy != 0.0 {
                mr = (points[nextright].x - points[right].x) / dy;
                let dy = fy - (points[right].y + y_ftrans);
                xr = (points[right].x + x_ftrans) + mr * dy.max(0.0);
            }
        }
        // generate scans while we have a left edge and a right edge
        let i = (points[nextleft].y.min(points[nextright].y) + y_ftrans) - fy;
        if i < EPSILON {
            if marked[nextleft] != 0 && marked[nextright] != 0 {
                // no more points
                break;
            }
            if y > ymax {
                break;
            }
            continue;
        }
        let mut j = i as i32;
        if j == 0 {
            j = 1;
        }
        while j > 0 {
            let cxl = iceil(xl);
            let cxr = iceil(xr);
            let (x, width) = if xl < xr {
                (cxl, cxr - cxl)
            } else {
                (cxr, cxl - cxr)
            };
            canvas.span(y + y_trans, x + x_trans, x + x_trans + width);
            y += 1;
            xl += ml;
            xr += mr;
            j -= 1;
        }
        if y > ymax {
            break;
        }
    }
}

/// `miGetArcPts`: an arc as a chain of points, appended after the first
/// `cpt` of `points`; the number appended.
fn arc_points(arc: &SppArc, cpt: usize, points: &mut Vec<Spp>) -> usize {
    // Positive angles run counterclockwise, and X's y runs down: negate.
    let st = -arc.angle1;
    let et = -arc.angle2;
    let mut cdt = arc.width;
    if arc.height > cdt {
        cdt = arc.height;
    }
    cdt /= 2.0;
    if cdt <= 0.0 {
        return 0;
    }
    if cdt < 1.0 {
        cdt = 1.0;
    }
    let mut dt = mi_dasin(1.0 / cdt);
    let mut count = (et / dt) as i32;
    count = count.abs() + 1;
    dt = et / f64::from(count);
    count += 1;
    let count = usize::try_from(count).unwrap_or(0);
    let cdt = 2.0 * mi_dcos(dt);
    points.resize(cpt + count, Spp::default());
    let mut xc = arc.width / 2.0;
    let mut yc = arc.height / 2.0;
    let mut x0 = xc * mi_dcos(st);
    let mut y0 = yc * mi_dsin(st);
    let mut x1 = xc * mi_dcos(st + dt);
    let mut y1 = yc * mi_dsin(st + dt);
    xc += arc.x;
    yc += arc.y;
    points[cpt] = Spp {
        x: xc + x0,
        y: yc + y0,
    };
    points[cpt + 1] = Spp {
        x: xc + x1,
        y: yc + y1,
    };
    let mut i = 2;
    while i < count {
        let x2 = cdt * x1 - x0;
        let y2 = cdt * y1 - y0;
        points[cpt + i] = Spp {
            x: xc + x2,
            y: yc + y2,
        };
        x0 = x1;
        y0 = y1;
        x1 = x2;
        y1 = y2;
        i += 1;
    }
    // adjust the last point; for a full turn mi copies the array's first
    // point, whatever cpt says.
    points[cpt + i - 1] = if arc.angle2.abs() >= 360.0 {
        points[0]
    } else {
        Spp {
            x: mi_dcos(st + et) * arc.width / 2.0 + xc,
            y: mi_dsin(st + et) * arc.height / 2.0 + yc,
        }
    };
    count
}

/// `angleBetween`, in degrees, from X coordinates reflected to y-up.
fn angle_between(center: Spp, point1: Spp, point2: Spp) -> f64 {
    let a1 = mi_datan2(-(point1.y - center.y), point1.x - center.x);
    let a2 = mi_datan2(-(point2.y - center.y), point2.x - center.x);
    let mut a = a2 - a1;
    if a <= -180.0 {
        a += 360.0;
    } else if a > 180.0 {
        a -= 360.0;
    }
    a
}

/// `translateBounds`
fn translate(face: &mut Face, x: i32, y: i32, fx: f64, fy: f64) {
    let fx = fx + f64::from(x);
    let fy = fy + f64::from(y);
    for point in [&mut face.clock, &mut face.center, &mut face.counter_clock] {
        point.x -= fx;
        point.y -= fy;
    }
}

/// An arc's origin and half size, which its faces are relative to.
#[derive(Clone, Copy, Debug)]
pub(super) struct FaceOrigin {
    pub x: i32,
    pub y: i32,
    pub fx: f64,
    pub fy: f64,
}

/// `miArcJoin`
pub(super) fn join(
    canvas: &mut Canvas,
    line_width: u16,
    join_style: u8,
    left: &Face,
    left_origin: FaceOrigin,
    right: &Face,
    right_origin: FaceOrigin,
) {
    let x_org = (right_origin.x + left_origin.x) / 2;
    let y_org = (right_origin.y + left_origin.y) / 2;
    let x_ftrans = (left_origin.fx + right_origin.fx) / 2.0;
    let y_ftrans = (left_origin.fy + right_origin.fy) / 2.0;
    let mut right = *right;
    translate(
        &mut right,
        x_org - right_origin.x,
        y_org - right_origin.y,
        x_ftrans - right_origin.fx,
        y_ftrans - right_origin.fy,
    );
    let mut left = *left;
    translate(
        &mut left,
        x_org - left_origin.x,
        y_org - left_origin.y,
        x_ftrans - left_origin.fx,
        y_ftrans - left_origin.fy,
    );
    if right.clock == left.counter_clock {
        return;
    }
    let center = right.center;
    let mut a = angle_between(center, right.clock, left.counter_clock);
    let (corner, other_corner);
    if (0.0..=180.0).contains(&a) {
        corner = right.clock;
        other_corner = left.counter_clock;
    } else {
        a = angle_between(center, left.clock, right.counter_clock);
        corner = left.clock;
        other_corner = right.counter_clock;
    }
    let mut style = join_style;
    if style == X_JOIN_ROUND {
        let width = if line_width != 0 {
            f64::from(line_width)
        } else {
            1.0
        };
        let arc = SppArc {
            x: center.x - width / 2.0,
            y: center.y - width / 2.0,
            width,
            height: width,
            angle1: -mi_datan2(corner.y - center.y, corner.x - center.x),
            angle2: a,
        };
        let mut points = vec![other_corner, center, corner];
        let cpt = arc_points(&arc, 3, &mut points);
        if cpt != 0 {
            // mi fills the first cpt points of the list: the three it
            // prefixed and all but three of the arc's.
            fill_spp_poly(
                canvas,
                &points[..cpt.min(points.len())],
                x_org,
                y_org,
                x_ftrans,
                y_ftrans,
            );
        }
        return;
    }
    // don't miter arcs with less than 11 degrees between them
    let mut poly: Vec<Spp> = Vec::with_capacity(5);
    if style == X_JOIN_MITER && a < 169.0 {
        let bc2 = (corner.x - other_corner.x) * (corner.x - other_corner.x)
            + (corner.y - other_corner.y) * (corner.y - other_corner.y);
        let ec2 = bc2 / 4.0;
        let ac2 = (corner.x - center.x) * (corner.x - center.x)
            + (corner.y - center.y) * (corner.y - center.y);
        let ae = (ac2 - ec2).sqrt();
        let de = ec2 / ae;
        let e = Spp {
            x: (corner.x + other_corner.x) / 2.0,
            y: (corner.y + other_corner.y) / 2.0,
        };
        poly.extend([
            corner,
            center,
            other_corner,
            Spp {
                x: e.x + de * (e.x - center.x) / ae,
                y: e.y + de * (e.y - center.y) / ae,
            },
            corner,
        ]);
    } else {
        if style == X_JOIN_MITER {
            style = X_JOIN_BEVEL;
        }
        if style == X_JOIN_BEVEL {
            poly.extend([corner, center, other_corner, corner]);
        }
    }
    fill_spp_poly(canvas, &poly, x_org, y_org, x_ftrans, y_ftrans);
}

/// `miArcCap`
pub(super) fn cap(
    canvas: &mut Canvas,
    line_width: u16,
    cap_style: u8,
    face: &Face,
    origin: FaceOrigin,
) {
    let corner = face.clock;
    let other_corner = face.counter_clock;
    let center = face.center;
    if cap_style == X_CAP_PROJECTING {
        let poly = [
            other_corner,
            corner,
            Spp {
                x: corner.x - (center.y - corner.y),
                y: corner.y + (center.x - corner.x),
            },
            Spp {
                x: other_corner.x - (other_corner.y - center.y),
                y: other_corner.y + (other_corner.x - center.x),
            },
            other_corner,
        ];
        fill_spp_poly(canvas, &poly, origin.x, origin.y, origin.fx, origin.fy);
    } else if cap_style == X_CAP_ROUND {
        // miRoundCap only needs the end to differ from the centre.
        let end = Spp {
            x: center.x + 100.0,
            y: center.y,
        };
        round_cap(
            canvas,
            line_width,
            center,
            end,
            corner,
            other_corner,
            origin,
        );
    }
}

/// `miRoundCap`, as `miArcCap` calls it.
fn round_cap(
    canvas: &mut Canvas,
    line_width: u16,
    center: Spp,
    end: Spp,
    corner: Spp,
    other_corner: Spp,
    origin: FaceOrigin,
) {
    let width = if line_width != 0 {
        f64::from(line_width)
    } else {
        1.0
    };
    let angle1 = -mi_datan2(corner.y - center.y, corner.x - center.x);
    let angle2 = if (center.x - end.x).abs() <= EPSILON && (center.y - end.y).abs() <= EPSILON {
        -180.0
    } else {
        let mut a = -mi_datan2(other_corner.y - center.y, other_corner.x - center.x) - angle1;
        if a < 0.0 {
            a += 360.0;
        }
        a
    };
    let arc = SppArc {
        x: center.x - width / 2.0,
        y: center.y - width / 2.0,
        width,
        height: width,
        angle1,
        angle2,
    };
    let mut points = Vec::new();
    let cpt = arc_points(&arc, 0, &mut points);
    if cpt != 0 {
        fill_spp_poly(
            canvas,
            &points[..cpt],
            origin.x,
            origin.y,
            origin.fx,
            origin.fy,
        );
    }
}
