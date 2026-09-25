//! The headless reference model of the native composition shaders
//! (composition.frag and sharp_reconstruction.frag), maintained here beside
//! the shader-text contracts in sampling.rs. The CPU instance path in
//! sophia-renderer-live is tested against this same file (t244); it is test
//! code and never production.
#![allow(dead_code)]

#[derive(Clone, Copy)]
pub(crate) enum AlphaMode {
    Opaque,
    Premultiplied,
}

pub(crate) fn finish_sample(mut color: [f32; 4], alpha_mode: AlphaMode, opacity: f32) -> [f32; 4] {
    match alpha_mode {
        AlphaMode::Opaque => color[3] = 1.0,
        AlphaMode::Premultiplied => {
            let alpha = color[3];
            for channel in &mut color[..3] {
                *channel = channel.min(alpha);
            }
        }
    }
    color.map(|channel| channel * opacity)
}

#[derive(Clone, Copy)]
pub(crate) enum Sample {
    Nearest,
    Linear,
    Sharp,
}

pub(crate) fn resample(
    source: &[f32],
    source_size: (usize, usize),
    target_size: (usize, usize),
    sample: Sample,
) -> Vec<f32> {
    let mut target = vec![0.0; target_size.0 * target_size.1];
    for y in 0..target_size.1 {
        for x in 0..target_size.0 {
            let source_x = (x as f32 + 0.5) * source_size.0 as f32 / target_size.0 as f32 - 0.5;
            let source_y = (y as f32 + 0.5) * source_size.1 as f32 / target_size.1 as f32 - 0.5;
            target[y * target_size.0 + x] = match sample {
                Sample::Nearest => texel(source, source_size, source_x.round(), source_y.round()),
                Sample::Linear => bilinear(source, source_size, source_x, source_y),
                Sample::Sharp => catmull_sample(source, source_size, source_x, source_y),
            };
        }
    }
    target
}

pub(crate) fn texel(source: &[f32], size: (usize, usize), x: f32, y: f32) -> f32 {
    let x = (x as isize).clamp(0, size.0 as isize - 1) as usize;
    let y = (y as isize).clamp(0, size.1 as isize - 1) as usize;
    source[y * size.0 + x]
}

pub(crate) fn bilinear(source: &[f32], size: (usize, usize), x: f32, y: f32) -> f32 {
    let left = x.floor();
    let top = y.floor();
    let fraction_x = x - left;
    let fraction_y = y - top;
    let top_value = texel(source, size, left, top) * (1.0 - fraction_x)
        + texel(source, size, left + 1.0, top) * fraction_x;
    let bottom_value = texel(source, size, left, top + 1.0) * (1.0 - fraction_x)
        + texel(source, size, left + 1.0, top + 1.0) * fraction_x;
    top_value * (1.0 - fraction_y) + bottom_value * fraction_y
}

pub(crate) fn catmull_sample(source: &[f32], size: (usize, usize), x: f32, y: f32) -> f32 {
    let origin_x = x.floor();
    let origin_y = y.floor();
    let fraction_x = x - origin_x;
    let fraction_y = y - origin_y;
    let mut sum = 0.0;
    let mut total = 0.0;
    for row in -1..=2 {
        let weight_y = catmull_rom(row as f32 - fraction_y);
        for column in -1..=2 {
            let weight = weight_y * catmull_rom(column as f32 - fraction_x);
            sum += texel(
                source,
                size,
                origin_x + column as f32,
                origin_y + row as f32,
            ) * weight;
            total += weight;
        }
    }
    (sum / total.max(0.0001)).clamp(0.0, 1.0)
}

/// `to_light` from the shader, for an opaque source.
pub(crate) fn to_light(encoded: f32) -> f32 {
    encoded * encoded
}

/// `to_bytes` from the shader, for an opaque source, including its guard.
pub(crate) fn to_bytes(light: f32) -> f32 {
    light.max(0.0).sqrt()
}

/// `to_light` for a premultiplied source: unpremultiply, decode, re-premultiply,
/// which under gamma 2.0 is `v*v/a`.
pub(crate) fn to_light_premultiplied(premultiplied: f32, alpha: f32) -> f32 {
    if alpha <= 0.0 {
        return 0.0;
    }
    premultiplied * premultiplied / alpha
}

/// The inverse: `sqrt(L/a) * a` is `sqrt(L*a)`.
pub(crate) fn to_bytes_premultiplied(light: f32, alpha: f32) -> f32 {
    if alpha <= 0.0 {
        return 0.0;
    }
    (light.max(0.0) * alpha).sqrt()
}

/// The same Catmull-Rom reduction, with the taps decoded before they are
/// weighted and the result re-encoded. The only difference from `Sample::Sharp`.
pub(crate) fn resample_in_light(
    source: &[f32],
    source_size: (usize, usize),
    target_size: (usize, usize),
) -> Vec<f32> {
    let light = source.iter().copied().map(to_light).collect::<Vec<_>>();
    resample(&light, source_size, target_size, Sample::Sharp)
        .into_iter()
        .map(to_bytes)
        .collect()
}

pub(crate) fn catmull_rom(value: f32) -> f32 {
    let x = value.abs();
    if x <= 1.0 {
        ((1.5 * x - 2.5) * x) * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}

pub(crate) fn quantized(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .map(|value| (value * 255.0).round() as u8)
        .collect()
}
