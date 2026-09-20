use std::collections::BTreeMap;
use std::sync::Arc;

use sophia_protocol::{Rect, Size};

use crate::{XFontHandle, XGraphicsContextValues, XPoint, XResourceId};

pub(crate) mod geometry;
mod pixmap_exports;
mod raster_ops;
mod raster_replay;
mod raster_variants;
mod render_gradient;
mod render_ops;
mod render_traps;
mod update;
use pixmap_exports::XPixmapExportDamage;

use raster_ops::{
    copy_buffer_region, copy_xrgb8888, draw_glyph, draw_line, draw_rectangle_outline, fill_rect,
    point_bounds, put_image_pixels, rectangle_outline_bounds, set_pixel,
};
pub(crate) use raster_variants::{
    XAuthorityRasterCommand, XAuthorityRasterStore, XOwnedTextDraw, XRasterPoint,
    XRasterSatisfyOutcome, XRasterUnsupportedKind,
};
pub use raster_variants::{XPutImageSemantics, XRasterFallbackCause};
pub use render_gradient::{XRenderGeneratedSource, XRenderGradientGeometry, XRenderGradientStop};
pub use render_ops::XRenderPictFormatKind;
pub use render_ops::{X_RENDER_IDENTITY_TRANSFORM, XRenderPictureFilter, XRenderRepeat};
pub(crate) use render_ops::{
    XRenderSampleMapping, XRenderSamplePlane, render_operator_is_implemented,
};
pub(crate) use render_traps::{
    XRenderTrapezoid, XRenderTriangle, rasterize_trapezoids, rasterize_triangles, trapezoid_bounds,
    triangle_bounds,
};

use render_ops::{mask_rect_to_shape, render_composite_rect, render_fill_rect};
pub use update::{
    X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS, XAuthorityCpuBufferPatch, XAuthorityCpuBufferPatchBatch,
    XAuthorityCpuBufferPatchRegion, XAuthorityCpuBufferSnapshot, XAuthorityCpuBufferUpdate,
    XAuthorityCpuDrawResult,
};
use update::{packed_patch_region, patch_is_representable};

pub const X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
/// The format a shaped presentation buffer takes. The renderer blends a
/// layer in this format over what is beneath it, which is what turns a
/// cleared region into a hole rather than a black patch.
pub const X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
pub const X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct XSoftwareBufferStore {
    next_handle: u64,
    buffers: BTreeMap<XResourceId, XAuthorityCpuBufferSnapshot>,
    presentations: BTreeMap<XResourceId, XAuthorityCpuBufferSnapshot>,
    export_damage: BTreeMap<XResourceId, XPixmapExportDamage>,
}

impl XSoftwareBufferStore {
    /// Whether this drawable already holds pixels written through the CPU path.
    ///
    /// Distinct from a presentation snapshot, which only windows acquire: a
    /// pixmap's pixels live here from its first upload.
    pub(crate) fn has_cpu_backing(&self, drawable: XResourceId) -> bool {
        self.buffers.contains_key(&drawable)
    }

    pub(crate) fn presentation_snapshot(
        &self,
        drawable: XResourceId,
    ) -> Option<&XAuthorityCpuBufferSnapshot> {
        self.presentations.get(&drawable)
    }
    pub fn remove(&mut self, drawable: XResourceId) -> Option<XAuthorityCpuBufferSnapshot> {
        self.presentations.remove(&drawable);
        self.export_damage.remove(&drawable);
        self.buffers.remove(&drawable)
    }

    /// Move retained pixmap pixels to a private key before its XID is reused.
    /// Existing exported snapshots keep their immutable storage and identity.
    pub(crate) fn rekey_pixmap(&mut self, from: XResourceId, to: XResourceId) {
        if let Some(damage) = self.export_damage.remove(&from) {
            self.export_damage.insert(to, damage);
        }
        if let Some(mut buffer) = self.buffers.remove(&from) {
            buffer.drawable = to;
            self.buffers.insert(to, buffer);
        }
    }

    /// Compose a window's damage into its toplevel's presentation buffer.
    ///
    /// `shape` is the toplevel's bounding shape when it has one. The pixels
    /// outside it are cleared to transparent and the buffer is published as
    /// ARGB rather than XRGB, which is all it takes to make the shape real:
    /// the renderer already alpha-blends an ARGB layer over whatever is
    /// beneath it, so the cleared area stops being this window and starts
    /// being the desktop behind it.
    #[allow(clippy::too_many_arguments)]
    pub fn present_window_damage(
        &mut self,
        presentation: XResourceId,
        presentation_size: Size,
        source: XResourceId,
        source_offset_x: i32,
        source_offset_y: i32,
        damage: &[Rect],
        shape: Option<&[Rect]>,
    ) -> Option<XAuthorityCpuBufferUpdate> {
        let (source_drawable, source_size) = {
            let source_buffer = self.buffers.get(&source)?;
            (source_buffer.drawable, source_buffer.size)
        };
        if presentation_size.width <= 0 || presentation_size.height <= 0 {
            return None;
        }
        let source_extent = Size {
            width: source_offset_x
                .saturating_add(source_size.width)
                .clamp(1, presentation_size.width),
            height: source_offset_y
                .saturating_add(source_size.height)
                .clamp(1, presentation_size.height),
        };
        let desired_size = if source_drawable == presentation {
            presentation_size
        } else {
            self.presentations
                .get(&presentation)
                .map(|buffer| Size {
                    width: buffer
                        .size
                        .width
                        .max(source_extent.width)
                        .min(presentation_size.width),
                    height: buffer
                        .size
                        .height
                        .max(source_extent.height)
                        .min(presentation_size.height),
                })
                .unwrap_or(source_extent)
        };
        let replace = self
            .presentations
            .get(&presentation)
            .is_none_or(|buffer| buffer.size != desired_size);
        if replace {
            let previous = self.presentations.get(&presentation).cloned();
            let width = usize::try_from(desired_size.width).ok()?;
            let height = usize::try_from(desired_size.height).ok()?;
            let stride = width.checked_mul(4)?;
            let byte_len = stride.checked_mul(height)?;
            if width == 0 || height == 0 || byte_len > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES {
                return None;
            }
            let handle = self.allocate_handle();
            let generation = self
                .presentations
                .get(&presentation)
                .map_or(0, |buffer| buffer.generation);
            self.presentations.insert(
                presentation,
                XAuthorityCpuBufferSnapshot {
                    handle,
                    drawable: presentation,
                    size: desired_size,
                    stride: u32::try_from(stride).ok()?,
                    format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
                    generation,
                    bytes: Arc::new(vec![0; byte_len]),
                },
            );
            if let Some(previous) = previous
                && let Some(buffer) = self.presentations.get_mut(&presentation)
            {
                copy_buffer_region(
                    &previous,
                    buffer,
                    Rect {
                        x: 0,
                        y: 0,
                        width: previous.size.width,
                        height: previous.size.height,
                    },
                    0,
                    0,
                );
            }
        }
        let source = self.buffers.get(&source)?;
        let presentation_buffer = self.presentations.get_mut(&presentation)?;
        let mut presentation_damage = Vec::with_capacity(damage.len());
        for rect in damage {
            if let Some(rect) = copy_buffer_region(
                source,
                presentation_buffer,
                *rect,
                source_offset_x,
                source_offset_y,
            ) {
                presentation_damage.push(rect);
            }
        }
        // A shaped presentation carries alpha, an unshaped one does not.
        // Crossing between the two changes how every pixel in the buffer is
        // read, so the whole buffer has to ship rather than a patch that the
        // receiver would interpret under the old format.
        let target_format = match shape {
            Some(_) => X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888,
            None => X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
        };
        let format_changed = presentation_buffer.format != target_format;
        presentation_buffer.format = target_format;
        if let Some(shape) = shape {
            // Only the damaged rectangles are masked. Everything outside them
            // was masked when it was drawn, and re-masking the whole buffer
            // every frame would cost the window's area per damage event.
            for rect in &presentation_damage {
                mask_rect_to_shape(presentation_buffer, *rect, shape);
            }
        }
        let replace = replace || format_changed;
        presentation_buffer.generation = presentation_buffer.generation.checked_add(1)?;
        // A busy client is not a reason to resend the window. The transport
        // carries at most 32 rectangles, and a damage list longer than that
        // used to fall back to replacing the whole presentation buffer -- which
        // is the common case for a browser, the one client whose buffers are
        // largest. Coalescing merges the list down to the bound instead, and
        // the merged cover is a superset of the damage, so the patch carries
        // pixels that are already correct in the buffer it is read from.
        //
        // The bound itself does not move. It is validated identically on both
        // sides of the wire, so raising it would have to move the encoder, both
        // guards, and the renderer's capacity refusal together.
        if replace {
            return Some(XAuthorityCpuBufferUpdate::Replace(
                presentation_buffer.clone(),
            ));
        }
        let presentation_damage =
            if presentation_damage.len() > X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS {
                let coalesced =
                    coalesce_damage(presentation_damage, X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS);
                // Past a point a merged cover stops being cheaper than the buffer.
                // Half the area is where it has lost the argument: the batch still
                // carries per-rectangle headers and the receiver still walks them,
                // for a saving that is no longer most of the frame.
                //
                // Only a coalesced list is measured. A short damage list is sent as
                // it stands whatever it covers, which is the behaviour every
                // existing caller and regression already depends on.
                let buffer_area = usize::try_from(presentation_buffer.size.width)
                    .ok()?
                    .saturating_mul(usize::try_from(presentation_buffer.size.height).ok()?);
                if coverage_area(&coalesced).saturating_mul(2) >= buffer_area {
                    return Some(XAuthorityCpuBufferUpdate::Replace(
                        presentation_buffer.clone(),
                    ));
                }
                coalesced
            } else {
                presentation_damage
            };
        let patches = presentation_damage
            .into_iter()
            .map(|rect| packed_patch_region(presentation_buffer, rect))
            .collect::<Option<Vec<_>>>()?;
        Some(XAuthorityCpuBufferUpdate::PatchBatch(
            XAuthorityCpuBufferPatchBatch {
                handle: presentation_buffer.handle,
                drawable: presentation_buffer.drawable,
                size: presentation_buffer.size,
                stride: presentation_buffer.stride,
                format: presentation_buffer.format,
                generation: presentation_buffer.generation,
                patches,
            },
        ))
    }

    pub fn paint_damage(
        &mut self,
        drawable: XResourceId,
        size: Size,
        damage: &[Rect],
        gc: &XGraphicsContextValues,
    ) -> Option<XAuthorityCpuDrawResult> {
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        for rect in damage {
            fill_rect(buffer, *rect, gc.foreground, gc);
        }
        let published_damage = union_rects(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    /// Lift a drawable's pixels into an owned sample plane, so compositing
    /// can read them while the destination is being written -- including
    /// when source and destination are the same drawable.
    ///
    /// A drawable with no backing yet samples as transparent black, which is
    /// what an untouched picture contains.
    pub(crate) fn render_sample_plane(
        &self,
        drawable: XResourceId,
        format: XRenderPictFormatKind,
        repeat: XRenderRepeat,
        transform: Option<[i32; 9]>,
        filter: XRenderPictureFilter,
    ) -> XRenderSamplePlane {
        let mapping = XRenderSampleMapping::new(transform, filter);
        match self.buffers.get(&drawable) {
            Some(buffer) => XRenderSamplePlane::from_buffer(buffer, format, repeat, mapping),
            None => XRenderSamplePlane::empty(repeat),
        }
    }

    /// Composite a sampled source, and optionally a mask, onto a rectangle
    /// of a picture.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_composite(
        &mut self,
        drawable: XResourceId,
        size: Size,
        op: u8,
        source: &XRenderSamplePlane,
        mask: Option<&XRenderSamplePlane>,
        component_alpha: bool,
        source_origin: (i32, i32),
        mask_origin: (i32, i32),
        rect: Rect,
        clip: Option<&[Rect]>,
        format: XRenderPictFormatKind,
    ) -> Option<XAuthorityCpuDrawResult> {
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        render_composite_rect(
            buffer,
            op,
            source,
            mask,
            component_alpha,
            source_origin,
            mask_origin,
            rect,
            clip,
            format,
        );
        let published_damage = Some(rect);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    /// Fill rectangles of a picture with one premultiplied color through a
    /// RENDER operator, clipped to the picture's translated clip list.
    pub(crate) fn render_fill(
        &mut self,
        drawable: XResourceId,
        size: Size,
        op: u8,
        color: [u8; 4],
        rects: &[Rect],
        clip: Option<&[Rect]>,
        format: XRenderPictFormatKind,
    ) -> Option<XAuthorityCpuDrawResult> {
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        for rect in rects {
            render_fill_rect(buffer, *rect, op, color, clip, format);
        }
        let published_damage = union_rects(rects);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    pub fn clear(
        &mut self,
        drawable: XResourceId,
        size: Size,
        rect: Rect,
        pixel: u32,
    ) -> Option<XAuthorityCpuDrawResult> {
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        fill_rect(buffer, rect, pixel, &XGraphicsContextValues::default());
        let published_damage = Some(rect);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    /// Paint one or more text runs.
    ///
    /// Placement is the server's: a glyph sits at its left side bearing from
    /// the pen and its own ascent above the baseline, and the pen advances by
    /// the character width whatever the glyph's ink measures
    /// (`mi/miglblt.c:139-172`). A character the face does not define takes
    /// the default character, and if the face has no usable default it is
    /// dropped entirely -- no ink and no advance.
    ///
    /// `ImageText` differs from `PolyText` in two ways, both taken from
    /// `miImageGlyphBlt`: it first fills a background box spanning the run's
    /// total advance and the *font's* ascent and descent, and it forces a copy
    /// function and a solid fill for both the box and the glyphs, so the
    /// graphics context's function and fill style are deliberately ignored.
    pub fn draw_text(
        &mut self,
        drawable: XResourceId,
        size: Size,
        draws: &[XTextDraw<'_>],
        gc: &XGraphicsContextValues,
    ) -> Option<XAuthorityCpuDrawResult> {
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        let mut damage = Vec::with_capacity(draws.len());
        for draw in draws {
            if draw.text.is_empty() {
                continue;
            }
            let metrics = &draw.font.metrics;
            let top = draw.baseline.saturating_sub(i32::from(metrics.font_ascent));
            let height = i32::from(metrics.font_ascent.saturating_add(metrics.font_descent));
            let width = metrics.text_extents(draw.text).overall_width;
            let draw_gc;
            let raster_gc = if draw.image {
                draw_gc = XGraphicsContextValues {
                    function: crate::X_GX_COPY,
                    fill_style: 0,
                    ..gc.clone()
                };
                &draw_gc
            } else {
                gc
            };
            if draw.image {
                fill_rect(
                    buffer,
                    Rect {
                        x: draw.x,
                        y: top,
                        width,
                        height,
                    },
                    gc.background,
                    raster_gc,
                );
            }
            let mut pen = draw.x;
            for code in draw.text.iter().copied() {
                let Some((info, glyph)) = draw.font.glyph(code) else {
                    continue;
                };
                draw_glyph(
                    buffer,
                    pen.saturating_add(i32::from(info.left_side_bearing)),
                    draw.baseline.saturating_sub(i32::from(info.ascent)),
                    glyph,
                    gc.foreground,
                    raster_gc,
                );
                pen = pen.saturating_add(i32::from(info.character_width));
            }
            damage.push(Rect {
                x: draw.x,
                y: top,
                width,
                height,
            });
        }
        let published_damage = union_rects(&damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    pub fn put_image(
        &mut self,
        drawable: XResourceId,
        size: Size,
        destination: Rect,
        data: &[u8],
        semantics: Option<&XPutImageSemantics>,
    ) -> Option<XAuthorityCpuDrawResult> {
        let len = usize::try_from(destination.width)
            .ok()?
            .checked_mul(usize::try_from(destination.height).ok()?)?
            .checked_mul(4)?;
        if data.len() < len
            || semantics.is_some_and(|s| crate::x11_pixmap_format(s.depth).is_none())
        {
            return None;
        }
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        put_image_pixels(buffer, destination, data, semantics);
        let published_damage = Some(destination);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    pub fn ensure_image_backing(&mut self, drawable: XResourceId, size: Size) -> Option<()> {
        let handle = self.allocate_handle();
        self.ensure(drawable, size, handle).map(|_| ())
    }

    pub fn put_image_backing(
        &mut self,
        drawable: XResourceId,
        size: Size,
        destination: Rect,
        data: &[u8],
    ) -> Option<()> {
        let handle = self.allocate_handle();
        let (buffer, _) = self.ensure(drawable, size, handle)?;
        copy_xrgb8888(buffer, destination, data);
        buffer.generation = buffer.generation.checked_add(1)?;
        self.note_export_damage(drawable, false, Some(destination));
        Some(())
    }

    pub fn image_region(&self, drawable: XResourceId, region: Rect) -> Option<Vec<u8>> {
        let width = usize::try_from(region.width).ok()?;
        let height = usize::try_from(region.height).ok()?;
        let byte_len = width.checked_mul(height)?.checked_mul(4)?;
        if byte_len > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES {
            return None;
        }
        if width == 0 || height == 0 {
            return Some(Vec::new());
        }
        let mut image = Vec::new();
        image.try_reserve_exact(byte_len).ok()?;
        image.resize(byte_len, 0);
        let Some(buffer) = self.buffers.get(&drawable) else {
            return Some(image);
        };
        let source_width = usize::try_from(buffer.size.width).ok()?;
        let source_height = usize::try_from(buffer.size.height).ok()?;
        let source_stride = usize::try_from(buffer.stride).ok()?;
        let destination_stride = width.checked_mul(4)?;
        for row in 0..height {
            let source_y = region
                .y
                .checked_add(i32::try_from(row).ok()?)
                .and_then(|y| usize::try_from(y).ok());
            let Some(source_y) = source_y.filter(|y| *y < source_height) else {
                continue;
            };
            for column in 0..width {
                let source_x = region
                    .x
                    .checked_add(i32::try_from(column).ok()?)
                    .and_then(|x| usize::try_from(x).ok());
                let Some(source_x) = source_x.filter(|x| *x < source_width) else {
                    continue;
                };
                let source_offset = source_y
                    .checked_mul(source_stride)?
                    .checked_add(source_x.checked_mul(4)?)?;
                let destination_offset = row
                    .checked_mul(destination_stride)?
                    .checked_add(column.checked_mul(4)?)?;
                image
                    .get_mut(destination_offset..destination_offset.checked_add(4)?)?
                    .copy_from_slice(
                        buffer
                            .bytes
                            .get(source_offset..source_offset.checked_add(4)?)?,
                    );
            }
        }
        Some(image)
    }

    /// Draw disjoint segments and report the rectangle they dirtied.
    pub fn draw_segments(
        &mut self,
        drawable: XResourceId,
        size: Size,
        segments: &[(XPoint, XPoint)],
        gc: &XGraphicsContextValues,
    ) -> Option<(XAuthorityCpuDrawResult, Rect)> {
        let points: Vec<XPoint> = segments
            .iter()
            .flat_map(|(from, to)| [*from, *to])
            .collect();
        let damage = point_bounds(&points, gc.line_width)?;
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        let width = i32::from(gc.line_width.max(1));
        let mut dashes = geometry::dash::XDashState::new(&gc.dashes, gc.dash_offset);
        for (from, to) in segments {
            draw_dashed(buffer, *from, *to, width, gc, &mut dashes);
        }
        let published_damage = Some(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        Some((result?, damage))
    }

    pub fn draw_lines(
        &mut self,
        drawable: XResourceId,
        size: Size,
        points: &[XPoint],
        gc: &XGraphicsContextValues,
    ) -> Option<XAuthorityCpuDrawResult> {
        let damage = point_bounds(points, gc.line_width)?;
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        let width = i32::from(gc.line_width.max(1));
        // The dash pattern is walked across the whole polyline rather than
        // restarted at each vertex, so a dashed outline is dashed evenly
        // around its corners instead of putting a dash at every one.
        let mut dashes = geometry::dash::XDashState::new(&gc.dashes, gc.dash_offset);
        for pair in points.windows(2) {
            draw_dashed(buffer, pair[0], pair[1], width, gc, &mut dashes);
        }
        let published_damage = Some(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result
    }

    pub fn draw_rectangles(
        &mut self,
        drawable: XResourceId,
        size: Size,
        rectangles: &[Rect],
        gc: &XGraphicsContextValues,
    ) -> Option<(XAuthorityCpuDrawResult, Rect)> {
        let damage = rectangle_outline_bounds(rectangles, gc.line_width)?;
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(drawable, size, handle)?;
        let line_width = i32::from(gc.line_width.max(1));
        for rectangle in rectangles {
            draw_rectangle_outline(buffer, *rectangle, line_width, gc);
        }
        let published_damage = Some(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(drawable, replaced, published_damage);
        result.map(|update| (update, damage))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn copy_area(
        &mut self,
        source: XResourceId,
        destination: XResourceId,
        destination_size: Size,
        source_rect: Rect,
        dst_x: i16,
        dst_y: i16,
        gc: &XGraphicsContextValues,
    ) -> Option<(XAuthorityCpuDrawResult, Rect)> {
        let source = self.buffers.get(&source)?.clone();
        let handle = self.allocate_handle();
        let (buffer, replaced) = self.ensure(destination, destination_size, handle)?;
        let source_width = source.size.width;
        let source_height = source.size.height;
        let destination_width = destination_size.width;
        let destination_height = destination_size.height;
        let requested_width = source_rect.width.max(0);
        let requested_height = source_rect.height.max(0);
        let offset_left = 0
            .max(source_rect.x.saturating_neg())
            .max(i32::from(dst_x).saturating_neg());
        let offset_top = 0
            .max(source_rect.y.saturating_neg())
            .max(i32::from(dst_y).saturating_neg());
        let offset_right = requested_width
            .min(source_width.saturating_sub(source_rect.x))
            .min(destination_width.saturating_sub(i32::from(dst_x)));
        let offset_bottom = requested_height
            .min(source_height.saturating_sub(source_rect.y))
            .min(destination_height.saturating_sub(i32::from(dst_y)));
        if offset_right <= offset_left || offset_bottom <= offset_top {
            return None;
        }
        let source_stride = usize::try_from(source.stride).ok()?;
        for y_offset in offset_top..offset_bottom {
            let source_y = usize::try_from(source_rect.y.saturating_add(y_offset)).ok()?;
            for x_offset in offset_left..offset_right {
                let source_x = usize::try_from(source_rect.x.saturating_add(x_offset)).ok()?;
                let offset = source_y
                    .saturating_mul(source_stride)
                    .saturating_add(source_x.saturating_mul(4));
                let pixel = u32::from_le_bytes(
                    source
                        .bytes
                        .get(offset..offset.saturating_add(4))?
                        .try_into()
                        .ok()?,
                );
                let target_x = i32::from(dst_x).saturating_add(x_offset);
                let target_y = i32::from(dst_y).saturating_add(y_offset);
                set_pixel(buffer, target_x, target_y, pixel, gc);
            }
        }
        let damage = Rect {
            x: i32::from(dst_x).saturating_add(offset_left),
            y: i32::from(dst_y).saturating_add(offset_top),
            width: offset_right.saturating_sub(offset_left),
            height: offset_bottom.saturating_sub(offset_top),
        };
        let published_damage = Some(damage);
        let result = finish_immutable_update(buffer, handle, replaced, published_damage);
        self.note_export_damage(destination, replaced, published_damage);
        result.map(|update| (update, damage))
    }

    fn ensure(
        &mut self,
        drawable: XResourceId,
        size: Size,
        handle: u64,
    ) -> Option<(&mut XAuthorityCpuBufferSnapshot, bool)> {
        let width = usize::try_from(size.width).ok()?;
        let height = usize::try_from(size.height).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let stride = width.checked_mul(4)?;
        let byte_len = stride.checked_mul(height)?;
        if byte_len > X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES {
            return None;
        }

        let replace = self
            .buffers
            .get(&drawable)
            .is_none_or(|buffer| buffer.size != size);
        if replace {
            let previous = self.buffers.get(&drawable);
            let generation = previous.map_or(0, |buffer| buffer.generation);
            self.buffers.insert(
                drawable,
                XAuthorityCpuBufferSnapshot {
                    handle,
                    drawable,
                    size,
                    stride: u32::try_from(stride).ok()?,
                    format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
                    generation,
                    bytes: Arc::new(vec![0; byte_len]),
                },
            );
        }
        self.buffers
            .get_mut(&drawable)
            .map(|buffer| (buffer, replace))
    }

    fn allocate_handle(&mut self) -> u64 {
        let handle = self.next_handle.max(1);
        self.next_handle = handle.saturating_add(1).max(1);
        handle
    }
}

/// Rebrand a drawable's snapshot after a core drawing operation.
///
/// The damage is still validated exactly as before. `patch_is_representable`
/// makes every refusal `packed_patch` makes about a rectangle -- an empty or
/// fully clipped rect, an unrepresentable offset, a snapshot too short for the
/// rows it claims -- and returning `None` here is what turns those into
/// `InvalidResource`. What is gone is the copy: the patch was built and dropped
/// without being read, and the snapshot was cloned whole for a caller that
/// wanted its handle.
///
/// The pixels reach Engine through `present_window_damage`, which composes this
/// drawable into its toplevel's presentation buffer and publishes that.
/// Merge a damage list down to at most `limit` rectangles.
///
/// Deterministic, because the wire tests replay a recorded sequence and compare
/// what came out: rectangles are ordered by row then column, split into `limit`
/// contiguous groups, and each group becomes its bounding box. Sorting by
/// position first is what makes the groups spatially close, so the boxes stay
/// near the damage rather than spanning it.
///
/// Over-approximating is safe and under-approximating is not. The patches are
/// read from the presentation buffer *after* the client's pixels were composed
/// into it, so a larger rectangle carries more already-correct pixels; a
/// smaller one would leave a region stale in a frame that is otherwise
/// presentable and self-consistent. `StableBackingLease`'s `RegistryMatchesStore`
/// is that property, and its second negative control is this function shrinking
/// a cover below the damage it owes.
fn coalesce_damage(mut rects: Vec<Rect>, limit: usize) -> Vec<Rect> {
    if rects.len() <= limit || limit == 0 {
        return rects;
    }
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.width, rect.height));
    let mut coalesced = Vec::with_capacity(limit);
    let total = rects.len();
    for group in 0..limit {
        // Contiguous groups over the sorted list, sized so every rectangle
        // lands in exactly one and no group is empty.
        let start = group.saturating_mul(total) / limit;
        let end = group.saturating_add(1).saturating_mul(total) / limit;
        if start >= end {
            continue;
        }
        let mut bounds = rects[start];
        for rect in &rects[start.saturating_add(1)..end] {
            bounds = union_rect(bounds, *rect);
        }
        coalesced.push(bounds);
    }
    coalesced
}

fn union_rect(left: Rect, right: Rect) -> Rect {
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

/// The area a damage list covers, counting overlap twice.
///
/// An over-count is the safe direction here: it can only push the decision
/// toward replacing the buffer, which is always correct and merely less clever.
fn coverage_area(rects: &[Rect]) -> usize {
    rects
        .iter()
        .map(|rect| {
            usize::try_from(rect.width.max(0))
                .unwrap_or(0)
                .saturating_mul(usize::try_from(rect.height.max(0)).unwrap_or(0))
        })
        .fold(0usize, usize::saturating_add)
}

fn finish_immutable_update(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    handle: u64,
    replaced: bool,
    damage: Option<Rect>,
) -> Option<XAuthorityCpuDrawResult> {
    if !replaced {
        patch_is_representable(buffer, damage?)?;
    }
    buffer.generation = buffer.generation.checked_add(1)?;
    buffer.handle = handle;
    Some(XAuthorityCpuDrawResult {
        handle,
        size: buffer.size,
        generation: buffer.generation,
    })
}

fn union_rects(rectangles: &[Rect]) -> Option<Rect> {
    let first = *rectangles.first()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x.saturating_add(first.width);
    let mut bottom = first.y.saturating_add(first.height);
    for rect in &rectangles[1..] {
        left = left.min(rect.x);
        top = top.min(rect.y);
        right = right.max(rect.x.saturating_add(rect.width));
        bottom = bottom.max(rect.y.saturating_add(rect.height));
    }
    Some(Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    })
}

pub(crate) struct XTextDraw<'a> {
    pub x: i32,
    pub baseline: i32,
    /// Characters as CHAR2B values, whichever request width carried them.
    pub text: &'a [u16],
    pub image: bool,
    pub font: XFontHandle,
}

/// Draw one line, honouring the graphics context's line style.
///
/// A solid line is drawn whole. `LineOnOffDash` paints only the on runs;
/// `LineDoubleDash` paints the off runs in the background colour, which is
/// what makes a two-colour dashed border possible.
fn draw_dashed(
    buffer: &mut XAuthorityCpuBufferSnapshot,
    from: XPoint,
    to: XPoint,
    width: i32,
    gc: &XGraphicsContextValues,
    state: &mut geometry::dash::XDashState,
) {
    if gc.line_style == crate::X_LINE_SOLID {
        draw_line(buffer, from, to, width, gc);
        return;
    }
    for run in geometry::dash::split(from, to, &gc.dashes, state) {
        if run.on {
            draw_line(buffer, run.from, run.to, width, gc);
        } else if gc.line_style == crate::X_LINE_DOUBLE_DASH {
            let background = XGraphicsContextValues {
                foreground: gc.background,
                ..gc.clone()
            };
            draw_line(buffer, run.from, run.to, width, &background);
        }
    }
}
