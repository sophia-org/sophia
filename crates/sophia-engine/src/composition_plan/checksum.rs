fn logical_scene_checksum(
    surfaces: &[OutputSceneSurface],
    display_list: &CompositorDisplayList,
    cursor: Option<OutputSceneCursor>,
) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(PRIME);
    };
    for surface in surfaces {
        mix(u64::from(surface.surface.index()));
        mix(u64::from(surface.surface.generation()));
        mix(surface.committed_generation);
        for value in [
            surface.geometry.x,
            surface.geometry.y,
            surface.geometry.width,
            surface.geometry.height,
        ] {
            mix(value as u32 as u64);
        }
        for variant in surface.content.variants() {
            mix(u64::from(variant.variant));
            mix(u64::from(variant.density_millis));
            let (kind, handle) = match variant.source {
                BufferSource::None => (0, 0),
                BufferSource::XPixmap { pixmap } => (1, u64::from(pixmap)),
                BufferSource::DmaBuf { handle } => (2, handle),
                BufferSource::CpuBuffer { handle } => (3, handle),
            };
            mix(kind);
            mix(handle);
        }
    }
    for command in &display_list.commands {
        match command {
            CompositorDisplayCommand::SurfacePreview(preview) => {
                mix(8);
                mix(preview.generation);
                mix(u64::from(preview.surface.index()));
                mix(u64::from(preview.surface.generation()));
                for rect in [preview.geometry, preview.clip] {
                    for value in [rect.x, rect.y, rect.width, rect.height] {
                        mix(value as u32 as u64);
                    }
                }
            }
            CompositorDisplayCommand::Surface { surface } => {
                mix(1);
                mix(u64::from(surface.index()));
                mix(u64::from(surface.generation()));
            }
            CompositorDisplayCommand::Border(border) => {
                mix(2);
                mix(border.generation);
                for value in [
                    border.outer.x,
                    border.outer.y,
                    border.outer.width,
                    border.outer.height,
                    border.inner.x,
                    border.inner.y,
                    border.inner.width,
                    border.inner.height,
                ] {
                    mix(value as u32 as u64);
                }
            }
            CompositorDisplayCommand::Rect(rect) => {
                mix(5);
                mix(rect.generation);
                for value in [
                    rect.geometry.x,
                    rect.geometry.y,
                    rect.geometry.width,
                    rect.geometry.height,
                ] {
                    mix(value as u32 as u64);
                }
                mix(u64::from(rect.color.red));
                mix(u64::from(rect.color.green));
                mix(u64::from(rect.color.blue));
                mix(u64::from(rect.opacity));
            }
            CompositorDisplayCommand::Text(text) => {
                mix(6);
                mix(text.generation);
                mix(u64::from(text.font_size_millis));
                for value in [
                    text.geometry.x,
                    text.geometry.y,
                    text.geometry.width,
                    text.geometry.height,
                ] {
                    mix(value as u32 as u64);
                }
                for byte in text.text.as_bytes() {
                    mix(u64::from(*byte));
                }
                mix(u64::from(text.color.red));
                mix(u64::from(text.color.green));
                mix(u64::from(text.color.blue));
            }
            CompositorDisplayCommand::IndicatorStrip(strip) => {
                mix(4);
                mix(strip.generation);
                mix(strip.strip.output.raw());
                for value in [
                    strip.strip.geometry.x,
                    strip.strip.geometry.y,
                    strip.strip.geometry.width,
                    strip.strip.geometry.height,
                ] {
                    mix(value as u32 as u64);
                }
            }
            CompositorDisplayCommand::ContentImage(image) => {
                mix(7);
                mix(image.generation);
                for value in [
                    image.geometry_px.x,
                    image.geometry_px.y,
                    image.geometry_px.width,
                    image.geometry_px.height,
                ] {
                    mix(value as u32 as u64);
                }
                mix(u64::from(image.stride));
                mix(u64::from(image.format));
            }
        }
    }
    if let Some(cursor) = cursor {
        mix(3);
        mix(cursor.generation);
        for value in [
            cursor.geometry.x,
            cursor.geometry.y,
            cursor.geometry.width,
            cursor.geometry.height,
        ] {
            mix(value as u32 as u64);
        }
    }
    hash
}
