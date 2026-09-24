//! `miPolyBuildEdge` and `miPolyBuildPoly` from `mi/miwideline.c`: the
//! edge walkers a wide line's polygons are filled with.

use super::*;

/// `miPolyBuildEdge`
#[allow(clippy::too_many_arguments)]
pub(super) fn build_edge(
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

pub(super) struct Poly {
    pub(super) y: i32,
    pub(super) height: i32,
    pub(super) left: Vec<Edge>,
    pub(super) right: Vec<Edge>,
}

pub(super) fn step_around(value: usize, increment: isize, max: usize) -> usize {
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
pub(super) fn build_poly(vertices: &[Vertex], slopes: &[Slope], xi: i32, yi: i32) -> Poly {
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
