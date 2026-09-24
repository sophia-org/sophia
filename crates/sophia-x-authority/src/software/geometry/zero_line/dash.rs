//! Dashed zero-width lines and arcs, as the reference server draws them.
//!
//! Lines are `fb`'s: `fbZeroLine` and `fbZeroSegment` hand each segment to
//! `fbSegment`, whose Bresenham walk is `miZeroLine`'s with the dash phase
//! stepped once per pixel (`fbBresDash`, `FbDashInit`, `FbDashStep`; copyright
//! 1998 Keith Packard). `fb` does not stop a polyline's last segment short of
//! a point it closes on, where `miZeroLine` does, and the dash offset runs on
//! from segment to segment. Arcs are `mi`'s: `fbPolyArc` hands every dashed
//! thin arc to `miZeroPolyArc`, whose `miZeroArcDashPts` walks the four
//! quadrants into separate arrays and then reads them back in angle order.
//! That reading is kept as `mi` does it, with the four arrays laid out one
//! after another in one buffer, because the reading compares positions
//! across the arrays' ends.

use super::{Walker, setup};
use crate::software::geometry::arc::XArc;
use crate::software::geometry::wide_line::{XInkedSpans, XSpan};
use crate::{X_CAP_NOT_LAST, X_LINE_DOUBLE_DASH, XGraphicsContextValues, XPoint};

const QUADRANT: i32 = 90 * 64;

/// The dash list as the server keeps it: an odd list stored twice over, so
/// that even and odd dashes alternate on every pass.
fn dash_list(gc: &XGraphicsContextValues) -> Vec<u8> {
    let mut dashes: Vec<u8> = gc.dashes.iter().copied().filter(|dash| *dash > 0).collect();
    if dashes.is_empty() {
        dashes.push(4);
    }
    if dashes.len() % 2 == 1 {
        dashes.extend_from_within(..);
    }
    dashes
}

/// The pixel an odd dash is painted with: the background where the fill is
/// solid or stippled, and the fill itself otherwise, as `fbBresFillDash` and
/// `miZeroPolyArc` choose.
fn odd_pixel(gc: &XGraphicsContextValues) -> u32 {
    if gc.fill_style == 0 || gc.fill_style == crate::X_FILL_STIPPLED {
        gc.background
    } else {
        gc.foreground
    }
}

/// Batches pixels in paint order, one batch per run of one parity.
struct Inked {
    out: XInkedSpans,
    last_even: Option<bool>,
}

impl Inked {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            last_even: None,
        }
    }

    fn push(&mut self, even: bool, pixel: u32, x: i32, y: i32) {
        if self.last_even != Some(even) {
            self.out.push((pixel, Vec::new()));
            self.last_even = Some(even);
        }
        if let Some((_, spans)) = self.out.last_mut() {
            spans.push(XSpan { x, y, width: 1 });
        }
    }
}

/// `fbSegment` without its clipping, which only resumes the same walk
/// inside each clip box: `len` pixels from `(x1, y1)`, the dash phase
/// starting `dash_offset` pixels in and stepping once per pixel.
#[allow(clippy::too_many_arguments)]
fn segment(
    (x1, y1): (i32, i32),
    (x2, y2): (i32, i32),
    draw_last: bool,
    dash_offset: &mut i32,
    dashes: &[u8],
    gc: &XGraphicsContextValues,
    inked: &mut Inked,
) {
    // CalcLineDeltas, then the error terms and FIXUP_ERROR as `fbSegment`
    // takes them, which is `miZeroLine`'s arithmetic.
    let mut octant = 0;
    let (mut adx, mut ady) = (x2 - x1, y2 - y1);
    let (mut signdx, mut signdy) = (1, 1);
    if adx < 0 {
        adx = -adx;
        signdx = -1;
        octant |= super::XDECREASING;
    }
    if ady < 0 {
        ady = -ady;
        signdy = -1;
        octant |= super::YDECREASING;
    }
    let x_axis = adx > ady;
    let (e1, e2, mut e, mut len) = if x_axis {
        (ady << 1, (ady << 1) - (adx << 1), (ady << 1) - adx, adx)
    } else {
        octant |= super::YMAJOR;
        (adx << 1, (adx << 1) - (ady << 1), (adx << 1) - ady, ady)
    };
    e -= ((super::BIAS >> octant) & 1) as i32;
    let e3 = e2 - e1;
    e -= e1;
    if draw_last {
        len += 1;
    }
    let offset = *dash_offset;
    *dash_offset = offset + len;

    // FbDashInit
    let total: i32 = dashes.iter().map(|dash| i32::from(*dash)).sum();
    let mut remaining = offset % total;
    let mut index = 0;
    let mut even = true;
    let mut dashlen;
    loop {
        dashlen = i32::from(dashes[index]);
        if remaining < dashlen {
            break;
        }
        remaining -= dashlen;
        even = !even;
        index = (index + 1) % dashes.len();
    }
    dashlen -= remaining;

    let double = gc.line_style == X_LINE_DOUBLE_DASH;
    let (mut x, mut y) = (x1, y1);
    for _ in 0..len {
        if even {
            inked.push(true, gc.foreground, x, y);
        } else if double {
            inked.push(false, odd_pixel(gc), x, y);
        }
        if x_axis {
            x += signdx;
            e += e1;
            if e >= 0 {
                y += signdy;
                e += e3;
            }
        } else {
            y += signdy;
            e += e1;
            if e >= 0 {
                e += e3;
                x += signdx;
            }
        }
        // FbDashStep
        dashlen -= 1;
        if dashlen == 0 {
            index = (index + 1) % dashes.len();
            dashlen = i32::from(dashes[index]);
            even = !even;
        }
    }
}

/// `fbZeroLine`: one dashed thin polyline, the dash offset running on from
/// segment to segment and only the last segment drawing its end point.
pub fn polyline(points: &[XPoint], gc: &XGraphicsContextValues) -> XInkedSpans {
    let dashes = dash_list(gc);
    let mut inked = Inked::new();
    let mut dash_offset = i32::from(gc.dash_offset);
    let draw_last = gc.cap_style != X_CAP_NOT_LAST;
    for (index, pair) in points.windows(2).enumerate() {
        let last = index + 2 == points.len();
        segment(
            (i32::from(pair[0].x), i32::from(pair[0].y)),
            (i32::from(pair[1].x), i32::from(pair[1].y)),
            last && draw_last,
            &mut dash_offset,
            &dashes,
            gc,
            &mut inked,
        );
    }
    inked.out
}

/// `DashInfo`: the dash state `miZeroPolyArc` carries from arc to arc.
#[derive(Default)]
struct DashInfo {
    dash_index_init: usize,
    dash_offset_init: i32,
    dash_index: usize,
    dash_offset: i32,
    have_start: bool,
    skip_start: bool,
    have_last: bool,
    skip_last: bool,
    start_pt: (i32, i32),
    end_pt: (i32, i32),
}

/// `miZeroArcDashPts`: one arc's pixels, split into even and odd dashes.
#[allow(clippy::too_many_lines)]
fn arc_dash_points(
    arc: &XArc,
    dinfo: &mut DashInfo,
    dashes: &[u8],
    max_pts: isize,
    even_out: &mut Vec<(i32, i32)>,
    odd_out: &mut Vec<(i32, i32)>,
) {
    // Four arrays of `max_pts`, one per quadrant, in one buffer.
    let mut points = vec![(0, 0); usize::try_from(max_pts * 4).unwrap_or(0)];
    let mut arc_pts: [isize; 4] = [0, max_pts, max_pts * 2, max_pts * 3];
    let (mut info, _) = setup(arc, false);
    let mut walk = Walker::new(&info);
    let mut mask = info.initial_mask;
    let startseg = usize::try_from(info.start_angle / QUADRANT).unwrap_or(0);
    let mut start_pt = arc_pts[startseg];
    let mut pix = |arc_pts: &mut [isize; 4], mask: i32, index: usize, x: i32, y: i32| {
        if mask & (1 << index) != 0 {
            if let Some(slot) = usize::try_from(arc_pts[index])
                .ok()
                .and_then(|at| points.get_mut(at))
            {
                *slot = (x, y);
            }
            arc_pts[index] += 1;
        }
    };
    if arc.width & 1 == 0 {
        pix(&mut arc_pts, mask, 1, info.xorgo, info.yorg);
        pix(&mut arc_pts, mask, 3, info.xorgo, info.yorgo);
    }
    if info.end.x == 0 || info.end.y == 0 {
        mask = info.end.mask;
        info.end = info.altend;
    }
    while walk.y < info.h || walk.x < info.w {
        walk.octant_shift(info.h);
        let (x, y) = (walk.x, walk.y);
        if x == info.first_x || y == info.first_y {
            start_pt = arc_pts[startseg];
        }
        if x == info.start.x || y == info.start.y {
            mask = info.start.mask;
            info.start = info.altstart;
        }
        pix(&mut arc_pts, mask, 0, info.xorg + x, info.yorg + y);
        pix(&mut arc_pts, mask, 1, info.xorgo - x, info.yorg + y);
        pix(&mut arc_pts, mask, 2, info.xorgo - x, info.yorgo - y);
        pix(&mut arc_pts, mask, 3, info.xorg + x, info.yorgo - y);
        if x == info.end.x || y == info.end.y {
            mask = info.end.mask;
            info.end = info.altend;
        }
        walk.step();
    }
    let (x, y) = (walk.x, walk.y);
    if x == info.first_x || y == info.first_y {
        start_pt = arc_pts[startseg];
    }
    if x == info.start.x || y == info.start.y {
        mask = info.start.mask;
    }
    pix(&mut arc_pts, mask, 0, info.xorg + x, info.yorg + y);
    pix(&mut arc_pts, mask, 2, info.xorgo - x, info.yorgo - y);
    if arc.height & 1 == 1 {
        pix(&mut arc_pts, mask, 1, info.xorgo - x, info.yorg + y);
        pix(&mut arc_pts, mask, 3, info.xorg + x, info.yorgo - y);
    }

    let mut start_pts = [0isize; 5];
    let mut end_pts = [0isize; 5];
    let mut deltas = [0isize; 5];
    for i in 0..4 {
        let seg = (startseg + i) & 3;
        let pt = isize::try_from(seg).unwrap_or(0) * max_pts;
        if seg & 1 == 1 {
            start_pts[i] = pt;
            end_pts[i] = arc_pts[seg];
            deltas[i] = 1;
        } else {
            start_pts[i] = arc_pts[seg] - 1;
            end_pts[i] = pt - 1;
            deltas[i] = -1;
        }
    }
    start_pts[4] = start_pts[0];
    end_pts[4] = start_pt;
    start_pts[0] = start_pt;
    if startseg & 1 == 1 {
        if start_pts[4] != end_pts[4] {
            end_pts[4] -= 1;
        }
        deltas[4] = 1;
    } else {
        if start_pts[0] > start_pts[4] {
            start_pts[0] -= 1;
        }
        if start_pts[4] < end_pts[4] {
            end_pts[4] -= 1;
        }
        deltas[4] = -1;
    }
    if arc.angle2 < 0 {
        let tmpd = deltas[0];
        let tmps = start_pts[0] - tmpd;
        let tmpe = end_pts[0] - tmpd;
        start_pts[0] = end_pts[4] - deltas[4];
        end_pts[0] = start_pts[4] - deltas[4];
        deltas[0] = -deltas[4];
        start_pts[4] = tmpe;
        end_pts[4] = tmps;
        deltas[4] = -tmpd;
        let tmpd = deltas[1];
        let tmps = start_pts[1] - tmpd;
        let tmpe = end_pts[1] - tmpd;
        start_pts[1] = end_pts[3] - deltas[3];
        end_pts[1] = start_pts[3] - deltas[3];
        deltas[1] = -deltas[3];
        start_pts[3] = tmpe;
        end_pts[3] = tmps;
        deltas[3] = -tmpd;
        let tmps = start_pts[2] - deltas[2];
        start_pts[2] = end_pts[2] - deltas[2];
        end_pts[2] = tmps;
        deltas[2] = -deltas[2];
    }
    let Some(first) = (0..5).find(|&i| start_pts[i] != end_pts[i]) else {
        return;
    };
    let at = |index: isize| {
        usize::try_from(index)
            .ok()
            .and_then(|index| points.get(index))
            .copied()
            .unwrap_or((i32::MIN, i32::MIN))
    };
    let pt = start_pts[first];
    let last = (0..5)
        .rev()
        .find(|&j| start_pts[j] != end_pts[j])
        .unwrap_or(first);
    let last_pt = end_pts[last] - deltas[last];
    if dinfo.have_last && at(pt) == dinfo.end_pt {
        start_pts[first] += deltas[first];
    } else {
        dinfo.dash_index = dinfo.dash_index_init;
        dinfo.dash_offset = dinfo.dash_offset_init;
    }
    if !dinfo.skip_start && info.start_angle != info.end_angle {
        dinfo.start_pt = at(pt);
        dinfo.have_start = true;
    } else if !dinfo.skip_last
        && dinfo.have_start
        && at(last_pt) == dinfo.start_pt
        && last_pt != start_pts[first]
    {
        end_pts[last] = last_pt;
    }
    if info.start_angle != info.end_angle {
        dinfo.have_last = true;
        dinfo.end_pt = at(last_pt);
    }
    let mut remaining = i32::from(dashes[dinfo.dash_index]) - dinfo.dash_offset;
    for i in 0..5 {
        let mut pt = start_pts[i];
        let last = end_pts[i];
        let delta = deltas[i];
        while pt != last {
            let odd = dinfo.dash_index & 1 == 1;
            while pt != last && {
                remaining -= 1;
                remaining >= 0
            } {
                if odd {
                    odd_out.push(at(pt));
                } else {
                    even_out.push(at(pt));
                }
                pt += delta;
            }
            if remaining <= 0 {
                dinfo.dash_index = (dinfo.dash_index + 1) % dashes.len();
                remaining = i32::from(dashes[dinfo.dash_index]);
            }
        }
    }
    dinfo.dash_offset = i32::from(dashes[dinfo.dash_index]) - remaining;
}

/// `miZeroPolyArc`'s dashed path for the arcs `miCanZeroArc` admits: each
/// arc's even dashes, then its odd ones in the order `mi` hands them over.
pub fn arcs(arcs: &[XArc], gc: &XGraphicsContextValues) -> XInkedSpans {
    let dashes = dash_list(gc);
    let max_pts = arcs
        .iter()
        .filter(|arc| super::can_zero_arc(arc))
        .map(|arc| {
            let (width, height) = (
                isize::try_from(arc.width).unwrap_or(0),
                isize::try_from(arc.height).unwrap_or(0),
            );
            if width > height {
                width + (height >> 1)
            } else {
                height + (width >> 1)
            }
        })
        .max()
        .unwrap_or(0);
    let mut dinfo = DashInfo::default();
    let mut index = 0;
    let mut offset = 0;
    crate::software::geometry::wide_line::step_dash(
        i32::from(gc.dash_offset),
        &mut index,
        &dashes,
        &mut offset,
    );
    dinfo.dash_index_init = index;
    dinfo.dash_offset_init = offset;
    let double = gc.line_style == X_LINE_DOUBLE_DASH;
    let mut out = Vec::new();
    for (position, arc) in arcs.iter().enumerate() {
        if !super::can_zero_arc(arc) {
            continue;
        }
        dinfo.skip_last = position + 1 != arcs.len();
        let (mut even, mut odd) = (Vec::new(), Vec::new());
        arc_dash_points(arc, &mut dinfo, &dashes, max_pts, &mut even, &mut odd);
        dinfo.skip_start = true;
        if !even.is_empty() {
            out.push((
                gc.foreground,
                even.into_iter()
                    .map(|(x, y)| XSpan { x, y, width: 1 })
                    .collect(),
            ));
        }
        if double && !odd.is_empty() {
            // `mi` writes the odd points backwards and hands them over from
            // the low end, so they are painted last written first.
            out.push((
                odd_pixel(gc),
                odd.into_iter()
                    .rev()
                    .map(|(x, y)| XSpan { x, y, width: 1 })
                    .collect(),
            ));
        }
    }
    out
}
