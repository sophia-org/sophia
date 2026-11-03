#![cfg(all(feature = "gbm-platform", target_os = "linux"))]
use sophia_renderer_native_egl::*;
use std::{
    fs::{File, OpenOptions},
    num::NonZeroU64,
};

fn context() -> NativeGbmRenderedScanoutContext<File> {
    let path = std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("explicit render-node grant");
    NativeGbmRenderedScanoutContext::from_backend_device_result(
        OpenOptions::new().read(true).write(true).open(path),
    )
    .context
    .expect("render context")
}
fn layer(pixels: &[u8], width: u32, height: u32, version: u64) -> NativeCompositionLayer<'_> {
    NativeCompositionLayer::Cpu(NativeCpuCompositionLayer {
        pixel_version: NonZeroU64::new(version),
        width,
        height,
        stride: width * 4,
        format: u32::from_le_bytes(*b"XR24"),
        pixels,
        target: NativeCompositionRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
        clip: None,
        alpha: 1.0,
        sampling: NativeCompositionSampling::ExactNearest,
    })
}
fn render(
    renderer: &mut NativeGbmRenderedScanoutContext<File>,
    layers: &[NativeCompositionLayer<'_>],
) -> Vec<u8> {
    render_size(renderer, layers, 2)
}
fn render_size(
    renderer: &mut NativeGbmRenderedScanoutContext<File>,
    layers: &[NativeCompositionLayer<'_>],
    extent: u32,
) -> Vec<u8> {
    let (report, snapshot) = renderer
        .export_composed_with_snapshot(
            NativeCompositionFrame {
                width: extent,
                height: extent,
                layers,
                trace: None,
                repaint: None,
            },
            NativeCompositionOutputRequest::default(),
        )
        .expect("composition");
    drop(report);
    snapshot.rgba().to_vec()
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
fn byte_budget_falls_back_and_keeps_the_cached_working_set() {
    let mut renderer = context();
    let a = [10, 0, 0, 255].repeat(4096 * 2048);
    let b = [20, 0, 0, 255].repeat(4096 * 2048);
    let c = [30, 0, 0, 255].repeat(4);
    let layers = [
        layer(&a, 4096, 2048, 1),
        layer(&b, 4096, 2048, 2),
        layer(&c, 2, 2, 3),
    ];
    for uploads in [3, 4] {
        assert_eq!(render(&mut renderer, &layers), [0, 0, 30, 255].repeat(4));
        let stats = renderer.persistent_render_stats().cpu_upload;
        assert_eq!(stats.retained_bytes, 64 * 1024 * 1024);
        assert_eq!(stats.uploads, uploads);
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
fn failed_gl_upload_does_not_leave_a_reusable_revision() {
    let mut renderer = context();
    let good = [10, 0, 0, 255].repeat(4);
    assert_eq!(
        render(&mut renderer, &[layer(&good, 2, 2, 1)]),
        [0, 0, 10, 255].repeat(4)
    );
    // Above the iGPU's maximum texture width, but only 4 MiB of host storage.
    let excessive = vec![255; 4 * 1024 * 1024];
    let bad = [layer(&excessive, 1024 * 1024, 1, 2)];
    let result = renderer.export_composed_with_snapshot(
        NativeCompositionFrame {
            width: 2,
            height: 2,
            layers: &bad,
            trace: None,
            repaint: None,
        },
        NativeCompositionOutputRequest::default(),
    );
    assert!(result.is_err(), "oversized texture must be refused");
    let before = renderer.persistent_render_stats().cpu_upload.uploads;
    assert_eq!(
        render(&mut renderer, &[layer(&good, 2, 2, 1)]),
        [0, 0, 10, 255].repeat(4)
    );
    assert!(renderer.persistent_render_stats().cpu_upload.uploads > before);
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
fn static_overlay_stays_cached_above_a_changing_layer() {
    let mut renderer = context();
    let fixed = [50, 0, 0, 255].repeat(4);
    for blue in 1..=8u8 {
        let changing = [blue, 0, 0, 255].repeat(4);
        let mut overlay = layer(&fixed, 2, 2, 1);
        let NativeCompositionLayer::Cpu(ref mut cpu) = overlay else {
            unreachable!()
        };
        cpu.clip = Some(NativeCompositionRect {
            x: 1,
            y: 0,
            width: 1,
            height: 2,
        });
        let layers = [layer(&changing, 2, 2, u64::from(blue) + 1), overlay];
        assert_eq!(
            render(&mut renderer, &layers),
            [0, 0, blue, 255, 0, 0, 50, 255].repeat(2)
        );
        let stats = renderer.persistent_render_stats().cpu_upload;
        assert_eq!(stats.uploads, u64::from(blue) + 1);
        assert_eq!(stats.allocations, 2);
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
fn retained_scene_changes_match_fresh_uploads_across_resize_and_reappearance() {
    let mut cached = context();
    let mut fresh = context();
    let red = [0, 0, 255, 255].repeat(4);
    let green = [0, 128, 0, 128].repeat(4);
    let blue = [255, 0, 0, 255].repeat(2);
    let mut final_image = Vec::new();
    for step in 0..8 {
        let extent = if step >= 5 { 4 } else { 2 };
        let mut backdrop = layer(&red, 2, 2, 1);
        let NativeCompositionLayer::Cpu(ref mut cpu) = backdrop else {
            unreachable!()
        };
        cpu.target.width = extent as i32;
        cpu.target.height = extent as i32;
        let mut overlay = if step >= 6 {
            layer(&blue, 1, 2, 3)
        } else {
            layer(&green, 2, 2, 2)
        };
        let NativeCompositionLayer::Cpu(ref mut cpu) = overlay else {
            unreachable!()
        };
        cpu.format = u32::from_le_bytes(*b"AR24");
        cpu.target.x = if step == 1 { 1 } else { 0 };
        if step == 2 {
            cpu.alpha = 0.5;
            cpu.clip = Some(NativeCompositionRect {
                x: 0,
                y: 0,
                width: 1,
                height: 2,
            });
        }
        let mut layers = vec![backdrop, overlay];
        if step == 3 || step == 4 {
            layers.pop();
        }
        let actual = render_size(&mut cached, &layers, extent);
        for layer in &mut layers {
            let NativeCompositionLayer::Cpu(cpu) = layer else {
                unreachable!()
            };
            cpu.pixel_version = None;
        }
        let expected = render_size(&mut fresh, &layers, extent);
        assert_eq!(actual, expected, "scene step {step}");
        if step == 0 {
            assert_eq!(actual, [127, 128, 0, 255].repeat(4));
        }
        if step == 3 || step == 4 {
            assert_eq!(actual, [255, 0, 0, 255].repeat(4));
        }
        final_image = actual;
    }
    assert!(cached.persistent_render_stats().cpu_upload.hits > 0);
    drop(cached);
    drop(fresh);
    assert_eq!(&final_image[..4], &[0, 0, 255, 255]);
}
