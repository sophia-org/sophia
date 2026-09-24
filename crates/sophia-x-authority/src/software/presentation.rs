//! Composing a window's pixels into its toplevel's presentation buffer.
//!
//! Each window keeps its own buffer, and the toplevel's presentation is what
//! the screen shows: a window's damage copied in, with whatever is stacked
//! over it copied over that (t188). IncludeInferiors runs the other way,
//! composing the inferiors into a window's buffer for a draw and scattering
//! the result back out (t181).

use std::sync::Arc;

use sophia_protocol::{Rect, Size};

use super::raster_ops::copy_buffer_region;
use super::render_ops::mask_rect_to_shape;
use super::{
    X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888, X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
    X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS, X_AUTHORITY_SOFTWARE_BUFFER_MAX_BYTES,
    XAuthorityCpuBufferPatchBatch, XAuthorityCpuBufferSnapshot, XAuthorityCpuBufferUpdate,
    XResourceId, XSoftwareBufferStore, coalesce_damage, packed_patch_region,
};

impl XSoftwareBufferStore {
    /// Compose a window's damage into its toplevel's presentation buffer.
    ///
    /// `shape` is the toplevel's bounding shape when it has one. The pixels
    /// outside it are cleared to transparent and the buffer is published as
    /// ARGB rather than XRGB, which is all it takes to make the shape real:
    /// the renderer already alpha-blends an ARGB layer over whatever is
    /// beneath it, so the cleared area stops being this window and starts
    /// being the desktop behind it.
    ///
    /// `stacking` says what else is on screen where the source is: the part
    /// of the source its ancestors leave visible, and the windows stacked
    /// over it. Each damaged rectangle is recomposed from the source and then
    /// from those windows, so a draw on a parent never covers a mapped child.
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
        stacking: &XPresentStacking,
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
            let rect = match stacking.source_clip {
                Some(clip) => {
                    let Some(visible) = intersect_rects(
                        translate_rect(*rect, source_offset_x, source_offset_y),
                        clip,
                    ) else {
                        continue;
                    };
                    translate_rect(visible, -source_offset_x, -source_offset_y)
                }
                None => *rect,
            };
            if let Some(rect) = copy_buffer_region(
                source,
                presentation_buffer,
                rect,
                source_offset_x,
                source_offset_y,
            ) {
                presentation_damage.push(rect);
            }
        }
        for rect in &presentation_damage {
            for layer in &stacking.above {
                let Some(part) = intersect_rects(*rect, layer.clip) else {
                    continue;
                };
                let Some(buffer) = self.buffers.get(&layer.window) else {
                    continue;
                };
                copy_buffer_region(
                    buffer,
                    presentation_buffer,
                    translate_rect(part, -layer.x, -layer.y),
                    layer.x,
                    layer.y,
                );
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

    /// Copy each inferior's visible pixels into `target`, bottom to top, so
    /// a draw through them starts from what is on screen (IncludeInferiors).
    pub(crate) fn compose_inferiors(
        &mut self,
        target: XResourceId,
        size: Size,
        inferiors: &[XPresentLayer],
    ) -> Option<()> {
        let handle = self.allocate_handle();
        self.ensure(target, size, handle)?;
        for layer in inferiors {
            let Some(source) = self.buffers.get(&layer.window).cloned() else {
                continue;
            };
            let buffer = self.buffers.get_mut(&target)?;
            copy_buffer_region(
                &source,
                buffer,
                translate_rect(layer.clip, -layer.x, -layer.y),
                layer.x,
                layer.y,
            );
        }
        Some(())
    }

    /// Copy what a draw through the inferiors left in `target` back into
    /// each of them, and return the inferiors that changed, each with the
    /// changed part in its own coordinates.
    pub(crate) fn scatter_to_inferiors(
        &mut self,
        target: XResourceId,
        inferiors: &[(XPresentLayer, Size)],
    ) -> Vec<(XResourceId, Rect)> {
        let mut changed = Vec::new();
        let Some(composed) = self.buffers.get(&target).cloned() else {
            return changed;
        };
        for (layer, size) in inferiors {
            let local = translate_rect(layer.clip, -layer.x, -layer.y);
            let Some(drawn) = self.image_region(target, layer.clip) else {
                continue;
            };
            if self.image_region(layer.window, local).as_ref() == Some(&drawn) {
                continue;
            }
            let handle = self.allocate_handle();
            let Some((buffer, replaced)) = self.ensure(layer.window, *size, handle) else {
                continue;
            };
            if copy_buffer_region(&composed, buffer, layer.clip, -layer.x, -layer.y).is_none() {
                continue;
            }
            let Some(generation) = buffer.generation.checked_add(1) else {
                continue;
            };
            buffer.generation = generation;
            self.note_export_damage(layer.window, replaced, Some(local));
            changed.push((layer.window, local));
        }
        changed
    }
}

/// What covers a window inside its toplevel, in presentation coordinates.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XPresentStacking {
    /// The part of the source its ancestors leave visible; `None` when the
    /// source is the toplevel itself.
    pub source_clip: Option<Rect>,
    /// The viewable windows over the source, bottom to top.
    pub above: Vec<XPresentLayer>,
}

/// One window composed over the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XPresentLayer {
    pub window: XResourceId,
    /// The window's origin.
    pub x: i32,
    pub y: i32,
    /// The part of the window its ancestors leave visible.
    pub clip: Rect,
}

fn translate_rect(rect: Rect, dx: i32, dy: i32) -> Rect {
    Rect {
        x: rect.x.saturating_add(dx),
        y: rect.y.saturating_add(dy),
        ..rect
    }
}

pub(crate) fn intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    (right > left && bottom > top).then(|| Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
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
