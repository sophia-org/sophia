// A surface instance on the CPU path: the whole source drawn into its
// destination, clipped, at the instance's opacity (t244). The sampling is
// the native renderer's: the same classification of source against target
// size (`head_sampling_class`, the Engine helper the head plan uses), exact
// texels at identity, and at any other scale a CPU port of
// sophia-renderer-native-egl's sharp_reconstruction.frag: a 4x4 Catmull-Rom
// in linear light (gamma 2), clamp-to-edge taps, alpha clamped before the
// encode, premultiplied colour clamped to alpha, then the layer opacity.
// The shader is the contract; its headless reference model in that crate's
// tests/support/reference_sampling.rs is what the CPU tests compare to.

use super::*;
use sophia_engine::{HeadSamplingClass, head_sampling_class};

/// Draws the whole source into `layer.geometry`, clipped to `clip` and the
/// frame, blended at `opacity_millis`: the premultiplied colour and its
/// alpha (opaque for XRGB) are both scaled by the opacity, then composed
/// over the frame, as a native layer alpha is.
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
    let exact = head_sampling_class(
        buffer.size,
        Size {
            width: layer.geometry.width,
            height: layer.geometry.height,
        },
    ) == HeadSamplingClass::Exact;
    let opaque = buffer.format == LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888;
    let opacity = u32::from(opacity_millis);
    for y in target.y..target.y + target.height {
        for x in target.x..target.x + target.width {
            let pixel = if exact {
                let source_x = usize::try_from(x - layer.geometry.x).unwrap_or(0);
                let source_y = usize::try_from(y - layer.geometry.y).unwrap_or(0);
                let mut pixel = texel_bytes(buffer, source_x, source_y);
                if opaque {
                    if opacity == 1_000 {
                        // Opaque and unattenuated: the texel as an ordinary
                        // layer copies it, padding byte included.
                        put_pixel(frame, x, y, pixel);
                        continue;
                    }
                    pixel[3] = u8::MAX;
                } else {
                    for channel in 0..3 {
                        pixel[channel] = pixel[channel].min(pixel[3]);
                    }
                }
                if opacity < 1_000 {
                    for channel in &mut pixel {
                        *channel = ((u32::from(*channel) * opacity + 500) / 1_000) as u8;
                    }
                }
                pixel
            } else {
                // The fragment centre, in source texel space, as the shader's
                // `texture_position * source_size - 0.5` over the whole
                // destination quad.
                let source_x = ((x - layer.geometry.x) as f32 + 0.5) * buffer.size.width as f32
                    / layer.geometry.width as f32
                    - 0.5;
                let source_y = ((y - layer.geometry.y) as f32 + 0.5) * buffer.size.height as f32
                    / layer.geometry.height as f32
                    - 0.5;
                let color = sharp_reconstruction(buffer, opaque, source_x, source_y);
                let scale = opacity as f32 / 1_000.0;
                color.map(|channel| (channel * scale * 255.0).round().clamp(0.0, 255.0) as u8)
            };
            blend_premultiplied_pixel(frame, x, y, pixel);
        }
    }
    true
}

fn texel_bytes(buffer: LiveCpuBufferSourceRef<'_>, x: usize, y: usize) -> [u8; 4] {
    let offset = y * buffer.stride as usize + x * 4;
    buffer.bytes[offset..offset + 4]
        .try_into()
        .expect("four bytes of a checked source row")
}

/// sharp_reconstruction.frag's `main`, before the opacity: the finished
/// premultiplied colour in [0, 1], in the frame's BGRA channel order.
fn sharp_reconstruction(
    buffer: LiveCpuBufferSourceRef<'_>,
    opaque: bool,
    source_x: f32,
    source_y: f32,
) -> [f32; 4] {
    let origin_x = source_x.floor();
    let origin_y = source_y.floor();
    let fraction_x = source_x - origin_x;
    let fraction_y = source_y - origin_y;
    let last_x = buffer.size.width as isize - 1;
    let last_y = buffer.size.height as isize - 1;
    let mut sum = [0.0_f32; 4];
    let mut total = 0.0_f32;
    for row in -1..=2_i32 {
        let weight_y = catmull_rom(row as f32 - fraction_y);
        // Clamp to edge, as the texture's wrap mode does.
        let tap_y = (origin_y as isize + row as isize).clamp(0, last_y) as usize;
        for column in -1..=2_i32 {
            let weight = weight_y * catmull_rom(column as f32 - fraction_x);
            let tap_x = (origin_x as isize + column as isize).clamp(0, last_x) as usize;
            let light = to_light(texel_bytes(buffer, tap_x, tap_y), opaque);
            for channel in 0..4 {
                sum[channel] += light[channel] * weight;
            }
            total += weight;
        }
    }
    let total = total.max(0.0001);
    let mixed = sum.map(|channel| channel / total);
    // Clamped before the encode: ringing below zero is dark, never a hole.
    let alpha = mixed[3].clamp(0.0, 1.0);
    let mut color = [0.0_f32; 4];
    for channel in 0..3 {
        color[channel] = to_bytes(mixed[channel], alpha, opaque).clamp(0.0, 1.0);
    }
    if opaque {
        color[3] = 1.0;
    } else {
        color[3] = alpha;
        for channel in &mut color[..3] {
            *channel = channel.min(alpha);
        }
    }
    color
}

fn catmull_rom(value: f32) -> f32 {
    let x = value.abs();
    if x <= 1.0 {
        ((1.5 * x - 2.5) * x) * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}

/// The shader's `to_light`: gamma 2 decode; a premultiplied colour is
/// unpremultiplied, decoded and premultiplied again (`v * v / a`).
fn to_light(texel: [u8; 4], opaque: bool) -> [f32; 4] {
    let encoded = texel.map(|channel| f32::from(channel) / 255.0);
    if opaque {
        return [
            encoded[0] * encoded[0],
            encoded[1] * encoded[1],
            encoded[2] * encoded[2],
            1.0,
        ];
    }
    let alpha = encoded[3];
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    [
        encoded[0] * encoded[0] / alpha,
        encoded[1] * encoded[1] / alpha,
        encoded[2] * encoded[2] / alpha,
        alpha,
    ]
}

/// The shader's `to_bytes`: the gamma 2 encode, re-premultiplied as
/// `sqrt(L * a)` for a premultiplied source.
fn to_bytes(light: f32, alpha: f32, opaque: bool) -> f32 {
    let safe = light.max(0.0);
    if opaque {
        safe.sqrt()
    } else if alpha <= 0.0 {
        0.0
    } else {
        (safe * alpha).sqrt()
    }
}
