//! `miAppendSpans`, `miSubtractSpans` and `miFillUniqueSpanGroup` from
//! `mi/mispans.c`: the span groups a raster function that must touch each
//! pixel once paints through.

use super::*;

/// `miAppendSpans`
pub(super) fn append_spans(
    group: &mut SpanGroup,
    other: Option<&mut SpanGroup>,
    spans: Vec<XSpan>,
) {
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
pub(super) fn subtract_spans(group: &mut SpanGroup, sub: &[XSpan]) {
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
pub(super) fn unique_spans(group: SpanGroup) -> Vec<XSpan> {
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
