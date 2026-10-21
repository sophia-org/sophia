use super::*;
use parley::{FontContext, FontFamily, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, Rect, Shape};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

/// One fixed bundled font, no client font/image ingestion or arbitrary paths.
/// Keep this on the bounded worker, never on the authority owner.
pub struct Rasterizer {
    fonts: FontContext,
    layouts: LayoutContext<()>,
}
impl Default for Rasterizer {
    fn default() -> Self {
        let mut fonts = FontContext::new();
        fonts.collection.register_fonts(
            include_bytes!("../../../assets/fonts/JetBrainsMonoNL-Regular.ttf")
                .to_vec()
                .into(),
            None,
        );
        Self {
            fonts,
            layouts: LayoutContext::new(),
        }
    }
}
impl Rasterizer {
    pub fn render(
        &mut self,
        batch: &Batch,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<u8>, Error> {
        batch.validate()?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut context = RenderContext::new(batch.width, batch.height);
        let mut resources = Resources::new();
        context.push_clip_path(
            &Rect::new(0., 0., f64::from(batch.width), f64::from(batch.height)).to_path(0.1),
        );
        for cmd in &batch.commands {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            match cmd {
                Command::Rect {
                    x,
                    y,
                    width,
                    height,
                    rgb,
                } => {
                    context.set_paint(AlphaColor::<Srgb>::from_rgb8(rgb[0], rgb[1], rgb[2]));
                    context.fill_path(
                        &Rect::new(
                            f64::from(*x),
                            f64::from(*y),
                            f64::from(*x) + f64::from(*width),
                            f64::from(*y) + f64::from(*height),
                        )
                        .to_path(0.1),
                    );
                }
                Command::Text {
                    x,
                    y,
                    size,
                    rgb,
                    text,
                } => {
                    let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1., true);
                    builder.push_default(FontFamily::named("JetBrains Mono NL"));
                    builder.push_default(StyleProperty::FontSize(f32::from(*size)));
                    let mut layout = builder.build(text);
                    layout.break_all_lines(None);
                    for line in layout.lines() {
                        for item in line.items() {
                            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                                continue;
                            };
                            let mut run_x = glyph_run.offset() + f32::from(*x);
                            let baseline = glyph_run.baseline() + f32::from(*y);
                            let glyphs = glyph_run.glyphs().map(move |g| {
                                let glyph = Glyph {
                                    id: g.id,
                                    x: g.x + run_x,
                                    y: baseline - g.y,
                                };
                                run_x += g.advance;
                                glyph
                            });
                            let run = glyph_run.run();
                            context.set_transform(Affine::IDENTITY);
                            context
                                .set_paint(AlphaColor::<Srgb>::from_rgb8(rgb[0], rgb[1], rgb[2]));
                            context
                                .glyph_run(&mut resources, run.font())
                                .font_size(run.font_size())
                                .hint(true)
                                .fill_glyphs(glyphs);
                        }
                    }
                }
            }
        }
        context.pop_clip_path();
        context.flush();
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut pixmap = Pixmap::new(batch.width, batch.height);
        context.render(&mut pixmap, &mut resources);
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut bytes = Vec::with_capacity(batch.pixel_bytes());
        for p in pixmap.data() {
            bytes.extend_from_slice(&[p.b, p.g, p.r, p.a]);
        }
        Ok(bytes)
    }
}
