//! `miWideDash` and `miWideDashSegment` from `mi/miwideline.c`, and
//! `miStepDash` from `mi/midash.c`: dashed wide lines, run by run.

use super::*;

impl Lines<'_> {
    /// `miWideDashSegment`
    #[allow(clippy::too_many_arguments)]
    pub(super) fn wide_dash_segment(
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

    /// `miWideDash`, for points in `CoordModeOrigin`.
    pub(super) fn wide_dash(&mut self, points: &[XPoint]) {
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

/// `miStepDash`: advance `dist` pixels into the dash pattern.
pub(in crate::software::geometry) fn step_dash(
    mut dist: i32,
    index: &mut usize,
    dashes: &[u8],
    offset: &mut i32,
) {
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
pub(super) fn dashes_walkable(dashes: &[u8]) -> bool {
    !dashes.is_empty() && dashes.iter().all(|length| *length > 0)
}
