//! Deterministic replay of the authority-private semantic journal into a
//! derived density store.
//!
//! Every projection here is exact integer arithmetic over the command's own
//! 1x coordinates. Replay never samples the canonical drawable, so a derived
//! store is independently rendered content rather than a resampled copy.

use sophia_protocol::{Rect, SurfaceRasterClass};

use crate::font::pcf::XGlyph;
use crate::{X_GX_COPY, XGraphicsContextValues};

use super::XAuthorityCpuBufferSnapshot;
use super::raster_ops::{draw_line, draw_rectangle_outline, fill_rect, set_pixel};
use super::raster_variants::{XAuthorityRasterCommand, XOwnedImagePixels, XOwnedTextDraw};

pub(super) fn floor_edge(value: i32, density: u32) -> i32 {
    let scaled = i64::from(value).saturating_mul(i64::from(density));
    let quotient = scaled.div_euclid(1_000);
    i32::try_from(quotient).unwrap_or(if quotient < 0 { i32::MIN } else { i32::MAX })
}

pub(super) fn ceil_edge(value: i32, density: u32) -> i32 {
    let scaled = i64::from(value).saturating_mul(i64::from(density));
    let quotient = scaled.saturating_add(999).div_euclid(1_000);
    i32::try_from(quotient).unwrap_or(if quotient < 0 { i32::MIN } else { i32::MAX })
}

pub(super) fn project_rect(rect: Rect, density: u32) -> Rect {
    let left = floor_edge(rect.x, density);
    let top = floor_edge(rect.y, density);
    let right = ceil_edge(rect.x.saturating_add(rect.width), density);
    let bottom = ceil_edge(rect.y.saturating_add(rect.height), density);
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

fn projected_gc(gc: &XGraphicsContextValues, density: u32) -> XGraphicsContextValues {
    let mut projected = gc.clone();
    projected.line_width =
        u16::try_from(ceil_edge(i32::from(gc.line_width.max(1)), density).max(1))
            .unwrap_or(u16::MAX);
    projected.clip_x_origin = i16::try_from(floor_edge(i32::from(gc.clip_x_origin), density))
        .unwrap_or(if gc.clip_x_origin < 0 {
            i16::MIN
        } else {
            i16::MAX
        });
    projected.clip_y_origin = i16::try_from(floor_edge(i32::from(gc.clip_y_origin), density))
        .unwrap_or(if gc.clip_y_origin < 0 {
            i16::MIN
        } else {
            i16::MAX
        });
    projected.clip_rectangles = gc.clip_rectangles.as_ref().map(|rects| {
        rects
            .iter()
            .map(|rect| project_rect(*rect, density))
            .collect()
    });
    projected
}

/// Replay one retained command into a derived density store, and say where.
///
/// The returned rectangle is the region this call asked to paint, already
/// projected into the variant's density space. `None` means the extent is not
/// known, and the caller owes a full replacement.
///
/// It is computed here, beside each branch's own projection, rather than by a
/// second function that reads the same command. A separate extent calculation
/// that disagreed with the paint would under-report by exactly the region it
/// forgot, and the result -- a variant stale in one place, at the right
/// generation, in an otherwise presentable frame -- is the failure
/// `StableBackingLease`'s `RegistryMatchesStore` forbids and that nothing
/// downstream could catch.
///
/// Over-reporting is safe: the extent is what the patch will carry, and the
/// bytes it carries are read from the variant after this call.
pub(super) fn apply_command(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    class: SurfaceRasterClass,
    command: &XAuthorityRasterCommand,
) -> Option<Rect> {
    let density = class.density_millis;
    match command {
        XAuthorityRasterCommand::Paint { rects, gc } => {
            let gc = projected_gc(gc, density);
            let mut painted: Option<Rect> = None;
            for rect in rects {
                let projected = project_rect(*rect, density);
                fill_rect(snapshot, projected, gc.foreground, &gc);
                painted = Some(painted.map_or(projected, |bounds| union_rect(bounds, projected)));
            }
            painted
        }
        XAuthorityRasterCommand::Clear { rect, pixel } => {
            let projected = project_rect(*rect, density);
            fill_rect(
                snapshot,
                projected,
                *pixel,
                &XGraphicsContextValues::default(),
            );
            Some(projected)
        }
        XAuthorityRasterCommand::Segments { points, gc } => {
            // Pairs, not a path: chunks rather than windows, so no line is
            // drawn between one segment's end and the next one's start.
            let gc = projected_gc(gc, density);
            let mut painted: Option<Rect> = None;
            let half_width = i32::from(gc.line_width.max(1)).saturating_add(1);
            for pair in points.chunks_exact(2) {
                for point in pair {
                    let x = floor_edge(point.x, density);
                    let y = floor_edge(point.y, density);
                    let cell = Rect {
                        x: x.saturating_sub(half_width),
                        y: y.saturating_sub(half_width),
                        width: half_width.saturating_mul(2).saturating_add(1),
                        height: half_width.saturating_mul(2).saturating_add(1),
                    };
                    painted = Some(painted.map_or(cell, |bounds| union_rect(bounds, cell)));
                }
                draw_line(
                    snapshot,
                    crate::XPoint {
                        x: i16::try_from(floor_edge(pair[0].x, density)).unwrap_or(i16::MAX),
                        y: i16::try_from(floor_edge(pair[0].y, density)).unwrap_or(i16::MAX),
                    },
                    crate::XPoint {
                        x: i16::try_from(floor_edge(pair[1].x, density)).unwrap_or(i16::MAX),
                        y: i16::try_from(floor_edge(pair[1].y, density)).unwrap_or(i16::MAX),
                    },
                    i32::from(gc.line_width.max(1)),
                    &gc,
                );
            }
            painted
        }
        XAuthorityRasterCommand::Lines { points, gc } => {
            let gc = projected_gc(gc, density);
            // The line rasteriser walks between projected endpoints and widens
            // by the line width, so the painted region is the bounding box of
            // the endpoints grown by that width on every side.
            let mut painted: Option<Rect> = None;
            let half_width = i32::from(gc.line_width.max(1)).saturating_add(1);
            for pair in points.windows(2) {
                for point in pair {
                    let x = floor_edge(point.x, density);
                    let y = floor_edge(point.y, density);
                    let cell = Rect {
                        x: x.saturating_sub(half_width),
                        y: y.saturating_sub(half_width),
                        width: half_width.saturating_mul(2).saturating_add(1),
                        height: half_width.saturating_mul(2).saturating_add(1),
                    };
                    painted = Some(painted.map_or(cell, |bounds| union_rect(bounds, cell)));
                }
            }
            for pair in points.windows(2) {
                draw_line(
                    snapshot,
                    crate::XPoint {
                        x: i16::try_from(floor_edge(pair[0].x, density)).unwrap_or(i16::MAX),
                        y: i16::try_from(floor_edge(pair[0].y, density)).unwrap_or(i16::MAX),
                    },
                    crate::XPoint {
                        x: i16::try_from(floor_edge(pair[1].x, density)).unwrap_or(i16::MAX),
                        y: i16::try_from(floor_edge(pair[1].y, density)).unwrap_or(i16::MAX),
                    },
                    i32::from(gc.line_width.max(1)),
                    &gc,
                );
            }
            painted
        }
        XAuthorityRasterCommand::Rectangles { rectangles, gc } => {
            let gc = projected_gc(gc, density);
            // An outline is stroked on the rectangle's edges, so it reaches
            // half a line width outside the rectangle itself.
            let width = i32::from(gc.line_width.max(1)).saturating_add(1);
            let mut painted: Option<Rect> = None;
            for rectangle in rectangles {
                let projected = project_rect(*rectangle, density);
                draw_rectangle_outline(snapshot, projected, i32::from(gc.line_width.max(1)), &gc);
                let grown = Rect {
                    x: projected.x.saturating_sub(width),
                    y: projected.y.saturating_sub(width),
                    width: projected.width.saturating_add(width.saturating_mul(2)),
                    height: projected.height.saturating_add(width.saturating_mul(2)),
                };
                painted = Some(painted.map_or(grown, |bounds| union_rect(bounds, grown)));
            }
            painted
        }
        XAuthorityRasterCommand::Text { draws, gc } => {
            let mut painted: Option<Rect> = None;
            for draw in draws {
                let extent = draw_projected_text(snapshot, density, draw, gc);
                painted = Some(painted.map_or(extent, |bounds| union_rect(bounds, extent)));
            }
            painted
        }
        XAuthorityRasterCommand::CopyArea {
            source,
            destination_x,
            destination_y,
            gc,
        } => {
            let projected = project_rect(*source, density);
            let destination_x = floor_edge(*destination_x, density);
            let destination_y = floor_edge(*destination_y, density);
            copy_projected_area(
                snapshot,
                projected,
                destination_x,
                destination_y,
                &projected_gc(gc, density),
            );
            // The copy lands at the destination, carrying the source extent.
            Some(Rect {
                x: destination_x,
                y: destination_y,
                width: projected.width,
                height: projected.height,
            })
        }
        XAuthorityRasterCommand::PutImage { image, gc } => {
            blit_projected_image(snapshot, density, image, gc);
            Some(project_rect(image.rect, density))
        }
        // Nothing was replayed, so nothing was painted. An empty region rather
        // than an unknown one: the caller may skip this command entirely.
        XAuthorityRasterCommand::Unsupported(_) => Some(Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }),
    }
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    if left.width <= 0 || left.height <= 0 {
        return right;
    }
    if right.width <= 0 || right.height <= 0 {
        return left;
    }
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = left
        .x
        .saturating_add(left.width)
        .max(right.x.saturating_add(right.width));
    let bottom_edge = left
        .y
        .saturating_add(left.height)
        .max(right.y.saturating_add(right.height));
    Rect {
        x,
        y,
        width: right_edge.saturating_sub(x),
        height: bottom_edge.saturating_sub(y),
    }
}

/// Projects retained client pixels into one derived density store.
///
/// The retained 1x pixels are the source of truth, not the canonical store, so
/// replay stays correctly ordered against later text and copy commands. Each
/// destination pixel is an exact rational area average of the source pixels it
/// covers, matching the glyph coverage rule. At 1x the projection degenerates
/// to full coverage of one source pixel, so the derived store reproduces the
/// canonical drawable bit for bit.
fn blit_projected_image(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    density: u32,
    image: &XOwnedImagePixels,
    gc: &XGraphicsContextValues,
) {
    let width = usize::try_from(image.rect.width.max(0)).unwrap_or(0);
    let height = usize::try_from(image.rect.height.max(0)).unwrap_or(0);
    if density == 0 || width == 0 || height == 0 {
        return;
    }
    let source_stride = width.saturating_mul(4);
    let gc = projected_gc(gc, density);
    let bounds = project_rect(image.rect, density);
    for y in bounds.y..bounds.y.saturating_add(bounds.height) {
        let (row_start, row_end) = source_span(y, density, image.rect.y, image.rect.height);
        for x in bounds.x..bounds.x.saturating_add(bounds.width) {
            let (column_start, column_end) =
                source_span(x, density, image.rect.x, image.rect.width);
            let mut area = 0_i64;
            let mut channels = [0_i64; 3];
            for sy in row_start..row_end {
                let overlap_y = overlap_scaled(y, sy, density);
                if overlap_y == 0 {
                    continue;
                }
                for sx in column_start..column_end {
                    let overlap_x = overlap_scaled(x, sx, density);
                    if overlap_x == 0 {
                        continue;
                    }
                    let Some(pixel) = source_pixel(image, source_stride, sx, sy) else {
                        continue;
                    };
                    let weight = overlap_x.saturating_mul(overlap_y);
                    area = area.saturating_add(weight);
                    for (index, channel) in channels.iter_mut().enumerate() {
                        // Accumulate squared components so the average is a
                        // mix of light rather than of gamma-encoded bytes;
                        // see `blend_copy_pixel` for why the plain mean
                        // darkens every partially covered pixel.
                        let component = i64::from((pixel >> (8 * index as u32)) & 0xff);
                        *channel = channel.saturating_add(
                            component.saturating_mul(component).saturating_mul(weight),
                        );
                    }
                }
            }
            if area <= 0 {
                continue;
            }
            let component = |channel: i64| -> u32 {
                let mixed = channel.saturating_add(area / 2) / area;
                u32::try_from(mixed.max(0).isqrt())
                    .unwrap_or(0xff)
                    .min(0xff)
            };
            let pixel =
                component(channels[2]) << 16 | component(channels[1]) << 8 | component(channels[0]);
            let coverage =
                u8::try_from(area.saturating_mul(255).saturating_add(500_000) / 1_000_000)
                    .unwrap_or(u8::MAX);
            if coverage == 0 {
                continue;
            }
            if gc.function == X_GX_COPY {
                blend_copy_pixel(snapshot, x, y, pixel, coverage, &gc);
            } else if coverage >= 128 {
                set_pixel(snapshot, x, y, pixel, &gc);
            }
        }
    }
}

/// Source index range, clamped to the image, that can overlap one destination
/// pixel. Bounding the inner loop keeps replay linear in destination pixels
/// instead of quadratic in the image.
fn source_span(destination: i32, density: u32, origin: i32, extent: i32) -> (i32, i32) {
    let density = i64::from(density).max(1);
    let left = i64::from(destination).saturating_mul(1_000);
    let start = left.div_euclid(density);
    let end = left
        .saturating_add(1_000)
        .saturating_add(density.saturating_sub(1))
        .div_euclid(density);
    let low = i64::from(origin);
    let high = low.saturating_add(i64::from(extent.max(0)));
    let clamp = |value: i64| i32::try_from(value.clamp(low, high)).unwrap_or(origin);
    (clamp(start), clamp(end))
}

fn source_pixel(image: &XOwnedImagePixels, stride: usize, sx: i32, sy: i32) -> Option<u32> {
    let column = usize::try_from(sx.checked_sub(image.rect.x)?).ok()?;
    let row = usize::try_from(sy.checked_sub(image.rect.y)?).ok()?;
    let offset = row
        .checked_mul(stride)?
        .checked_add(column.checked_mul(4)?)?;
    let bytes = image.pixels.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// Replay one text draw, returning the cell box it covered.
///
/// The box is the font's own extent for the string, which is what an image
/// draw fills and what the glyphs are rasterised inside, so it contains every
/// pixel this call can touch.
fn draw_projected_text(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    density: u32,
    draw: &XOwnedTextDraw,
    gc: &XGraphicsContextValues,
) -> Rect {
    let metrics = &draw.font.metrics;
    let top = draw.baseline.saturating_sub(i32::from(metrics.font_ascent));
    let height = i32::from(metrics.font_ascent.saturating_add(metrics.font_descent));
    let width = metrics.text_extents(&draw.text).overall_width;
    let mut raster_gc = projected_gc(gc, density);
    if draw.image {
        raster_gc.function = X_GX_COPY;
        raster_gc.fill_style = 0;
        fill_rect(
            snapshot,
            project_rect(
                Rect {
                    x: draw.x,
                    y: top,
                    width,
                    height,
                },
                density,
            ),
            gc.background,
            &raster_gc,
        );
    }
    let mut pen = draw.x;
    for code in draw.text.iter().copied() {
        let Some((info, glyph)) = draw.font.glyph(code) else {
            continue;
        };
        draw_coverage_glyph(
            snapshot,
            density,
            pen.saturating_add(i32::from(info.left_side_bearing)),
            draw.baseline.saturating_sub(i32::from(info.ascent)),
            glyph,
            gc.foreground,
            &raster_gc,
        );
        pen = pen.saturating_add(i32::from(info.character_width));
    }
    project_rect(
        Rect {
            x: draw.x,
            y: top,
            width,
            height,
        },
        density,
    )
}

/// Paint one glyph at a fractional density with exact area coverage.
///
/// The glyph's own pixels are projected, so a face of any cell size scales the
/// same way the fixed face did.
fn draw_coverage_glyph(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    density: u32,
    left: i32,
    top: i32,
    glyph: &XGlyph,
    pixel: u32,
    gc: &XGraphicsContextValues,
) {
    let bounds = project_rect(
        Rect {
            x: left,
            y: top,
            width: i32::from(glyph.width),
            height: i32::from(glyph.height),
        },
        density,
    );
    for y in bounds.y..bounds.y.saturating_add(bounds.height) {
        for x in bounds.x..bounds.x.saturating_add(bounds.width) {
            let mut area = 0_i64;
            for row in 0..glyph.height {
                for column in 0..glyph.width {
                    if !glyph.pixel(column, row) {
                        continue;
                    }
                    let source_x = left.saturating_add(i32::from(column));
                    let source_y = top.saturating_add(i32::from(row));
                    let overlap_x = overlap_scaled(x, source_x, density);
                    let overlap_y = overlap_scaled(y, source_y, density);
                    area = area.saturating_add(overlap_x.saturating_mul(overlap_y));
                }
            }
            let coverage =
                u8::try_from(area.saturating_mul(255).saturating_add(500_000) / 1_000_000)
                    .unwrap_or(u8::MAX);
            if coverage == 0 {
                continue;
            }
            if gc.function == X_GX_COPY {
                blend_copy_pixel(snapshot, x, y, pixel, coverage, gc);
            } else if coverage >= 128 {
                set_pixel(snapshot, x, y, pixel, gc);
            }
        }
    }
}

/// Exact rational overlap, in millis, between one destination pixel and one
/// source pixel projected at `density`.
pub(super) fn overlap_scaled(destination: i32, source: i32, density: u32) -> i64 {
    let destination_left = i64::from(destination).saturating_mul(1_000);
    let destination_right = destination_left.saturating_add(1_000);
    let source_left = i64::from(source).saturating_mul(i64::from(density));
    let source_right = source_left.saturating_add(i64::from(density));
    destination_right
        .min(source_right)
        .saturating_sub(destination_left.max(source_left))
        .max(0)
}

pub(super) fn blend_copy_pixel(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    x: i32,
    y: i32,
    source: u32,
    coverage: u8,
    gc: &XGraphicsContextValues,
) {
    let Ok(xu) = usize::try_from(x) else { return };
    let Ok(yu) = usize::try_from(y) else { return };
    let Ok(stride) = usize::try_from(snapshot.stride) else {
        return;
    };
    let Some(offset) = yu
        .checked_mul(stride)
        .and_then(|row| row.checked_add(xu.saturating_mul(4)))
    else {
        return;
    };
    let Some(bytes) = snapshot.bytes.get(offset..offset.saturating_add(4)) else {
        return;
    };
    let destination = u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]));
    let alpha = u32::from(coverage);
    // Partial coverage is a light-intensity mix, so it has to be weighted in
    // linear space. Channel bytes are gamma-encoded, and averaging them
    // directly makes every antialiased edge far darker than its coverage:
    // half-covered white on black lands near 128, roughly a fifth of the
    // intended luminance instead of half. Strokes thinner than a pixel are
    // then uniformly under-weighted, which reads as out of focus rather than
    // merely soft.
    //
    // Squaring approximates the encoding closely enough to fix that while
    // staying exact integer arithmetic, so replay remains bit-reproducible;
    // a real transfer function needs a power the platform is free to round
    // differently. Full and zero coverage still map to the endpoints exactly,
    // so canonical-density text stays bit-identical to the 1x drawable.
    let blend = |shift: u32| -> u32 {
        let source = (source >> shift) & 0xff;
        let destination = (destination >> shift) & 0xff;
        let mixed = source
            .saturating_mul(source)
            .saturating_mul(alpha)
            .saturating_add(
                destination
                    .saturating_mul(destination)
                    .saturating_mul(255 - alpha),
            )
            .saturating_add(127)
            / 255;
        mixed.isqrt().min(0xff)
    };
    let pixel = blend(16) << 16 | blend(8) << 8 | blend(0);
    set_pixel(snapshot, x, y, pixel, gc);
}

fn copy_projected_area(
    snapshot: &mut XAuthorityCpuBufferSnapshot,
    source: Rect,
    destination_x: i32,
    destination_y: i32,
    gc: &XGraphicsContextValues,
) {
    let previous = snapshot.clone();
    let Ok(stride) = usize::try_from(previous.stride) else {
        return;
    };
    for y in 0..source.height.max(0) {
        for x in 0..source.width.max(0) {
            let sx = source.x.saturating_add(x);
            let sy = source.y.saturating_add(y);
            let Ok(sxu) = usize::try_from(sx) else {
                continue;
            };
            let Ok(syu) = usize::try_from(sy) else {
                continue;
            };
            let Some(offset) = syu
                .checked_mul(stride)
                .and_then(|row| row.checked_add(sxu.saturating_mul(4)))
            else {
                continue;
            };
            let Some(bytes) = previous.bytes.get(offset..offset.saturating_add(4)) else {
                continue;
            };
            set_pixel(
                snapshot,
                destination_x.saturating_add(x),
                destination_y.saturating_add(y),
                u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])),
                gc,
            );
        }
    }
}
