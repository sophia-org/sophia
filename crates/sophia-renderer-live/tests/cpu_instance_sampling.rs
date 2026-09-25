//! t244: the CPU instance path samples as the native renderer does. At any
//! scale other than identity, sharp_reconstruction.frag's Catmull-Rom in
//! linear light, compared pixel for pixel with the native crate's own
//! headless reference model (its tests/support/reference_sampling.rs,
//! included here as test code). At identity, exact texels as native's
//! ExactNearest.

use sophia_protocol::{Rect, Size};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
    LiveCpuBufferSourceRef, LiveCpuCompositionElementRef, LiveCpuCompositionLayerRef,
    compose_live_cpu_display_list_frame,
};

#[path = "../../sophia-renderer-native-egl/tests/support/reference_sampling.rs"]
mod reference_sampling;
use reference_sampling::{
    AlphaMode, Sample, finish_sample, resample, resample_in_light, to_bytes_premultiplied,
    to_light_premultiplied,
};

const FRAME: Size = Size {
    width: 24,
    height: 24,
};

/// A deterministic source, BGRA bytes; `alpha` None for opaque XRGB (the
/// padding byte set to garbage, which must be ignored).
fn source(width: usize, height: usize, alpha: Option<fn(usize, usize) -> u8>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let a = alpha.map_or(255, |alpha| alpha(x, y));
            let colour = |seed: usize| {
                let value = ((x * 67 + y * 131 + seed * 29) % 256) as u8;
                // Premultiplied: never above alpha.
                value.min(a)
            };
            bytes.extend([
                colour(1),
                colour(2),
                colour(3),
                if alpha.is_some() { a } else { 17 },
            ]);
        }
    }
    bytes
}

fn channel(bytes: &[u8], index: usize) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|texel| f32::from(texel[index]) / 255.0)
        .collect()
}

/// The reference model's frame for a source drawn over black at the frame
/// origin, BGR in [0, 1]: opaque channels resampled in light, premultiplied
/// channels unpremultiplied into light and alpha resampled alongside, then
/// the shader's finish (alpha clamp, colour clamp, opacity).
fn reference(
    bytes: &[u8],
    source_size: (usize, usize),
    target_size: (usize, usize),
    opaque: bool,
    opacity: f32,
) -> Vec<[f32; 4]> {
    let identity = source_size == target_size;
    let mut planes = Vec::new();
    if opaque {
        for index in 0..3 {
            planes.push(if identity {
                resample(
                    &channel(bytes, index),
                    source_size,
                    target_size,
                    Sample::Nearest,
                )
            } else {
                resample_in_light(&channel(bytes, index), source_size, target_size)
            });
        }
        planes.push(vec![1.0; target_size.0 * target_size.1]);
    } else {
        let alpha = channel(bytes, 3);
        let sampled_alpha = if identity {
            alpha.clone()
        } else {
            resample(&alpha, source_size, target_size, Sample::Sharp)
        };
        for index in 0..3 {
            let values = channel(bytes, index);
            planes.push(if identity {
                values
            } else {
                let light = values
                    .iter()
                    .zip(&alpha)
                    .map(|(value, alpha)| to_light_premultiplied(*value, *alpha))
                    .collect::<Vec<_>>();
                resample(&light, source_size, target_size, Sample::Sharp)
                    .into_iter()
                    .zip(&sampled_alpha)
                    .map(|(light, alpha)| to_bytes_premultiplied(light, *alpha).clamp(0.0, 1.0))
                    .collect()
            });
        }
        planes.push(sampled_alpha);
    }
    (0..target_size.0 * target_size.1)
        .map(|index| {
            finish_sample(
                [
                    planes[0][index],
                    planes[1][index],
                    planes[2][index],
                    planes[3][index],
                ],
                if opaque {
                    AlphaMode::Opaque
                } else {
                    AlphaMode::Premultiplied
                },
                opacity,
            )
        })
        .collect()
}

fn compose(
    bytes: &[u8],
    source_size: (usize, usize),
    destination: Rect,
    clip: Rect,
    format: u32,
    opacity_millis: u16,
) -> Vec<u8> {
    compose_over(
        bytes,
        source_size,
        destination,
        clip,
        format,
        opacity_millis,
        0,
    )
}

fn compose_over(
    bytes: &[u8],
    source_size: (usize, usize),
    destination: Rect,
    clip: Rect,
    format: u32,
    opacity_millis: u16,
    background: u8,
) -> Vec<u8> {
    let backdrop = LiveCpuCompositionElementRef::Solid {
        opacity: 255,
        geometry: rect(0, 0, FRAME.width, FRAME.height),
        color: sophia_engine::CompositorRgb8 {
            red: background,
            green: background,
            blue: background,
        },
    };
    let report = compose_live_cpu_display_list_frame(
        FRAME,
        &[
            backdrop,
            LiveCpuCompositionElementRef::ScaledLayer {
                layer: LiveCpuCompositionLayerRef {
                    geometry: destination,
                    buffer: LiveCpuBufferSourceRef {
                        handle: 1,
                        size: Size {
                            width: source_size.0 as i32,
                            height: source_size.1 as i32,
                        },
                        stride: (source_size.0 * 4) as u32,
                        format,
                        generation: 1,
                        bytes,
                    },
                },
                clip,
                opacity_millis,
            },
        ],
        None,
    )
    .unwrap();
    report.frame.bytes.to_vec()
}

/// Every pixel of the frame: inside `destination ∩ clip` the reference to
/// within one byte step, elsewhere untouched black.
fn assert_matches_reference(
    bytes: &[u8],
    source_size: (usize, usize),
    destination: Rect,
    clip: Rect,
    opaque: bool,
    opacity_millis: u16,
) {
    let frame = compose(
        bytes,
        source_size,
        destination,
        clip,
        if opaque {
            LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
        } else {
            LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
        },
        opacity_millis,
    );
    let target_size = (destination.width as usize, destination.height as usize);
    let expected = reference(
        bytes,
        source_size,
        target_size,
        opaque,
        f32::from(opacity_millis) / 1_000.0,
    );
    let inside = |x: i32, y: i32, rect: Rect| {
        x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
    };
    let mut worst = 0.0_f32;
    for y in 0..FRAME.height {
        for x in 0..FRAME.width {
            let offset = ((y * FRAME.width + x) * 4) as usize;
            let pixel = &frame[offset..offset + 3];
            if inside(x, y, destination) && inside(x, y, clip) {
                let local =
                    ((y - destination.y) * destination.width + (x - destination.x)) as usize;
                for (byte, value) in pixel.iter().zip(&expected[local][..3]) {
                    worst = worst.max((f32::from(*byte) - value * 255.0).abs());
                }
            } else {
                assert_eq!(pixel, &[0, 0, 0], "({x}, {y}) is outside the instance");
            }
        }
    }
    assert!(
        worst <= 1.0,
        "worst difference {worst} byte steps from the reference"
    );
}

fn whole(destination: Rect) -> Rect {
    destination
}

fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn an_upscaled_instance_matches_the_native_reconstruction() {
    let bytes = source(4, 3, None);
    assert_matches_reference(
        &bytes,
        (4, 3),
        rect(0, 0, 8, 6),
        whole(rect(0, 0, 8, 6)),
        true,
        1_000,
    );
}

#[test]
fn a_downscaled_instance_matches_the_native_reconstruction() {
    let bytes = source(12, 9, None);
    assert_matches_reference(
        &bytes,
        (12, 9),
        rect(0, 0, 5, 4),
        whole(rect(0, 0, 5, 4)),
        true,
        1_000,
    );
}

#[test]
fn a_fractional_instance_matches_the_native_reconstruction() {
    let bytes = source(4, 4, None);
    assert_matches_reference(
        &bytes,
        (4, 4),
        rect(0, 0, 6, 6),
        whole(rect(0, 0, 6, 6)),
        true,
        1_000,
    );
}

#[test]
fn a_mixed_scale_instance_matches_the_native_reconstruction() {
    let bytes = source(6, 2, None);
    assert_matches_reference(
        &bytes,
        (6, 2),
        rect(0, 0, 4, 5),
        whole(rect(0, 0, 4, 5)),
        true,
        1_000,
    );
}

#[test]
fn a_hard_edge_rings_to_black_and_white_not_to_holes() {
    // Left half black, right half white: Catmull-Rom overshoots on both
    // sides, which the shader clamps before and after the encode.
    let mut bytes = Vec::new();
    for _ in 0..4 {
        for x in 0..6 {
            let value = if x < 3 { 0 } else { 255 };
            bytes.extend([value, value, value, 0]);
        }
    }
    assert_matches_reference(
        &bytes,
        (6, 4),
        rect(0, 0, 15, 10),
        whole(rect(0, 0, 15, 10)),
        true,
        1_000,
    );
}

#[test]
fn a_clipped_offset_instance_draws_only_its_clipped_allocation() {
    let bytes = source(4, 3, None);
    assert_matches_reference(
        &bytes,
        (4, 3),
        rect(3, 5, 10, 7),
        rect(6, 7, 5, 3),
        true,
        1_000,
    );
}

#[test]
fn a_translucent_premultiplied_instance_at_partial_opacity_matches() {
    let bytes = source(5, 4, Some(|x, y| ((x * 50 + y * 40) % 256) as u8));
    assert_matches_reference(
        &bytes,
        (5, 4),
        rect(1, 2, 9, 7),
        whole(rect(1, 2, 9, 7)),
        false,
        600,
    );
}

#[test]
fn an_opaque_instance_at_partial_opacity_matches() {
    let bytes = source(3, 3, None);
    assert_matches_reference(
        &bytes,
        (3, 3),
        rect(0, 0, 7, 7),
        whole(rect(0, 0, 7, 7)),
        true,
        350,
    );
}

#[test]
fn identity_scale_keeps_exact_texels_as_native_exact_nearest() {
    // A one-texel checkerboard would be softened by any reconstruction.
    let mut bytes = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            let value = if (x + y) % 2 == 0 { 0 } else { 255 };
            bytes.extend([value, value, value, 255]);
        }
    }
    assert_matches_reference(
        &bytes,
        (4, 4),
        rect(2, 2, 4, 4),
        whole(rect(2, 2, 4, 4)),
        true,
        1_000,
    );
    let frame = compose(
        &bytes,
        (4, 4),
        rect(2, 2, 4, 4),
        rect(2, 2, 4, 4),
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        1_000,
    );
    let offset = ((2 * FRAME.width + 3) * 4) as usize;
    assert_eq!(
        &frame[offset..offset + 3],
        &[255, 255, 255],
        "crisp at identity"
    );
}

#[test]
fn a_hard_alpha_edge_rings_within_the_clamped_alpha_over_a_backdrop() {
    // Premultiplied: left half transparent, right half opaque white. The
    // reconstruction overshoots alpha on both sides of the edge; the shader
    // clamps it to [0, 1] before the encode, and over a grey backdrop an
    // unclamped alpha shows as a lighter or darker fringe.
    let mut bytes = Vec::new();
    for _ in 0..4 {
        for x in 0..6 {
            let value = if x < 3 { 0 } else { 255 };
            bytes.extend([value, value, value, value]);
        }
    }
    let destination = rect(0, 0, 15, 10);
    let background = 128_u8;
    let frame = compose_over(
        &bytes,
        (6, 4),
        destination,
        destination,
        LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        1_000,
        background,
    );
    let expected = reference(&bytes, (6, 4), (15, 10), false, 1.0);
    let mut worst = 0.0_f32;
    for y in 0..10 {
        for x in 0..15 {
            let offset = ((y * FRAME.width + x) * 4) as usize;
            let [red, green, blue, alpha] = expected[(y * 15 + x) as usize];
            for (byte, colour) in frame[offset..offset + 3].iter().zip([red, green, blue]) {
                let over = colour * 255.0 + f32::from(background) * (1.0 - alpha);
                worst = worst.max((f32::from(*byte) - over).abs());
            }
        }
    }
    assert!(
        worst <= 1.5,
        "worst difference {worst} byte steps from the reference"
    );
}
