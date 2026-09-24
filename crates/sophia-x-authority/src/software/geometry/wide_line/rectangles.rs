//! The wide branch of `miPolyRectangle`, from `mi/mipolyrect.c`, with the
//! protocol's rule that an outline paints each pixel once.

use super::*;

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
