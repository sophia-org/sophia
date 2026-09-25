use sophia_protocol::{Rect, Size};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSourceRef, LiveCpuCompositionElementRef,
    LiveCpuCompositionLayerRef, LiveCpuFrameMetricsMode, compose_live_cpu_display_list_frame,
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage,
};

#[test]
fn preview_clips_without_changing_sampling_origin_and_repaints_retained_pixels() {
    let pixels = [1, 2, 3, 255, 4, 5, 6, 255];
    let layer = LiveCpuCompositionLayerRef {
        geometry: Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        },
        buffer: LiveCpuBufferSourceRef {
            handle: 1,
            generation: 1,
            size: Size {
                width: 2,
                height: 1,
            },
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            bytes: &pixels,
        },
    };
    let clip = Rect {
        x: 1,
        y: 0,
        width: 2,
        height: 1,
    };
    let elements = [LiveCpuCompositionElementRef::ClippedLayer { layer, clip }];
    let size = Size {
        width: 4,
        height: 2,
    };
    let full = compose_live_cpu_display_list_frame(size, &elements, None).unwrap();
    assert_eq!(&full.frame.bytes[0..4], &[0; 4]);
    assert_eq!(&full.frame.bytes[4..8], &pixels[0..4]);
    assert_eq!(&full.frame.bytes[8..12], &pixels[4..8]);
    assert!(full.frame.bytes[12..].iter().all(|b| *b == 0));
    let expected = full.frame.bytes.clone();
    let partial = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &elements,
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(full.frame.bytes),
        Some(&sophia_protocol::Region {
            rects: vec![Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            }],
        }),
    )
    .unwrap();
    assert_eq!(partial.frame.bytes, expected);
    assert_eq!(pixels, [1, 2, 3, 255, 4, 5, 6, 255]);
}
