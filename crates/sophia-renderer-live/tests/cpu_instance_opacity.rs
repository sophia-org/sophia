//! t244: a surface instance's opacity on the CPU path is the native layer
//! alpha: the premultiplied source (colour clamped to its alpha, XRGB
//! opaque) is scaled once by the opacity and composed over the frame. At
//! 1000 an unscaled instance is pixel for pixel an ordinary layer.

use sophia_engine::CompositorRgb8;
use sophia_protocol::{Rect, Size};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
    LiveCpuBufferSourceRef, LiveCpuCompositionElementRef, LiveCpuCompositionLayerRef,
    compose_live_cpu_display_list_frame,
};

const FRAME: Size = Size {
    width: 8,
    height: 4,
};

fn background() -> LiveCpuCompositionElementRef<'static> {
    LiveCpuCompositionElementRef::Solid {
        opacity: 255,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        },
        color: CompositorRgb8 {
            red: 200,
            green: 200,
            blue: 200,
        },
    }
}

fn layer(bytes: &[u8], size: Size, format: u32, geometry: Rect) -> LiveCpuCompositionLayerRef<'_> {
    LiveCpuCompositionLayerRef {
        geometry,
        buffer: LiveCpuBufferSourceRef {
            handle: 3,
            size,
            stride: u32::try_from(size.width * 4).unwrap(),
            format,
            generation: 1,
            bytes,
        },
    }
}

fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    let offset = (y * 8 + x) * 4;
    bytes[offset..offset + 4].try_into().unwrap()
}

#[test]
fn full_opacity_unscaled_instances_keep_an_ordinary_layers_pixels() {
    let size = Size {
        width: 4,
        height: 2,
    };
    let bytes = (0..32).map(|value| value as u8 * 7).collect::<Vec<_>>();
    let geometry = Rect {
        x: 2,
        y: 1,
        width: 4,
        height: 2,
    };
    for format in [
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
    ] {
        // Premultiplied content for ARGB: colour never above alpha.
        let bytes = if format == LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888 {
            bytes
                .chunks_exact(4)
                .flat_map(|texel| {
                    [
                        texel[0].min(texel[3]),
                        texel[1].min(texel[3]),
                        texel[2].min(texel[3]),
                        texel[3],
                    ]
                })
                .collect()
        } else {
            bytes.clone()
        };
        let ordinary = compose_live_cpu_display_list_frame(
            FRAME,
            &[
                background(),
                LiveCpuCompositionElementRef::Layer(layer(&bytes, size, format, geometry)),
            ],
            None,
        )
        .unwrap();
        let instance = compose_live_cpu_display_list_frame(
            FRAME,
            &[
                background(),
                LiveCpuCompositionElementRef::ScaledLayer {
                    layer: layer(&bytes, size, format, geometry),
                    clip: geometry,
                    opacity_millis: 1_000,
                },
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            ordinary.frame.bytes, instance.frame.bytes,
            "format {format}"
        );
    }
}

#[test]
fn partial_opacity_over_a_translucent_source_is_applied_once() {
    // One premultiplied texel, half transparent, drawn at half opacity.
    let texel = [40_u8, 80, 120, 128];
    let bytes = texel.repeat(4);
    let size = Size {
        width: 2,
        height: 2,
    };
    let report = compose_live_cpu_display_list_frame(
        FRAME,
        &[
            background(),
            LiveCpuCompositionElementRef::ScaledLayer {
                layer: layer(
                    &bytes,
                    size,
                    LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
                    Rect {
                        x: 0,
                        y: 0,
                        width: 4,
                        height: 4,
                    },
                ),
                clip: Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                opacity_millis: 500,
            },
        ],
        None,
    )
    .unwrap();
    // Native: rgb * o and a * o, then over the frame.
    let scaled = texel.map(|channel| ((u32::from(channel) * 500 + 500) / 1_000) as u8);
    let over = |channel: u8| {
        (u16::from(channel) + (200 * u16::from(255 - scaled[3]) + 127) / 255).min(255) as u8
    };
    let expected = [over(scaled[0]), over(scaled[1]), over(scaled[2]), 255];
    assert_eq!(pixel(&report.frame.bytes, 1, 1), expected);
    // Applying the opacity twice would have been a different pixel.
    let twice = scaled.map(|channel| ((u32::from(channel) * 500 + 500) / 1_000) as u8);
    let twice = (u16::from(twice[0]) + (200 * u16::from(255 - twice[3]) + 127) / 255) as u8;
    assert_ne!(expected[0], twice);
    // Outside the clip the background is untouched.
    assert_eq!(pixel(&report.frame.bytes, 6, 1), [200, 200, 200, 255]);
}

#[test]
fn an_instance_scales_its_whole_source_into_its_destination_and_clips() {
    // A 2x1 source, left black, right white, doubled into 4x2 and clipped
    // to its right half.
    let bytes = [0, 0, 0, 0, 255, 255, 255, 0];
    let report = compose_live_cpu_display_list_frame(
        FRAME,
        &[
            background(),
            LiveCpuCompositionElementRef::ScaledLayer {
                layer: layer(
                    &bytes,
                    Size {
                        width: 2,
                        height: 1,
                    },
                    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    Rect {
                        x: 1,
                        y: 1,
                        width: 4,
                        height: 2,
                    },
                ),
                clip: Rect {
                    x: 3,
                    y: 1,
                    width: 2,
                    height: 2,
                },
                opacity_millis: 1_000,
            },
        ],
        None,
    )
    .unwrap();
    assert_eq!(
        pixel(&report.frame.bytes, 1, 1),
        [200, 200, 200, 255],
        "clipped away"
    );
    // XRGB: the padding byte is the source's, as an ordinary layer copies it.
    assert_eq!(pixel(&report.frame.bytes, 3, 1)[..3], [255, 255, 255]);
    assert_eq!(pixel(&report.frame.bytes, 4, 2)[..3], [255, 255, 255]);
    assert_eq!(
        pixel(&report.frame.bytes, 5, 1),
        [200, 200, 200, 255],
        "outside the destination"
    );
}
