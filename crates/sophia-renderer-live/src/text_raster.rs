use std::collections::VecDeque;
use std::sync::Arc;

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use sophia_engine::HeadCompositorText;
use sophia_protocol::Size;
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Rect as KurboRect, Shape};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

use crate::{LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LiveSharedCpuBufferSource};

pub const COMPOSITOR_TEXT_FONT_RELEASE: &str = "JetBrains Mono 2.304";
pub const COMPOSITOR_TEXT_FONT_SHA256: &str =
    "fb3b2575d7b0657359707993288f12a7360344d39387bb26050e276d61f6bd2a";
// A complete 256-row reference can contain 513 text nodes. Keep an entire
// admitted page resident across repaints while retaining the byte ceiling.
pub const COMPOSITOR_TEXT_CACHE_MAX_ENTRIES: usize = 1024;
pub const COMPOSITOR_TEXT_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMonoNL-Regular.ttf");
const FONT_FAMILY: &str = "JetBrains Mono NL";

#[derive(Clone, Copy, Debug, PartialEq)]
struct TextBrush(AlphaColor<Srgb>);

impl Default for TextBrush {
    fn default() -> Self {
        Self(AlphaColor::from_rgb8(0xee, 0xee, 0xee))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompositorTextCacheKey(HeadCompositorText);

#[derive(Clone, Debug)]
struct CompositorTextCacheEntry {
    key: CompositorTextCacheKey,
    buffer: LiveSharedCpuBufferSource,
    bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompositorTextCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositorTextRasterError {
    InvalidGeometry,
    InvalidText,
    OutputTooLarge,
    FontUnavailable,
    HandleExhausted,
}

impl core::fmt::Display for CompositorTextRasterError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompositorTextRasterError {}

/// Renderer-private cache for bounded sanitized compositor text.
///
/// The cache key contains every pixel-affecting Engine field. Returned buffers
/// retain their allocation through `Arc`, so eviction cannot invalidate an
/// in-flight frame.
pub struct CompositorTextRasterCache {
    font_context: FontContext,
    layout_context: LayoutContext<TextBrush>,
    entries: VecDeque<CompositorTextCacheEntry>,
    bytes: usize,
    next_handle: u64,
    hits: usize,
    misses: usize,
    evictions: usize,
}

impl Default for CompositorTextRasterCache {
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
            next_handle: 0x8100_0000_0000_0000,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }
}

impl CompositorTextRasterCache {
    /// Use the same bundled font and line metrics as compositor rasterization.
    pub fn measure(&mut self, text: &str, size: u16) -> (i32, i32) {
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, text, 1.0, true);
        builder.push_default(FontFamily::named(FONT_FAMILY));
        builder.push_default(StyleProperty::FontSize(f32::from(size)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
            f32::from(size) * 1.2,
        )));
        let mut layout: Layout<TextBrush> = builder.build(text);
        layout.break_all_lines(None);
        // Fit the rounded advance and retain one edge pixel for raster
        // coverage beyond the fractional line box.
        (
            layout.width().round() as i32,
            layout.height().ceil() as i32 + 1,
        )
    }

    pub fn raster_for(
        &mut self,
        text: &HeadCompositorText,
    ) -> Result<LiveSharedCpuBufferSource, CompositorTextRasterError> {
        validate_text(text)?;
        let key = CompositorTextCacheKey(text.clone());
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
            .ok_or(CompositorTextRasterError::HandleExhausted)?;
        let buffer = self.rasterize(text, handle)?;
        let bytes = buffer.bytes.len();
        if bytes > COMPOSITOR_TEXT_CACHE_MAX_BYTES {
            return Err(CompositorTextRasterError::OutputTooLarge);
        }
        while self.entries.len() >= COMPOSITOR_TEXT_CACHE_MAX_ENTRIES
            || self.bytes.saturating_add(bytes) > COMPOSITOR_TEXT_CACHE_MAX_BYTES
        {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
            self.evictions = self.evictions.saturating_add(1);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.push_back(CompositorTextCacheEntry {
            key,
            buffer: buffer.clone(),
            bytes,
        });
        Ok(buffer)
    }

    pub fn stats(&self) -> CompositorTextCacheStats {
        CompositorTextCacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            entries: self.entries.len(),
            bytes: self.bytes,
        }
    }

    fn rasterize(
        &mut self,
        text: &HeadCompositorText,
        handle: u64,
    ) -> Result<LiveSharedCpuBufferSource, CompositorTextRasterError> {
        let width = u16::try_from(text.geometry.width)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(CompositorTextRasterError::InvalidGeometry)?;
        let height = u16::try_from(text.geometry.height)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(CompositorTextRasterError::InvalidGeometry)?;
        let font_size = text.font_size_millis as f32 / 1_000.0;
        let color = AlphaColor::from_rgb8(text.color.red, text.color.green, text.color.blue);
        let mut builder =
            self.layout_context
                .ranged_builder(&mut self.font_context, &text.text, 1.0, true);
        builder.push_default(FontFamily::named(FONT_FAMILY));
        builder.push_default(StyleProperty::FontSize(font_size));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
            font_size * 1.2,
        )));
        builder.push_default(StyleProperty::Brush(TextBrush(color)));
        let mut layout: Layout<TextBrush> = builder.build(&text.text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        if !layout.width().is_finite() || !layout.height().is_finite() {
            return Err(CompositorTextRasterError::FontUnavailable);
        }

        let mut context = RenderContext::new(width, height);
        let mut resources = Resources::new();
        let clip = KurboRect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let origin_y = ((f32::from(height) - layout.height()) / 2.0).max(0.0);
        context.push_clip_path(&clip.to_path(0.1));
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
                        x: glyph.x + run_x,
                        y: run_y - glyph.y + origin_y,
                    };
                    run_x += glyph.advance;
                    positioned
                });
                let run = glyph_run.run();
                context.set_transform(Affine::IDENTITY);
                context.set_paint(glyph_run.style().brush.0);
                context
                    .glyph_run(&mut resources, run.font())
                    .font_size(run.font_size())
                    .hint(true)
                    .fill_glyphs(glyphs);
            }
        }
        context.pop_clip_path();
        context.flush();
        let mut pixmap = Pixmap::new(width, height);
        context.render(&mut pixmap, &mut resources);
        let mut bytes = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
        for pixel in pixmap.data() {
            bytes.extend_from_slice(&[pixel.b, pixel.g, pixel.r, pixel.a]);
        }
        Ok(LiveSharedCpuBufferSource {
            handle,
            size: Size {
                width: i32::from(width),
                height: i32::from(height),
            },
            stride: u32::from(width) * 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
            generation: text.generation.max(1),
            bytes: Arc::new(bytes).into(),
        })
    }
}

fn validate_text(text: &HeadCompositorText) -> Result<(), CompositorTextRasterError> {
    if text.generation == 0
        || text.geometry.is_empty()
        || text.text.is_empty()
        || text.text.len() > sophia_protocol::MAX_CHROME_LABEL_LEN
        || text.text.chars().any(char::is_control)
        || text.font_size_millis == 0
    {
        return Err(CompositorTextRasterError::InvalidText);
    }
    Ok(())
}
