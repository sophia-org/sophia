//! `miLineArc` and its helpers from `mi/miwideline.c`: round caps and
//! joins, as spans or as edges clipped against a face.

use super::*;

impl Lines<'_> {
    /// `miLineArc`
    pub(super) fn arc(
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
}

/// `miLineArcI`: a round cap or join centred on an integer point. `mi`
/// fills a fixed array of `lw` spans from both ends; for even widths the
/// two halves meet on one slot and the last write wins, so the array is
/// kept rather than a list grown.
pub(super) fn line_arc_integer(lw: i32, xorg: i32, yorg: i32) -> Vec<XSpan> {
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
pub(super) fn clip_step_edge(
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
pub(super) fn line_arc_double(
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
pub(super) fn round_join_face(face: &Face, edge: &mut Edge, left_edge: &mut bool) -> i32 {
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
pub(super) fn round_join_clip(
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
pub(super) fn round_cap_clip(
    face: &Face,
    is_int: bool,
    edge: &mut Edge,
    left_edge: &mut bool,
) -> i32 {
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
