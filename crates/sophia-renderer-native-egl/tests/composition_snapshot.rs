use sophia_renderer_native_egl::NativeCompositionSnapshot;

#[test]
fn snapshot_rejects_invalid_extents_and_nonexact_storage() {
    for (width, height) in [(0, 1), (1, 0), (u32::MAX, 2), (8192, 8192)] {
        assert!(NativeCompositionSnapshot::byte_len(width, height).is_none());
        assert!(NativeCompositionSnapshot::from_rgba(width, height, vec![]).is_none());
    }
    assert!(NativeCompositionSnapshot::from_rgba(1, 1, vec![0; 3]).is_none());
    assert!(NativeCompositionSnapshot::from_rgba(1, 1, vec![0; 5]).is_none());
    assert_eq!(
        NativeCompositionSnapshot::byte_len(3840, 2160),
        Some(33_177_600)
    );
}

#[test]
fn snapshot_clones_retain_private_immutable_storage() {
    let image = NativeCompositionSnapshot::from_rgba(1, 1, vec![1, 2, 3, 255]).unwrap();
    let retained = image.clone();
    drop(image);
    assert_eq!((retained.width(), retained.height()), (1, 1));
    assert_eq!(retained.rgba(), [1, 2, 3, 255]);
}

#[cfg(all(feature = "gbm-platform", target_os = "linux"))]
mod native {
    use sophia_renderer_native_egl::*;
    use std::{
        fs::{File, OpenOptions},
        os::fd::AsFd,
    };

    fn context() -> NativeGbmRenderedScanoutContext<File> {
        let path = std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("explicit render-node grant");
        NativeGbmRenderedScanoutContext::from_backend_device_result(
            OpenOptions::new().read(true).write(true).open(path),
        )
        .context
        .expect("render context")
    }
    fn rect() -> NativeCompositionRect {
        NativeCompositionRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }
    }
    fn frame<'a>(layers: &'a [NativeCompositionLayer<'a>]) -> NativeCompositionFrame<'a> {
        NativeCompositionFrame {
            width: 2,
            height: 2,
            layers,
            trace: None,
            repaint: None,
        }
    }

    #[test]
    #[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only"]
    fn cpu_and_gpu_compositions_return_exact_top_down_pixels_without_storage_aliases() {
        let mut renderer = context();
        let pixels = [0, 0, 255, 0, 0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0];
        let cpu = [NativeCompositionLayer::Cpu(NativeCpuCompositionLayer {
            width: 2,
            height: 2,
            stride: 8,
            format: 0x3432_5258,
            pixels: &pixels,
            target: rect(),
            clip: None,
            alpha: 1.0,
            sampling: NativeCompositionSampling::ExactNearest,
        })];
        let (report, cpu_image) = renderer
            .export_composed_with_snapshot(frame(&cpu), NativeCompositionOutputRequest::default())
            .expect("CPU image");
        drop(report);
        let expected = [
            255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ];
        assert_eq!(cpu_image.rgba(), expected);

        // A distinct GPU producer supplies an actual DMA-BUF, not CPU pixels
        // relabelled as an accelerated source.
        let mut producer = context();
        let solids = [
            NativeCompositionLayer::Solid(NativeSolidCompositionLayer {
                target: rect(),
                color: [0, 0, 255],
            }),
            NativeCompositionLayer::Solid(NativeSolidCompositionLayer {
                target: NativeCompositionRect {
                    height: 1,
                    ..rect()
                },
                color: [255, 0, 0],
            }),
        ];
        let buffer = producer
            .export_composed_owned_scanout_buffer(
                frame(&solids),
                NativeCompositionOutputRequest::default(),
            )
            .buffer
            .expect("GPU source");
        let fds = buffer.export_plane_fds().unwrap().into_plane_fds();
        let source = NativeMultiPlaneDmaBufFrame {
            width: 2,
            height: 2,
            format: buffer.format(),
            modifier: buffer
                .modifier()
                .unwrap_or(u64::from(gbm::Modifier::Invalid)),
            plane_count: buffer.plane_count(),
            planes: std::array::from_fn(|i| {
                fds[i].as_ref().map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: buffer.plane_offsets()[i],
                    stride: buffer.plane_pitches()[i],
                })
            }),
        };
        let gpu = [NativeCompositionLayer::DmaBuf(
            NativeDmaBufCompositionLayer {
                image_id: NativeRendererImageId::from_raw(1),
                frame: source,
                custody: None,
                target: rect(),
                clip: None,
                alpha: 1.0,
                sampling: NativeCompositionSampling::ExactNearest,
            },
        )];
        let (report, gpu_image) = renderer
            .export_composed_with_snapshot(frame(&gpu), NativeCompositionOutputRequest::default())
            .expect("GPU image");
        drop(report);
        assert_eq!(gpu_image.rgba(), expected);

        let replacement = [NativeCompositionLayer::Solid(NativeSolidCompositionLayer {
            target: rect(),
            color: [0, 255, 0],
        })];
        let ordinary = renderer.export_composed_owned_scanout_buffer(
            frame(&replacement),
            NativeCompositionOutputRequest::default(),
        );
        assert!(ordinary.buffer.is_some());
        drop(ordinary);
        assert!(
            renderer
                .export_composed_with_snapshot(
                    NativeCompositionFrame {
                        width: 0,
                        ..frame(&replacement)
                    },
                    NativeCompositionOutputRequest::default(),
                )
                .is_err()
        );
        assert!(
            renderer
                .export_composed_with_snapshot(
                    frame(&replacement),
                    NativeCompositionOutputRequest {
                        preferred_modifiers: &[],
                        format: Some(NativeCompositionFormatRequest::Required(0)),
                    },
                )
                .is_err()
        );
        let (report, green) = renderer
            .export_composed_with_snapshot(
                frame(&replacement),
                NativeCompositionOutputRequest::default(),
            )
            .expect("fresh capture after failed request");
        drop(report);
        assert_eq!(green.rgba(), [0, 255, 0, 255].repeat(4));
        drop(renderer);
        drop(buffer);
        drop(producer);
        assert_eq!(cpu_image.rgba(), expected);
        assert_eq!(gpu_image.rgba(), expected);
    }
}
