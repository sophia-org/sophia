use std::collections::VecDeque;
use std::sync::Arc;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use sophia_engine::HeadCompositorIndicatorStrip;
use sophia_protocol::{
    POLICY_INDICATOR_STATE_ACTIVE, POLICY_INDICATOR_STATE_OCCUPIED, POLICY_INDICATOR_STATE_URGENT,
    POLICY_INDICATOR_STATE_VISIBLE_ELSEWHERE, POLICY_OUTPUT_STATUS_FOCUS_MASK, Rect, Size,
};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Rect as KurboRect, Shape};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::{LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveSharedCpuBufferSource};

pub const INDICATOR_STRIP_FONT_RELEASE: &str = "JetBrains Mono 2.304";
pub const INDICATOR_STRIP_FONT_SHA256: &str =
    "fb3b2575d7b0657359707993288f12a7360344d39387bb26050e276d61f6bd2a";
pub const INDICATOR_STRIP_CACHE_MAX_ENTRIES: usize = 128;
pub const INDICATOR_STRIP_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMonoNL-Regular.ttf");
const FONT_FAMILY: &str = "JetBrains Mono NL";
const LOGICAL_STRIP_HEIGHT: f32 = 14.0;
const LOGICAL_FONT_SIZE: f32 = 10.0;
const LOGICAL_LINE_HEIGHT: f32 = 12.0;

const BACKGROUND: AlphaColor<Srgb> = AlphaColor::from_rgb8(0x11, 0x11, 0x11);
const OCCUPIED_TEXT: AlphaColor<Srgb> = AlphaColor::from_rgb8(0xee, 0xee, 0xee);
const IDLE_TEXT: AlphaColor<Srgb> = AlphaColor::from_rgb8(0x7c, 0x7c, 0x7c);
const ACTIVE: AlphaColor<Srgb> = AlphaColor::from_rgb8(0x70, 0xb7, 0xff);
const URGENT: AlphaColor<Srgb> = AlphaColor::from_rgb8(0xff, 0xb6, 0xb0);

#[derive(Clone, Copy, Debug, PartialEq)]
struct IndicatorBrush(AlphaColor<Srgb>);

impl Default for IndicatorBrush {
    fn default() -> Self {
        Self(OCCUPIED_TEXT)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndicatorStripCacheKey {
    node: sophia_engine::CompositorNodeId,
    generation: u64,
    strip: sophia_engine::IndicatorChromeStrip,
}

#[derive(Clone, Debug)]
struct IndicatorStripCacheEntry {
    key: IndicatorStripCacheKey,
    buffer: LiveSharedCpuBufferSource,
    bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IndicatorStripCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndicatorStripRasterError {
    InvalidGeometry,
    OutputTooLarge,
    FontUnavailable,
    HandleExhausted,
}

impl core::fmt::Display for IndicatorStripRasterError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for IndicatorStripRasterError {}

/// Renderer-private cold-path cache for immutable Tier-0 strip rasters.
///
/// Entries own no Engine or protocol buffer identity. A returned source holds
/// its pixels through `Arc`, so eviction cannot invalidate an in-flight frame.
pub struct IndicatorStripRasterCache {
    font_context: FontContext,
    layout_context: LayoutContext<IndicatorBrush>,
    entries: VecDeque<IndicatorStripCacheEntry>,
    bytes: usize,
    next_handle: u64,
    hits: usize,
    misses: usize,
    evictions: usize,
}

impl Default for IndicatorStripRasterCache {
    fn default() -> Self {
        let mut font_context = FontContext::new();
        font_context
            .collection
            .register_fonts(FONT_BYTES.to_vec().into(), None);
        Self {
            font_context,
            layout_context: LayoutContext::new(),
            entries: VecDeque::new(),
            bytes: 0,
            next_handle: 0x8000_0000_0000_0000,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }
}

impl IndicatorStripRasterCache {
    pub fn raster_for(
        &mut self,
        strip: &HeadCompositorIndicatorStrip,
    ) -> Result<LiveSharedCpuBufferSource, IndicatorStripRasterError> {
        let key = IndicatorStripCacheKey {
            node: strip.node,
            generation: strip.generation,
            strip: strip.strip.clone(),
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = self
                .entries
                .remove(index)
                .expect("cache index came from iteration");
            let buffer = entry.buffer.clone();
            self.entries.push_back(entry);
            self.hits = self.hits.saturating_add(1);
            return Ok(buffer);
        }
        self.misses = self.misses.saturating_add(1);
        let handle = self.next_handle;
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .ok_or(IndicatorStripRasterError::HandleExhausted)?;
        let buffer = self.rasterize(strip, handle)?;
        let bytes = buffer.bytes.len();
        if bytes > INDICATOR_STRIP_CACHE_MAX_BYTES {
            return Err(IndicatorStripRasterError::OutputTooLarge);
        }
        while self.entries.len() >= INDICATOR_STRIP_CACHE_MAX_ENTRIES
            || self.bytes.saturating_add(bytes) > INDICATOR_STRIP_CACHE_MAX_BYTES
        {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
            self.evictions = self.evictions.saturating_add(1);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.push_back(IndicatorStripCacheEntry {
            key,
            buffer: buffer.clone(),
            bytes,
        });
        Ok(buffer)
    }

    pub fn stats(&self) -> IndicatorStripCacheStats {
        IndicatorStripCacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            entries: self.entries.len(),
            bytes: self.bytes,
        }
    }

    fn rasterize(
        &mut self,
        strip: &HeadCompositorIndicatorStrip,
        handle: u64,
    ) -> Result<LiveSharedCpuBufferSource, IndicatorStripRasterError> {
        let geometry = strip.strip.geometry;
        let width = u16::try_from(geometry.width)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(IndicatorStripRasterError::InvalidGeometry)?;
        let height = u16::try_from(geometry.height)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(IndicatorStripRasterError::InvalidGeometry)?;
        let scale = f32::from(height) / LOGICAL_STRIP_HEIGHT;
        let marker = (2.0 * scale).round().max(1.0);
        let padding = (4.0 * scale).round().max(1.0);
        let mut context = RenderContext::new(width, height);
        let mut resources = Resources::new();
        context.set_paint(BACKGROUND);
        context.fill_rect(&KurboRect::new(
            0.0,
            0.0,
            f64::from(width),
            f64::from(height),
        ));

        for (cell, label, state) in &strip.strip.labels {
            let Some(cell) = local_rect(*cell, geometry) else {
                continue;
            };
            if state & POLICY_INDICATOR_STATE_ACTIVE != 0 {
                context.set_paint(ACTIVE);
                context.fill_rect(&KurboRect::new(
                    cell.x0,
                    (cell.y1 - f64::from(marker)).max(cell.y0),
                    cell.x1,
                    cell.y1,
                ));
            }
            if state & POLICY_INDICATOR_STATE_URGENT != 0 {
                context.set_paint(URGENT);
                context.fill_rect(&KurboRect::new(
                    cell.x0,
                    cell.y0,
                    cell.x1,
                    (cell.y0 + f64::from(marker)).min(cell.y1),
                ));
            }
            if state & POLICY_INDICATOR_STATE_VISIBLE_ELSEWHERE != 0 {
                context.set_paint(ACTIVE);
                context.fill_rect(&KurboRect::new(
                    cell.x0,
                    cell.y0,
                    (cell.x0 + f64::from(marker)).min(cell.x1),
                    cell.y1,
                ));
            }
            let color = if state & POLICY_INDICATOR_STATE_OCCUPIED != 0 {
                OCCUPIED_TEXT
            } else {
                IDLE_TEXT
            };
            self.draw_text(
                &mut context,
                &mut resources,
                label,
                cell,
                color,
                scale,
                padding,
                false,
            )?;
        }

        if let Some((cell, label, focus_bits)) = &strip.strip.status
            && let Some(cell) = local_rect(*cell, geometry)
        {
            // Reuse the active palette for the existing policy label. A
            // detached square reads as a rendering artifact, not focus state.
            let color = if focus_bits & POLICY_OUTPUT_STATUS_FOCUS_MASK != 0 {
                ACTIVE
            } else {
                OCCUPIED_TEXT
            };
            self.draw_text(
                &mut context,
                &mut resources,
                label,
                cell,
                color,
                scale,
                padding,
                true,
            )?;
        }

        context.flush();
        let mut pixmap = Pixmap::new(width, height);
        context.render(&mut pixmap, &mut resources);
        let mut bytes = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
        for pixel in pixmap.data() {
            bytes.extend_from_slice(&[pixel.b, pixel.g, pixel.r, 0xff]);
        }
        Ok(LiveSharedCpuBufferSource {
            handle,
            size: Size {
                width: i32::from(width),
                height: i32::from(height),
            },
            stride: u32::from(width) * 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: strip.generation.max(1),
            bytes: Arc::new(bytes).into(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        context: &mut RenderContext,
        resources: &mut Resources,
        text: &str,
        cell: KurboRect,
        color: AlphaColor<Srgb>,
        scale: f32,
        padding: f32,
        align_end: bool,
    ) -> Result<(), IndicatorStripRasterError> {
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, text, 1.0, true);
        builder.push_default(FontFamily::named(FONT_FAMILY));
        builder.push_default(StyleProperty::FontSize(LOGICAL_FONT_SIZE * scale));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
            LOGICAL_LINE_HEIGHT * scale,
        )));
        builder.push_default(StyleProperty::Brush(IndicatorBrush(color)));
        let mut layout: Layout<IndicatorBrush> = builder.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        if !layout.width().is_finite() || !layout.height().is_finite() {
            return Err(IndicatorStripRasterError::FontUnavailable);
        }
        let origin_x = if align_end {
            (cell.x1 - f64::from(padding) - f64::from(layout.width())).max(cell.x0)
        } else {
            (cell.x0 + f64::from(padding)).min(cell.x1)
        };
        let origin_y = ((cell.y0 + cell.y1 - f64::from(layout.height())) / 2.0).max(cell.y0);
        context.push_clip_path(&cell.to_path(0.1));
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let mut run_x = glyph_run.offset();
                let run_y = glyph_run.baseline();
                let glyphs = glyph_run.glyphs().map(move |glyph| {
                    let positioned = Glyph {
                        id: glyph.id,
                        x: glyph.x + run_x + origin_x as f32,
                        y: run_y - glyph.y + origin_y as f32,
                    };
                    run_x += glyph.advance;
                    positioned
                });
                let run = glyph_run.run();
                context.set_transform(Affine::IDENTITY);
                context.set_paint(glyph_run.style().brush.0);
                context
                    .glyph_run(resources, run.font())
                    .font_size(run.font_size())
                    .hint(true)
                    .fill_glyphs(glyphs);
            }
        }
        context.pop_clip_path();
        Ok(())
    }
}

fn local_rect(rect: Rect, strip: Rect) -> Option<KurboRect> {
    let left = rect.x.max(strip.x);
    let top = rect.y.max(strip.y);
    let right = rect
        .x
        .saturating_add(rect.width)
        .min(strip.x.saturating_add(strip.width));
    let bottom = rect
        .y
        .saturating_add(rect.height)
        .min(strip.y.saturating_add(strip.height));
    (right > left && bottom > top).then(|| {
        KurboRect::new(
            f64::from(left.saturating_sub(strip.x)),
            f64::from(top.saturating_sub(strip.y)),
            f64::from(right.saturating_sub(strip.x)),
            f64::from(bottom.saturating_sub(strip.y)),
        )
    })
}
