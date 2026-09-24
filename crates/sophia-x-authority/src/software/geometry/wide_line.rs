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

mod tests;

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

    /// `miLineArc`
    fn arc(
        &mut self,
        pixel: u32,
        left_face: Option<&mut Face>,
        right_face: Option<&mut Face>,
        mut xorg: f64,
        mut yorg: f64,
        mut is_int: bool,
    ) {
        let (mut xorgi, mut yorgi) = (0, 0);
        if is_int {
            let face = left_face.as_deref().or(right_face.as_deref());
            if let Some(face) = face {
                xorgi = face.x;
                yorgi = face.y;
            }
        }
        let mut edge1 = Edge {
            dy: -1,
            ..Edge::default()
        };
        let mut edge2 = Edge {
            dy: -1,
            ..Edge::default()
        };
        let (mut edgey1, mut edgey2) = (65_536, 65_536);
        let (mut edgeleft1, mut edgeleft2) = (false, false);
        let gc = self.gc;
        if (gc.line_style != X_LINE_SOLID || self.lw > 2)
            && ((gc.cap_style == X_CAP_ROUND && gc.join_style != X_JOIN_ROUND)
                || (gc.join_style == X_JOIN_ROUND && gc.cap_style == crate::X_CAP_BUTT))
        {
            if is_int {
                xorg = f64::from(xorgi);
                yorg = f64::from(yorgi);
            }
            match (left_face, right_face) {
                (Some(left), Some(right)) => {
                    round_join_clip(
                        left,
                        right,
                        &mut edge1,
                        &mut edge2,
                        &mut edgey1,
                        &mut edgey2,
                        &mut edgeleft1,
                        &mut edgeleft2,
                    );
                }
                (Some(left), None) => {
                    edgey1 = round_cap_clip(left, is_int, &mut edge1, &mut edgeleft1);
                }
                (None, Some(right)) => {
                    edgey2 = round_cap_clip(right, is_int, &mut edge2, &mut edgeleft2);
                }
                (None, None) => {}
            }
            is_int = false;
        }
        let spans = if is_int {
            line_arc_integer(self.lw, xorgi, yorgi)
        } else {
            line_arc_double(
                self.lw, xorg, yorg, &mut edge1, edgey1, edgeleft1, &mut edge2, edgey2, edgeleft2,
            )
        };
        self.fill_spans(pixel, spans);
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

    /// `miWideDashSegment`
    #[allow(clippy::too_many_arguments)]
    fn wide_dash_segment(
        &mut self,
        dash_offset: &mut i32,
        dash_index: &mut usize,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        project_left: bool,
        project_right: bool,
        left_face: &mut Face,
        right_face: &mut Face,
    ) {
        const V_TOP: usize = 0;
        const V_RIGHT: usize = 1;
        const V_BOTTOM: usize = 2;
        const V_LEFT: usize = 3;
        let gc = self.gc;
        let dashes = &gc.dashes;
        let dx = x2 - x1;
        let dy = y2 - y1;
        let mut index = *dash_index;
        let mut dash_remain = i32::from(dashes[index]) - *dash_offset;
        let fg_pixel = gc.foreground;
        let mut bg_pixel = gc.background;
        if gc.fill_style == X_FILL_OPAQUE_STIPPLED || gc.fill_style == X_FILL_TILED {
            bg_pixel = fg_pixel;
        }
        let (fdx, fdy) = (f64::from(dx), f64::from(dy));

        let l = f64::from(self.lw) / 2.0;
        let (big_l, rdx, rdy);
        if dx == 0 {
            big_l = if dy < 0 { -fdy } else { fdy };
            rdx = 0.0;
            rdy = if dy < 0 { -l } else { l };
        } else if dy == 0 {
            big_l = if dx < 0 { -fdx } else { fdx };
            rdx = if dx < 0 { -l } else { l };
            rdy = 0.0;
        } else {
            big_l = fdx.hypot(fdy);
            let r = l / big_l;
            rdx = r * fdx;
            rdy = r * fdy;
        }
        let k = l * big_l;
        let mut l_remain = big_l;

        let mut slopes = [Slope::default(); 4];
        slopes[V_TOP] = Slope { dx, dy, k };
        slopes[V_RIGHT] = Slope {
            dx: -dy,
            dy: dx,
            k: 0.0,
        };
        slopes[V_BOTTOM] = Slope {
            dx: -dx,
            dy: -dy,
            k,
        };
        slopes[V_LEFT] = Slope {
            dx: dy,
            dy: -dx,
            k: 0.0,
        };

        let mut vertices = [Vertex::default(); 4];
        vertices[V_RIGHT] = Vertex { x: rdy, y: -rdx };
        vertices[V_TOP] = vertices[V_RIGHT];
        vertices[V_BOTTOM] = Vertex { x: -rdy, y: rdx };
        vertices[V_LEFT] = vertices[V_BOTTOM];

        if project_left {
            vertices[V_TOP].x -= rdx;
            vertices[V_TOP].y -= rdy;
            vertices[V_LEFT].x -= rdx;
            vertices[V_LEFT].y -= rdy;
            slopes[V_LEFT].k = rdx * fdx + rdy * fdy;
        }

        let (mut lcenterx, mut lcentery) = (f64::from(x1), f64::from(y1));
        let (mut rcenterx, mut rcentery) = (0.0, 0.0);
        let mut lcap = Face::default();
        let mut rcap = Face::default();
        if gc.cap_style == X_CAP_ROUND {
            lcap = Face {
                dx,
                dy,
                x: x1,
                y: y1,
                ..Face::default()
            };
            rcap = Face {
                dx: -dx,
                dy: -dy,
                x: x1,
                y: y1,
                ..Face::default()
            };
        }
        let mut first = true;
        let mut save_right = Vertex::default();
        let mut save_bottom = Vertex::default();
        let mut save_k = 0.0;
        let mut pixel;
        while l_remain > f64::from(dash_remain) {
            let dash_dx = (f64::from(dash_remain) * fdx) / big_l;
            let dash_dy = (f64::from(dash_remain) * fdy) / big_l;
            rcenterx = lcenterx + dash_dx;
            rcentery = lcentery + dash_dy;
            vertices[V_RIGHT].x += dash_dx;
            vertices[V_RIGHT].y += dash_dy;
            vertices[V_BOTTOM].x += dash_dx;
            vertices[V_BOTTOM].y += dash_dy;
            slopes[V_RIGHT].k = vertices[V_RIGHT].x * fdx + vertices[V_RIGHT].y * fdy;

            if gc.line_style == X_LINE_DOUBLE_DASH || index & 1 == 0 {
                if gc.line_style == X_LINE_ON_OFF_DASH && gc.cap_style == X_CAP_PROJECTING {
                    save_right = vertices[V_RIGHT];
                    save_bottom = vertices[V_BOTTOM];
                    save_k = slopes[V_RIGHT].k;
                    if !first {
                        vertices[V_TOP].x -= rdx;
                        vertices[V_TOP].y -= rdy;
                        vertices[V_LEFT].x -= rdx;
                        vertices[V_LEFT].y -= rdy;
                        slopes[V_LEFT].k = vertices[V_LEFT].x * f64::from(slopes[V_LEFT].dy)
                            - vertices[V_LEFT].y * f64::from(slopes[V_LEFT].dx);
                    }
                    vertices[V_RIGHT].x += rdx;
                    vertices[V_RIGHT].y += rdy;
                    vertices[V_BOTTOM].x += rdx;
                    vertices[V_BOTTOM].y += rdy;
                    slopes[V_RIGHT].k = vertices[V_RIGHT].x * f64::from(slopes[V_RIGHT].dy)
                        - vertices[V_RIGHT].y * f64::from(slopes[V_RIGHT].dx);
                }
                let poly = build_poly(&vertices, &slopes, x1, y1);
                pixel = if index & 1 == 1 { bg_pixel } else { fg_pixel };
                self.fill_poly(pixel, poly.y, poly.height, &poly.left, &poly.right);

                if gc.line_style == X_LINE_ON_OFF_DASH {
                    if gc.cap_style == X_CAP_PROJECTING {
                        vertices[V_BOTTOM] = save_bottom;
                        vertices[V_RIGHT] = save_right;
                        slopes[V_RIGHT].k = save_k;
                    } else if gc.cap_style == X_CAP_ROUND {
                        if !first {
                            if dx < 0 {
                                lcap.xa = -vertices[V_LEFT].x;
                                lcap.ya = -vertices[V_LEFT].y;
                                lcap.k = slopes[V_LEFT].k;
                            } else {
                                lcap.xa = vertices[V_TOP].x;
                                lcap.ya = vertices[V_TOP].y;
                                lcap.k = -slopes[V_LEFT].k;
                            }
                            self.arc(pixel, Some(&mut lcap), None, lcenterx, lcentery, false);
                        }
                        if dx < 0 {
                            rcap.xa = vertices[V_BOTTOM].x;
                            rcap.ya = vertices[V_BOTTOM].y;
                            rcap.k = slopes[V_RIGHT].k;
                        } else {
                            rcap.xa = -vertices[V_RIGHT].x;
                            rcap.ya = -vertices[V_RIGHT].y;
                            rcap.k = -slopes[V_RIGHT].k;
                        }
                        self.arc(pixel, None, Some(&mut rcap), rcenterx, rcentery, false);
                    }
                }
            }
            l_remain -= f64::from(dash_remain);
            index += 1;
            if index == dashes.len() {
                index = 0;
            }
            dash_remain = i32::from(dashes[index]);
            lcenterx = rcenterx;
            lcentery = rcentery;
            vertices[V_TOP] = vertices[V_RIGHT];
            vertices[V_LEFT] = vertices[V_BOTTOM];
            slopes[V_LEFT].k = -slopes[V_RIGHT].k;
            first = false;
        }

        if gc.line_style == X_LINE_DOUBLE_DASH || index & 1 == 0 {
            vertices[V_TOP].x -= fdx;
            vertices[V_TOP].y -= fdy;
            vertices[V_LEFT].x -= fdx;
            vertices[V_LEFT].y -= fdy;
            vertices[V_RIGHT] = Vertex { x: rdy, y: -rdx };
            vertices[V_BOTTOM] = Vertex { x: -rdy, y: rdx };
            if project_right {
                vertices[V_RIGHT].x += rdx;
                vertices[V_RIGHT].y += rdy;
                vertices[V_BOTTOM].x += rdx;
                vertices[V_BOTTOM].y += rdy;
                slopes[V_RIGHT].k = vertices[V_RIGHT].x * f64::from(slopes[V_RIGHT].dy)
                    - vertices[V_RIGHT].y * f64::from(slopes[V_RIGHT].dx);
            } else {
                slopes[V_RIGHT].k = 0.0;
            }
            if !first && gc.line_style == X_LINE_ON_OFF_DASH && gc.cap_style == X_CAP_PROJECTING {
                vertices[V_TOP].x -= rdx;
                vertices[V_TOP].y -= rdy;
                vertices[V_LEFT].x -= rdx;
                vertices[V_LEFT].y -= rdy;
                slopes[V_LEFT].k = vertices[V_LEFT].x * f64::from(slopes[V_LEFT].dy)
                    - vertices[V_LEFT].y * f64::from(slopes[V_LEFT].dx);
            } else {
                slopes[V_LEFT].k += f64::from(dx * dx + dy * dy);
            }
            let poly = build_poly(&vertices, &slopes, x2, y2);
            // The final dash takes the GC's own background, not the
            // substituted one above -- as `mi` has it.
            pixel = if index & 1 == 1 {
                gc.background
            } else {
                gc.foreground
            };
            self.fill_poly(pixel, poly.y, poly.height, &poly.left, &poly.right);
            if !first && gc.line_style == X_LINE_ON_OFF_DASH && gc.cap_style == X_CAP_ROUND {
                lcap.x = x2;
                lcap.y = y2;
                if dx < 0 {
                    lcap.xa = -vertices[V_LEFT].x;
                    lcap.ya = -vertices[V_LEFT].y;
                    lcap.k = slopes[V_LEFT].k;
                } else {
                    lcap.xa = vertices[V_TOP].x;
                    lcap.ya = vertices[V_TOP].y;
                    lcap.k = -slopes[V_LEFT].k;
                }
                self.arc(pixel, Some(&mut lcap), None, rcenterx, rcentery, false);
            }
        }
        // A double assigned to an int: truncated, as C converts it.
        dash_remain = (f64::from(dash_remain) - l_remain) as i32;
        if dash_remain == 0 {
            index += 1;
            if index == dashes.len() {
                index = 0;
            }
            dash_remain = i32::from(dashes[index]);
        }

        *left_face = Face {
            x: x1,
            y: y1,
            dx,
            dy,
            xa: rdy,
            ya: -rdx,
            k,
        };
        *right_face = Face {
            x: x2,
            y: y2,
            dx: -dx,
            dy: -dy,
            xa: -rdy,
            ya: rdx,
            k,
        };
        *dash_index = index;
        *dash_offset = i32::from(dashes[index]) - dash_remain;
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

    /// `miWideDash`, for points in `CoordModeOrigin`.
    fn wide_dash(&mut self, points: &[XPoint]) {
        let gc = self.gc;
        let npt = points.len();
        let (mut x2, mut y2) = (i32::from(points[0].x), i32::from(points[0].y));
        let self_join = points[0].x == points[npt - 1].x && points[0].y == points[npt - 1].y;
        let mut project_left = gc.cap_style == X_CAP_PROJECTING && !self_join;
        let mut project_right = false;
        let mut dash_index = 0usize;
        let mut dash_offset = 0i32;
        step_dash(
            i32::from(gc.dash_offset),
            &mut dash_index,
            &gc.dashes,
            &mut dash_offset,
        );
        let mut left_face = Face::default();
        let mut right_face = Face::default();
        let mut prev_right_face = Face::default();
        let mut first_face = Face::default();
        let mut first = true;
        let mut something_drawn = false;
        let (mut end_is_fg, mut first_is_fg, mut prev_is_fg) = (false, false, false);
        for (index, point) in points.iter().enumerate().skip(1) {
            let last = index == npt - 1;
            let (x1, y1) = (x2, y2);
            x2 = i32::from(point.x);
            y2 = i32::from(point.y);
            if x1 != x2 || y1 != y2 {
                something_drawn = true;
                if last && gc.cap_style == X_CAP_PROJECTING && (!self_join || !first_is_fg) {
                    project_right = true;
                }
                let prev_dash_index = dash_index;
                self.wide_dash_segment(
                    &mut dash_offset,
                    &mut dash_index,
                    x1,
                    y1,
                    x2,
                    y2,
                    project_left,
                    project_right,
                    &mut left_face,
                    &mut right_face,
                );
                let start_is_fg = prev_dash_index & 1 == 0;
                end_is_fg = ((dash_index & 1) == 1) ^ (dash_offset != 0);
                if gc.line_style == X_LINE_DOUBLE_DASH || start_is_fg {
                    let pixel = if start_is_fg {
                        gc.foreground
                    } else {
                        gc.background
                    };
                    if first || (gc.line_style == X_LINE_ON_OFF_DASH && !prev_is_fg) {
                        if first && self_join {
                            first_face = left_face;
                            first_is_fg = start_is_fg;
                        } else if gc.cap_style == X_CAP_ROUND {
                            self.arc(pixel, Some(&mut left_face), None, 0.0, 0.0, true);
                        }
                    } else {
                        self.join(pixel, &mut left_face, &mut prev_right_face);
                    }
                }
                prev_right_face = right_face;
                prev_is_fg = end_is_fg;
                first = false;
                project_left = false;
            }
            if last && something_drawn {
                if gc.line_style == X_LINE_DOUBLE_DASH || end_is_fg {
                    let pixel = if end_is_fg {
                        gc.foreground
                    } else {
                        gc.background
                    };
                    if self_join && (gc.line_style == X_LINE_DOUBLE_DASH || first_is_fg) {
                        self.join(pixel, &mut first_face, &mut right_face);
                    } else if gc.cap_style == X_CAP_ROUND {
                        self.arc(pixel, None, Some(&mut right_face), 0.0, 0.0, true);
                    }
                } else if self_join && first_is_fg {
                    // An OnOffDash that ended on an odd dash: glue a cap to
                    // the start of the line.
                    let pixel = gc.foreground;
                    if gc.cap_style == X_CAP_PROJECTING {
                        let face = first_face;
                        self.projecting_cap(pixel, &face, true, true);
                    } else if gc.cap_style == X_CAP_ROUND {
                        self.arc(pixel, Some(&mut first_face), None, 0.0, 0.0, true);
                    }
                }
            }
        }
        // Every point coincident.
        if !something_drawn && (gc.line_style == X_LINE_DOUBLE_DASH || dash_index & 1 == 0) {
            let pixel = if dash_index & 1 == 1 {
                gc.background
            } else {
                gc.foreground
            };
            if gc.cap_style == X_CAP_ROUND {
                self.arc(pixel, None, None, f64::from(x2), f64::from(y2), false);
            } else if gc.cap_style == X_CAP_PROJECTING {
                let lw = self.lw;
                self.fill_rect(pixel, x2 - (lw >> 1), y2 - (lw >> 1), lw, lw);
            }
        }
    }
}

/// `miAppendSpans`
fn append_spans(group: &mut SpanGroup, other: Option<&mut SpanGroup>, spans: Vec<XSpan>) {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
        return;
    };
    let (ymin, ymax) = (first.y, last.y);
    group.ymin = group.ymin.min(ymin);
    group.ymax = group.ymax.max(ymax);
    if let Some(other) = other
        && other.ymin < ymax
        && ymin < other.ymax
    {
        subtract_spans(other, &spans);
    }
    group.spans.push(spans);
}

/// `miSubtractSpans`: take `sub`'s pixels out of every list in `group`.
///
/// Both walk in y order, and a row's first span in `sub` is the one
/// compared -- a later span of `sub` on the same row is passed over when the
/// group moves to the next row. That is `mi`'s behaviour, kept.
fn subtract_spans(group: &mut SpanGroup, sub: &[XSpan]) {
    let (Some(first), Some(last)) = (sub.first(), sub.last()) else {
        return;
    };
    let (ymin, ymax) = (first.y, last.y);
    for spans in &mut group.spans {
        let (Some(lo), Some(hi)) = (spans.first(), spans.last()) else {
            continue;
        };
        if !(lo.y <= ymax && ymin <= hi.y) {
            continue;
        }
        let (mut si, mut pi) = (0usize, 0usize);
        loop {
            while pi < spans.len() && spans[pi].y < sub[si].y {
                pi += 1;
            }
            if pi >= spans.len() {
                break;
            }
            while si < sub.len() && sub[si].y < spans[pi].y {
                si += 1;
            }
            if si >= sub.len() {
                break;
            }
            if sub[si].y == spans[pi].y {
                let xmin = sub[si].x;
                let xmax = xmin + sub[si].width;
                let (sx, sw) = (spans[pi].x, spans[pi].width);
                if xmin >= sx + sw || sx >= xmax {
                } else if xmin <= sx {
                    if xmax >= sx + sw {
                        spans.remove(pi);
                        continue;
                    }
                    spans[pi].width = sw - (xmax - sx);
                    spans[pi].x = xmax;
                } else if xmax >= sx + sw {
                    spans[pi].width = xmin - sx;
                } else {
                    let y = spans[pi].y;
                    spans.insert(
                        pi,
                        XSpan {
                            x: sx,
                            y,
                            width: xmin - sx,
                        },
                    );
                    pi += 1;
                    spans[pi].width = sw - (xmax - sx);
                    spans[pi].x = xmax;
                }
            }
            pi += 1;
        }
    }
}

/// `miFillUniqueSpanGroup`: one list is painted as it stands; several are
/// merged row by row into their union.
fn unique_spans(group: SpanGroup) -> Vec<XSpan> {
    match group.spans.len() {
        0 => Vec::new(),
        1 => group.spans.into_iter().next().unwrap_or_default(),
        _ => {
            let mut all: Vec<XSpan> = group
                .spans
                .into_iter()
                .flatten()
                .filter(|span| span.y >= group.ymin && span.y <= group.ymax)
                .collect();
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
}

/// `miPolyBuildEdge`
#[allow(clippy::too_many_arguments)]
fn build_edge(
    x0: f64,
    y0: f64,
    mut k: f64,
    mut dx: i32,
    mut dy: i32,
    xi: i32,
    yi: i32,
    left: bool,
    edge: &mut Edge,
) -> i32 {
    let _ = x0;
    if dy < 0 {
        dy = -dy;
        dx = -dx;
        k = -k;
    }
    let y = iceil(y0);
    let xady = iceil(k) + y * dx;
    let x = if xady <= 0 {
        -(-xady / dy) - 1
    } else {
        (xady - 1) / dy
    };
    let mut e = xady - x * dy;
    if dx >= 0 {
        edge.signdx = 1;
        edge.stepx = dx / dy;
        edge.dx = dx % dy;
    } else {
        edge.signdx = -1;
        edge.stepx = -(-dx / dy);
        edge.dx = -dx % dy;
        e = dy - e + 1;
    }
    edge.dy = dy;
    edge.x = x + i32::from(left) + xi;
    // Biased to compare against 0 instead of dy.
    edge.e = e - dy;
    y + yi
}

struct Poly {
    y: i32,
    height: i32,
    left: Vec<Edge>,
    right: Vec<Edge>,
}

fn step_around(value: usize, increment: isize, max: usize) -> usize {
    let next = value as isize + increment;
    if next < 0 {
        max - 1
    } else if next as usize == max {
        0
    } else {
        next as usize
    }
}

/// `miPolyBuildPoly`
fn build_poly(vertices: &[Vertex], slopes: &[Slope], xi: i32, yi: i32) -> Poly {
    let count = vertices.len();
    let mut top = 0;
    let mut bottom = 0;
    let mut miny = vertices[0].y;
    let mut maxy = vertices[0].y;
    for (i, vertex) in vertices.iter().enumerate().skip(1) {
        if vertex.y < miny {
            top = i;
            miny = vertex.y;
        }
        if vertex.y >= maxy {
            bottom = i;
            maxy = vertex.y;
        }
    }
    let mut clockwise: isize = 1;
    let mut slopeoff: isize = 0;
    let j = step_around(top, -1, count);
    if i64::from(slopes[j].dy) * i64::from(slopes[top].dx)
        > i64::from(slopes[top].dy) * i64::from(slopes[j].dx)
    {
        clockwise = -1;
        slopeoff = -1;
    }
    let bottomy = iceil(maxy) + yi;

    let mut right = Vec::new();
    let mut lasty = 0;
    let mut topy = 0;
    let mut s = step_around(top, slopeoff, count);
    let mut i = top;
    while i != bottom {
        if slopes[s].dy != 0 {
            let mut edge = Edge::default();
            let y = build_edge(
                vertices[i].x,
                vertices[i].y,
                slopes[s].k,
                slopes[s].dx,
                slopes[s].dy,
                xi,
                yi,
                false,
                &mut edge,
            );
            if let Some(previous) = right.last_mut() {
                let previous: &mut Edge = previous;
                previous.height = y - lasty;
            } else {
                topy = y;
            }
            right.push(edge);
            lasty = y;
        }
        i = step_around(i, clockwise, count);
        s = step_around(s, clockwise, count);
    }
    if let Some(last) = right.last_mut() {
        last.height = bottomy - lasty;
    }

    slopeoff = if slopeoff == 0 { -1 } else { 0 };
    let mut left = Vec::new();
    s = step_around(top, slopeoff, count);
    i = top;
    while i != bottom {
        if slopes[s].dy != 0 {
            let mut edge = Edge::default();
            let y = build_edge(
                vertices[i].x,
                vertices[i].y,
                slopes[s].k,
                slopes[s].dx,
                slopes[s].dy,
                xi,
                yi,
                true,
                &mut edge,
            );
            if let Some(previous) = left.last_mut() {
                let previous: &mut Edge = previous;
                previous.height = y - lasty;
            }
            left.push(edge);
            lasty = y;
        }
        i = step_around(i, -clockwise, count);
        s = step_around(s, -clockwise, count);
    }
    if let Some(last) = left.last_mut() {
        last.height = bottomy - lasty;
    }
    Poly {
        y: topy,
        height: bottomy - topy,
        left,
        right,
    }
}

/// `miLineArcI`: a round cap or join centred on an integer point. `mi`
/// fills a fixed array of `lw` spans from both ends; for even widths the
/// two halves meet on one slot and the last write wins, so the array is
/// kept rather than a list grown.
fn line_arc_integer(lw: i32, xorg: i32, yorg: i32) -> Vec<XSpan> {
    let size = usize::try_from(lw.max(1)).unwrap_or(1);
    let mut spans = vec![
        XSpan {
            x: 0,
            y: 0,
            width: 0
        };
        size
    ];
    if lw == 1 {
        spans[0] = XSpan {
            x: xorg,
            y: yorg,
            width: 1,
        };
        return spans;
    }
    let mut top = 0usize;
    let mut bottom = size;
    let mut y = (lw >> 1) + 1;
    let mut e = if lw & 1 == 1 {
        -((y << 2) + 3)
    } else {
        -(y << 3)
    };
    let mut ex = -4;
    let mut x = 0;
    while y != 0 {
        e += (y << 3) - 4;
        while e >= 0 {
            x += 1;
            ex = -((x << 3) + 4);
            e += ex;
        }
        y -= 1;
        let mut slw = (x << 1) + 1;
        if e == ex && slw > 1 {
            slw -= 1;
        }
        if let Some(slot) = spans.get_mut(top) {
            *slot = XSpan {
                x: xorg - x,
                y: yorg - y,
                width: slw,
            };
        }
        top += 1;
        if y != 0 && (slw > 1 || e != ex) {
            bottom = bottom.saturating_sub(1);
            if let Some(slot) = spans.get_mut(bottom) {
                *slot = XSpan {
                    x: xorg - x,
                    y: yorg + y,
                    width: slw,
                };
            }
        }
    }
    spans
}

/// `CLIPSTEPEDGE`: narrow the row to an edge's side once the walk reaches it.
fn clip_step_edge(
    ybase: i32,
    edgey: &mut i32,
    edge: &mut Edge,
    edgeleft: bool,
    xcl: &mut i32,
    xcr: &mut i32,
) {
    if ybase == *edgey {
        if edgeleft {
            if edge.x > *xcl {
                *xcl = edge.x;
            }
        } else if edge.x < *xcr {
            *xcr = edge.x;
        }
        *edgey += 1;
        edge.x += edge.stepx;
        edge.e += edge.dx;
        if edge.e > 0 {
            edge.x += edge.signdx;
            edge.e -= edge.dy;
        }
    }
}

/// `miLineArcD`: a round cap or join at a real centre, clipped by up to two
/// edges so that it covers only what the line body does not.
#[allow(clippy::too_many_arguments)]
fn line_arc_double(
    lw: i32,
    xorg: f64,
    yorg: f64,
    edge1: &mut Edge,
    mut edgey1: i32,
    edgeleft1: bool,
    edge2: &mut Edge,
    mut edgey2: i32,
    edgeleft2: bool,
) -> Vec<XSpan> {
    let mut spans = Vec::new();
    let xbase_f = xorg.floor();
    let xbase = xbase_f as i32;
    let x0 = xorg - xbase_f;
    let mut ybase = iceil(yorg);
    let y0 = yorg - f64::from(ybase);
    let xlk = x0 + x0 + 1.0;
    let xrk = x0 + x0 - 1.0;
    let yk = y0 + y0 - 1.0;
    let radius = f64::from(lw) / 2.0;
    let mut y = (radius - y0 + 1.0).floor() as i32;
    ybase -= y;
    let mut ymin = ybase;
    let mut ymax = 65_536;
    let mut edge1_is_min = false;
    let ymin1 = edgey1;
    if edge1.dy >= 0 {
        if edge1.dy == 0 {
            if edgeleft1 {
                edge1_is_min = true;
            } else {
                ymax = edgey1;
            }
            edgey1 = 65_536;
        } else if (edge1.signdx < 0) == edgeleft1 {
            edge1_is_min = true;
        }
    }
    let mut edge2_is_min = false;
    let ymin2 = edgey2;
    if edge2.dy >= 0 {
        if edge2.dy == 0 {
            if edgeleft2 {
                edge2_is_min = true;
            } else {
                ymax = edgey2;
            }
            edgey2 = 65_536;
        } else if (edge2.signdx < 0) == edgeleft2 {
            edge2_is_min = true;
        }
    }
    if edge1_is_min {
        ymin = ymin1;
        if edge2_is_min && ymin1 > ymin2 {
            ymin = ymin2;
        }
    } else if edge2_is_min {
        ymin = ymin2;
    }
    let mut el = radius * radius - ((f64::from(y) + y0) * (f64::from(y) + y0)) - (x0 * x0);
    let mut er = el + xrk;
    let mut xl = 1;
    let mut xr = 0;
    if x0 < 0.5 {
        xl = 0;
        el -= xlk;
    }
    let mut boty = if y0 < -0.5 { 1 } else { 0 };
    if ybase + y - boty > ymax {
        boty = ymax - ybase - y;
    }
    let row = |ybase: i32,
               xl: i32,
               xr: i32,
               edgey1: &mut i32,
               edgey2: &mut i32,
               edge1: &mut Edge,
               edge2: &mut Edge,
               spans: &mut Vec<XSpan>| {
        let mut xcl = xl + xbase;
        let mut xcr = xr + xbase;
        clip_step_edge(ybase, edgey1, edge1, edgeleft1, &mut xcl, &mut xcr);
        clip_step_edge(ybase, edgey2, edge2, edgeleft2, &mut xcl, &mut xcr);
        if xcr >= xcl {
            spans.push(XSpan {
                x: xcl,
                y: ybase,
                width: xcr - xcl + 1,
            });
        }
    };
    while y > boty {
        let k = f64::from(y * 2) + yk;
        er += k;
        while er > 0.0 {
            xr += 1;
            er += xrk - f64::from(xr * 2);
        }
        el += k;
        while el >= 0.0 {
            xl -= 1;
            el += f64::from(xl * 2) - xlk;
        }
        y -= 1;
        ybase += 1;
        if ybase < ymin {
            continue;
        }
        row(
            ybase,
            xl,
            xr,
            &mut edgey1,
            &mut edgey2,
            edge1,
            edge2,
            &mut spans,
        );
    }
    er = xrk - f64::from(xr * 2) - er;
    el = f64::from(xl * 2) - xlk - el;
    boty = (-y0 - radius + 1.0).floor() as i32;
    if ybase + y - boty > ymax {
        boty = ymax - ybase - y;
    }
    while y > boty {
        let k = f64::from(y * 2) + yk;
        er -= k;
        while er >= 0.0 && xr >= 0 {
            xr -= 1;
            er += xrk - f64::from(xr * 2);
        }
        el -= k;
        while el > 0.0 && xl <= 0 {
            xl += 1;
            el += f64::from(xl * 2) - xlk;
        }
        y -= 1;
        ybase += 1;
        if ybase < ymin {
            continue;
        }
        row(
            ybase,
            xl,
            xr,
            &mut edgey1,
            &mut edgey2,
            edge1,
            edge2,
            &mut spans,
        );
    }
    spans
}

/// `miRoundJoinFace`
fn round_join_face(face: &Face, edge: &mut Edge, left_edge: &mut bool) -> i32 {
    let mut dx = -face.dy;
    let mut dy = face.dx;
    let mut xa = face.xa;
    let mut ya = face.ya;
    let mut left = true;
    if ya > 0.0 {
        ya = 0.0;
        xa = 0.0;
    }
    if dy < 0 || (dy == 0 && dx > 0) {
        dx = -dx;
        dy = -dy;
        left = !left;
    }
    if dx == 0 && dy == 0 {
        dy = 1;
    }
    let y;
    if dy == 0 {
        y = iceil(face.ya) + face.y;
        *edge = Edge {
            x: -32_767,
            stepx: 0,
            signdx: 0,
            e: -1,
            dy: 0,
            dx: 0,
            height: 0,
        };
    } else {
        y = build_edge(xa, ya, 0.0, dx, dy, face.x, face.y, !left, edge);
        edge.height = 32_767;
    }
    *left_edge = !left;
    y
}

/// `miRoundJoinClip`
#[allow(clippy::too_many_arguments)]
fn round_join_clip(
    p_left: &mut Face,
    p_right: &mut Face,
    edge1: &mut Edge,
    edge2: &mut Edge,
    y1: &mut i32,
    y2: &mut i32,
    left1: &mut bool,
    left2: &mut bool,
) {
    let denom = -f64::from(p_left.dx) * f64::from(p_right.dy)
        + f64::from(p_right.dx) * f64::from(p_left.dy);
    if denom >= 0.0 {
        p_left.xa = -p_left.xa;
        p_left.ya = -p_left.ya;
    } else {
        p_right.xa = -p_right.xa;
        p_right.ya = -p_right.ya;
    }
    *y1 = round_join_face(p_left, edge1, left1);
    *y2 = round_join_face(p_right, edge2, left2);
}

/// `miRoundCapClip`
fn round_cap_clip(face: &Face, is_int: bool, edge: &mut Edge, left_edge: &mut bool) -> i32 {
    let mut dx = -face.dy;
    let mut dy = face.dx;
    let mut xa = face.xa;
    let mut ya = face.ya;
    let k = if is_int { 0.0 } else { face.k };
    let mut left = true;
    if dy < 0 || (dy == 0 && dx > 0) {
        dx = -dx;
        dy = -dy;
        xa = -xa;
        ya = -ya;
        left = !left;
    }
    if dx == 0 && dy == 0 {
        dy = 1;
    }
    let y;
    if dy == 0 {
        y = iceil(face.ya) + face.y;
        *edge = Edge {
            x: -32_767,
            stepx: 0,
            signdx: 0,
            e: -1,
            dy: 0,
            dx: 0,
            height: 0,
        };
    } else {
        y = build_edge(xa, ya, k, dx, dy, face.x, face.y, !left, edge);
        edge.height = 32_767;
    }
    *left_edge = !left;
    y
}

/// `miStepDash`: advance `dist` pixels into the dash pattern.
fn step_dash(mut dist: i32, index: &mut usize, dashes: &[u8], offset: &mut i32) {
    let dash = |i: usize| i32::from(dashes[i]);
    if dist < dash(*index) - *offset {
        *offset += dist;
        return;
    }
    dist -= dash(*index) - *offset;
    *index += 1;
    if *index == dashes.len() {
        *index = 0;
    }
    let total: i32 = dashes.iter().map(|length| i32::from(*length)).sum();
    if total <= dist {
        dist %= total;
    }
    while dist >= dash(*index) {
        dist -= dash(*index);
        *index += 1;
        if *index == dashes.len() {
            *index = 0;
        }
    }
    *offset = dist;
}

/// A dash list `mi` can walk: non-empty with no zero-length dash. The GC
/// refuses anything else, but a default GC's list is checked all the same.
fn dashes_walkable(dashes: &[u8]) -> bool {
    !dashes.is_empty() && dashes.iter().all(|length| *length > 0)
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
/// `miZeroLine` when solid; a dashed zero-width line is, as in `mi`'s
/// `miZeroDashLine`, the wide dash at width one.
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
    let one = XGraphicsContextValues {
        line_width: 1,
        ..gc.clone()
    };
    polyline(points, &one)
}

/// `miPolySegment`: each segment its own two-point polyline, so each has
/// its own caps and its own dash phase.
pub fn segments(segments: &[(XPoint, XPoint)], gc: &XGraphicsContextValues) -> XInkedSpans {
    segments
        .iter()
        .flat_map(|(from, to)| polylines(&[*from, *to], gc))
        .collect()
}

/// `miPolyRectangle`. A solid, mitred, wide outline is four filled bands;
/// anything else, zero width included, is the closed polyline around each
/// rectangle, which joins where it starts.
pub fn rectangles(rectangles: &[Rect], gc: &XGraphicsContextValues) -> XInkedSpans {
    let clamp_min = |value: i32| value.max(-32_768);
    let clamp_max = |value: i32| value.min(32_767);
    let clamp_umax = |value: i32| value.min(65_535);
    let point = |x: i32, y: i32| XPoint {
        x: i16::try_from(x).unwrap_or(if x < 0 { i16::MIN } else { i16::MAX }),
        y: i16::try_from(y).unwrap_or(if y < 0 { i16::MIN } else { i16::MAX }),
    };
    if gc.line_style == X_LINE_SOLID && gc.join_style == X_JOIN_MITER && gc.line_width != 0 {
        let offset2 = i32::from(gc.line_width);
        let offset1 = offset2 >> 1;
        let offset3 = offset2 - offset1;
        let mut out = Vec::new();
        let mut bands = Vec::new();
        for rect in rectangles {
            let (x, y, width, height) = (rect.x, rect.y, rect.width, rect.height);
            if width == 0 && height == 0 {
                out.extend(polyline(&[point(x, y), point(x, y)], gc));
            } else if height < offset2 || width < offset1 {
                let (bx, bw) = if height == 0 {
                    (x, width)
                } else {
                    (clamp_min(x - offset1), clamp_umax(width + offset2))
                };
                let (by, bh) = if width == 0 {
                    (y, height)
                } else {
                    (clamp_min(y - offset1), clamp_umax(height + offset2))
                };
                bands.push((bx, by, bw, bh));
            } else {
                bands.push((
                    clamp_min(x - offset1),
                    clamp_min(y - offset1),
                    clamp_umax(width + offset2),
                    offset2,
                ));
                bands.push((
                    clamp_min(x - offset1),
                    clamp_max(y + offset3),
                    offset2,
                    height - offset2,
                ));
                bands.push((
                    clamp_max(x + width - offset1),
                    clamp_max(y + offset3),
                    offset2,
                    height - offset2,
                ));
                bands.push((
                    clamp_min(x - offset1),
                    clamp_max(y + height - offset1),
                    clamp_umax(width + offset2),
                    offset2,
                ));
            }
        }
        // `PolyFillRect` of the bands, after any degenerate polylines, as
        // `mi` issues them.
        let spans: Vec<XSpan> = bands
            .into_iter()
            .flat_map(|(x, y, width, height)| {
                (0..height.max(0)).map(move |row| XSpan {
                    x,
                    y: y + row,
                    width,
                })
            })
            .collect();
        if !spans.is_empty() {
            out.push((gc.foreground, spans));
        }
        return out;
    }
    rectangles
        .iter()
        .flat_map(|rect| {
            let right = clamp_max(rect.x + rect.width);
            let bottom = clamp_max(rect.y + rect.height);
            let outline = polylines(
                &[
                    point(rect.x, rect.y),
                    point(right, rect.y),
                    point(right, bottom),
                    point(rect.x, bottom),
                    point(rect.x, rect.y),
                ],
                gc,
            );
            once_per_rectangle(outline, gc)
        })
        .collect()
}

/// The protocol's rule for PolyRectangle: "for any given rectangle, no pixel
/// is drawn more than once". A zero-width outline of a rectangle with no
/// width or height runs down a line and back, and `mi` paints the return
/// over the way out -- under GXxor, erasing it. The pixels are `mi`'s; each
/// is painted once. A wide outline needs no help: its span groups already
/// paint a union.
fn once_per_rectangle(outline: XInkedSpans, gc: &XGraphicsContextValues) -> XInkedSpans {
    if gc.line_width != 0 {
        return outline;
    }
    let mut seen = std::collections::BTreeSet::new();
    outline
        .into_iter()
        .map(|(pixel, spans)| {
            let spans = spans
                .into_iter()
                .filter(|span| seen.insert((span.x, span.y)))
                .collect::<Vec<_>>();
            (pixel, spans)
        })
        .filter(|(_, spans)| !spans.is_empty())
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
