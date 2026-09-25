// A surface instance on the CPU path: the whole source scaled into its
// destination, clipped, at the instance's opacity (t244).

use super::*;

/// Samples the whole source into `layer.geometry` (nearest texel centre),
/// clips to `clip` and the frame, and blends at `opacity_millis`: the
/// premultiplied colour and its alpha (opaque for XRGB) are both scaled by
/// the opacity, then composed over the frame, as a native layer alpha is.
pub(super) fn compose_scaled_layer(
    frame: &mut LiveCpuComposedFrame,
    layer: &LiveCpuCompositionLayerRef<'_>,
    clip: Rect,
    opacity_millis: u16,
) -> bool {
    let buffer = layer.buffer;
    if !matches!(
        buffer.format,
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 | LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
    ) || layer.geometry.width <= 0
        || layer.geometry.height <= 0
        || buffer.size.width <= 0
        || buffer.size.height <= 0
        || opacity_millis == 0
        || opacity_millis > 1_000
        || u64::from(buffer.stride) < buffer.size.width as u64 * 4
        || (buffer.bytes.len() as u64) < u64::from(buffer.stride) * buffer.size.height as u64
    {
        return false;
    }
    let Some(target) =
        clip_rect(layer.geometry, clip).and_then(|rect| clip_rect(rect, output_rect(frame.size)))
    else {
        return false;
    };
    let opacity = u32::from(opacity_millis);
    for y in target.y..target.y + target.height {
        let source_y = ((i64::from(y) - i64::from(layer.geometry.y)) * 2 + 1)
            * i64::from(buffer.size.height)
            / (2 * i64::from(layer.geometry.height));
        for x in target.x..target.x + target.width {
            let source_x = ((i64::from(x) - i64::from(layer.geometry.x)) * 2 + 1)
                * i64::from(buffer.size.width)
                / (2 * i64::from(layer.geometry.width));
            let offset = source_y as usize * buffer.stride as usize + source_x as usize * 4;
            let mut pixel: [u8; 4] = buffer.bytes[offset..offset + 4]
                .try_into()
                .expect("four bytes of a checked source row");
            if buffer.format == LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888 {
                if opacity == 1_000 {
                    // Opaque and unattenuated: the texel as an ordinary
                    // layer copies it, padding byte included.
                    put_pixel(frame, x, y, pixel);
                    continue;
                }
                pixel[3] = u8::MAX;
            } else {
                // As the native shader does: premultiplied colour never
                // exceeds its alpha.
                for channel in 0..3 {
                    pixel[channel] = pixel[channel].min(pixel[3]);
                }
            }
            if opacity < 1_000 {
                for channel in &mut pixel {
                    *channel = ((u32::from(*channel) * opacity + 500) / 1_000) as u8;
                }
            }
            blend_premultiplied_pixel(frame, x, y, pixel);
        }
    }
    true
}
