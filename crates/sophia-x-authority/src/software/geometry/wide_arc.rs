//! Wide and dashed arcs, as the X server's `mi` draws them.
//!
//! Copyright 1987, 1998 The Open Group. Copyright 1987 by Digital Equipment
//! Corporation, Maynard, Massachusetts. Both permission notices are kept in
//! full in `THIRD-PARTY-NOTICES.md`.
//!
//! A port of `mi/miarc.c` (Keith Packard and Bob Scheifler), in three parts:
//! this file sequences arcs -- `miComputeArcs`, which walks the dash pattern
//! along each arc and decides where arcs join and where they are capped, its
//! dash length map, and `miWideArc`; `spans` scan-converts one wide arc;
//! `faces` draws the caps and joins. As with the other ports, `mi`'s
//! arithmetic is kept expression by expression, because the pixels depend on
//! its rounding, and its oddities are kept with a note.
//!
//! One translation of `mi`'s mechanics: for a raster function that reads the
//! destination, `mi` draws each rendered group into a scratch bitmap and
//! pushes it through the GC once; for the others it paints directly, and a
//! pixel painted twice comes out the same. Here each rendered group is the
//! union of what it drew, painted once -- the same pixels either way --
//! clipped for the first kind to the scratch bitmap's extent, as `mi`'s is.

mod faces;
mod spans;
mod tests;

use super::arc::XArc;
use super::wide_line::{XInkedSpans, XSpan};
use crate::{
    X_FILL_OPAQUE_STIPPLED, X_FILL_TILED, X_LINE_DOUBLE_DASH, X_LINE_SOLID, XGraphicsContextValues,
};
use faces::FaceOrigin;
use spans::{Canvas, FULLCIRCLE};

const EPSILON: f64 = 0.000_001;
pub(super) const RIGHT_END: usize = 0;
pub(super) const LEFT_END: usize = 1;

/// `SppPointRec`: a point at sub-pixel position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Spp {
    pub x: f64,
    pub y: f64,
}

/// `miArcFaceRec`: where an arc ends -- its centre line and the two edges.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Face {
    pub clock: Spp,
    pub center: Spp,
    pub counter_clock: Spp,
}

/// `mi`'s trigonometry in degrees, exact at multiples of 90 so the
/// cardinal points land where they should.
pub(super) fn mi_dcos(a: f64) -> f64 {
    if (a / 90.0).floor() == a / 90.0 {
        return match ((a / 90.0) as i32).rem_euclid(4) {
            0 => 1.0,
            2 => -1.0,
            _ => 0.0,
        };
    }
    (a * std::f64::consts::PI / 180.0).cos()
}

pub(super) fn mi_dsin(a: f64) -> f64 {
    if (a / 90.0).floor() == a / 90.0 {
        return match ((a / 90.0) as i32).rem_euclid(4) {
            1 => 1.0,
            3 => -1.0,
            _ => 0.0,
        };
    }
    (a * std::f64::consts::PI / 180.0).sin()
}

pub(super) fn mi_dasin(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else if v == 1.0 {
        90.0
    } else if v == -1.0 {
        -90.0
    } else {
        v.asin() * (180.0 / std::f64::consts::PI)
    }
}

pub(super) fn mi_datan2(dy: f64, dx: f64) -> f64 {
    if dy == 0.0 {
        if dx >= 0.0 { 0.0 } else { 180.0 }
    } else if dx == 0.0 {
        if dy > 0.0 { 90.0 } else { -90.0 }
    } else if dy.abs() == dx.abs() {
        match (dy > 0.0, dx > 0.0) {
            (true, true) => 45.0,
            (true, false) => 135.0,
            (false, true) => 315.0,
            (false, false) => 225.0,
        }
    } else {
        dy.atan2(dx) * (180.0 / std::f64::consts::PI)
    }
}

/// `miArcDataRec`
#[derive(Clone, Copy, Debug)]
struct ArcData {
    arc: XArc,
    render: bool,
    join: usize,
    cap: usize,
    self_join: bool,
    bounds: [Face; 2],
}

/// `miArcCapRec`
#[derive(Clone, Copy, Debug)]
struct Cap {
    arc_index: usize,
    end: usize,
}

/// `miArcJoinRec`
#[derive(Clone, Copy, Debug)]
struct Join {
    arc_index0: usize,
    arc_index1: usize,
    phase0: usize,
    phase1: usize,
    end0: usize,
    end1: usize,
}

/// `miPolyArcRec`: one phase's arcs, caps and joins.
#[derive(Clone, Debug, Default)]
struct PolyArc {
    arcs: Vec<ArcData>,
    caps: Vec<Cap>,
    joins: Vec<Join>,
}

impl PolyArc {
    fn add_arc(&mut self, arc: XArc) -> usize {
        self.arcs.push(ArcData {
            arc,
            render: false,
            join: 0,
            cap: 0,
            self_join: false,
            bounds: [Face::default(); 2],
        });
        self.arcs.len() - 1
    }

    fn add_cap(&mut self, end: usize, arc_index: usize) {
        self.caps.push(Cap { arc_index, end });
    }
}

const DASH_MAP_SIZE: usize = 91;
/// `dashXAngleStep`
const DASH_X_ANGLE_STEP: f64 = (90.0 * 64.0) / (DASH_MAP_SIZE as f64 - 1.0);

/// `dashMap`: arc length from 0 degrees, sampled across the first quadrant.
struct DashMap([f64; DASH_MAP_SIZE]);

/// `computeDashMap`
fn dash_map(arc: &XArc) -> DashMap {
    let mut map = [0.0; DASH_MAP_SIZE];
    let (mut px, mut py) = (0.0, 0.0);
    for di in 0..DASH_MAP_SIZE {
        let a = (di as f64 * 90.0) / (DASH_MAP_SIZE as f64 - 1.0);
        let x = (f64::from(arc.width) / 2.0) * mi_dcos(a);
        let y = (f64::from(arc.height) / 2.0) * mi_dsin(a);
        map[di] = if di == 0 {
            0.0
        } else {
            map[di - 1] + (x - px).hypot(y - py)
        };
        px = x;
        py = y;
    }
    DashMap(map)
}

fn x_angle_to_dash_index(angle: i32) -> usize {
    usize::try_from((i64::from(angle) * (DASH_MAP_SIZE as i64 - 1)) / (90 * 64)).unwrap_or(0)
}

fn dash_index_to_x_angle(di: usize) -> i32 {
    i32::try_from((di as i64 * (90 * 64)) / (DASH_MAP_SIZE as i64 - 1)).unwrap_or(0)
}

/// `angleToLength`
fn angle_to_length(mut angle: i32, map: &DashMap) -> f64 {
    let sidelen = map.0[DASH_MAP_SIZE - 1];
    let mut totallen = 0.0;
    let mut odd_side = false;
    if angle >= 0 {
        while angle >= 90 * 64 {
            angle -= 90 * 64;
            totallen += sidelen;
            odd_side = !odd_side;
        }
    } else {
        while angle < 0 {
            angle += 90 * 64;
            totallen -= sidelen;
            odd_side = !odd_side;
        }
    }
    if odd_side {
        angle = 90 * 64 - angle;
    }
    let di = x_angle_to_dash_index(angle);
    let excess = angle - dash_index_to_x_angle(di);
    let mut len = map.0[di];
    // linearly interpolate between this point and the next
    if excess > 0 {
        len += (map.0[di + 1] - map.0[di]) * f64::from(excess) / DASH_X_ANGLE_STEP;
    }
    if odd_side {
        totallen += sidelen - len;
    } else {
        totallen += len;
    }
    totallen
}

/// `lengthToAngle`
fn length_to_angle(mut len: f64, map: &DashMap) -> i32 {
    let sidelen = map.0[DASH_MAP_SIZE - 1];
    let mut angle = 0;
    let mut odd_side = false;
    if len >= 0.0 {
        if sidelen == 0.0 {
            return 2 * FULLCIRCLE;
        }
        while len >= sidelen {
            angle += 90 * 64;
            len -= sidelen;
            odd_side = !odd_side;
        }
    } else {
        if sidelen == 0.0 {
            return -2 * FULLCIRCLE;
        }
        while len < 0.0 {
            angle -= 90 * 64;
            len += sidelen;
            odd_side = !odd_side;
        }
    }
    if odd_side {
        len = sidelen - len;
    }
    let (mut a0, mut a1) = (0usize, DASH_MAP_SIZE - 1);
    // binary search for the closest pre-computed length
    while a1 - a0 > 1 {
        let a = (a0 + a1) / 2;
        if len > map.0[a] {
            a0 = a;
        } else {
            a1 = a;
        }
    }
    // An int incremented by a double: truncated, as C converts it.
    let angleexcess = (f64::from(dash_index_to_x_angle(a0))
        + (len - map.0[a0]) / (map.0[a0 + 1] - map.0[a0]) * DASH_X_ANGLE_STEP)
        as i32;
    if odd_side {
        angle += (90 * 64) - angleexcess;
    } else {
        angle += angleexcess;
    }
    angle
}

/// `computeAngleFromPath`: where along the arc `*len` more of the dash
/// pattern ends; `*len` becomes what is left over.
fn angle_from_path(
    start_angle: i32,
    end_angle: i32,
    map: &DashMap,
    lenp: &mut i32,
    backwards: bool,
) -> i32 {
    let (mut a0, mut a1) = (start_angle, end_angle);
    let mut len = *lenp;
    if backwards {
        a0 = FULLCIRCLE - a0;
        a1 = FULLCIRCLE - a1;
    }
    if a1 < a0 {
        a1 += FULLCIRCLE;
    }
    let len0 = angle_to_length(a0, map);
    let mut a = length_to_angle(len0 + f64::from(len), map);
    if a > a1 {
        a = a1;
        len = (f64::from(len) - (angle_to_length(a1, map) - len0)) as i32;
    } else {
        len = 0;
    }
    if backwards {
        a = FULLCIRCLE - a;
    }
    *lenp = len;
    a
}

/// `struct arcData`: an arc's end points, for deciding where arcs join.
#[derive(Clone, Copy, Debug, Default)]
struct Ends {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    self_join: bool,
}

fn unequal(a: f64, b: f64) -> bool {
    (a - b).abs() > EPSILON
}

fn normalize(angle: i32) -> i32 {
    let mut angle = angle;
    if angle < 0 {
        angle = FULLCIRCLE - (-angle) % FULLCIRCLE;
    }
    if angle >= FULLCIRCLE {
        angle %= FULLCIRCLE;
    }
    angle
}

/// `miComputeArcs`: the arcs of each phase, cut at dashes, with their caps
/// and joins. "This routine is a bit gory", as `mi` says.
#[allow(clippy::too_many_lines)]
fn compute_arcs(parcs: &[XArc], gc: &XGraphicsContextValues) -> Vec<PolyArc> {
    let narcs = parcs.len();
    let is_dashed = gc.line_style != X_LINE_SOLID;
    let is_double = gc.line_style == X_LINE_DOUBLE_DASH;
    let dashes = &gc.dashes;
    let mut dash_offset = i32::from(gc.dash_offset);
    let mut arcs = vec![PolyArc::default(); if is_double { 2 } else { 1 }];
    let data: Vec<Ends> = parcs
        .iter()
        .map(|arc| {
            let a0 = f64::from(arc.angle1) / 64.0;
            let angle2 = i32::from(arc.angle2).clamp(-FULLCIRCLE, FULLCIRCLE);
            let a1 = f64::from(i32::from(arc.angle1) + angle2) / 64.0;
            let (x, y) = (f64::from(arc.x), f64::from(arc.y));
            let (w, h) = (f64::from(arc.width), f64::from(arc.height));
            Ends {
                x0: x + w / 2.0 * (1.0 + mi_dcos(a0)),
                y0: y + h / 2.0 * (1.0 - mi_dsin(a0)),
                x1: x + w / 2.0 * (1.0 + mi_dcos(a1)),
                y1: y + h / 2.0 * (1.0 - mi_dsin(a1)),
                self_join: angle2 == FULLCIRCLE || angle2 == -FULLCIRCLE,
            }
        })
        .collect();
    let dash = |index: usize| i32::from(dashes.get(index).copied().unwrap_or(1));
    let mut iphase = 0usize;
    let (mut i_dash, mut dash_remaining) = (0usize, 0i32);
    if is_dashed {
        dash_remaining = dash(0);
        while dash_offset > 0 {
            if dash_offset >= dash_remaining {
                dash_offset -= dash_remaining;
                iphase = 1 - iphase;
                i_dash += 1;
                if i_dash == dashes.len() {
                    i_dash = 0;
                }
                dash_remaining = dash(i_dash);
            } else {
                dash_remaining -= dash_offset;
                dash_offset = 0;
            }
        }
    }
    let (i_dash_start, dash_remaining_start, iphase_start) = (i_dash, dash_remaining, iphase);

    // Find where the sequence starts: after the last arc that does not join
    // the one after it.
    let mut i: isize = narcs as isize - 1;
    while i >= 0 {
        let iu = i as usize;
        let j = if iu + 1 == narcs { 0 } else { iu + 1 };
        if data[iu].self_join
            || iu == j
            || unequal(data[iu].x1, data[j].x0)
            || unequal(data[iu].y1, data[j].y0)
        {
            if iphase == 0 || is_double {
                arcs[iphase].add_cap(RIGHT_END, 0);
            }
            break;
        }
        i -= 1;
    }
    let mut start = (i + 1) as usize;
    if start == narcs {
        start = 0;
    }
    let mut i = start;
    let mut prevphase = 0usize;
    let (mut k, mut nextk) = (0usize, 0usize);
    loop {
        let j = if i + 1 == narcs { 0 } else { i + 1 };
        let nexti = j;
        let mut arc: Option<(usize, usize)> = None;
        if is_dashed {
            // Special rules for certain zero-area arcs, as mi has them.
            #[derive(PartialEq)]
            enum Kind {
                Horizontal,
                Vertical,
                Other,
            }
            let parc = parcs[i];
            let kind = if parc.height == 0
                && i32::from(parc.angle1) % FULLCIRCLE == 0x2d00
                && parc.angle2 == 0x2d00
            {
                Kind::Horizontal
            } else if parc.width == 0
                && i32::from(parc.angle1) % FULLCIRCLE == 0x1680
                && parc.angle2 == 0x2d00
            {
                Kind::Vertical
            } else {
                Kind::Other
            };
            let mut xarc = parc;
            let (start_angle, end_angle);
            let mut backwards = false;
            let mut map = None;
            if kind == Kind::Other {
                map = Some(dash_map(&parc));
                let span_angle = i32::from(parc.angle2).clamp(-FULLCIRCLE, FULLCIRCLE);
                start_angle = normalize(i32::from(parc.angle1));
                end_angle = start_angle + span_angle;
                backwards = span_angle < 0;
            } else if kind == Kind::Vertical {
                xarc.angle1 = 0x1680;
                start_angle = i32::from(parc.y);
                end_angle = start_angle + i32::from(parc.height);
            } else {
                xarc.angle1 = 0x2d00;
                start_angle = i32::from(parc.x);
                end_angle = start_angle + i32::from(parc.width);
            }
            let mut dash_angle = start_angle;
            let self_join = data[i].self_join && (iphase == 0 || is_double);
            while dash_angle != end_angle {
                let prev_dash_angle = dash_angle;
                if let Some(map) = &map {
                    dash_angle = angle_from_path(
                        prev_dash_angle,
                        end_angle,
                        map,
                        &mut dash_remaining,
                        backwards,
                    );
                    // avoid troubles with huge arcs and small dashes
                    if dash_angle == prev_dash_angle {
                        if backwards {
                            dash_angle -= 1;
                        } else {
                            dash_angle += 1;
                        }
                    }
                } else {
                    // CARD16, as mi declares it.
                    let this_length = if dash_angle + dash_remaining <= end_angle {
                        dash_remaining
                    } else {
                        end_angle - dash_angle
                    } as u16;
                    if kind == Kind::Vertical {
                        xarc.y = dash_angle as i16;
                        xarc.height = this_length;
                    } else {
                        xarc.x = dash_angle as i16;
                        xarc.width = this_length;
                    }
                    dash_angle += i32::from(this_length);
                    dash_remaining -= i32::from(this_length);
                }
                if iphase == 0 || is_double {
                    if kind == Kind::Other {
                        xarc = parc;
                        xarc.angle1 = normalize(prev_dash_angle) as i16;
                        let mut span_angle = dash_angle - prev_dash_angle;
                        if backwards {
                            if dash_angle > prev_dash_angle {
                                span_angle += -FULLCIRCLE;
                            }
                        } else if dash_angle < prev_dash_angle {
                            span_angle += FULLCIRCLE;
                        }
                        xarc.angle2 = span_angle.clamp(-FULLCIRCLE, FULLCIRCLE) as i16;
                    }
                    let index = arcs[iphase].add_arc(xarc);
                    // cap each end of an on/off dash
                    if !is_double {
                        if prev_dash_angle != start_angle {
                            arcs[iphase].add_cap(RIGHT_END, index);
                        }
                        if dash_angle != end_angle {
                            arcs[iphase].add_cap(LEFT_END, index);
                        }
                    }
                    let (ncaps, njoins) = (arcs[iphase].caps.len(), arcs[iphase].joins.len());
                    let data_arc = &mut arcs[iphase].arcs[index];
                    data_arc.cap = ncaps;
                    data_arc.join = njoins;
                    data_arc.render = false;
                    data_arc.self_join = dash_angle == end_angle && self_join;
                    arc = Some((iphase, index));
                }
                prevphase = iphase;
                if dash_remaining <= 0 {
                    i_dash += 1;
                    if i_dash == dashes.len() {
                        i_dash = 0;
                    }
                    iphase = 1 - iphase;
                    dash_remaining = dash(i_dash);
                }
            }
            // a place for the position data of a zero-length arc
            if start_angle == end_angle {
                prevphase = iphase;
                if !is_double && iphase == 1 {
                    prevphase = 0;
                }
                let index = arcs[prevphase].add_arc(parcs[i]);
                let (ncaps, njoins) = (arcs[prevphase].caps.len(), arcs[prevphase].joins.len());
                let data_arc = &mut arcs[prevphase].arcs[index];
                data_arc.join = njoins;
                data_arc.cap = ncaps;
                data_arc.self_join = data[i].self_join;
                arc = Some((prevphase, index));
            }
        } else {
            let index = arcs[iphase].add_arc(parcs[i]);
            let (ncaps, njoins) = (arcs[iphase].caps.len(), arcs[iphase].joins.len());
            let data_arc = &mut arcs[iphase].arcs[index];
            data_arc.join = njoins;
            data_arc.cap = ncaps;
            data_arc.self_join = data[i].self_join;
            prevphase = iphase;
            arc = Some((iphase, index));
        }
        if prevphase == 0 || is_double {
            k = arcs[prevphase].arcs.len().saturating_sub(1);
        }
        if iphase == 0 || is_double {
            nextk = arcs[iphase].arcs.len();
        }
        if nexti == start {
            nextk = 0;
            if is_dashed {
                i_dash = i_dash_start;
                iphase = iphase_start;
                dash_remaining = dash_remaining_start;
            }
        }
        let arcs_join = narcs > 1
            && i != j
            && !unequal(data[i].x1, data[j].x0)
            && !unequal(data[i].y1, data[j].y0)
            && !data[i].self_join
            && !data[j].self_join;
        if let Some((phase, index)) = arc {
            arcs[phase].arcs[index].render = !arcs_join;
        }
        if arcs_join && (prevphase == 0 || is_double) && (iphase == 0 || is_double) {
            let mut joinphase = iphase;
            if is_double {
                if nexti == start {
                    joinphase = iphase_start;
                }
                // A join right at the dash is drawn in the foreground, whose
                // arcs are computed second.
                if joinphase != prevphase {
                    joinphase = 0;
                }
            }
            if joinphase == 0 || is_double {
                arcs[joinphase].joins.push(Join {
                    end0: LEFT_END,
                    arc_index0: k,
                    phase0: prevphase,
                    end1: RIGHT_END,
                    arc_index1: nextk,
                    phase1: iphase,
                });
                let njoins = arcs[prevphase].joins.len();
                if let Some((phase, index)) = arc {
                    arcs[phase].arcs[index].join = njoins;
                }
            }
        } else {
            // cap the left end of this arc unless it joins itself
            let self_joined = arc.is_some_and(|(phase, index)| arcs[phase].arcs[index].self_join);
            if (prevphase == 0 || is_double) && !self_joined {
                arcs[prevphase].add_cap(LEFT_END, k);
                let ncaps = arcs[prevphase].caps.len();
                if let Some((phase, index)) = arc {
                    arcs[phase].arcs[index].cap = ncaps;
                }
            }
            if is_dashed && !arcs_join {
                i_dash = i_dash_start;
                iphase = iphase_start;
                dash_remaining = dash_remaining_start;
            }
            // mi reads the arc count of phase 1 here even for an on/off dash,
            // which has no phase 1 -- past the end of its array. Every later
            // use of the value is guarded by the phase, so it is kept as it was.
            if let Some(phase) = arcs.get(iphase) {
                nextk = phase.arcs.len();
            }
            if nexti == start {
                nextk = 0;
                i_dash = i_dash_start;
                iphase = iphase_start;
                dash_remaining = dash_remaining_start;
            }
            // cap the right end of the next arc; if the next arc is the first,
            // only if it joins this one (a final off dash of an on/off line)
            if (iphase == 0 || is_double) && (nexti != start || (arcs_join && is_dashed)) {
                arcs[iphase].add_cap(RIGHT_END, nextk);
            }
        }
        i = nexti;
        if i == start {
            break;
        }
    }
    // make sure the last section is rendered
    for phase in &mut arcs {
        let (ncaps, njoins) = (phase.caps.len(), phase.joins.len());
        if let Some(last) = phase.arcs.last_mut() {
            last.render = true;
            last.join = njoins;
            last.cap = ncaps;
        }
    }
    arcs
}

/// `miArcSegment`: scan-convert one arc into the canvas, recording its end
/// faces when it has them.
fn arc_segment(canvas: &mut Canvas, line_width: u16, arc: &XArc, faces: Option<&mut [Face; 2]>) {
    let l = if line_width == 0 {
        1
    } else {
        i32::from(line_width)
    };
    if arc.width == 0 || arc.height == 0 {
        spans::draw_zero_arc(canvas, arc, l, faces);
        return;
    }
    let a0 = i32::from(arc.angle1);
    let a1 = i32::from(arc.angle2).clamp(-FULLCIRCLE, FULLCIRCLE);
    let (mut start_angle, mut end_angle, right, left);
    if a1 < 0 {
        start_angle = a0 + a1;
        end_angle = a0;
        right = LEFT_END;
        left = RIGHT_END;
    } else {
        start_angle = a0;
        end_angle = a0 + a1;
        right = RIGHT_END;
        left = LEFT_END;
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
    if end_angle > FULLCIRCLE {
        end_angle = (end_angle - 1) % FULLCIRCLE + 1;
    }
    if start_angle == end_angle && a1 != 0 {
        start_angle = 0;
        end_angle = FULLCIRCLE;
    }
    let sp = spans::wide_ellipse(l, arc);
    spans::draw_arc(
        canvas,
        arc,
        l,
        start_angle,
        end_angle,
        faces,
        right,
        left,
        &sp,
    );
}

fn origin(arc: &XArc) -> FaceOrigin {
    FaceOrigin {
        x: i32::from(arc.x),
        y: i32::from(arc.y),
        fx: f64::from(arc.width) / 2.0,
        fy: f64::from(arc.height) / 2.0,
    }
}

/// The raster functions `mi` draws through a scratch bitmap: all but clear,
/// copy, copy-inverted and set.
fn tricky(function: u8) -> bool {
    !matches!(function, 0 | 3 | 12 | 15)
}

/// `miWideArc`: arcs of width one or more, or of zero width that the thin
/// walker cannot take.
#[allow(clippy::too_many_lines)]
pub fn wide_arcs(parcs: &[XArc], gc: &XGraphicsContextValues) -> XInkedSpans {
    let mut out: XInkedSpans = Vec::new();
    let width = gc.line_width;
    let fg = gc.foreground;
    let mut canvas = Canvas::default();
    if width == 0 && gc.line_style == X_LINE_SOLID {
        for arc in parcs {
            arc_segment(&mut canvas, width, arc, None);
        }
        let spans = canvas.take_union();
        if !spans.is_empty() {
            out.push((fg, spans));
        }
        return out;
    }
    let mut parcs = parcs;
    if gc.line_style == X_LINE_SOLID {
        // Whole solid ellipses at the head of the list are filled directly,
        // before the raster function is looked at.
        while let Some(arc) = parcs.first() {
            let whole = i32::from(arc.angle2) >= FULLCIRCLE || i32::from(arc.angle2) <= -FULLCIRCLE;
            if arc.width == 0 || arc.height == 0 || !whole {
                break;
            }
            let mut spans = Vec::new();
            spans::fill_wide_ellipse(i32::from(width), arc, &mut spans);
            spans.retain(|span| span.width > 0);
            if !spans.is_empty() {
                out.push((fg, spans));
            }
            parcs = &parcs[1..];
        }
        if parcs.is_empty() {
            return out;
        }
    }
    // The scratch bitmap's extent, for the raster functions drawn through
    // one: the arcs' box grown by half the line width, never left of or
    // above the drawable's origin.
    let clip = tricky(gc.function).then(|| {
        let half = (i32::from(width) + 1) / 2;
        let x_min = parcs.iter().map(|arc| i32::from(arc.x)).min().unwrap_or(0) - half;
        let y_min = parcs.iter().map(|arc| i32::from(arc.y)).min().unwrap_or(0) - half;
        let x_max = parcs
            .iter()
            .map(|arc| i32::from(arc.x) + i32::from(arc.width))
            .max()
            .unwrap_or(0)
            + half;
        let y_max = parcs
            .iter()
            .map(|arc| i32::from(arc.y) + i32::from(arc.height))
            .max()
            .unwrap_or(0)
            + half;
        (x_min.max(0), y_min.max(0), x_max, y_max)
    });
    if let Some((x0, y0, x1, y1)) = clip
        && (x1 - x0 <= 0 || y1 - y0 <= 0)
    {
        return out;
    }
    let mut bg = gc.background;
    // the protocol says these don't cause colour changes
    if gc.fill_style == X_FILL_TILED || gc.fill_style == X_FILL_OPAQUE_STIPPLED {
        bg = fg;
    }
    let mut poly = compute_arcs(parcs, gc);
    let mut cap = [0usize; 2];
    let mut join = [0usize; 2];
    let first_phase = usize::from(gc.line_style == X_LINE_DOUBLE_DASH);
    for iphase in (0..=first_phase).rev() {
        let pixel = if iphase == 1 { bg } else { fg };
        for i in 0..poly[iphase].arcs.len() {
            let arc = poly[iphase].arcs[i].arc;
            arc_segment(
                &mut canvas,
                width,
                &arc,
                Some(&mut poly[iphase].arcs[i].bounds),
            );
            if !poly[iphase].arcs[i].render {
                continue;
            }
            // don't cap self-joining arcs
            if poly[iphase].arcs[i].self_join && cap[iphase] < poly[iphase].arcs[i].cap {
                cap[iphase] += 1;
            }
            while cap[iphase] < poly[iphase].arcs[i].cap {
                let Some(c) = poly[iphase].caps.get(cap[iphase]).copied() else {
                    break;
                };
                let data = poly[iphase].arcs[c.arc_index];
                faces::cap(
                    &mut canvas,
                    width,
                    gc.cap_style,
                    &data.bounds[c.end],
                    origin(&data.arc),
                );
                cap[iphase] += 1;
            }
            while join[iphase] < poly[iphase].arcs[i].join {
                let Some(j) = poly[iphase].joins.get(join[iphase]).copied() else {
                    break;
                };
                let data0 = poly
                    .get(j.phase0)
                    .and_then(|phase| phase.arcs.get(j.arc_index0))
                    .copied();
                let data1 = poly
                    .get(j.phase1)
                    .and_then(|phase| phase.arcs.get(j.arc_index1))
                    .copied();
                if let (Some(data0), Some(data1)) = (data0, data1) {
                    faces::join(
                        &mut canvas,
                        width,
                        gc.join_style,
                        &data0.bounds[j.end0],
                        origin(&data0.arc),
                        &data1.bounds[j.end1],
                        origin(&data1.arc),
                    );
                }
                join[iphase] += 1;
            }
            let mut spans = canvas.take_union();
            if let Some((x0, y0, x1, y1)) = clip {
                spans = clip_spans(spans, x0, y0, x1, y1);
            }
            if !spans.is_empty() {
                out.push((pixel, spans));
            }
        }
        // What a phase leaves unrendered is dropped with it, as mi drops it.
        let _ = canvas.take_union();
    }
    out
}

fn clip_spans(spans: Vec<XSpan>, x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<XSpan> {
    spans
        .into_iter()
        .filter(|span| span.y >= y0 && span.y < y1)
        .filter_map(|span| {
            let left = span.x.max(x0);
            let right = (span.x + span.width).min(x1);
            (right > left).then_some(XSpan {
                x: left,
                y: span.y,
                width: right - left,
            })
        })
        .collect()
}

/// `miPolyArc`: stroked arcs at any width and style. A solid thin arc the
/// walker can take is `zero_line`'s, dashed or not; the rest are
/// `miWideArc`'s.
pub fn poly_arc(arcs: &[XArc], gc: &XGraphicsContextValues) -> XInkedSpans {
    if gc.line_width != 0 {
        return wide_arcs(arcs, gc);
    }
    if gc.line_style != X_LINE_SOLID {
        // `miZeroPolyArc`: the arcs the walker cannot take go to `miWideArc`
        // first, then the rest are walked with their dashes.
        let large: Vec<XArc> = arcs
            .iter()
            .filter(|arc| !super::zero_line::can_zero_arc(arc))
            .copied()
            .collect();
        let mut out = if large.is_empty() {
            Vec::new()
        } else {
            wide_arcs(&large, gc)
        };
        out.extend(super::zero_line::dash::arcs(arcs, gc));
        return out;
    }
    let mut out = Vec::new();
    let thin: Vec<XSpan> = super::zero_line::arcs(arcs)
        .into_iter()
        .map(|(x, y)| XSpan { x, y, width: 1 })
        .collect();
    if !thin.is_empty() {
        out.push((gc.foreground, thin));
    }
    let large: Vec<XArc> = arcs
        .iter()
        .filter(|arc| !super::zero_line::can_zero_arc(arc))
        .copied()
        .collect();
    if !large.is_empty() {
        out.extend(wide_arcs(&large, gc));
    }
    out
}
