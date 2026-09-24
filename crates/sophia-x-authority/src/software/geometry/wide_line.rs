//! Wide lines: every line whose width is one or more, with its caps, joins
//! and dashes, as spans to paint.
//!
//! Copyright 1988, 1998 The Open Group. Copyright 1989 by Digital Equipment
//! Corporation, Maynard, Massachusetts. Both permission notices are kept in
//! full in `THIRD-PARTY-NOTICES.md`.
//!
//! A port of the X server's `mi/miwideline.c` (Keith Packard, MIT X
//! Consortium), with `miStepDash` from `mi/midash.c` and the wide branches
//! of `miPolySegment` and `miPolyRectangle`. The protocol defines a wide
//! line's pixels exactly, and `mi` is what the reference server and every
//! conformance oracle draw with, so this follows it routine by routine
//! rather than restating the
//! geometry: the same integer edge walkers, the same floating-point
//! expressions in the same order, and the same span-group rules for the
//! raster functions that must touch each pixel once. Where `mi` does
//! something that looks like a slip, the port keeps it and says so, because
//! the point is to agree with it pixel for pixel.
//!
//! What is not here: zero-width lines, which the protocol leaves to the
//! server and the store draws itself, and wide arcs, which are `miarc.c`.
//! Fill styles are not applied: spans carry the pixel `mi` would have put in
//! the foreground, and the caller paints them solid, as it did before.

use crate::{
    X_CAP_PROJECTING, X_CAP_ROUND, X_FILL_OPAQUE_STIPPLED, X_FILL_TILED, X_JOIN_BEVEL,
    X_JOIN_MITER, X_JOIN_ROUND, X_LINE_DOUBLE_DASH, X_LINE_ON_OFF_DASH, X_LINE_SOLID,
    XGraphicsContextValues, XPoint,
};
use sophia_protocol::Rect;

mod dash;
mod poly;
mod rectangles;
mod round;
mod span_group;
mod tests;

use dash::dashes_walkable;
pub(super) use dash::step_dash;
use poly::{build_edge, build_poly};
pub use rectangles::rectangles;
use span_group::{append_spans, unique_spans};

/// One horizontal run of pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XSpan {
    pub x: i32,
    pub y: i32,
    pub width: i32,
}

/// Spans in the order they are to be painted, each batch with its pixel.
/// The order matters: without a span group, a later batch lands on an
/// earlier one, which is how `mi` resolves a double dash's overlaps.
pub type XInkedSpans = Vec<(u32, Vec<XSpan>)>;

/// `1/sin^2(11/2)`, the miter limit: a miter longer than this is beveled.
const SQSECANT: f64 = 108.856_472_512_142;
const MAXSHORT: i32 = 32_767;
const MINSHORT: i32 = -32_768;

/// `ICEIL` from `mifpoly.h`: the ceiling, computed through a truncating
/// conversion as the C does.
pub(crate) fn iceil(x: f64) -> i32 {
    let truncated = x as i32;
    if x == f64::from(truncated) || x < 0.0 {
        truncated
    } else {
        truncated + 1
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Edge {
    height: i32,
    x: i32,
    stepx: i32,
    signdx: i32,
    e: i32,
    dy: i32,
    dx: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Vertex {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Slope {
    dx: i32,
    dy: i32,
    /// `x0 * dy - y0 * dx`
    k: f64,
}

/// A line's end, for the cap or join drawn there.
#[derive(Clone, Copy, Debug, Default)]
struct Face {
    xa: f64,
    ya: f64,
    dx: i32,
    dy: i32,
    x: i32,
    y: i32,
    k: f64,
}

struct SpanGroup {
    spans: Vec<Vec<XSpan>>,
    ymin: i32,
    ymax: i32,
}

impl SpanGroup {
    fn new() -> Self {
        Self {
            spans: Vec::new(),
            ymin: MAXSHORT,
            ymax: MINSHORT,
        }
    }
}

/// The raster functions whose result changes when a pixel is painted twice;
/// for these, and only these, `mi` collects spans and paints their union.
fn careful_rop(function: u8) -> bool {
    (function & 0xc) == 0x8 || (function & 0x3) == 0x2
}

struct Lines<'a> {
    gc: &'a XGraphicsContextValues,
    lw: i32,
    /// The foreground and background span groups, when the raster function
    /// needs them (`miSetupSpanData`).
    groups: Option<(SpanGroup, SpanGroup)>,
    out: XInkedSpans,
}

impl<'a> Lines<'a> {
    fn new(gc: &'a XGraphicsContextValues, npt: usize) -> Self {
        let groups = if (npt < 3 && gc.cap_style != X_CAP_ROUND) || !careful_rop(gc.function) {
            None
        } else {
            Some((SpanGroup::new(), SpanGroup::new()))
        };
        Self {
            gc,
            lw: i32::from(gc.line_width),
            groups,
            out: Vec::new(),
        }
    }

    fn has_groups(&self) -> bool {
        self.groups.is_some()
    }

    /// `fillSpans`: paint now, or append to the group of this pixel.
    fn fill_spans(&mut self, pixel: u32, spans: Vec<XSpan>) {
        let double_dash = self.gc.line_style == X_LINE_DOUBLE_DASH;
        let foreground = self.gc.foreground;
        match &mut self.groups {
            None => {
                if !spans.is_empty() {
                    self.out.push((pixel, spans));
                }
            }
            // `AppendSpanGroup` chooses the group by comparing the pixel with
            // the foreground, so a background equal to it joins the
            // foreground's group.
            Some((fg, bg)) => {
                if pixel == foreground {
                    append_spans(fg, double_dash.then_some(bg), spans);
                } else {
                    append_spans(bg, Some(fg), spans);
                }
            }
        }
    }

    /// `miCleanupSpanData`: the background's union, then the foreground's.
    fn finish(mut self) -> XInkedSpans {
        if let Some((fg, bg)) = self.groups.take() {
            if self.gc.line_style == X_LINE_DOUBLE_DASH {
                let spans = unique_spans(bg);
                if !spans.is_empty() {
                    self.out.push((self.gc.background, spans));
                }
            }
            let spans = unique_spans(fg);
            if !spans.is_empty() {
                self.out.push((self.gc.foreground, spans));
            }
        }
        self.out
    }

    /// `miFillPolyHelper`
    fn fill_poly(
        &mut self,
        pixel: u32,
        mut y: i32,
        overall_height: i32,
        left: &[Edge],
        right: &[Edge],
    ) {
        let mut spans = Vec::with_capacity(usize::try_from(overall_height.max(0)).unwrap_or(0));
        let (mut l, mut r) = (Edge::default(), Edge::default());
        let (mut left_height, mut right_height) = (0, 0);
        let (mut left_index, mut right_index) = (0, 0);
        while (left_index < left.len() || left_height != 0)
            && (right_index < right.len() || right_height != 0)
        {
            if left_height == 0 && left_index < left.len() {
                l = left[left_index];
                left_height = l.height;
                left_index += 1;
            }
            if right_height == 0 && right_index < right.len() {
                r = right[right_index];
                right_height = r.height;
                right_index += 1;
            }
            let mut height = left_height.min(right_height);
            left_height -= height;
            right_height -= height;
            while height > 0 {
                height -= 1;
                if r.x >= l.x {
                    spans.push(XSpan {
                        x: l.x,
                        y,
                        width: r.x - l.x + 1,
                    });
                }
                y += 1;
                l.x += l.stepx;
                l.e += l.dx;
                if l.e > 0 {
                    l.x += l.signdx;
                    l.e -= l.dy;
                }
                r.x += r.stepx;
                r.e += r.dx;
                if r.e > 0 {
                    r.x += r.signdx;
                    r.e -= r.dy;
                }
            }
        }
        self.fill_spans(pixel, spans);
    }

    /// `miFillRectPolyHelper`
    fn fill_rect(&mut self, pixel: u32, x: i32, y: i32, w: i32, h: i32) {
        let spans = (0..h.max(0))
            .map(|row| XSpan {
                x,
                y: y + row,
                width: w,
            })
            .collect();
        self.fill_spans(pixel, spans);
    }

    /// `miLineOnePoint`
    fn one_point(&mut self, pixel: u32, x: i32, y: i32) {
        self.fill_spans(pixel, vec![XSpan { x, y, width: 1 }]);
    }

    /// `miLineJoin`
    fn join(&mut self, pixel: u32, p_left: &mut Face, p_right: &mut Face) {
        let mut join_style = self.gc.join_style;
        let lw = self.lw;
        let mut denom;
        if lw == 1 && !self.has_groups() {
            // One of the lines may already draw the joining pixel.
            if p_left.dx > 0 || (p_left.dx == 0 && p_left.dy > 0) {
                return;
            }
            if p_right.dx > 0 || (p_right.dx == 0 && p_right.dy > 0) {
                return;
            }
            denom = 0.0;
            if join_style != X_JOIN_ROUND {
                denom = -f64::from(p_left.dx) * f64::from(p_right.dy)
                    + f64::from(p_right.dx) * f64::from(p_left.dy);
                if denom == 0.0 {
                    return;
                }
            }
            if join_style != X_JOIN_MITER {
                self.one_point(pixel, p_left.x, p_left.y);
                return;
            }
        } else {
            if join_style == X_JOIN_ROUND {
                self.arc(pixel, Some(p_left), Some(p_right), 0.0, 0.0, true);
                return;
            }
            denom = -f64::from(p_left.dx) * f64::from(p_right.dy)
                + f64::from(p_right.dx) * f64::from(p_left.dy);
            if denom == 0.0 {
                return;
            }
        }

        let mut swapslopes = false;
        if denom > 0.0 {
            p_left.xa = -p_left.xa;
            p_left.ya = -p_left.ya;
            p_left.dx = -p_left.dx;
            p_left.dy = -p_left.dy;
        } else {
            swapslopes = true;
            p_right.xa = -p_right.xa;
            p_right.ya = -p_right.ya;
            p_right.dx = -p_right.dx;
            p_right.dy = -p_right.dy;
        }

        let mut vertices = [Vertex::default(); 4];
        let mut slopes = [Slope::default(); 4];
        vertices[0] = Vertex {
            x: p_right.xa,
            y: p_right.ya,
        };
        slopes[0] = Slope {
            dx: -p_right.dy,
            dy: p_right.dx,
            k: 0.0,
        };
        vertices[1] = Vertex { x: 0.0, y: 0.0 };
        slopes[1] = Slope {
            dx: p_left.dy,
            dy: -p_left.dx,
            k: 0.0,
        };
        vertices[2] = Vertex {
            x: p_left.xa,
            y: p_left.ya,
        };

        let (mut mx, mut my) = (0.0, 0.0);
        if join_style == X_JOIN_MITER {
            my = (f64::from(p_left.dy)
                * (p_right.xa * f64::from(p_right.dy) - p_right.ya * f64::from(p_right.dx))
                - f64::from(p_right.dy)
                    * (p_left.xa * f64::from(p_left.dy) - p_left.ya * f64::from(p_left.dx)))
                / denom;
            if p_left.dy != 0 {
                mx = p_left.xa + (my - p_left.ya) * f64::from(p_left.dx) / f64::from(p_left.dy);
            } else {
                mx = p_right.xa + (my - p_right.ya) * f64::from(p_right.dx) / f64::from(p_right.dy);
            }
            if (mx * mx + my * my) * 4.0 > SQSECANT * f64::from(lw) * f64::from(lw) {
                join_style = X_JOIN_BEVEL;
            }
        }

        let edgecount;
        if join_style == X_JOIN_MITER {
            let sign = if swapslopes { -1 } else { 1 };
            slopes[2] = Slope {
                dx: p_left.dx * sign,
                dy: p_left.dy * sign,
                k: p_left.k * f64::from(sign),
            };
            vertices[3] = Vertex { x: mx, y: my };
            slopes[3] = Slope {
                dx: p_right.dx * sign,
                dy: p_right.dy * sign,
                k: p_right.k * f64::from(sign),
            };
            edgecount = 4;
        } else {
            let dx = p_right.xa - p_left.xa;
            let dy = p_right.ya - p_left.ya;
            let scale = dx.abs().max(dy.abs());
            slopes[2].dx = ((dx * 65_536.0) / scale) as i32;
            slopes[2].dy = ((dy * 65_536.0) / scale) as i32;
            slopes[2].k = ((p_left.xa + p_right.xa) * f64::from(slopes[2].dy)
                - (p_left.ya + p_right.ya) * f64::from(slopes[2].dx))
                / 2.0;
            edgecount = 3;
        }

        let poly = build_poly(
            &vertices[..edgecount],
            &slopes[..edgecount],
            p_left.x,
            p_left.y,
        );
        self.fill_poly(pixel, poly.y, poly.height, &poly.left, &poly.right);
    }

    /// `miLineProjectingCap`
    fn projecting_cap(&mut self, pixel: u32, face: &Face, mut is_left: bool, is_int: bool) {
        let (mut xorgi, mut yorgi) = (0, 0);
        if is_int {
            xorgi = face.x;
            yorgi = face.y;
        }
        let lw = self.lw;
        let dx = face.dx;
        let mut dy = face.dy;
        let mut k = face.k;
        let mut lefts = [Edge::default(); 2];
        let mut rights = [Edge::default(); 2];
        if dy == 0 {
            lefts[0] = Edge {
                height: lw,
                x: xorgi - if is_left { lw >> 1 } else { 0 },
                stepx: 0,
                signdx: 1,
                e: -lw,
                dx: 0,
                dy: lw,
            };
            rights[0] = Edge {
                height: lw,
                x: xorgi + if is_left { 0 } else { (lw + 1) >> 1 },
                stepx: 0,
                signdx: 1,
                e: -lw,
                dx: 0,
                dy: lw,
            };
            self.fill_poly(pixel, yorgi - (lw >> 1), lw, &lefts[..1], &rights[..1]);
        } else if dx == 0 {
            if dy < 0 {
                dy = -dy;
                is_left = !is_left;
            }
            let mut topy = yorgi;
            let mut bottomy = yorgi + dy;
            if is_left {
                topy -= lw >> 1;
            } else {
                bottomy += lw >> 1;
            }
            lefts[0] = Edge {
                height: bottomy - topy,
                x: xorgi - (lw >> 1),
                stepx: 0,
                signdx: 1,
                e: -dy,
                dx,
                dy,
            };
            rights[0] = Edge {
                x: lefts[0].x + (lw - 1),
                ..lefts[0]
            };
            self.fill_poly(pixel, topy, bottomy - topy, &lefts[..1], &rights[..1]);
        } else {
            let mut xa = face.xa;
            let mut ya = face.ya;
            let project_x_off = -ya;
            let project_y_off = xa;
            // Which of the four edges sits where depends on the direction,
            // as in `miWideSegment`.
            let mut right = Edge::default();
            let mut left = Edge::default();
            let mut top = Edge::default();
            let mut bottom = Edge::default();
            let (righty, lefty, topy, bottomy, maxy);
            if is_left {
                righty = build_edge(xa, ya, k, dx, dy, xorgi, yorgi, false, &mut right);
                xa = -xa;
                ya = -ya;
                k = -k;
                lefty = build_edge(
                    xa - project_x_off,
                    ya - project_y_off,
                    k,
                    dx,
                    dy,
                    xorgi,
                    yorgi,
                    true,
                    &mut left,
                );
                if dx > 0 {
                    ya = -ya;
                    xa = -xa;
                }
                let xap = xa - project_x_off;
                let yap = ya - project_y_off;
                topy = build_edge(
                    xap,
                    yap,
                    xap * f64::from(dx) + yap * f64::from(dy),
                    -dy,
                    dx,
                    xorgi,
                    yorgi,
                    dx > 0,
                    &mut top,
                );
                bottomy = build_edge(xa, ya, 0.0, -dy, dx, xorgi, yorgi, dx < 0, &mut bottom);
                maxy = -ya;
            } else {
                righty = build_edge(
                    xa - project_x_off,
                    ya - project_y_off,
                    k,
                    dx,
                    dy,
                    xorgi,
                    yorgi,
                    false,
                    &mut right,
                );
                xa = -xa;
                ya = -ya;
                k = -k;
                lefty = build_edge(xa, ya, k, dx, dy, xorgi, yorgi, true, &mut left);
                if dx > 0 {
                    ya = -ya;
                    xa = -xa;
                }
                let xap = xa - project_x_off;
                let yap = ya - project_y_off;
                // `mi` passes `xorgi` for both origins here, where `yorgi`
                // looks meant. Kept: the reference server draws with it.
                topy = build_edge(xa, ya, 0.0, -dy, dx, xorgi, xorgi, dx > 0, &mut top);
                bottomy = build_edge(
                    xap,
                    yap,
                    xap * f64::from(dx) + yap * f64::from(dy),
                    -dy,
                    dx,
                    xorgi,
                    xorgi,
                    dx < 0,
                    &mut bottom,
                );
                maxy = -ya + project_y_off;
            }
            let finaly = iceil(maxy) + yorgi;
            if dx < 0 {
                left.height = bottomy - lefty;
                right.height = finaly - righty;
                top.height = righty - topy;
            } else {
                right.height = bottomy - righty;
                left.height = finaly - lefty;
                top.height = lefty - topy;
            }
            bottom.height = finaly - bottomy;
            // lefts[0], lefts[1], rights[0], rights[1] in `mi`'s layout.
            let (lefts, rights) = if dx < 0 {
                ([left, bottom], [top, right])
            } else {
                ([top, left], [right, bottom])
            };
            self.fill_poly(pixel, topy, bottom.height + bottomy - topy, &lefts, &rights);
        }
    }

    /// `miWideSegment`
    #[allow(clippy::too_many_arguments)]
    fn wide_segment(
        &mut self,
        pixel: u32,
        mut x1: i32,
        mut y1: i32,
        mut x2: i32,
        mut y2: i32,
        mut project_left: bool,
        mut project_right: bool,
        left_face: &mut Face,
        right_face: &mut Face,
    ) {
        let lw = self.lw;
        // Draw top to bottom always; the faces swap with the ends, so the
        // caller's left face may receive the far end.
        let (mut left_face, mut right_face) = (left_face, right_face);
        if y2 < y1 || (y2 == y1 && x2 < x1) {
            std::mem::swap(&mut x1, &mut x2);
            std::mem::swap(&mut y1, &mut y2);
            std::mem::swap(&mut project_left, &mut project_right);
            std::mem::swap(&mut left_face, &mut right_face);
        }
        let mut dy = y2 - y1;
        let mut dx = x2 - x1;
        let signdx = if dx < 0 { -1 } else { 1 };

        left_face.x = x1;
        left_face.y = y1;
        left_face.dx = dx;
        left_face.dy = dy;
        right_face.x = x2;
        right_face.y = y2;
        right_face.dx = -dx;
        right_face.dy = -dy;

        if dy == 0 {
            right_face.xa = 0.0;
            right_face.ya = f64::from(lw) / 2.0;
            right_face.k = -f64::from(lw * dx) / 2.0;
            left_face.xa = 0.0;
            left_face.ya = -right_face.ya;
            left_face.k = right_face.k;
            let mut x = x1;
            if project_left {
                x -= lw >> 1;
            }
            let y = y1 - (lw >> 1);
            dx = x2 - x;
            if project_right {
                dx += (lw + 1) >> 1;
            }
            dy = lw;
            self.fill_rect(pixel, x, y, dx, dy);
        } else if dx == 0 {
            left_face.xa = f64::from(lw) / 2.0;
            left_face.ya = 0.0;
            left_face.k = f64::from(lw * dy) / 2.0;
            right_face.xa = -left_face.xa;
            right_face.ya = 0.0;
            right_face.k = left_face.k;
            let mut y = y1;
            if project_left {
                y -= lw >> 1;
            }
            let x = x1 - (lw >> 1);
            dy = y2 - y;
            if project_right {
                dy += (lw + 1) >> 1;
            }
            dx = lw;
            self.fill_rect(pixel, x, y, dx, dy);
        } else {
            let l = f64::from(lw) / 2.0;
            let big_l = f64::from(dx).hypot(f64::from(dy));
            let r = l / big_l;

            // coord of upper bound at integral y
            let mut ya = -r * f64::from(dx);
            let mut xa = r * f64::from(dy);
            let (mut project_x_off, mut project_y_off) = (0.0, 0.0);
            if project_left || project_right {
                project_x_off = -ya;
                project_y_off = xa;
            }
            // xa * dy - ya * dx
            let mut k = l * big_l;
            left_face.xa = xa;
            left_face.ya = ya;
            left_face.k = k;
            right_face.xa = -xa;
            right_face.ya = -ya;
            right_face.k = k;

            let mut right = Edge::default();
            let mut left = Edge::default();
            let mut top = Edge::default();
            let mut bottom = Edge::default();
            let righty = if project_left {
                build_edge(
                    xa - project_x_off,
                    ya - project_y_off,
                    k,
                    dx,
                    dy,
                    x1,
                    y1,
                    false,
                    &mut right,
                )
            } else {
                build_edge(xa, ya, k, dx, dy, x1, y1, false, &mut right)
            };

            // coord of lower bound at integral y
            ya = -ya;
            xa = -xa;
            k = -k;
            let lefty = if project_left {
                build_edge(
                    xa - project_x_off,
                    ya - project_y_off,
                    k,
                    dx,
                    dy,
                    x1,
                    y1,
                    true,
                    &mut left,
                )
            } else {
                build_edge(xa, ya, k, dx, dy, x1, y1, true, &mut left)
            };

            // coord of top face at integral y
            if signdx > 0 {
                ya = -ya;
                xa = -xa;
            }
            let topy = if project_left {
                let xap = xa - project_x_off;
                let yap = ya - project_y_off;
                build_edge(
                    xap,
                    yap,
                    xap * f64::from(dx) + yap * f64::from(dy),
                    -dy,
                    dx,
                    x1,
                    y1,
                    dx > 0,
                    &mut top,
                )
            } else {
                build_edge(xa, ya, 0.0, -dy, dx, x1, y1, dx > 0, &mut top)
            };

            // coord of bottom face at integral y
            let (bottomy, maxy) = if project_right {
                let xap = xa + project_x_off;
                let yap = ya + project_y_off;
                (
                    build_edge(
                        xap,
                        yap,
                        xap * f64::from(dx) + yap * f64::from(dy),
                        -dy,
                        dx,
                        x2,
                        y2,
                        dx < 0,
                        &mut bottom,
                    ),
                    -ya + project_y_off,
                )
            } else {
                (
                    build_edge(xa, ya, 0.0, -dy, dx, x2, y2, dx < 0, &mut bottom),
                    -ya,
                )
            };

            let finaly = iceil(maxy) + y2;
            if dx < 0 {
                left.height = bottomy - lefty;
                right.height = finaly - righty;
                top.height = righty - topy;
            } else {
                right.height = bottomy - righty;
                left.height = finaly - lefty;
                top.height = lefty - topy;
            }
            bottom.height = finaly - bottomy;
            let (lefts, rights) = if dx < 0 {
                ([left, bottom], [top, right])
            } else {
                ([top, left], [right, bottom])
            };
            self.fill_poly(pixel, topy, bottom.height + bottomy - topy, &lefts, &rights);
        }
    }

    /// `miWideLine`, for points in `CoordModeOrigin`.
    fn wide_line(&mut self, points: &[XPoint]) {
        let gc = self.gc;
        let pixel = gc.foreground;
        let npt = points.len();
        let (mut x2, mut y2) = (i32::from(points[0].x), i32::from(points[0].y));
        let self_join =
            npt > 1 && points[0].x == points[npt - 1].x && points[0].y == points[npt - 1].y;
        let mut project_left = gc.cap_style == X_CAP_PROJECTING && !self_join;
        let mut project_right = false;
        let mut left_face = Face::default();
        let mut right_face = Face::default();
        let mut prev_right_face = Face::default();
        let mut first_face = Face::default();
        let mut first = true;
        let mut something_drawn = false;
        let round_one_point = |lines: &Self| lines.lw == 1 && !lines.has_groups();
        for (index, point) in points.iter().enumerate().skip(1) {
            let last = index == npt - 1;
            let (x1, y1) = (x2, y2);
            x2 = i32::from(point.x);
            y2 = i32::from(point.y);
            if x1 != x2 || y1 != y2 {
                something_drawn = true;
                if last && gc.cap_style == X_CAP_PROJECTING && !self_join {
                    project_right = true;
                }
                self.wide_segment(
                    pixel,
                    x1,
                    y1,
                    x2,
                    y2,
                    project_left,
                    project_right,
                    &mut left_face,
                    &mut right_face,
                );
                if first {
                    if self_join {
                        first_face = left_face;
                    } else if gc.cap_style == X_CAP_ROUND {
                        if round_one_point(self) {
                            self.one_point(pixel, x1, y1);
                        } else {
                            self.arc(pixel, Some(&mut left_face), None, 0.0, 0.0, true);
                        }
                    }
                } else {
                    self.join(pixel, &mut left_face, &mut prev_right_face);
                }
                prev_right_face = right_face;
                first = false;
                project_left = false;
            }
            if last && something_drawn {
                if self_join {
                    self.join(pixel, &mut first_face, &mut right_face);
                } else if gc.cap_style == X_CAP_ROUND {
                    if round_one_point(self) {
                        self.one_point(pixel, x2, y2);
                    } else {
                        self.arc(pixel, None, Some(&mut right_face), 0.0, 0.0, true);
                    }
                }
            }
        }
        // Every point coincident: a segment of no length, with its caps.
        if !something_drawn {
            project_left = gc.cap_style == X_CAP_PROJECTING;
            self.wide_segment(
                pixel,
                x2,
                y2,
                x2,
                y2,
                project_left,
                project_left,
                &mut left_face,
                &mut right_face,
            );
            if gc.cap_style == X_CAP_ROUND {
                self.arc(pixel, Some(&mut left_face), None, 0.0, 0.0, true);
                // `mi`'s "sleezy hack to make it work".
                right_face.dx = -1;
                self.arc(pixel, None, Some(&mut right_face), 0.0, 0.0, true);
            }
        }
    }
}

/// `miPolylines` for a line width of one or more: one connected polyline.
pub fn polyline(points: &[XPoint], gc: &XGraphicsContextValues) -> XInkedSpans {
    if points.is_empty() || gc.line_width == 0 {
        return Vec::new();
    }
    let mut lines = Lines::new(gc, points.len());
    let dashed = gc.line_style != X_LINE_SOLID
        && dashes_walkable(&gc.dashes)
        // `miWideDash` hands a double dash with a tile or opaque stipple to
        // `miWideLine`.
        && !(gc.line_style == X_LINE_DOUBLE_DASH
            && (gc.fill_style == X_FILL_OPAQUE_STIPPLED || gc.fill_style == X_FILL_TILED));
    if dashed {
        lines.wide_dash(points);
    } else {
        lines.wide_line(points);
    }
    lines.finish()
}

/// `miPolylines`: a connected polyline of any width. Zero width is
/// `miZeroLine` when solid, and `fb`'s dashed Bresenham walk when dashed,
/// which is what the reference server draws.
pub fn polylines(points: &[XPoint], gc: &XGraphicsContextValues) -> XInkedSpans {
    if gc.line_width != 0 {
        return polyline(points, gc);
    }
    if gc.line_style == X_LINE_SOLID {
        let spans: Vec<XSpan> =
            super::zero_line::polyline(points, gc.cap_style == crate::X_CAP_NOT_LAST)
                .into_iter()
                .map(|(x, y)| XSpan { x, y, width: 1 })
                .collect();
        return if spans.is_empty() {
            Vec::new()
        } else {
            vec![(gc.foreground, spans)]
        };
    }
    super::zero_line::dash::polyline(points, gc)
}

/// `miPolySegment`: each segment its own two-point polyline, so each has
/// its own caps and its own dash phase.
pub fn segments(segments: &[(XPoint, XPoint)], gc: &XGraphicsContextValues) -> XInkedSpans {
    segments
        .iter()
        .flat_map(|(from, to)| polylines(&[*from, *to], gc))
        .collect()
}

/// The smallest rectangle holding every span, if any has width.
pub fn bounds(spans: &XInkedSpans) -> Option<Rect> {
    let mut extent: Option<(i32, i32, i32, i32)> = None;
    for span in spans.iter().flat_map(|(_, spans)| spans) {
        if span.width <= 0 {
            continue;
        }
        let (left, top, right, bottom) = (span.x, span.y, span.x + span.width, span.y + 1);
        extent = Some(match extent {
            None => (left, top, right, bottom),
            Some((l, t, r, b)) => (l.min(left), t.min(top), r.max(right), b.max(bottom)),
        });
    }
    extent.map(|(left, top, right, bottom)| Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}
