#![cfg(all(feature = "gbm-platform", target_os = "linux"))]
use sophia_renderer_native_egl::{
    NativeCompositionFrame, NativeCompositionLayer, NativeCompositionRect,
    NativeCompositionSampling, NativeDmaBufPlane, NativeGbmOwnedScanoutBuffer,
    NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame, NativePixmapImportProbe,
    NativeRendererImageCompositionLayer, NativeRendererImageId, native_dmabuf_cpu_write_access,
};
use std::{
    fs::{File, OpenOptions},
    os::fd::AsFd,
    path::Path,
};

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}
fn context(path: &Path) -> NativeGbmRenderedScanoutContext<File> {
    NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(path)))
        .context
        .expect("render context")
}
fn read(path: &Path, buffer: &NativeGbmOwnedScanoutBuffer) -> Vec<u8> {
    let fds = buffer.export_plane_fds().unwrap().into_plane_fds();
    NativePixmapImportProbe::new(
        open(path),
        NativeMultiPlaneDmaBufFrame {
            width: buffer.width(),
            height: buffer.height(),
            format: buffer.format(),
            modifier: buffer.modifier().unwrap(),
            plane_count: buffer.plane_count(),
            planes: std::array::from_fn(|i| {
                fds[i].as_ref().map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: buffer.plane_offsets()[i],
                    stride: buffer.plane_pitches()[i],
                })
            }),
        },
    )
    .unwrap()
    .read_rgba()
    .unwrap()
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn fresh_images_reuse_execution_across_resize_and_survive_local_eviction() {
    let path =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"));
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let mut capture = context(&path);
    let mut snapshots = Vec::new();
    for (index, (width, height, format)) in [
        (2, 1, gbm::Format::Xrgb8888),
        (11, 7, gbm::Format::Argb8888),
        (3, 5, gbm::Format::Xrgb8888),
        (2, 1, gbm::Format::Argb8888),
        (11, 7, gbm::Format::Xrgb8888),
        (3, 5, gbm::Format::Argb8888),
    ]
    .into_iter()
    .enumerate()
    {
        let mut source = allocator
            .create_buffer_object_with_modifiers2::<()>(
                width,
                height,
                format,
                std::iter::once(gbm::Modifier::Linear),
                gbm::BufferObjectFlags::RENDERING,
            )
            .unwrap();
        let fd = source.fd_for_plane(0).unwrap();
        native_dmabuf_cpu_write_access(&fd, false).unwrap();
        source
            .map_mut(0, 0, width, height, |mapped| {
                let stride = mapped.stride() as usize;
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        mapped.buffer_mut()[y * stride + x * 4..y * stride + x * 4 + 4]
                            .copy_from_slice(&[x as u8 + 10, y as u8 + 30, index as u8 + 70, 255]);
                    }
                }
            })
            .unwrap();
        native_dmabuf_cpu_write_access(&fd, true).unwrap();
        let image = NativeRendererImageId::from_raw(index as u64 + 1);
        capture
            .capture_renderer_image(
                image,
                NativeMultiPlaneDmaBufFrame {
                    width,
                    height,
                    format: format as u32,
                    modifier: u64::from(source.modifier()),
                    plane_count: 1,
                    planes: [
                        Some(NativeDmaBufPlane {
                            fd: fd.as_fd(),
                            offset: source.offset(0),
                            stride: source.stride_for_plane(0),
                        }),
                        None,
                        None,
                        None,
                    ],
                },
            )
            .unwrap();
        capture.promote_renderer_image(image).unwrap();
        let snapshot = capture
            .export_promoted_renderer_image(image)
            .unwrap()
            .unwrap();
        // Reuse the client's storage immediately, then destroy it. A cached
        // import or a capture surface reused as storage corrupts this witness.
        native_dmabuf_cpu_write_access(&fd, false).unwrap();
        source
            .map_mut(0, 0, width, height, |mapped| mapped.buffer_mut().fill(0))
            .unwrap();
        native_dmabuf_cpu_write_access(&fd, true).unwrap();
        capture.evict_renderer_image(image).unwrap();
        snapshots.push((snapshot, width, height, index));
    }
    let stats = capture.persistent_render_stats();
    eprintln!("capture stats: {stats:?}");
    assert_eq!(stats.snapshot_live_entries, 0);
    assert_eq!(
        stats.capture_context_creations, 2,
        "one execution context per format"
    );
    assert_eq!(stats.capture_context_reuses, 4);
    assert_eq!(stats.gl_pipeline_creations, 2);
    assert_eq!(stats.import_cache.imports, 6);
    assert_eq!(stats.import_cache.evictions, 6);
    assert_eq!(stats.import_cache.hits, 0);
    assert_eq!(stats.import_cache.live_entries, 0);
    assert_eq!(
        stats.capture_surface_creations, 6,
        "every image owns fresh storage"
    );
    drop(capture);
    let mut replacement = context(&path);
    for (snapshot, width, height, index) in snapshots {
        let image_id = snapshot.image_id();
        replacement
            .restore_promoted_renderer_image(snapshot)
            .unwrap();
        let layers = [NativeCompositionLayer::RendererImage(
            NativeRendererImageCompositionLayer {
                image_id,
                target: NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: width as i32,
                    height: height as i32,
                },
                clip: None,
                alpha: 1.0,
                sampling: NativeCompositionSampling::ExactNearest,
            },
        )];
        let report = replacement.export_composed_owned_scanout_buffer_with_modifiers(
            NativeCompositionFrame {
                width,
                height,
                layers: &layers,
                trace: None,
                repaint: None,
            },
            &[0],
        );
        let buffer = report
            .buffer
            .unwrap_or_else(|| panic!("composition: {:?}", report.detail));
        let expected: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| [index as u8 + 70, y as u8 + 30, x as u8 + 10, 255])
            })
            .collect();
        assert_eq!(
            read(&path, &buffer),
            expected,
            "retained immutable image {index}"
        );
        drop(buffer);
        replacement.evict_renderer_image(image_id).unwrap();
    }
}
