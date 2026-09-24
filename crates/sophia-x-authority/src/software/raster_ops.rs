//! Writing pixels into a CPU snapshot.
//!
//! The rules a drawing operation obeys once its damage has been accepted:
//! clipping to the drawable, the graphics context's raster function and clip
//! list, and the byte layout of an XRGB8888 buffer. Split from the store
//! because the two answer different questions -- this one is how a pixel is
//! written, the store is which buffer is published and what it owes.
//!
//! `raster_replay` drives these same primitives to project a retained command
//! into a derived density store, which is why they are `pub(super)` rather
//! than private here.

use std::sync::Arc;

use sophia_protocol::{Rect, Size};

use crate::font::pcf::XGlyph;
use crate::{XGraphicsContextValues, XPoint};

use super::XAuthorityCpuBufferSnapshot;

/// The bytes of a snapshot, ready to be written.
///
/// `Arc::make_mut` copies only when somebody else still holds the allocation,
/// which is exactly the guarantee immutability used to buy with an
/// unconditional clone: a presentation handed these bytes keeps reading them
/// until it retires, and the drawable's next update lands on a copy.
///
/// In the steady state nothing else holds them -- a published snapshot is
/// consumed by the session and the registry takes its own reference -- so a
/// draw mutates in place and allocates nothing. Called once per operation
/// rather than once per pixel, because the refcount check is per call.
pub(super) fn bytes_mut(buffer: &mut XAuthorityCpuBufferSnapshot) -> &mut Vec<u8> {
    Arc::make_mut(&mut buffer.bytes)
}

pub(super) fn copy_buffer_region(
    source: &XAuthorityCpuBufferSnapshot,
    destination: &mut XAuthorityCpuBufferSnapshot,
    source_rect: Rect,
    destination_x: i32,
    destination_y: i32,
) -> Option<Rect> {
    let (mut left, mut top, right, bottom) = clipped_bounds(source.size, source_rect)?;
    let mut target_x = destination_x.saturating_add(i32::try_from(left).unwrap_or(i32::MAX));
    let mut target_y = destination_y.saturating_add(i32::try_from(top).unwrap_or(i32::MAX));
    if target_x < 0 {
        left =
            left.saturating_add(usize::try_from(target_x.saturating_neg()).unwrap_or(usize::MAX));
        target_x = 0;
    }
    if target_y < 0 {
        top = top.saturating_add(usize::try_from(target_y.saturating_neg()).unwrap_or(usize::MAX));
        target_y = 0;
    }
    let Ok(target_x) = usize::try_from(target_x) else {
        return None;
    };
    let Ok(target_y) = usize::try_from(target_y) else {
        return None;
    };
    let Ok(destination_width) = usize::try_from(destination.size.width) else {
        return None;
    };
    let Ok(destination_height) = usize::try_from(destination.size.height) else {
        return None;
    };
    let width = right
        .saturating_sub(left)
        .min(destination_width.saturating_sub(target_x));
    let height = bottom
        .saturating_sub(top)
        .min(destination_height.saturating_sub(target_y));
    let byte_width = width.saturating_mul(4);
    let source_stride = usize::try_from(source.stride).unwrap_or(0);
    let destination_stride = usize::try_from(destination.stride).unwrap_or(0);
    let destination_bytes = bytes_mut(destination);
    for row in 0..height {
        let source_offset = top
            .saturating_add(row)
            .saturating_mul(source_stride)
            .saturating_add(left.saturating_mul(4));
        let destination_offset = target_y
            .saturating_add(row)
            .saturating_mul(destination_stride)
            .saturating_add(target_x.saturating_mul(4));
        let source_row = source
            .bytes
            .get(source_offset..source_offset.saturating_add(byte_width))?;
        let destination_row = destination_bytes
            .get_mut(destination_offset..destination_offset.saturating_add(byte_width))?;
        destination_row.copy_from_slice(source_row);
    }
    (width != 0 && height != 0).then_some(Rect {
        x: i32::try_from(target_x).ok()?,
        y: i32::try_from(target_y).ok()?,
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
    })
}

/// Where a fill takes its colour, pixel by pixel.
///
/// `FillSolid` is the foreground everywhere. A tile repeats a pixmap's own
/// pixels; a stipple uses a depth-one pixmap as a mask, painting the
/// foreground where a bit is set and either leaving the rest alone
/// (`FillStippled`) or painting the background there (`FillOpaqueStippled`).
/// The pattern is anchored at the graphics context's tile-stipple origin, so
/// abutting fills line up instead of each starting at its own corner.
#[derive(Clone, Copy, Debug)]
pub(super) enum XFillPattern<'a> {
    Solid(u32),
    Tile {
        pixels: &'a XAuthorityCpuBufferSnapshot,
        origin: (i32, i32),
    },
    Stipple {
        pixels: &'a XAuthorityCpuBufferSnapshot,
        origin: (i32, i32),
        foreground: u32,
        /// `None` leaves an unset bit untouched.
        background: Option<u32>,
    },
}

impl XFillPattern<'_> {
    /// The colour for one destination pixel, or `None` to leave it alone.
    fn pixel_at(&self, x: i32, y: i32) -> Option<u32> {
        match self {
            Self::Solid(pixel) => Some(*pixel),
            Self::Tile { pixels, origin } => sample(pixels, x - origin.0, y - origin.1),
            Self::Stipple {
                pixels,
                origin,
                foreground,
                background,
            } => {
                // Any non-zero pixel is a set bit: a depth-one pixmap is held
                // here as an ordinary buffer, so its bits arrived as whole
                // pixels rather than packed.
                match sample(pixels, x - origin.0, y - origin.1) {
                    Some(value) if value & 0x00ff_ffff != 0 => Some(*foreground),
                    Some(_) => *background,
                    None => None,
                }
            }
        }
    }
}

/// Read one pixel of a pattern, wrapping so it repeats.
fn sample(pixels: &XAuthorityCpuBufferSnapshot, x: i32, y: i32) -> Option<u32> {
    if pixels.size.width <= 0 || pixels.size.height <= 0 {
        return None;
    }
    let x = x.rem_euclid(pixels.size.width);
    let y = y.rem_euclid(pixels.size.height);
    let stride = usize::try_from(pixels.stride).ok()?;
    let offset =
        usize::try_from(y).ok()?.checked_mul(stride)? + usize::try_from(x).ok()?.checked_mul(4)?;
    let bytes = pixels.bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// A depth-one pixmap used as a clip mask.
///
/// Held beside the pattern rather than inside the graphics context, because
/// the mask's pixels live in the drawable store and the context carries only
/// its identifier.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct XClipMask<'a> {
    pub pixels: Option<&'a XAuthorityCpuBufferSnapshot>,
    pub origin: (i32, i32),
}

impl XClipMask<'_> {
    /// Whether the mask admits a pixel. A mask that is present clips to its
    /// own extent: outside it, nothing is drawn.
    fn admits(&self, x: i32, y: i32) -> bool {
        let Some(pixels) = self.pixels else {
            return true;
        };
        let x = x - self.origin.0;
        let y = y - self.origin.1;
        if x < 0 || y < 0 || x >= pixels.size.width || y >= pixels.size.height {
            return false;
        }
        let stride = usize::try_from(pixels.stride).unwrap_or(0);
        let offset = usize::try_from(y).unwrap_or(0) * stride + usize::try_from(x).unwrap_or(0) * 4;
        pixels.bytes.get(offset..offset + 4).is_some_and(|bytes| {
            u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4])) & 0x00ff_ffff != 0
        })
    }

    /// Put back, from the bytes the buffer held before a draw, every pixel of
    /// `rect` the mask does not admit.
    ///
    /// Strokes, text, copies and images go through helpers that apply only
    /// the clip list. Undoing what they wrote outside the mask gives the
    /// result of never having written it, because each output pixel depends
    /// only on its own destination and on a source read before the draw.
    pub(super) fn restore_withheld(
        &self,
        buffer: &mut XAuthorityCpuBufferSnapshot,
        before: &[u8],
        rect: Rect,
    ) {
        if self.pixels.is_none() {
            return;
        }
        let Some((left, top, right, bottom)) = clipped_bounds(buffer.size, rect) else {
            return;
        };
        let stride = usize::try_from(buffer.stride).unwrap_or(0);
        let bytes = bytes_mut(buffer);
        for y in top..bottom {
            for x in left..right {
                let admitted = self.admits(
                    i32::try_from(x).unwrap_or(i32::MAX),
                    i32::try_from(y).unwrap_or(i32::MAX),
                );
                if admitted {
                    continue;
                }
                let offset = y.saturating_mul(stride).saturating_add(x.saturating_mul(4));
                let end = offset.saturating_add(4);
                if let (Some(target), Some(saved)) =
                    (bytes.get_mut(offset..end), before.get(offset..end))
                {
                    target.copy_from_slice(saved);
                }
            }
        }
    }
}

/// Fill a rectangle from a pattern, under a clip mask.
pub(super) fn fill_rect_masked(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    rect: Rect,
    pattern: XFillPattern<'_>,
    clip_mask: XClipMask<'_>,
    gc: &XGraphicsContextValues,
) {
    let Some((left, top, right, bottom)) = clipped_bounds(buffer.size, rect) else {
        return;
    };
    let stride = usize::try_from(buffer.stride).unwrap_or(0);
    let bytes = bytes_mut(buffer);
    for y in top..bottom {
        for x in left..right {
            if !pixel_in_clip_rects(x, y, gc) {
                continue;
            }
            let (pixel_x, pixel_y) = (
                i32::try_from(x).unwrap_or(i32::MAX),
                i32::try_from(y).unwrap_or(i32::MAX),
            );
            if !clip_mask.admits(pixel_x, pixel_y) {
                continue;
            }
            let Some(pixel) = pattern.pixel_at(pixel_x, pixel_y) else {
                continue;
            };
            let offset = y.saturating_mul(stride).saturating_add(x.saturating_mul(4));
            if let Some(target) = bytes.get_mut(offset..offset.saturating_add(4)) {
                let destination = u32::from_le_bytes(target.try_into().unwrap_or([0; 4]));
                let output = apply_raster_function(pixel, destination, gc);
                target.copy_from_slice(&output.to_le_bytes());
            }
        }
    }
}

pub(super) fn fill_rect(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    rect: Rect,
    pixel: u32,
    gc: &XGraphicsContextValues,
) {
    let Some((left, top, right, bottom)) = clipped_bounds(buffer.size, rect) else {
        return;
    };
    let stride = usize::try_from(buffer.stride).unwrap_or(0);
    let bytes = bytes_mut(buffer);
    for y in top..bottom {
        for x in left..right {
            if !pixel_in_clip(x, y, gc) {
                continue;
            }
            let offset = y.saturating_mul(stride).saturating_add(x.saturating_mul(4));
            if let Some(target) = bytes.get_mut(offset..offset.saturating_add(4)) {
                let destination = u32::from_le_bytes(target.try_into().unwrap_or([0; 4]));
                let output = apply_raster_function(pixel, destination, gc);
                target.copy_from_slice(&output.to_le_bytes());
            }
        }
    }
}

/// Fills a whole buffer with one pixel value.
///
/// Background painting has no graphics context, so this is not `fill_rect`
/// with a default one: there is no clip to test, no raster function to apply
/// and no plane mask. The window is simply that colour.
pub(super) fn fill_solid(buffer: &mut XAuthorityCpuBufferSnapshot, pixel: u32) {
    let value = pixel.to_le_bytes();
    for chunk in bytes_mut(buffer).chunks_exact_mut(4) {
        chunk.copy_from_slice(&value);
    }
}

/// Repeats `tile` across a whole buffer from its origin, as a background
/// pixmap is repeated across the window it backs.
pub(super) fn tile_solid(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    tile: &[u8],
    tile_size: Size,
    tile_stride: usize,
    (origin_x, origin_y): (i32, i32),
) {
    let Ok(tile_width) = usize::try_from(tile_size.width) else {
        return;
    };
    let Ok(tile_height) = usize::try_from(tile_size.height) else {
        return;
    };
    if tile_width == 0 || tile_height == 0 {
        return;
    }
    let width = usize::try_from(buffer.size.width).unwrap_or(0);
    let height = usize::try_from(buffer.size.height).unwrap_or(0);
    let stride = usize::try_from(buffer.stride).unwrap_or(0);
    let bytes = bytes_mut(buffer);
    // The tile's origin in the buffer's coordinates: the pixel at `(x, y)`
    // takes the tile's pixel at `(x - origin_x, y - origin_y)`, wrapped.
    let wrap = |value: usize, origin: i32, extent: usize| {
        let extent = i64::try_from(extent).unwrap_or(1).max(1);
        let at = i64::try_from(value).unwrap_or(0) - i64::from(origin);
        usize::try_from(at.rem_euclid(extent)).unwrap_or(0)
    };
    for y in 0..height {
        let source_row = wrap(y, origin_y, tile_height).saturating_mul(tile_stride);
        for x in 0..width {
            let source = source_row.saturating_add(wrap(x, origin_x, tile_width).saturating_mul(4));
            let Some(pixel) = tile.get(source..source.saturating_add(4)) else {
                continue;
            };
            let offset = y.saturating_mul(stride).saturating_add(x.saturating_mul(4));
            if let Some(target) = bytes.get_mut(offset..offset.saturating_add(4)) {
                target.copy_from_slice(pixel);
            }
        }
    }
}

pub(super) fn set_pixel(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    x: i32,
    y: i32,
    pixel: u32,
    gc: &XGraphicsContextValues,
) {
    if x < 0 || y < 0 || x >= buffer.size.width || y >= buffer.size.height {
        return;
    }
    let Ok(x) = usize::try_from(x) else {
        return;
    };
    let Ok(y) = usize::try_from(y) else {
        return;
    };
    if !pixel_in_clip(x, y, gc) {
        return;
    }
    let stride = usize::try_from(buffer.stride).unwrap_or(0);
    let offset = y.saturating_mul(stride).saturating_add(x.saturating_mul(4));
    if let Some(target) = bytes_mut(buffer).get_mut(offset..offset.saturating_add(4)) {
        let destination = u32::from_le_bytes(target.try_into().unwrap_or([0; 4]));
        target.copy_from_slice(&apply_raster_function(pixel, destination, gc).to_le_bytes());
    }
}

pub(super) fn draw_line(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    from: XPoint,
    to: XPoint,
    width: i32,
    gc: &XGraphicsContextValues,
) {
    let mut x = i32::from(from.x);
    let mut y = i32::from(from.y);
    let target_x = i32::from(to.x);
    let target_y = i32::from(to.y);
    let dx = (target_x - x).abs();
    let sx = if x < target_x { 1 } else { -1 };
    let dy = -(target_y - y).abs();
    let sy = if y < target_y { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        let offset = width / 2;
        fill_rect(
            buffer,
            Rect {
                x: x.saturating_sub(offset),
                y: y.saturating_sub(offset),
                width,
                height: width,
            },
            gc.foreground,
            gc,
        );
        if x == target_x && y == target_y {
            break;
        }
        let doubled = error.saturating_mul(2);
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

pub(super) fn draw_rectangle_outline(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    rectangle: Rect,
    line_width: i32,
    gc: &XGraphicsContextValues,
) {
    let half = line_width / 2;
    let outer_left = rectangle.x.saturating_sub(half);
    let outer_top = rectangle.y.saturating_sub(half);
    let outer_right = rectangle
        .x
        .saturating_add(rectangle.width)
        .saturating_sub(half)
        .saturating_add(line_width);
    let outer_bottom = rectangle
        .y
        .saturating_add(rectangle.height)
        .saturating_sub(half)
        .saturating_add(line_width);
    let inner_left = outer_left.saturating_add(line_width);
    let inner_top = outer_top.saturating_add(line_width);
    let inner_right = outer_right.saturating_sub(line_width);
    let inner_bottom = outer_bottom.saturating_sub(line_width);
    let outer = Rect {
        x: outer_left,
        y: outer_top,
        width: outer_right.saturating_sub(outer_left),
        height: outer_bottom.saturating_sub(outer_top),
    };
    if inner_left >= inner_right || inner_top >= inner_bottom {
        fill_rect(buffer, outer, gc.foreground, gc);
        return;
    }

    // The four bands do not overlap. That keeps every pixel to one raster operation,
    // and fill_rect clips each band before walking it.
    for band in [
        Rect {
            x: outer_left,
            y: outer_top,
            width: outer.width,
            height: inner_top.saturating_sub(outer_top),
        },
        Rect {
            x: outer_left,
            y: inner_bottom,
            width: outer.width,
            height: outer_bottom.saturating_sub(inner_bottom),
        },
        Rect {
            x: outer_left,
            y: inner_top,
            width: inner_left.saturating_sub(outer_left),
            height: inner_bottom.saturating_sub(inner_top),
        },
        Rect {
            x: inner_right,
            y: inner_top,
            width: outer_right.saturating_sub(inner_right),
            height: inner_bottom.saturating_sub(inner_top),
        },
    ] {
        fill_rect(buffer, band, gc.foreground, gc);
    }
}

pub(super) fn copy_xrgb8888(buffer: &mut XAuthorityCpuBufferSnapshot, rect: Rect, data: &[u8]) {
    let Some((left, top, right, bottom)) = clipped_bounds(buffer.size, rect) else {
        return;
    };
    let source_width = usize::try_from(rect.width.max(0)).unwrap_or(0);
    let source_height = usize::try_from(rect.height.max(0)).unwrap_or(0);
    let Some(source_stride) = source_width.checked_mul(4) else {
        return;
    };
    if data.len() < source_stride.saturating_mul(source_height) {
        return;
    }
    let target_stride = usize::try_from(buffer.stride).unwrap_or(0);
    let target_bytes = bytes_mut(buffer);
    for y in top..bottom {
        let source_y = usize::try_from(y as i64 - i64::from(rect.y)).unwrap_or(0);
        let source_x = usize::try_from(left as i64 - i64::from(rect.x)).unwrap_or(0);
        let width = right.saturating_sub(left);
        let source_offset = source_y
            .saturating_mul(source_stride)
            .saturating_add(source_x.saturating_mul(4));
        let target_offset = y
            .saturating_mul(target_stride)
            .saturating_add(left.saturating_mul(4));
        let byte_len = width.saturating_mul(4);
        let Some(source) = data.get(source_offset..source_offset.saturating_add(byte_len)) else {
            continue;
        };
        if let Some(target) =
            target_bytes.get_mut(target_offset..target_offset.saturating_add(byte_len))
        {
            target.copy_from_slice(source);
        }
    }
}

pub(super) fn put_image_pixels(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    rect: Rect,
    data: &[u8],
    semantics: Option<&super::XPutImageSemantics>,
) {
    let Some(semantics) = semantics else {
        return copy_xrgb8888(buffer, rect, data);
    };
    let gc = &semantics.gc;
    let mask = if semantics.depth == 32 {
        u32::MAX
    } else {
        (1u32 << semantics.depth) - 1
    };
    if gc.function == crate::X_GX_COPY
        && gc.plane_mask & mask == mask
        && gc.clip_rectangles.is_none()
    {
        return copy_xrgb8888(buffer, rect, data);
    }
    let Some((left, top, right, bottom)) = clipped_bounds(buffer.size, rect) else {
        return;
    };
    let stride = buffer.stride as usize;
    let source_stride = rect.width as usize * 4;
    let bytes = bytes_mut(buffer);
    for y in top..bottom {
        for x in left..right {
            if !pixel_in_clip(x, y, gc) {
                continue;
            }
            let src = (y as i64 - i64::from(rect.y)) as usize * source_stride
                + (x as i64 - i64::from(rect.x)) as usize * 4;
            let dst = y * stride + x * 4;
            let source = u32::from_le_bytes(data[src..src + 4].try_into().unwrap());
            let destination = u32::from_le_bytes(bytes[dst..dst + 4].try_into().unwrap());
            let planes = gc.plane_mask & mask;
            let result = (raster_function(source, destination, gc.function) & planes)
                | (destination & !planes);
            bytes[dst..dst + 4].copy_from_slice(&result.to_le_bytes());
        }
    }
}

/// Paint one glyph's set pixels at `(left, top)`.
///
/// Every pixel goes through `fill_rect`, so a glyph inherits the graphics
/// context's clip, raster function and plane mask exactly as a filled
/// rectangle does. Glyphs are any size the face declares, not one cell.
pub(super) fn draw_glyph(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    left: i32,
    top: i32,
    glyph: &XGlyph,
    pixel: u32,
    gc: &XGraphicsContextValues,
) {
    for row in 0..glyph.height {
        for column in 0..glyph.width {
            if !glyph.pixel(column, row) {
                continue;
            }
            fill_rect(
                buffer,
                Rect {
                    x: left.saturating_add(i32::from(column)),
                    y: top.saturating_add(i32::from(row)),
                    width: 1,
                    height: 1,
                },
                pixel,
                gc,
            );
        }
    }
}

pub(super) fn clipped_bounds(size: Size, rect: Rect) -> Option<(usize, usize, usize, usize)> {
    if size.width <= 0 || size.height <= 0 || rect.width <= 0 || rect.height <= 0 {
        return None;
    }
    let left = rect.x.max(0).min(size.width);
    let top = rect.y.max(0).min(size.height);
    let right = rect.x.saturating_add(rect.width).max(0).min(size.width);
    let bottom = rect.y.saturating_add(rect.height).max(0).min(size.height);
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        usize::try_from(left).ok()?,
        usize::try_from(top).ok()?,
        usize::try_from(right).ok()?,
        usize::try_from(bottom).ok()?,
    ))
}

pub(super) fn pixel_in_clip(x: usize, y: usize, gc: &XGraphicsContextValues) -> bool {
    pixel_in_clip_rects(x, y, gc)
}

/// Whether the clip *rectangles* admit a pixel. A pixmap mask is consulted
/// separately, by the caller that holds the store.
fn pixel_in_clip_rects(x: usize, y: usize, gc: &XGraphicsContextValues) -> bool {
    if gc.clip_rectangles.is_none() {
        return true;
    }
    let x = i32::try_from(x).unwrap_or(i32::MAX);
    let y = i32::try_from(y).unwrap_or(i32::MAX);
    gc.clip_rectangles.iter().flatten().any(|rect| {
        let left = rect.x.saturating_add(i32::from(gc.clip_x_origin));
        let top = rect.y.saturating_add(i32::from(gc.clip_y_origin));
        x >= left
            && y >= top
            && x < left.saturating_add(rect.width)
            && y < top.saturating_add(rect.height)
    })
}

pub(super) fn apply_raster_function(
    source: u32,
    destination: u32,
    gc: &XGraphicsContextValues,
) -> u32 {
    let result = raster_function(source, destination, gc.function);
    (result & gc.plane_mask) | (destination & !gc.plane_mask)
}

fn raster_function(source: u32, destination: u32, function: u8) -> u32 {
    match function {
        0 => 0,
        1 => source & destination,
        2 => source & !destination,
        3 => source,
        4 => !source & destination,
        5 => destination,
        6 => source ^ destination,
        7 => source | destination,
        8 => !(source | destination),
        9 => !(source ^ destination),
        10 => !destination,
        11 => source | !destination,
        12 => !source,
        13 => !source | destination,
        14 => !(source & destination),
        15 => u32::MAX,
        _ => source,
    }
}
